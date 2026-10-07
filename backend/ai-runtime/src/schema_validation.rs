//! Offline schemas; neither caller nor provider data may fetch files or URLs.
use crate::error::RuntimeError;
use admin_panel_domain::inference::{InferenceRequest, ToolCall};
use jsonschema::Validator;
use serde_json::Value;
use std::collections::BTreeMap;

// No Debug: schemas and model output may contain private requirements/evidence.
pub struct InferenceSchemas {
    tools: BTreeMap<String, Validator>,
    output: Option<Validator>,
}

fn compile(schema: &Value) -> Result<Validator, RuntimeError> {
    jsonschema::options()
        .offline()
        .should_validate_formats(true)
        .should_ignore_unknown_formats(false)
        .build(schema)
        .map_err(|_| RuntimeError::InvalidRequest)
}

impl InferenceSchemas {
    pub fn compile(request: &InferenceRequest) -> Result<Self, RuntimeError> {
        request
            .validate_conversation()
            .map_err(|_| RuntimeError::InvalidRequest)?;
        Ok(Self {
            tools: request
                .tools
                .iter()
                .map(|tool| Ok((tool.name.clone(), compile(&tool.parameters)?)))
                .collect::<Result<_, RuntimeError>>()?,
            output: request.output_schema.as_ref().map(compile).transpose()?,
        })
    }

    pub fn validate_tool_calls(&self, calls: &[ToolCall]) -> Result<(), RuntimeError> {
        if calls.iter().any(|call| {
            !self
                .tools
                .get(&call.name)
                .is_some_and(|schema| schema.is_valid(&call.arguments))
        }) {
            return Err(RuntimeError::Protocol);
        }
        Ok(())
    }

    /// The adapter must call this only after terminal success, never on partial JSON.
    pub fn validate_output(&self, text: &str) -> Result<(), RuntimeError> {
        if let Some(schema) = &self.output {
            if text.len() > 8 * 1024 * 1024 {
                return Err(RuntimeError::Protocol);
            }
            let output: Value = serde_json::from_str(text).map_err(|_| RuntimeError::Protocol)?;
            if !schema.is_valid(&output) {
                return Err(RuntimeError::Protocol);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validates_nested_local_refs_and_rejects_missing_wrong_extra_and_unrepaired_json() {
        let schema = json!({"type":"object","properties":{"result":{"$ref":"#/$defs/result"}},
            "required":["result"],"additionalProperties":false,"$defs":{"result":{"type":"object",
            "properties":{"count":{"type":"integer","minimum":1}},"required":["count"],"additionalProperties":false}}});
        let validator = compile(&schema).unwrap();
        let schemas = InferenceSchemas {
            tools: BTreeMap::from([("own_tool".into(), compile(&schema).unwrap())]),
            output: Some(validator),
        };
        schemas
            .validate_output(r#"{"result":{"count":1}}"#)
            .unwrap();
        for text in [
            r#"{}"#,
            r#"{"result":{"count":"1"}}"#,
            r#"{"result":{"count":0}}"#,
            r#"{"result":{"count":1,"extra":true}}"#,
            "```json\n{\"result\":{\"count\":1}}\n```",
            "{\"result\":",
        ] {
            assert_eq!(schemas.validate_output(text), Err(RuntimeError::Protocol));
        }
        let mut call = ToolCall {
            id: "call_1".into(),
            name: "own_tool".into(),
            arguments: json!({"result":{"count":1}}),
        };
        schemas.validate_tool_calls(&[call.clone()]).unwrap();
        call.arguments = json!({"result":{"count":"1"}});
        assert_eq!(
            schemas.validate_tool_calls(&[call.clone()]),
            Err(RuntimeError::Protocol)
        );
        call.name = "unlisted_tool".into();
        assert_eq!(
            schemas.validate_tool_calls(&[call]),
            Err(RuntimeError::Protocol)
        );
    }

    #[test]
    fn invalid_schemas_unknown_formats_and_external_refs_fail_offline_without_leaking_details() {
        for schema in [
            json!({"type":"invalid_type"}),
            json!({"type":"string","format":"unrecognised-private-format"}),
            json!({"$ref":"file:///private/credentials.json"}),
            json!({"$ref":"https://127.0.0.1/private"}),
        ] {
            assert!(matches!(
                compile(&schema),
                Err(RuntimeError::InvalidRequest)
            ));
        }
        let uuid = compile(&json!({"type":"string","format":"uuid"})).unwrap();
        assert!(!uuid.is_valid(&json!("not-a-uuid")));
        assert!(uuid.is_valid(&json!("64937936-6c96-4c0f-8903-8345d1057b7f")));
    }

    #[test]
    fn malformed_tool_schema_cannot_create_an_execution_or_reservation() {
        use crate::{execution_grant::fixtures, vault::Vault};
        use admin_panel_domain::inference::ToolDefinition;
        let (seed, claims, id) = crate::inference_journal::tests::prepared_fixture_state();
        let mut request = seed.inference_runs[&id].request.clone();
        request.tools.push(ToolDefinition {
            name: "own_tool".into(),
            description: "Own tool".into(),
            parameters: json!({"type":"unknown-type"}),
        });
        let directory = tempfile::tempdir().unwrap();
        let state = directory.path().join("state");
        let key = directory.path().join("key");
        Vault::initialize(&state, &key, "sdlc2").unwrap();
        let mut vault = Vault::open(&state, &key, "sdlc2").unwrap();
        let now = chrono::Utc::now();
        let grant = fixtures::authorize(&claims, now);
        assert_eq!(
            vault.prepare_inference(request, &grant, now),
            Err(RuntimeError::InvalidRequest)
        );
        assert!(
            vault.state().inference_runs.is_empty()
                && vault.state().root_profile_bindings.is_empty()
                && vault.state().budget.reservations.is_empty()
        );
    }
}
