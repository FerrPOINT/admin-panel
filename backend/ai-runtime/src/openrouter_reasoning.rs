//! Private provider history. Never accept this from an agent or emit it as events.
use crate::error::RuntimeError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_DETAILS: usize = 4096;

// Deliberately no Debug: these fields can contain private reasoning/signatures.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenRouterReasoning {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    details: Vec<Value>,
}

impl OpenRouterReasoning {
    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.details.is_empty()
    }

    /// Each provider delta is retained in order, including opaque block metadata.
    pub(crate) fn append_delta(&mut self, delta: &Value) -> Result<(), RuntimeError> {
        for key in ["reasoning", "reasoning_content"] {
            if let Some(value) = delta.get(key).filter(|v| !v.is_null()) {
                let text = value.as_str().ok_or(RuntimeError::Protocol)?;
                if delta.get("reasoning").is_some_and(|v| !v.is_null())
                    && delta.get("reasoning_content").is_some_and(|v| !v.is_null())
                {
                    return Err(RuntimeError::Protocol);
                }
                if self.text.len().saturating_add(text.len()) > MAX_BYTES {
                    return Err(RuntimeError::Protocol);
                }
                self.text.push_str(text);
            }
        }
        if let Some(value) = delta.get("reasoning_details").filter(|v| !v.is_null()) {
            let values = value.as_array().ok_or(RuntimeError::Protocol)?;
            if self.details.len().saturating_add(values.len()) > MAX_DETAILS {
                return Err(RuntimeError::Protocol);
            }
            for value in values {
                if !value.is_object()
                    || !matches!(
                        value["type"].as_str(),
                        Some("reasoning.text" | "reasoning.summary" | "reasoning.encrypted")
                    )
                {
                    return Err(RuntimeError::Protocol);
                }
            }
            self.details.extend(values.iter().cloned());
        }
        self.validate()
    }

    pub(crate) fn validate(&self) -> Result<(), RuntimeError> {
        if self.details.len() > MAX_DETAILS
            || serde_json::to_vec(self)
                .map_err(|_| RuntimeError::Protocol)?
                .len()
                > MAX_BYTES
            || self.details.iter().any(|value| {
                !value.is_object()
                    || !matches!(
                        value["type"].as_str(),
                        Some("reasoning.text" | "reasoning.summary" | "reasoning.encrypted")
                    )
            })
        {
            return Err(RuntimeError::Protocol);
        }
        Ok(())
    }

    pub(crate) fn apply_to_message(&self, message: &mut Value) -> Result<(), RuntimeError> {
        self.validate()?;
        if message["role"] != "assistant" {
            return Err(RuntimeError::Protocol);
        }
        if !self.text.is_empty() {
            message["reasoning"] = Value::String(self.text.clone());
        }
        if !self.details.is_empty() {
            message["reasoning_details"] = Value::Array(self.details.clone());
        }
        Ok(())
    }
}
