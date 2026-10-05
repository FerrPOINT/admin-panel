//! Secret-free execution identity and complete conversation validation.
//! Authentication and project access must be verified before constructing a grant.
use crate::ai::{AiError, ContextBudget, ModelCapabilities, RuntimeProfile};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeSet, HashSet};
use uuid::Uuid;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionScope {
    pub workspace: String,
    pub owner_subject: String,
    pub project_id: Uuid,
    pub root_task_id: Uuid,
    pub task_id: Uuid,
    pub agent_id: Uuid,
    pub execution_id: Uuid,
}

impl ExecutionScope {
    pub fn validate(&self) -> Result<(), InferenceError> {
        if self.workspace != "sdlc2"
            || self.owner_subject.is_empty()
            || self.owner_subject.len() > 256
            || self
                .owner_subject
                .chars()
                .any(|c| c.is_control() || c.is_whitespace())
            || [
                self.project_id,
                self.root_task_id,
                self.task_id,
                self.agent_id,
                self.execution_id,
            ]
            .iter()
            .any(Uuid::is_nil)
        {
            return Err(InferenceError::ExecutionScope);
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub content: String,
}

/// Conversation, tools, model and context are inherited from the parent run.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolContinuation {
    pub schema_version: u32,
    pub request_id: Uuid,
    pub parent_request_id: Uuid,
    pub profile_revision: u64,
    pub execution: ExecutionScope,
    pub results: Vec<ToolResult>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case", deny_unknown_fields)]
pub enum Message {
    System {
        content: String,
    },
    Developer {
        content: String,
    },
    User {
        content: String,
    },
    Assistant {
        content: Option<String>,
        #[serde(default)]
        tool_calls: Vec<ToolCall>,
    },
    Tool {
        tool_call_id: String,
        content: String,
    },
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceRequest {
    pub schema_version: u32,
    pub request_id: Uuid,
    pub profile_revision: u64,
    pub execution: ExecutionScope,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub tools: Vec<ToolDefinition>,
    pub output_schema: Option<Value>,
    pub output_reserve_tokens: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, thiserror::Error, Debug)]
pub enum InferenceError {
    #[error("execution_scope_mismatch")]
    ExecutionScope,
    #[error("frozen_profile_revision_mismatch")]
    Revision,
    #[error("frozen_profile_provider_mismatch")]
    Provider,
    #[error("invalid_conversation")]
    Conversation,
    #[error("invalid_tool_schema")]
    ToolSchema,
    #[error("context_accounting_not_confirmed")]
    Accounting,
    #[error("output_reserve_not_enforceable")]
    OutputReserve,
    #[error(transparent)]
    Context(#[from] AiError),
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.'))
}

fn valid_schema(value: &Value, depth: usize) -> bool {
    if depth > 32 {
        return false;
    }
    match value {
        Value::Object(object) => object.iter().all(|(key, value)| {
            (!matches!(key.as_str(), "$ref" | "$dynamicRef" | "$recursiveRef")
                || value
                    .as_str()
                    .is_some_and(|reference| reference.starts_with('#')))
                && valid_schema(value, depth + 1)
        }),
        Value::Array(values) => values.iter().all(|value| valid_schema(value, depth + 1)),
        _ => true,
    }
}

impl InferenceRequest {
    pub fn validate_returned_tools(&self, calls: &[ToolCall]) -> Result<(), InferenceError> {
        let declared: BTreeSet<_> = self.tools.iter().map(|tool| tool.name.as_str()).collect();
        let mut ids: BTreeSet<_> = self
            .messages
            .iter()
            .filter_map(|message| match message {
                Message::Assistant { tool_calls, .. } => {
                    Some(tool_calls.iter().map(|call| call.id.as_str()))
                }
                _ => None,
            })
            .flatten()
            .collect();
        if calls.is_empty()
            || calls.len() > 128
            || calls.iter().any(|call| {
                !valid_name(&call.id)
                    || !valid_name(&call.name)
                    || !call.arguments.is_object()
                    || !declared.contains(call.name.as_str())
                    || !ids.insert(call.id.as_str())
            })
        {
            return Err(InferenceError::Conversation);
        }
        Ok(())
    }

    /// `authorized` comes from the verified machine grant, never the request body.
    pub fn validate_binding(
        &self,
        authorized: &ExecutionScope,
        frozen: &RuntimeProfile,
    ) -> Result<(), InferenceError> {
        authorized.validate()?;
        if self.execution != *authorized
            || frozen.workspace != authorized.workspace
            || self.request_id.is_nil()
        {
            return Err(InferenceError::ExecutionScope);
        }
        if self.schema_version != 1
            || frozen.revision == 0
            || self.profile_revision != frozen.revision
        {
            return Err(InferenceError::Revision);
        }
        Ok(())
    }

    pub fn validate_conversation(&self) -> Result<(), InferenceError> {
        if self.messages.is_empty() || self.messages.len() > 4096 || self.tools.len() > 128 {
            return Err(InferenceError::Conversation);
        }
        let mut definitions = HashSet::new();
        for tool in &self.tools {
            if !valid_name(&tool.name)
                || !definitions.insert(&tool.name)
                || !tool.parameters.is_object()
                || !valid_schema(&tool.parameters, 0)
            {
                return Err(InferenceError::ToolSchema);
            }
        }
        if self
            .output_schema
            .as_ref()
            .is_some_and(|schema| !schema.is_object() || !valid_schema(schema, 0))
        {
            return Err(InferenceError::ToolSchema);
        }
        let mut seen = BTreeSet::new();
        let mut pending = BTreeSet::new();
        for message in &self.messages {
            match message {
                Message::Tool { tool_call_id, .. } => {
                    if !pending.remove(tool_call_id) {
                        return Err(InferenceError::Conversation);
                    }
                }
                Message::Assistant {
                    content,
                    tool_calls,
                } => {
                    if !pending.is_empty()
                        || (content.as_deref().unwrap_or("").is_empty() && tool_calls.is_empty())
                    {
                        return Err(InferenceError::Conversation);
                    }
                    for call in tool_calls {
                        if !valid_name(&call.id)
                            || !valid_name(&call.name)
                            || !call.arguments.is_object()
                            || !seen.insert(call.id.clone())
                        {
                            return Err(InferenceError::Conversation);
                        }
                        pending.insert(call.id.clone());
                    }
                }
                _ if !pending.is_empty() => return Err(InferenceError::Conversation),
                _ => {}
            }
        }
        if !pending.is_empty() {
            return Err(InferenceError::Conversation);
        }
        Ok(())
    }

    /// Adapters supply a proven framing bound. Unknown overhead fails closed.
    pub fn check_byte_upper_bound(
        &self,
        configured: u32,
        model: &ModelCapabilities,
        framing_tokens: Option<u64>,
        output_limit_supported: bool,
    ) -> Result<u64, InferenceError> {
        self.validate_conversation()?;
        if !output_limit_supported && self.output_reserve_tokens != model.max_output_tokens {
            return Err(InferenceError::OutputReserve);
        }
        let framing = framing_tokens.ok_or(InferenceError::Accounting)?;
        let serialized = serde_json::to_vec(self).map_err(|_| InferenceError::Conversation)?;
        let input = (serialized.len() as u64)
            .checked_add(framing)
            .ok_or(InferenceError::Accounting)?;
        ContextBudget::new(configured, model)?.check_request(input, self.output_reserve_tokens)?;
        Ok(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::ProviderId;
    use serde_json::json;

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
            messages: vec![
                Message::System {
                    content: "Immutable requirements and evidence".into(),
                },
                Message::User {
                    content: "Complete the assigned task".into(),
                },
            ],
            tools: vec![],
            output_schema: None,
            output_reserve_tokens: 128000,
        }
    }

    fn profile() -> RuntimeProfile {
        RuntimeProfile {
            schema_version: 1,
            revision: 3,
            workspace: "sdlc2".into(),
            provider: ProviderId::Chatgpt,
            model: "gpt-6-luna".into(),
            context_window_tokens: 256000,
            verification_id: Uuid::new_v4(),
        }
    }

    fn model() -> ModelCapabilities {
        ModelCapabilities {
            model: "gpt-6-luna".into(),
            context_limit_tokens: 1050000,
            max_output_tokens: 128000,
            tools: true,
            structured_output: true,
            streaming: true,
            cancellation: true,
        }
    }

    #[test]
    fn every_execution_boundary_and_the_frozen_revision_are_checked() {
        let request = request();
        let authorized = request.execution.clone();
        let frozen = profile();
        assert!(request.validate_binding(&authorized, &frozen).is_ok());
        for field in 0..7 {
            let mut foreign = authorized.clone();
            match field {
                0 => foreign.workspace = "sdlc1".into(),
                1 => foreign.owner_subject = "another-user".into(),
                2 => foreign.project_id = Uuid::new_v4(),
                3 => foreign.root_task_id = Uuid::new_v4(),
                4 => foreign.task_id = Uuid::new_v4(),
                5 => foreign.agent_id = Uuid::new_v4(),
                _ => foreign.execution_id = Uuid::new_v4(),
            }
            assert_eq!(
                request.validate_binding(&foreign, &frozen),
                Err(InferenceError::ExecutionScope)
            );
        }
        let mut switched = frozen;
        switched.revision = 4;
        assert_eq!(
            request.validate_binding(&authorized, &switched),
            Err(InferenceError::Revision)
        );
    }

    #[test]
    fn request_cannot_override_provider_model_or_supply_credentials() {
        let original = serde_json::to_value(request()).unwrap();
        for field in [
            "provider",
            "model",
            "credential",
            "endpoint",
            "access_token",
            "openrouter_history",
            "reasoning",
        ] {
            let mut incoming = original.clone();
            incoming[field] = json!("forbidden");
            assert!(
                serde_json::from_value::<InferenceRequest>(incoming).is_err(),
                "{field}"
            );
        }
    }

    #[test]
    fn tool_results_cannot_be_orphaned_duplicated_or_dropped_from_history() {
        let mut request = request();
        let assistant = Message::Assistant {
            content: None,
            tool_calls: vec![ToolCall {
                id: "call_1".into(),
                name: "read_evidence".into(),
                arguments: json!({"task":"own"}),
            }],
        };
        let result = Message::Tool {
            tool_call_id: "call_1".into(),
            content: "Required evidence".into(),
        };
        request.messages.extend([assistant.clone(), result.clone()]);
        assert!(request.validate_conversation().is_ok());
        request.messages.push(result.clone());
        assert_eq!(
            request.validate_conversation(),
            Err(InferenceError::Conversation)
        );
        request.messages.pop();
        request.messages.pop();
        assert_eq!(
            request.validate_conversation(),
            Err(InferenceError::Conversation)
        );
        request.messages.push(result);
        request.messages.push(assistant);
        assert_eq!(
            request.validate_conversation(),
            Err(InferenceError::Conversation)
        );
    }

    #[test]
    fn unknown_accounting_and_unenforceable_small_output_reserve_fail_closed() {
        let mut request = request();
        let model = model();
        assert_eq!(
            request.check_byte_upper_bound(256000, &model, None, false),
            Err(InferenceError::Accounting)
        );
        let bytes = request
            .check_byte_upper_bound(256000, &model, Some(1000), false)
            .unwrap();
        assert!(bytes > 1000);
        request.output_reserve_tokens = 4000;
        assert_eq!(
            request.check_byte_upper_bound(256000, &model, Some(1000), false),
            Err(InferenceError::OutputReserve)
        );
        assert!(
            request
                .check_byte_upper_bound(256000, &model, Some(1000), true)
                .is_ok()
        );
        request.messages.push(Message::User {
            content: "x".repeat(256000),
        });
        assert_eq!(
            request.check_byte_upper_bound(256000, &model, Some(1000), true),
            Err(InferenceError::Context(
                AiError::RequiredContextExceedsBudget
            ))
        );
    }

    #[test]
    fn remote_schema_references_and_duplicate_tools_are_rejected() {
        let mut request = request();
        let tool = ToolDefinition {
            name: "read_evidence".into(),
            description: "Read assigned evidence".into(),
            parameters: json!({"type":"object"}),
        };
        request.tools.push(tool.clone());
        assert!(request.validate_conversation().is_ok());
        request.tools.push(tool);
        assert_eq!(
            request.validate_conversation(),
            Err(InferenceError::ToolSchema)
        );
        request.tools.pop();
        request.output_schema =
            Some(json!({"properties":{"secret":{"$ref":"http://localhost/secret"}}}));
        assert_eq!(
            request.validate_conversation(),
            Err(InferenceError::ToolSchema)
        );
        request.output_schema = Some(json!({"$dynamicRef":"https://remote/schema"}));
        assert_eq!(
            request.validate_conversation(),
            Err(InferenceError::ToolSchema)
        );
    }
}
