//! Error codes from the pinned app-server protocol, without upstream messages.
use crate::error::RuntimeError;
use serde_json::Value;

/// Only typed codes are trusted. Provider text can contain credentials or prompts.
pub fn turn_error(error: &Value) -> RuntimeError {
    let info = &error["codexErrorInfo"];
    match info.as_str() {
        Some("contextWindowExceeded") => RuntimeError::ContextExceeded,
        Some("sessionBudgetExceeded" | "usageLimitExceeded" | "rateLimitExceeded") => {
            RuntimeError::Quota
        }
        Some("unauthorized") => RuntimeError::ProviderAuth,
        Some("flexUnavailable" | "serverOverloaded") => RuntimeError::Unavailable,
        _ => {
            let Some(object) = info.as_object().filter(|value| value.len() == 1) else {
                return RuntimeError::Protocol;
            };
            let (kind, details) = object.iter().next().unwrap();
            if !matches!(
                kind.as_str(),
                "httpConnectionFailed"
                    | "responseStreamConnectionFailed"
                    | "responseStreamDisconnected"
                    | "responseTooManyFailedAttempts"
            ) {
                return RuntimeError::Protocol;
            }
            match details["httpStatusCode"].as_u64() {
                Some(401 | 403) => RuntimeError::ProviderAuth,
                Some(402 | 429) => RuntimeError::Quota,
                Some(500..=599) => RuntimeError::Unavailable,
                None if details["httpStatusCode"].is_null() => RuntimeError::Unavailable,
                _ => RuntimeError::Protocol,
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalTurn {
    Completed,
    Cancelled,
    Failed(RuntimeError),
}

/// Called only after thread/turn ownership is matched by the execution adapter.
pub fn completed_turn(turn: &Value) -> Result<TerminalTurn, RuntimeError> {
    match turn["status"].as_str() {
        Some("completed") if turn["error"].is_null() => Ok(TerminalTurn::Completed),
        Some("interrupted") => Ok(TerminalTurn::Cancelled),
        Some("failed") if turn["error"].is_object() => {
            Ok(TerminalTurn::Failed(turn_error(&turn["error"])))
        }
        _ => Err(RuntimeError::Protocol),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_only_pinned_typed_codes_without_interpreting_secret_text() {
        for (code, expected) in [
            ("contextWindowExceeded", RuntimeError::ContextExceeded),
            ("usageLimitExceeded", RuntimeError::Quota),
            ("sessionBudgetExceeded", RuntimeError::Quota),
            ("rateLimitExceeded", RuntimeError::Quota),
            ("unauthorized", RuntimeError::ProviderAuth),
            ("serverOverloaded", RuntimeError::Unavailable),
        ] {
            assert_eq!(
                turn_error(&json!({"codexErrorInfo":code,"message":"sensitive provider text"})),
                expected
            );
        }
        assert_eq!(
            turn_error(&json!({"message":"unauthorized 429 sensitive token"})),
            RuntimeError::Protocol
        );
        assert_eq!(
            turn_error(
                &json!({"codexErrorInfo":{"responseStreamDisconnected":{"httpStatusCode":429}}})
            ),
            RuntimeError::Quota
        );
        assert_eq!(
            turn_error(&json!({"codexErrorInfo":{"httpConnectionFailed":{"httpStatusCode":401}}})),
            RuntimeError::ProviderAuth
        );
        assert_eq!(
            turn_error(
                &json!({"codexErrorInfo":{"httpConnectionFailed":{"httpStatusCode":"401"}}})
            ),
            RuntimeError::Protocol
        );
        assert_eq!(
            turn_error(&json!({"codexErrorInfo":{"unrecognized":{"httpStatusCode":401}}})),
            RuntimeError::Protocol
        );
    }

    #[test]
    fn successful_rpc_does_not_make_a_failed_or_unknown_turn_successful() {
        assert_eq!(
            completed_turn(&json!({"status":"completed","error":null})),
            Ok(TerminalTurn::Completed)
        );
        assert_eq!(
            completed_turn(&json!({"status":"interrupted","error":null})),
            Ok(TerminalTurn::Cancelled)
        );
        assert_eq!(
            completed_turn(
                &json!({"status":"failed","error":{"codexErrorInfo":"usageLimitExceeded"}})
            ),
            Ok(TerminalTurn::Failed(RuntimeError::Quota))
        );
        assert_eq!(
            completed_turn(&json!({"status":"inProgress"})),
            Err(RuntimeError::Protocol)
        );
        assert_eq!(
            completed_turn(&json!({"status":"completed","error":{"message":"contradiction"}})),
            Err(RuntimeError::Protocol)
        );
    }
}
