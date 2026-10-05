//! Native conversation encoding; this is not accounting evidence or I/O authority.
use crate::{codex::NATIVE_PROVIDER_ID, schema_validation::InferenceSchemas};
use admin_panel_domain::{
    ai::{ContextBudget, ModelCapabilities, ProviderId, ProviderSettings, RuntimeProfile},
    inference::{ExecutionScope, InferenceError, InferenceRequest, Message},
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::Path};

// Leave headroom for the bounded RPC envelope. Never truncate conversation data.
const MAX_PARAMETER_BYTES: usize = 1024 * 1024;

// No Debug: instructions, schemas and history may contain private task evidence.
pub struct NativeConversation {
    start: Value,
    history: Vec<Value>,
    turn: Value,
}

impl NativeConversation {
    /// Capabilities and scope come from trusted runtime records, not the caller.
    /// This method alone never permits provider dispatch or proves token accounting.
    pub fn from_frozen(
        request: &InferenceRequest,
        authorized: &ExecutionScope,
        profile: &RuntimeProfile,
        capabilities: &ModelCapabilities,
    ) -> Result<Self, InferenceError> {
        request.validate_binding(authorized, profile)?;
        request.validate_conversation()?;
        if profile.provider != ProviderId::Chatgpt || profile.model != "gpt-6-luna" {
            return Err(InferenceError::Provider);
        }
        ProviderSettings {
            provider: profile.provider,
            model: profile.model.clone(),
            context_window_tokens: profile.context_window_tokens,
        }
        .validate(capabilities)?;
        if request.output_reserve_tokens != capabilities.max_output_tokens {
            return Err(InferenceError::OutputReserve);
        }
        ContextBudget::new(profile.context_window_tokens, capabilities)?
            .input_limit(request.output_reserve_tokens)?;
        let schemas = InferenceSchemas::compile(request).map_err(|_| InferenceError::ToolSchema)?;
        let (history_length, input) = match request.messages.last() {
            Some(Message::User { content }) if !content.is_empty() => (
                request.messages.len() - 1,
                json!([{"type":"text","text":content}]),
            ),
            // Pinned API accepts an empty turn after injected complete tool pairs.
            Some(Message::Tool { .. }) => (request.messages.len(), json!([])),
            _ => return Err(InferenceError::Conversation),
        };
        let declared: BTreeSet<_> = request
            .tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect();
        let mut instructions = Vec::new();
        let mut history = Vec::new();
        let mut prefix = true;
        for message in &request.messages[..history_length] {
            match message {
                Message::System { content } => {
                    if !prefix || content.is_empty() {
                        return Err(InferenceError::Conversation);
                    }
                    instructions.push(content.as_str());
                }
                Message::Developer { content } | Message::User { content } => {
                    prefix = false;
                    let role = if matches!(message, Message::Developer { .. }) {
                        "developer"
                    } else {
                        "user"
                    };
                    history.push(json!({"type":"message","role":role,
                        "content":[{"type":"input_text","text":content}]}));
                }
                Message::Assistant {
                    content,
                    tool_calls,
                } => {
                    prefix = false;
                    schemas
                        .validate_tool_calls(tool_calls)
                        .map_err(|_| InferenceError::ToolSchema)?;
                    if let Some(content) = content.as_deref().filter(|text| !text.is_empty()) {
                        history.push(json!({"type":"message","role":"assistant",
                            "content":[{"type":"output_text","text":content}]}));
                    }
                    for call in tool_calls {
                        if !declared.contains(call.name.as_str()) {
                            return Err(InferenceError::Conversation);
                        }
                        history.push(json!({"type":"function_call","call_id":call.id,
                            "name":call.name,"namespace":"functions",
                            "arguments":serde_json::to_string(&call.arguments).map_err(|_| InferenceError::Conversation)?}));
                    }
                }
                Message::Tool {
                    tool_call_id,
                    content,
                } => {
                    prefix = false;
                    history.push(json!({"type":"function_call_output","call_id":tool_call_id,"output":content}));
                }
            }
        }
        let start = json!({"model":profile.model,"modelProvider":NATIVE_PROVIDER_ID,
            "allowProviderModelFallback":false,"ephemeral":true,"approvalPolicy":"never","sandbox":"read-only",
            "baseInstructions":instructions.join("\n\n"),
            "config":{"model_context_window":profile.context_window_tokens},
            "dynamicTools":request.tools.iter().map(|tool|json!({"type":"function","name":tool.name,
                "description":tool.description,"inputSchema":tool.parameters})).collect::<Vec<_>>()});
        let mut turn = json!({"input":input});
        if let Some(schema) = &request.output_schema {
            turn["outputSchema"] = schema.clone();
        }
        let conversation = Self {
            start,
            history,
            turn,
        };
        for parameters in [
            &conversation.start,
            &json!(conversation.history),
            &conversation.turn,
        ] {
            bounded(parameters)?;
        }
        Ok(conversation)
    }

    /// `workdir` is the deployment-owned empty directory; never caller checkout.
    pub fn thread_start_params(&self, workdir: &Path) -> Result<Value, InferenceError> {
        if !workdir.is_absolute() || workdir.to_str().is_none() {
            return Err(InferenceError::Conversation);
        }
        let mut params = self.start.clone();
        params["cwd"] = json!(workdir);
        bounded(&params)?;
        Ok(params)
    }

    pub fn history_params(&self, thread_id: &str) -> Result<Value, InferenceError> {
        thread_id_valid(thread_id)?;
        let params = json!({"threadId":thread_id,"items":self.history});
        bounded(&params)?;
        Ok(params)
    }

    pub fn turn_start_params(&self, thread_id: &str) -> Result<Value, InferenceError> {
        thread_id_valid(thread_id)?;
        let mut params = self.turn.clone();
        params["threadId"] = json!(thread_id);
        bounded(&params)?;
        Ok(params)
    }
}

fn bounded(params: &Value) -> Result<(), InferenceError> {
    if serde_json::to_vec(params)
        .map_err(|_| InferenceError::Conversation)?
        .len()
        > MAX_PARAMETER_BYTES
    {
        return Err(InferenceError::Conversation);
    }
    Ok(())
}

fn thread_id_valid(thread_id: &str) -> Result<(), InferenceError> {
    if thread_id.is_empty()
        || thread_id.len() > 256
        || thread_id
            .chars()
            .any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(InferenceError::Conversation);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use admin_panel_domain::inference::{ToolCall, ToolDefinition};
    use uuid::Uuid;

    fn fixture() -> (InferenceRequest, RuntimeProfile, ModelCapabilities) {
        let profile = RuntimeProfile {
            schema_version: 1,
            revision: 7,
            workspace: "sdlc2".into(),
            provider: ProviderId::Chatgpt,
            model: "gpt-6-luna".into(),
            context_window_tokens: 256000,
            verification_id: Uuid::new_v4(),
        };
        // Synthetic values for encoding tests, never production capability proof.
        let capabilities = ModelCapabilities {
            model: profile.model.clone(),
            context_limit_tokens: 300000,
            max_output_tokens: 65536,
            tools: true,
            structured_output: true,
            streaming: true,
            cancellation: true,
        };
        let request = InferenceRequest {
            schema_version: 1,
            request_id: Uuid::new_v4(),
            profile_revision: 7,
            execution: ExecutionScope {
                workspace: "sdlc2".into(),
                owner_subject: "owner".into(),
                project_id: Uuid::new_v4(),
                root_task_id: Uuid::new_v4(),
                task_id: Uuid::new_v4(),
                agent_id: Uuid::new_v4(),
                execution_id: Uuid::new_v4(),
            },
            messages: vec![
                Message::System {
                    content: "Immutable requirements\nРусский текст".into(),
                },
                Message::System {
                    content: "Mandatory evidence".into(),
                },
                Message::Developer {
                    content: "Agent instructions".into(),
                },
                Message::User {
                    content: "Previous user".into(),
                },
                Message::Assistant {
                    content: Some("Previous answer".into()),
                    tool_calls: vec![],
                },
                Message::User {
                    content: "Current user".into(),
                },
            ],
            tools: vec![],
            output_schema: None,
            output_reserve_tokens: 65536,
        };
        (request, profile, capabilities)
    }

    fn encode(
        request: &InferenceRequest,
        profile: &RuntimeProfile,
        caps: &ModelCapabilities,
    ) -> Result<NativeConversation, InferenceError> {
        NativeConversation::from_frozen(request, &request.execution, profile, caps)
    }

    #[test]
    fn system_prefix_is_preserved_in_supported_base_instructions_without_duplicate_user() {
        let (request, profile, caps) = fixture();
        let encoded = encode(&request, &profile, &caps).unwrap();
        let start = encoded
            .thread_start_params(Path::new("/run/native-work"))
            .unwrap();
        assert_eq!(
            start["baseInstructions"],
            "Immutable requirements\nРусский текст\n\nMandatory evidence"
        );
        assert_eq!(start["model"], "gpt-6-luna");
        assert_eq!(start["modelProvider"], NATIVE_PROVIDER_ID);
        assert_eq!(start["allowProviderModelFallback"], false);
        assert_eq!(start["config"]["model_context_window"], 256000);
        assert!(start.get("max_output_tokens").is_none());
        let history = encoded.history_params("own-thread").unwrap();
        let items = history["items"].as_array().unwrap();
        assert_eq!(
            items
                .iter()
                .map(|item| item["role"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["developer", "user", "assistant"]
        );
        assert!(
            !serde_json::to_string(items)
                .unwrap()
                .contains("Current user")
        );
        let turn = encoded.turn_start_params("own-thread").unwrap();
        assert_eq!(turn["input"][0]["text"], "Current user");
        assert!(
            !serde_json::to_string(&turn)
                .unwrap()
                .contains("Previous user")
        );
    }

    #[test]
    fn late_system_and_empty_system_or_current_user_are_not_hoisted_or_dropped() {
        let (request, profile, caps) = fixture();
        for replacement in [
            Message::System {
                content: "late system".into(),
            },
            Message::System {
                content: String::new(),
            },
        ] {
            let mut invalid = request.clone();
            invalid.messages[3] = replacement;
            assert!(matches!(
                encode(&invalid, &profile, &caps),
                Err(InferenceError::Conversation)
            ));
        }
        let mut invalid = request.clone();
        invalid.messages[0] = Message::System {
            content: String::new(),
        };
        assert!(matches!(
            encode(&invalid, &profile, &caps),
            Err(InferenceError::Conversation)
        ));
        *invalid.messages.last_mut().unwrap() = Message::User {
            content: String::new(),
        };
        assert!(encode(&invalid, &profile, &caps).is_err());
    }

    fn with_tools() -> (InferenceRequest, RuntimeProfile, ModelCapabilities) {
        let (mut request, profile, caps) = fixture();
        request.tools.push(ToolDefinition { name:"read_evidence".into(), description:"Read assigned evidence".into(),
            parameters:json!({"type":"object","properties":{"value":{"type":"string"}},"required":["value"],"additionalProperties":false}) });
        request.messages.splice(
            4..5,
            [
                Message::Assistant {
                    content: None,
                    tool_calls: vec![ToolCall {
                        id: "call_own".into(),
                        name: "read_evidence".into(),
                        arguments: json!({"value":"quotes \" and newline\n"}),
                    }],
                },
                Message::Tool {
                    tool_call_id: "call_own".into(),
                    content: "Private evidence".into(),
                },
            ],
        );
        request.output_schema = Some(
            json!({"type":"object","properties":{"ok":{"type":"boolean"}},"required":["ok"],"additionalProperties":false}),
        );
        (request, profile, caps)
    }

    #[test]
    fn complete_tool_pairs_schema_and_caller_namespace_survive_encoding() {
        let (request, profile, caps) = with_tools();
        let encoded = encode(&request, &profile, &caps).unwrap();
        let start = encoded
            .thread_start_params(Path::new("/run/native-work"))
            .unwrap();
        assert_eq!(start["dynamicTools"][0]["type"], "function");
        assert_eq!(start["dynamicTools"][0]["name"], "read_evidence");
        let history = encoded.history_params("own-thread").unwrap();
        let call = &history["items"][2];
        assert_eq!(call["type"], "function_call");
        assert_eq!(call["name"], "read_evidence");
        assert_eq!(call["namespace"], "functions");
        assert_eq!(call["call_id"], history["items"][3]["call_id"]);
        assert_eq!(
            serde_json::from_str::<Value>(call["arguments"].as_str().unwrap()).unwrap(),
            json!({"value":"quotes \" and newline\n"})
        );
        assert_eq!(history["items"][3]["output"], "Private evidence");
        assert_eq!(
            encoded.turn_start_params("own-thread").unwrap()["outputSchema"],
            request.output_schema.unwrap()
        );
    }

    #[test]
    fn undeclared_invalid_or_incomplete_tool_history_is_rejected() {
        let (request, profile, caps) = with_tools();
        let mut invalid = request.clone();
        invalid.tools.clear();
        assert!(encode(&invalid, &profile, &caps).is_err());
        let mut invalid = request.clone();
        if let Message::Assistant { tool_calls, .. } = &mut invalid.messages[4] {
            tool_calls[0].arguments = json!({"value":42});
        }
        assert!(matches!(
            encode(&invalid, &profile, &caps),
            Err(InferenceError::ToolSchema)
        ));
        let mut invalid = request;
        invalid.messages.remove(5);
        assert!(encode(&invalid, &profile, &caps).is_err());
    }

    #[test]
    fn complete_tool_tail_starts_empty_input_without_manufacturing_a_user_message() {
        let (mut request, profile, caps) = with_tools();
        request.messages.pop();
        let encoded = encode(&request, &profile, &caps).unwrap();
        assert_eq!(
            encoded.turn_start_params("own-thread").unwrap()["input"],
            json!([])
        );
        let history = encoded.history_params("own-thread").unwrap();
        let items = history["items"].as_array().unwrap();
        assert_eq!(items.last().unwrap()["type"], "function_call_output");
        assert_eq!(items.last().unwrap()["call_id"], "call_own");
        assert!(
            !serde_json::to_string(&history)
                .unwrap()
                .contains("Current user")
        );
        let (mut assistant_tail, _, _) = fixture();
        assistant_tail.messages.pop();
        assert!(matches!(
            encode(&assistant_tail, &profile, &caps),
            Err(InferenceError::Conversation)
        ));
    }

    #[test]
    fn scope_revision_provider_model_and_physical_output_reserve_are_checked() {
        let (request, profile, caps) = fixture();
        let mut foreign = request.execution.clone();
        foreign.task_id = Uuid::new_v4();
        assert!(matches!(
            NativeConversation::from_frozen(&request, &foreign, &profile, &caps),
            Err(InferenceError::ExecutionScope)
        ));
        let mut invalid = request.clone();
        invalid.profile_revision += 1;
        assert!(matches!(
            encode(&invalid, &profile, &caps),
            Err(InferenceError::Revision)
        ));
        let mut other = profile.clone();
        other.provider = ProviderId::Openrouter;
        assert!(matches!(
            encode(&request, &other, &caps),
            Err(InferenceError::Provider)
        ));
        other = profile.clone();
        other.model = "other-model".into();
        assert!(matches!(
            encode(&request, &other, &caps),
            Err(InferenceError::Provider)
        ));
        invalid = request.clone();
        invalid.output_reserve_tokens -= 1;
        assert!(matches!(
            encode(&invalid, &profile, &caps),
            Err(InferenceError::OutputReserve)
        ));
        let mut unknown = caps.clone();
        unknown.max_output_tokens = 0;
        assert!(encode(&request, &profile, &unknown).is_err());
        let mut reduced = caps.clone();
        reduced.context_limit_tokens = 128000;
        assert!(encode(&request, &profile, &reduced).is_err());
    }

    #[test]
    fn rpc_overflow_and_unsafe_protocol_identifiers_fail_without_truncation() {
        let (mut request, profile, caps) = fixture();
        let encoded = encode(&request, &profile, &caps).unwrap();
        for id in ["", "foreign\nthread", "thread with spaces"] {
            assert!(encoded.turn_start_params(id).is_err());
        }
        assert!(
            encoded
                .thread_start_params(Path::new("relative/checkout"))
                .is_err()
        );
        *request.messages.last_mut().unwrap() = Message::User {
            content: "x".repeat(MAX_PARAMETER_BYTES + 1),
        };
        assert!(matches!(
            encode(&request, &profile, &caps),
            Err(InferenceError::Conversation)
        ));
    }
}
