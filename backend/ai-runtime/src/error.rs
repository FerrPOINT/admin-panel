use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

/// Provider responses and filesystem errors never become client-visible messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeError {
    #[error(transparent)]
    Budget(#[from] crate::budget::BudgetError),
    #[error("invalid_configuration")]
    Configuration,
    #[error("state_not_initialized")]
    NotInitialized,
    #[error("state_integrity_failure")]
    StateIntegrity,
    #[error("state_write_failed")]
    StateWrite,
    #[error("invalid_request")]
    InvalidRequest,
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("connection_not_configured")]
    Disconnected,
    #[error("operation_conflict")]
    Conflict,
    #[error("provider_authentication_failed")]
    ProviderAuth,
    #[error("provider_quota_exceeded")]
    Quota,
    #[error("provider_unavailable")]
    Unavailable,
    #[error("provider_protocol_error")]
    Protocol,
    #[error("required_capability_not_verified")]
    CapabilityNotVerified,
    #[error("required_context_exceeds_budget")]
    ContextExceeded,
    #[error("provider_output_truncated")]
    OutputTruncated,
    #[error("external_calls_disabled")]
    ExternalCallsDisabled,
    #[error("codex_version_mismatch")]
    CodexVersion,
    #[error("native_filesystem_isolation_unavailable")]
    NativeFilesystemIsolation,
    #[error("native_tool_policy_mismatch")]
    NativeToolPolicy,
}

impl IntoResponse for RuntimeError {
    fn into_response(self) -> Response {
        let status = match self {
            Self::Budget(crate::budget::BudgetError::Exhausted) => StatusCode::TOO_MANY_REQUESTS,
            Self::Budget(crate::budget::BudgetError::InvalidEstimate) => {
                StatusCode::FAILED_DEPENDENCY
            }
            Self::Budget(crate::budget::BudgetError::UpperBoundExceeded) => StatusCode::BAD_GATEWAY,
            Self::Budget(_) => StatusCode::CONFLICT,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::InvalidRequest | Self::ContextExceeded | Self::OutputTruncated => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            Self::Conflict => StatusCode::CONFLICT,
            Self::ProviderAuth
            | Self::Disconnected
            | Self::CapabilityNotVerified
            | Self::CodexVersion
            | Self::NativeFilesystemIsolation => StatusCode::FAILED_DEPENDENCY,
            Self::NativeToolPolicy => StatusCode::FAILED_DEPENDENCY,
            Self::Quota => StatusCode::TOO_MANY_REQUESTS,
            Self::Protocol => StatusCode::BAD_GATEWAY,
            Self::Unavailable | Self::ExternalCallsDisabled => StatusCode::SERVICE_UNAVAILABLE,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(json!({"error": {"code": self.to_string()}}))).into_response()
    }
}
