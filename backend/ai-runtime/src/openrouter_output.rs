//! A complete provider turn, validated before it can supply terminal evidence.
use crate::{
    error::RuntimeError,
    openrouter_reasoning::OpenRouterReasoning,
    openrouter_stream::{Decoder, StreamFrame},
    schema_validation::InferenceSchemas,
};
use admin_panel_domain::inference::{InferenceRequest, ToolCall};
use serde_json::Value;
use std::collections::BTreeMap;

const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Default)]
struct ToolFragments {
    id: Option<String>,
    name: String,
    arguments: String,
}

// No Debug: this includes private text, evidence and provider reasoning.
pub struct DecodedTurn {
    pub(crate) text: String,
    pub(crate) calls: Vec<ToolCall>,
    pub(crate) reasoning: Option<OpenRouterReasoning>,
    pub(crate) generation_id: String,
    pub(crate) usage: Value,
}

impl DecodedTurn {
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn tool_calls(&self) -> &[ToolCall] {
        &self.calls
    }
    pub fn generation_id(&self) -> &str {
        &self.generation_id
    }
    pub fn usage(&self) -> &Value {
        &self.usage
    }
    /// Broker adapter state, not part of the external transcript contract.
    pub fn reasoning(&self) -> Option<&OpenRouterReasoning> {
        self.reasoning.as_ref()
    }
}

pub struct OutputDecoder {
    decoder: Decoder,
    request: InferenceRequest,
    schemas: InferenceSchemas,
    text: String,
    tools: BTreeMap<usize, ToolFragments>,
    reasoning: OpenRouterReasoning,
    finish_reason: Option<String>,
    generation_id: Option<String>,
    usage: Option<Value>,
    received_output_bytes: usize,
    failed: bool,
}

impl OutputDecoder {
    pub fn new(model: &str, request: &InferenceRequest) -> Result<Self, RuntimeError> {
        Ok(Self {
            decoder: Decoder::new(model)?,
            request: request.clone(),
            schemas: InferenceSchemas::compile(request)?,
            text: String::new(),
            tools: BTreeMap::new(),
            reasoning: Default::default(),
            finish_reason: None,
            generation_id: None,
            usage: None,
            received_output_bytes: 0,
            failed: false,
        })
    }

    pub fn generation_id(&self) -> Option<&str> {
        self.generation_id.as_deref()
    }

    /// Only visible text is returned; private reasoning stays in the broker.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<String>, RuntimeError> {
        if self.failed {
            return Err(RuntimeError::Protocol);
        }
        let result = self.feed_inner(bytes);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn feed_inner(&mut self, bytes: &[u8]) -> Result<Vec<String>, RuntimeError> {
        let mut visible = vec![];
        for frame in self.decoder.feed(bytes)? {
            let value = match frame {
                StreamFrame::Chunk(value) | StreamFrame::Usage(value) => value,
                StreamFrame::Completed => continue,
            };
            self.generation_id = value["id"].as_str().map(str::to_owned);
            if let Some(usage) = value.get("usage").filter(|v| !v.is_null()) {
                self.usage = Some(usage.clone());
            }
            let Some(choice) = value["choices"].as_array().and_then(|v| v.first()) else {
                continue;
            };
            if let Some(reason) = choice["finish_reason"].as_str() {
                self.finish_reason = Some(reason.into());
            }
            let delta = &choice["delta"];
            if delta
                .as_object()
                .ok_or(RuntimeError::Protocol)?
                .keys()
                .any(|key| {
                    !matches!(
                        key.as_str(),
                        "role"
                            | "content"
                            | "tool_calls"
                            | "reasoning"
                            | "reasoning_content"
                            | "reasoning_details"
                    )
                })
                || delta
                    .get("role")
                    .filter(|v| !v.is_null())
                    .is_some_and(|v| v != "assistant")
            {
                return Err(RuntimeError::Protocol);
            }
            let bytes = serde_json::to_vec(delta)
                .map_err(|_| RuntimeError::Protocol)?
                .len();
            self.received_output_bytes = self
                .received_output_bytes
                .checked_add(bytes)
                .ok_or(RuntimeError::Protocol)?;
            if self.received_output_bytes > MAX_OUTPUT_BYTES {
                return Err(RuntimeError::Protocol);
            }
            self.reasoning.append_delta(delta)?;
            if let Some(content) = delta.get("content").filter(|v| !v.is_null()) {
                let text = content.as_str().ok_or(RuntimeError::Protocol)?;
                self.text.push_str(text);
                if !text.is_empty() {
                    visible.push(text.into());
                }
            }
            if let Some(calls) = delta.get("tool_calls").filter(|v| !v.is_null()) {
                for call in calls.as_array().ok_or(RuntimeError::Protocol)? {
                    self.append_tool(call)?;
                }
            }
        }
        Ok(visible)
    }

    fn append_tool(&mut self, call: &Value) -> Result<(), RuntimeError> {
        let index = call["index"]
            .as_u64()
            .filter(|v| *v < 128)
            .ok_or(RuntimeError::Protocol)? as usize;
        if call
            .get("type")
            .filter(|v| !v.is_null())
            .is_some_and(|v| v != "function")
        {
            return Err(RuntimeError::Protocol);
        }
        let fragment = self.tools.entry(index).or_default();
        if let Some(id) = call.get("id").filter(|v| !v.is_null()) {
            let id = id
                .as_str()
                .filter(|v| !v.is_empty() && v.len() <= 256)
                .ok_or(RuntimeError::Protocol)?;
            if fragment.id.as_ref().is_some_and(|previous| previous != id) {
                return Err(RuntimeError::Protocol);
            }
            fragment.id = Some(id.into());
        }
        if let Some(function) = call.get("function").filter(|v| !v.is_null()) {
            if !function.is_object() {
                return Err(RuntimeError::Protocol);
            }
            for (field, destination) in [
                ("name", &mut fragment.name),
                ("arguments", &mut fragment.arguments),
            ] {
                if let Some(value) = function.get(field).filter(|v| !v.is_null()) {
                    destination.push_str(value.as_str().ok_or(RuntimeError::Protocol)?);
                }
            }
            if fragment.name.len() > 256 {
                return Err(RuntimeError::Protocol);
            }
        }
        Ok(())
    }

    pub fn finish(self) -> Result<DecodedTurn, RuntimeError> {
        if self.failed {
            return Err(RuntimeError::Protocol);
        }
        self.decoder.finish()?;
        let usage = self.usage.ok_or(RuntimeError::Protocol)?;
        if usage["completion_tokens"]
            .as_u64()
            .ok_or(RuntimeError::Protocol)?
            > u64::from(self.request.output_reserve_tokens)
        {
            return Err(RuntimeError::OutputTruncated);
        }
        let mut calls = Vec::new();
        for (expected, (index, fragments)) in self.tools.into_iter().enumerate() {
            if expected != index {
                return Err(RuntimeError::Protocol);
            }
            calls.push(ToolCall {
                id: fragments.id.ok_or(RuntimeError::Protocol)?,
                name: fragments.name,
                arguments: serde_json::from_str(&fragments.arguments)
                    .map_err(|_| RuntimeError::Protocol)?,
            });
        }
        match self.finish_reason.as_deref() {
            Some("stop") if calls.is_empty() && !self.text.is_empty() => {
                self.schemas.validate_output(&self.text)?
            }
            Some("tool_calls") if !calls.is_empty() => {
                self.request
                    .validate_returned_tools(&calls)
                    .map_err(|_| RuntimeError::Protocol)?;
                self.schemas.validate_tool_calls(&calls)?;
            }
            _ => return Err(RuntimeError::Protocol),
        }
        Ok(DecodedTurn {
            text: self.text,
            calls,
            reasoning: (!self.reasoning.is_empty()).then_some(self.reasoning),
            generation_id: self.generation_id.ok_or(RuntimeError::Protocol)?,
            usage,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use admin_panel_domain::inference::{ExecutionScope, Message, ToolDefinition};
    use serde_json::json;
    use uuid::Uuid;

    fn request() -> InferenceRequest {
        InferenceRequest {
            schema_version: 1,
            request_id: Uuid::new_v4(),
            profile_revision: 3,
            execution: ExecutionScope {
                workspace: "sdlc2".into(),
                owner_subject: "own-user".into(),
                project_id: Uuid::new_v4(),
                root_task_id: Uuid::new_v4(),
                task_id: Uuid::new_v4(),
                agent_id: Uuid::new_v4(),
                execution_id: Uuid::new_v4(),
            },
            messages: vec![Message::User {
                content: "required evidence".into(),
            }],
            tools: vec![ToolDefinition {
                name: "read_evidence".into(),
                description: "Own evidence".into(),
                parameters: json!({"type":"object","properties":{"task":{"type":"string"}},"required":["task"],"additionalProperties":false}),
            }],
            output_schema: Some(
                json!({"type":"object","properties":{"ok":{"type":"boolean"}},"required":["ok"],"additionalProperties":false}),
            ),
            output_reserve_tokens: 1000,
        }
    }

    fn chunk(delta: Value, reason: Option<&str>) -> String {
        format!(
            "data: {}\n\n",
            json!({"id":"gen-own","model":"deepseek/deepseek-v4.1-flash",
            "choices":[{"index":0,"delta":delta,"finish_reason":reason}]})
        )
    }

    fn ending(reason: &str, completion: u64) -> String {
        let value = json!({"id":"gen-own","model":"deepseek/deepseek-v4.1-flash",
            "choices":[{"index":0,"delta":{},"finish_reason":reason}],"usage":{"prompt_tokens":20,"completion_tokens":completion}});
        format!("data: {value}\n\ndata: [DONE]\n\n")
    }

    fn decode(stream: &str, request: &InferenceRequest) -> Result<DecodedTurn, RuntimeError> {
        let mut decoder = OutputDecoder::new("deepseek/deepseek-v4.1-flash", request)?;
        for bytes in stream.as_bytes().chunks(1) {
            decoder.feed(bytes)?;
        }
        decoder.finish()
    }

    #[test]
    fn fragmented_parallel_tools_preserve_private_reasoning_without_executing_or_exposing_it() {
        let reasoning = json!({"type":"reasoning.encrypted","data":"fixture-opaque","signature":"fixture-signature","index":0});
        let stream = format!(
            "{}{}{}{}",
            chunk(
                json!({"role":"assistant","reasoning":"private fixture ","reasoning_details":[reasoning.clone()]}),
                None
            ),
            chunk(
                json!({"content":"Привет","reasoning":"continuation","reasoning_details":[{"type":"reasoning.text","text":"fixture only","index":1}],
                "tool_calls":[{"index":1,"id":"call_b","type":"function","function":{"name":"read_","arguments":"{\"task\":"}},
                {"index":0,"id":"call_a","type":"function","function":{"name":"read_evidence","arguments":"{\"task\":"}}]}),
                None
            ),
            chunk(
                json!({"tool_calls":[{"index":0,"function":{"arguments":"\"a\"}"}},
                {"index":1,"function":{"name":"evidence","arguments":"\"b\"}"}}]}),
                None
            ),
            ending("tool_calls", 12)
        );
        let mut decoder = OutputDecoder::new("deepseek/deepseek-v4.1-flash", &request()).unwrap();
        let mut visible = vec![];
        for bytes in stream.as_bytes().chunks(1) {
            visible.extend(decoder.feed(bytes).unwrap());
        }
        assert_eq!(visible, vec!["Привет"]);
        let turn = decoder.finish().unwrap();
        assert_eq!(turn.tool_calls().len(), 2);
        assert_eq!(turn.tool_calls()[0].id, "call_a");
        assert_eq!(turn.tool_calls()[1].arguments, json!({"task":"b"}));
        assert_eq!(turn.generation_id(), "gen-own");
        assert_eq!(turn.usage()["completion_tokens"], 12);
        let mut message = json!({"role":"assistant"});
        turn.reasoning()
            .unwrap()
            .apply_to_message(&mut message)
            .unwrap();
        assert_eq!(message["reasoning"], "private fixture continuation");
        assert_eq!(message["reasoning_details"][0], reasoning);
        assert_eq!(message["reasoning_details"][1]["index"], 1);
    }

    #[test]
    fn validates_actual_final_json_and_never_accepts_schema_errors_truncation_or_missing_receipts()
    {
        let request = request();
        let valid = format!(
            "{}{}",
            chunk(json!({"content":"{\"ok\":true}"}), None),
            ending("stop", 8)
        );
        assert_eq!(decode(&valid, &request).unwrap().text(), "{\"ok\":true}");
        for text in [
            "{}",
            "{\"ok\":\"yes\"}",
            "```json\n{\"ok\":true}\n```",
            "{\"ok\":",
        ] {
            assert!(matches!(
                decode(
                    &format!(
                        "{}{}",
                        chunk(json!({"content":text}), None),
                        ending("stop", 8)
                    ),
                    &request
                ),
                Err(RuntimeError::Protocol)
            ));
        }
        assert!(matches!(
            decode(&valid.replace("data: [DONE]\n\n", ""), &request),
            Err(RuntimeError::Protocol)
        ));
        assert!(matches!(
            decode(
                &valid.replace("\"completion_tokens\":8", "\"completion_tokens\":1001"),
                &request
            ),
            Err(RuntimeError::OutputTruncated)
        ));
        assert!(matches!(
            decode(&valid.replace("\"stop\"", "\"length\""), &request),
            Err(RuntimeError::OutputTruncated)
        ));
    }

    #[test]
    fn rejects_wrong_tool_schema_identity_finish_and_unrecognized_private_blocks() {
        let request = request();
        for arguments in ["{}", "{\"task\":1}", "{\"task\":\"a\",\"extra\":true}", "{"] {
            let stream = format!(
                "{}{}",
                chunk(
                    json!({"tool_calls":[{"index":0,"id":"call_a","type":"function",
                "function":{"name":"read_evidence","arguments":arguments}}]}),
                    None
                ),
                ending("tool_calls", 8)
            );
            assert!(matches!(
                decode(&stream, &request),
                Err(RuntimeError::Protocol)
            ));
        }
        for delta in [
            json!({"reasoning_details":[{"type":"unknown_private_format"}]}),
            json!({"reasoning":"a","reasoning_content":"b"}),
            json!({"role":"system"}),
            json!({"refusal":"private provider failure"}),
            json!({"tool_calls":[{"index":128}]}),
        ] {
            assert!(matches!(
                decode(
                    &format!("{}{}", chunk(delta, None), ending("stop", 8)),
                    &request
                ),
                Err(RuntimeError::Protocol)
            ));
        }
        let stream = format!(
            "{}{}{}",
            chunk(
                json!({"tool_calls":[{"index":0,"id":"first","function":{"name":"read_evidence","arguments":"{\"task\":\"a\"}"}}]}),
                None
            ),
            chunk(json!({"tool_calls":[{"index":0,"id":"changed"}]}), None),
            ending("tool_calls", 8)
        );
        assert!(matches!(
            decode(&stream, &request),
            Err(RuntimeError::Protocol)
        ));
        assert!(matches!(
            decode(
                &format!(
                    "{}{}",
                    chunk(json!({"content":"{\"ok\":true}"}), None),
                    ending("tool_calls", 8)
                ),
                &request
            ),
            Err(RuntimeError::Protocol)
        ));
    }
}
