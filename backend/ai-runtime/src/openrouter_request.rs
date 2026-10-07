//! Exact-profile wire encoding. The caller must first authenticate the scope.
use admin_panel_domain::{
    ai::{ContextBudget, ModelCapabilities, ProviderId, ProviderSettings, RuntimeProfile},
    inference::{ExecutionScope, InferenceError, InferenceRequest, Message},
};
use serde_json::{Value, json};

// No Debug: the body can contain private conversation and task evidence.
pub struct ChatRequest {
    body: Value,
    model: String,
    input_upper_bound_tokens: u64,
}

impl ChatRequest {
    /// `authorized` and `profile` are trusted execution records, not browser input.
    pub fn from_frozen(
        request: &InferenceRequest,
        authorized: &ExecutionScope,
        profile: &RuntimeProfile,
        capabilities: &ModelCapabilities,
        proven_framing_tokens: Option<u64>,
    ) -> Result<Self, InferenceError> {
        Self::encode(
            request,
            authorized,
            profile,
            capabilities,
            proven_framing_tokens,
            &Default::default(),
        )
    }

    pub(crate) fn from_stored(
        run: &crate::inference_journal::StoredInference,
        authorized: &ExecutionScope,
        capabilities: &ModelCapabilities,
        proven_framing_tokens: Option<u64>,
    ) -> Result<Self, InferenceError> {
        Self::encode(
            &run.request,
            authorized,
            &run.registration.registration.profile,
            capabilities,
            proven_framing_tokens,
            &run.openrouter_history,
        )
    }

    /// The same final wire is checked by the journal and the consuming transport.
    /// Prices originate in server evidence; inference callers cannot supply them.
    pub(crate) fn from_paid_stored(
        run: &crate::inference_journal::StoredInference,
        authorized: &ExecutionScope,
        capabilities: &ModelCapabilities,
        proven_framing_tokens: Option<u64>,
        cost: Option<&crate::budget::CostEstimate>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Self, crate::error::RuntimeError> {
        use crate::error::RuntimeError;
        let cost = cost.ok_or(RuntimeError::CapabilityNotVerified)?;
        let profile = &run.registration.registration.profile;
        if cost.model != profile.model
            || cost.credential_generation != run.registration.registration.credential_generation
        {
            return Err(RuntimeError::CapabilityNotVerified);
        }
        cost.ceiling_microdollars(now)?;
        let mut wire = Self::from_stored(run, authorized, capabilities, proven_framing_tokens)
            .map_err(wire_error)?;
        wire.body["provider"]["max_price"] = json!({
            "prompt": million_token_price(cost.input_nanodollars_per_token),
            "completion": million_token_price(cost.output_nanodollars_per_token),
            "request": "0"
        });
        let framing = proven_framing_tokens.ok_or(RuntimeError::CapabilityNotVerified)?;
        wire.input_upper_bound_tokens = (serde_json::to_vec(&wire.body)
            .map_err(|_| RuntimeError::CapabilityNotVerified)?
            .len() as u64)
            .checked_add(framing)
            .ok_or(RuntimeError::CapabilityNotVerified)?;
        ContextBudget::new(profile.context_window_tokens, capabilities)
            .and_then(|budget| {
                budget.check_request(
                    wire.input_upper_bound_tokens,
                    run.request.output_reserve_tokens,
                )
            })
            .map_err(|_| RuntimeError::ContextExceeded)?;
        if u64::from(cost.input_tokens) < wire.input_upper_bound_tokens
            || cost.output_tokens < run.request.output_reserve_tokens
        {
            return Err(RuntimeError::CapabilityNotVerified);
        }
        Ok(wire)
    }

    fn encode(
        request: &InferenceRequest,
        authorized: &ExecutionScope,
        profile: &RuntimeProfile,
        capabilities: &ModelCapabilities,
        proven_framing_tokens: Option<u64>,
        reasoning: &std::collections::BTreeMap<
            usize,
            crate::openrouter_reasoning::OpenRouterReasoning,
        >,
    ) -> Result<Self, InferenceError> {
        request.validate_binding(authorized, profile)?;
        request.validate_conversation()?;
        if profile.provider != ProviderId::Openrouter {
            return Err(InferenceError::Provider);
        }
        ProviderSettings {
            provider: profile.provider,
            model: profile.model.clone(),
            context_window_tokens: profile.context_window_tokens,
        }
        .validate(capabilities)?;
        let framing = proven_framing_tokens.ok_or(InferenceError::Accounting)?;
        let mut messages: Vec<Value> = request
            .messages
            .iter()
            .map(|message| match message {
                Message::System { content } => json!({"role":"system","content":content}),
                Message::Developer { content } => json!({"role":"developer","content":content}),
                Message::User { content } => json!({"role":"user","content":content}),
                Message::Tool {
                    tool_call_id,
                    content,
                } => json!({"role":"tool","tool_call_id":tool_call_id,"content":content}),
                Message::Assistant {
                    content,
                    tool_calls,
                } => {
                    let mut message = json!({"role":"assistant","content":content});
                    if !tool_calls.is_empty() {
                        message["tool_calls"] = json!(
                            tool_calls
                                .iter()
                                .map(|call| json!({"id":call.id,"type":"function",
                        "function":{"name":call.name,"arguments":call.arguments.to_string()}}))
                                .collect::<Vec<_>>()
                        );
                    }
                    message
                }
            })
            .collect();
        for (index, private) in reasoning {
            let message = messages
                .get_mut(*index)
                .ok_or(InferenceError::Conversation)?;
            private
                .apply_to_message(message)
                .map_err(|_| InferenceError::Conversation)?;
        }
        let mut body = json!({"model":profile.model,"messages":messages,"max_tokens":request.output_reserve_tokens,
            "stream":true,"stream_options":{"include_usage":true},
            "provider":{"allow_fallbacks":false,"require_parameters":true}});
        if !request.tools.is_empty() {
            body["tools"] = json!(
                request
                    .tools
                    .iter()
                    .map(|tool| json!({"type":"function","function":{
                "name":tool.name,"description":tool.description,"parameters":tool.parameters}}))
                    .collect::<Vec<_>>()
            );
        }
        if let Some(schema) = &request.output_schema {
            body["response_format"] = json!({"type":"json_schema","json_schema":{"name":"sdlc_result","strict":true,"schema":schema}});
        }
        // Count the actual encoding, including escaped tool argument JSON strings.
        // The unified DTO size is not an upper bound on this provider payload.
        let wire = serde_json::to_vec(&body).map_err(|_| InferenceError::Conversation)?;
        let input_upper_bound_tokens = (wire.len() as u64)
            .checked_add(framing)
            .ok_or(InferenceError::Accounting)?;
        ContextBudget::new(profile.context_window_tokens, capabilities)?
            .check_request(input_upper_bound_tokens, request.output_reserve_tokens)?;
        Ok(Self {
            body,
            model: profile.model.clone(),
            input_upper_bound_tokens,
        })
    }

    pub fn body(&self) -> &Value {
        &self.body
    }
    pub fn model(&self) -> &str {
        &self.model
    }
    pub fn input_upper_bound_tokens(&self) -> u64 {
        self.input_upper_bound_tokens
    }
}

fn wire_error(error: InferenceError) -> crate::error::RuntimeError {
    match error {
        InferenceError::Context(_) => crate::error::RuntimeError::ContextExceeded,
        _ => crate::error::RuntimeError::CapabilityNotVerified,
    }
}

/// USD/million tokens equals nanodollars/token divided by 1000. No float rounding.
fn million_token_price(nanodollars: u64) -> String {
    let whole = nanodollars / 1000;
    let fraction = nanodollars % 1000;
    if fraction == 0 {
        whole.to_string()
    } else {
        format!("{whole}.{fraction:03}")
            .trim_end_matches('0')
            .to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use admin_panel_domain::inference::{ToolCall, ToolDefinition};
    use uuid::Uuid;

    #[test]
    fn price_ceiling_conversion_is_exact_even_at_integer_boundaries() {
        for (nanos, decimal) in [
            (1, "0.001"),
            (9, "0.009"),
            (10, "0.01"),
            (100, "0.1"),
            (300, "0.3"),
            (1000, "1"),
            (1200, "1.2"),
            (1001, "1.001"),
            (u64::MAX, "18446744073709551.615"),
        ] {
            assert_eq!(million_token_price(nanos), decimal);
        }
    }

    fn input() -> (InferenceRequest, RuntimeProfile, ModelCapabilities) {
        let profile = RuntimeProfile {
            schema_version: 1,
            revision: 7,
            workspace: "sdlc2".into(),
            provider: ProviderId::Openrouter,
            model: "deepseek/deepseek-v4.1-flash".into(),
            context_window_tokens: 256000,
            verification_id: Uuid::new_v4(),
        };
        let capabilities = ModelCapabilities {
            model: profile.model.clone(),
            context_limit_tokens: 1048576,
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
                    content: "Immutable requirements".into(),
                },
                Message::Developer {
                    content: "Agent rules".into(),
                },
                Message::User {
                    content: "Assigned task".into(),
                },
                Message::Assistant {
                    content: None,
                    tool_calls: vec![ToolCall {
                        id: "call_1".into(),
                        name: "read_evidence".into(),
                        arguments: json!({"value":"quotes \" and newline\n"}),
                    }],
                },
                Message::Tool {
                    tool_call_id: "call_1".into(),
                    content: "Evidence".into(),
                },
            ],
            tools: vec![ToolDefinition {
                name: "read_evidence".into(),
                description: "Read assigned evidence".into(),
                parameters: json!({"type":"object"}),
            }],
            output_schema: Some(
                json!({"type":"object","properties":{"ok":{"type":"boolean"}},"required":["ok"],"additionalProperties":false}),
            ),
            output_reserve_tokens: 4000,
        };
        (request, profile, capabilities)
    }

    #[test]
    fn retains_roles_tools_schema_and_exact_model_without_fallback_overrides() {
        let (request, profile, capabilities) = input();
        let wire = ChatRequest::from_frozen(
            &request,
            &request.execution,
            &profile,
            &capabilities,
            Some(1000),
        )
        .unwrap();
        assert_eq!(wire.model(), profile.model);
        assert_eq!(wire.body()["max_tokens"], 4000);
        assert_eq!(wire.body()["provider"]["allow_fallbacks"], false);
        assert_eq!(wire.body()["provider"]["require_parameters"], true);
        assert_eq!(wire.body()["messages"][0]["role"], "system");
        assert_eq!(wire.body()["messages"][1]["role"], "developer");
        assert_eq!(wire.body()["messages"][4]["tool_call_id"], "call_1");
        let arguments = wire.body()["messages"][3]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(arguments).unwrap(),
            json!({"value":"quotes \" and newline\n"})
        );
        assert_eq!(wire.body()["response_format"]["type"], "json_schema");
        assert!(wire.body().get("models").is_none() && wire.body().get("api_key").is_none());
        assert_eq!(
            wire.input_upper_bound_tokens(),
            serde_json::to_vec(wire.body()).unwrap().len() as u64 + 1000
        );
    }

    #[test]
    fn foreign_scope_revision_unknown_framing_and_mandatory_overflow_block_encoding() {
        let (mut request, profile, capabilities) = input();
        let mut foreign = request.execution.clone();
        foreign.owner_subject = "other".into();
        assert!(matches!(
            ChatRequest::from_frozen(&request, &foreign, &profile, &capabilities, Some(1000)),
            Err(InferenceError::ExecutionScope)
        ));
        assert!(matches!(
            ChatRequest::from_frozen(&request, &request.execution, &profile, &capabilities, None),
            Err(InferenceError::Accounting)
        ));
        request.profile_revision = 8;
        assert!(matches!(
            ChatRequest::from_frozen(
                &request,
                &request.execution,
                &profile,
                &capabilities,
                Some(1000)
            ),
            Err(InferenceError::Revision)
        ));
        request.profile_revision = 7;
        request.messages.push(Message::System {
            content: "x".repeat(256000),
        });
        assert!(matches!(
            ChatRequest::from_frozen(
                &request,
                &request.execution,
                &profile,
                &capabilities,
                Some(1000)
            ),
            Err(InferenceError::Context(_))
        ));
    }
}
