//! Secret-free AI profile publication and request budgeting.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const DEFAULT_CONTEXT_WINDOW_TOKENS: u32 = 256_000;
pub const MIN_AGENT_CONTEXT_WINDOW_TOKENS: u32 = 64_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderId {
    Chatgpt,
    Openrouter,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderSettings {
    pub provider: ProviderId,
    pub model: String,
    #[serde(default = "default_context")]
    pub context_window_tokens: u32,
}

fn default_context() -> u32 {
    DEFAULT_CONTEXT_WINDOW_TOKENS
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCapabilities {
    pub model: String,
    pub context_limit_tokens: u32,
    pub max_output_tokens: u32,
    pub tools: bool,
    pub structured_output: bool,
    pub streaming: bool,
    pub cancellation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationEvidence {
    pub id: Uuid,
    pub settings: ProviderSettings,
    pub credential_generation: Uuid,
    pub capabilities: ModelCapabilities,
    pub verified_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// Runtime-owned proof; the human API accepts only its ID, never this payload.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedAdapterEvidence {
    pub evidence: VerificationEvidence,
    pub adapter_version: String,
    pub accounting_policy: String,
    /// Missing in historical vaults; such proof cannot enable a new publication.
    #[serde(default)]
    pub draft_revision: Option<i64>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationStatus {
    pub schema_version: u32,
    pub workspace: String,
    pub provider: ProviderId,
    pub verification_id: Uuid,
    pub verified_adapter: Option<VerifiedAdapterEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeProfile {
    pub schema_version: u32,
    pub revision: u64,
    pub workspace: String,
    pub provider: ProviderId,
    pub model: String,
    pub context_window_tokens: u32,
    pub verification_id: Uuid,
}

/// Internal secret-free registration contract; verification stays runtime-owned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionRegistration {
    pub schema_version: u32,
    pub operation_id: Uuid,
    #[serde(default)]
    pub draft_revision: i64,
    pub profile: RuntimeProfile,
    pub credential_generation: Uuid,
    pub adapter_version: String,
    pub accounting_policy: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisteredRevision {
    pub registration: RevisionRegistration,
    pub registered_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistrationStatus {
    pub schema_version: u32,
    pub workspace: String,
    pub operation_id: Uuid,
    pub receipt: Option<RegisteredRevision>,
}

/// Aggregate acceptance ledger only. Decimal microdollars survive JS without rounding.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceBudget {
    pub schema_version: u32,
    pub workspace: String,
    pub currency: String,
    pub limit_microdollars: String,
    pub settled_microdollars: String,
    pub reserved_microdollars: String,
    pub uncertain_microdollars: String,
    pub available_microdollars: String,
    pub unsettled_requests: u64,
    pub uncertain_requests: u64,
    pub blocked_reason: Option<String>,
}

impl AcceptanceBudget {
    pub fn valid_for(&self, workspace: &str) -> bool {
        let amount = |text: &str| {
            text.parse::<u64>()
                .ok()
                .filter(|value| value.to_string() == text)
        };
        let (Some(limit), Some(settled), Some(reserved), Some(uncertain), Some(available)) = (
            amount(&self.limit_microdollars),
            amount(&self.settled_microdollars),
            amount(&self.reserved_microdollars),
            amount(&self.uncertain_microdollars),
            amount(&self.available_microdollars),
        ) else {
            return false;
        };
        let Some(committed) = settled.checked_add(reserved) else {
            return false;
        };
        self.schema_version == 1
            && self.workspace == workspace
            && self.currency == "USD"
            && limit == 30_000_000
            && uncertain <= reserved
            && self.uncertain_requests <= self.unsettled_requests
            && (uncertain == 0) == (self.uncertain_requests == 0)
            && (reserved == 0) == (self.unsettled_requests == 0)
            && available == limit.saturating_sub(committed)
            && match self.blocked_reason.as_deref() {
                None => committed < limit,
                Some("ai_acceptance_budget_exhausted") => committed >= limit,
                Some("provider_cost_exceeds_reservation") => true,
                _ => false,
            }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AiError {
    #[error("invalid_model")]
    InvalidModel,
    #[error("invalid_context_budget")]
    InvalidContextBudget,
    #[error("context_exceeds_model_limit")]
    ContextExceedsModelLimit,
    #[error("output_exceeds_model_limit")]
    OutputExceedsModelLimit,
    #[error("required_context_exceeds_budget")]
    RequiredContextExceedsBudget,
    #[error("required_model_capability_missing")]
    RequiredCapabilityMissing,
    #[error("configuration_not_verified")]
    ConfigurationNotVerified,
    #[error("verification_expired")]
    VerificationExpired,
    #[error("connection_changed")]
    ConnectionChanged,
    #[error("revision_conflict")]
    RevisionConflict,
    #[error("invalid_workspace")]
    InvalidWorkspace,
}

impl ProviderSettings {
    pub fn validate(&self, capabilities: &ModelCapabilities) -> Result<(), AiError> {
        if self.model.is_empty()
            || self.model.len() > 256
            || self.model.trim() != self.model
            || self.model.chars().any(char::is_control)
            || self.model != capabilities.model
        {
            return Err(AiError::InvalidModel);
        }
        if self.context_window_tokens < MIN_AGENT_CONTEXT_WINDOW_TOKENS {
            return Err(AiError::InvalidContextBudget);
        }
        ContextBudget::new(self.context_window_tokens, capabilities)?;
        if !capabilities.tools
            || !capabilities.structured_output
            || !capabilities.streaming
            || !capabilities.cancellation
        {
            return Err(AiError::RequiredCapabilityMissing);
        }
        Ok(())
    }
}

/// The configured budget never changes when native metadata reports a bigger window.
#[derive(Debug, Clone, Copy)]
pub struct ContextBudget {
    configured_tokens: u32,
    model_output_limit_tokens: u32,
}

impl ContextBudget {
    pub fn new(configured_tokens: u32, model: &ModelCapabilities) -> Result<Self, AiError> {
        if configured_tokens == 0 || model.context_limit_tokens == 0 || model.max_output_tokens == 0
        {
            return Err(AiError::InvalidContextBudget);
        }
        if configured_tokens > model.context_limit_tokens {
            return Err(AiError::ContextExceedsModelLimit);
        }
        Ok(Self {
            configured_tokens,
            model_output_limit_tokens: model.max_output_tokens,
        })
    }

    pub fn input_limit(&self, reserved_output_tokens: u32) -> Result<u32, AiError> {
        if reserved_output_tokens == 0 || reserved_output_tokens > self.model_output_limit_tokens {
            return Err(AiError::OutputExceedsModelLimit);
        }
        self.configured_tokens
            .checked_sub(reserved_output_tokens)
            .filter(|limit| *limit > 0)
            .ok_or(AiError::InvalidContextBudget)
    }

    pub fn check_request(
        &self,
        input_tokens: u64,
        reserved_output_tokens: u32,
    ) -> Result<(), AiError> {
        if input_tokens > u64::from(self.input_limit(reserved_output_tokens)?) {
            return Err(AiError::RequiredContextExceedsBudget);
        }
        Ok(())
    }
}

pub fn publish_profile(
    workspace: &str,
    current_revision: u64,
    expected_revision: u64,
    settings: &ProviderSettings,
    connection_generation: Uuid,
    evidence: &VerificationEvidence,
    now: DateTime<Utc>,
) -> Result<RuntimeProfile, AiError> {
    if !matches!(workspace, "sdlc1" | "sdlc2") {
        return Err(AiError::InvalidWorkspace);
    }
    if current_revision != expected_revision {
        return Err(AiError::RevisionConflict);
    }
    if settings != &evidence.settings {
        return Err(AiError::ConfigurationNotVerified);
    }
    if connection_generation != evidence.credential_generation {
        return Err(AiError::ConnectionChanged);
    }
    if evidence.verified_at > now
        || evidence.expires_at <= now
        || evidence.verified_at >= evidence.expires_at
        || evidence
            .expires_at
            .signed_duration_since(evidence.verified_at)
            > chrono::Duration::minutes(15)
    {
        return Err(AiError::VerificationExpired);
    }
    settings.validate(&evidence.capabilities)?;
    Ok(RuntimeProfile {
        schema_version: 1,
        revision: current_revision
            .checked_add(1)
            .ok_or(AiError::RevisionConflict)?,
        workspace: workspace.into(),
        provider: settings.provider,
        model: settings.model.clone(),
        context_window_tokens: settings.context_window_tokens,
        verification_id: evidence.id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn budget_projection_rejects_wrong_scope_rounding_and_inconsistent_totals() {
        let valid = AcceptanceBudget {
            schema_version: 1,
            workspace: "sdlc2".into(),
            currency: "USD".into(),
            limit_microdollars: "30000000".into(),
            settled_microdollars: "123".into(),
            reserved_microdollars: "400".into(),
            uncertain_microdollars: "400".into(),
            available_microdollars: "29999477".into(),
            unsettled_requests: 1,
            uncertain_requests: 1,
            blocked_reason: None,
        };
        assert!(valid.valid_for("sdlc2"));
        for scenario in 0..10 {
            let mut value = valid.clone();
            match scenario {
                0 => value.workspace = "sdlc1".into(),
                1 => value.schema_version = 2,
                2 => value.limit_microdollars = "60000000".into(),
                3 => value.reserved_microdollars = "4e2".into(),
                4 => value.settled_microdollars = "0123".into(),
                5 => value.available_microdollars = "30000000".into(),
                6 => value.uncertain_requests = 2,
                7 => value.uncertain_microdollars = "401".into(),
                8 => value.blocked_reason = Some("retry_another_provider".into()),
                _ => value.settled_microdollars = u64::MAX.to_string(),
            }
            assert!(!value.valid_for("sdlc2"));
        }
    }

    fn evidence() -> VerificationEvidence {
        let now = Utc::now();
        VerificationEvidence {
            id: Uuid::new_v4(),
            settings: ProviderSettings {
                provider: ProviderId::Chatgpt,
                model: "gpt-6-luna".into(),
                context_window_tokens: DEFAULT_CONTEXT_WINDOW_TOKENS,
            },
            credential_generation: Uuid::new_v4(),
            capabilities: ModelCapabilities {
                model: "gpt-6-luna".into(),
                context_limit_tokens: 1_050_000,
                max_output_tokens: 128_000,
                tools: true,
                structured_output: true,
                streaming: true,
                cancellation: true,
            },
            verified_at: now,
            expires_at: now + Duration::minutes(15),
        }
    }

    #[test]
    fn default_is_256000_and_unknown_fields_cannot_smuggle_credentials() {
        let settings: ProviderSettings =
            serde_json::from_str(r#"{"provider":"chatgpt","model":"gpt-6-luna"}"#).unwrap();
        assert_eq!(settings.context_window_tokens, 256_000);
        assert!(
            serde_json::from_str::<ProviderSettings>(
                r#"{"provider":"chatgpt","model":"gpt-6-luna","api_key":"not-allowed"}"#
            )
            .is_err()
        );
    }

    #[test]
    fn reserve_and_native_metadata_cannot_expand_user_budget() {
        let e = evidence();
        let budget = ContextBudget::new(256_000, &e.capabilities).unwrap();
        assert_eq!(budget.input_limit(4_000).unwrap(), 252_000);
        assert!(budget.check_request(252_000, 4_000).is_ok());
        assert_eq!(
            budget.check_request(252_001, 4_000),
            Err(AiError::RequiredContextExceedsBudget)
        );
        assert_eq!(
            budget.check_request(u64::MAX, 4_000),
            Err(AiError::RequiredContextExceedsBudget)
        );
        assert_eq!(
            budget.input_limit(128_001),
            Err(AiError::OutputExceedsModelLimit)
        );
        assert_eq!(
            ContextBudget::new(64_000, &e.capabilities)
                .unwrap()
                .input_limit(64_000),
            Err(AiError::InvalidContextBudget)
        );
    }

    #[test]
    fn model_limits_and_required_capabilities_fail_closed() {
        let mut e = evidence();
        e.capabilities.context_limit_tokens = 128_000;
        assert_eq!(
            e.settings.validate(&e.capabilities),
            Err(AiError::ContextExceedsModelLimit)
        );
        e.capabilities.context_limit_tokens = 1_050_000;
        e.capabilities.tools = false;
        assert_eq!(
            e.settings.validate(&e.capabilities),
            Err(AiError::RequiredCapabilityMissing)
        );
    }

    #[test]
    fn publication_requires_exact_configuration_connection_and_fresh_evidence() {
        let e = evidence();
        let now = Utc::now();
        assert!(
            publish_profile("sdlc2", 0, 0, &e.settings, e.credential_generation, &e, now).is_ok()
        );
        let mut edited = e.settings.clone();
        edited.context_window_tokens = 128_000;
        assert_eq!(
            publish_profile("sdlc2", 0, 0, &edited, e.credential_generation, &e, now),
            Err(AiError::ConfigurationNotVerified)
        );
        assert_eq!(
            publish_profile("sdlc2", 0, 0, &e.settings, Uuid::new_v4(), &e, now),
            Err(AiError::ConnectionChanged)
        );
        assert_eq!(
            publish_profile("sdlc2", 1, 0, &e.settings, e.credential_generation, &e, now),
            Err(AiError::RevisionConflict)
        );
        assert_eq!(
            publish_profile(
                "sdlc2",
                0,
                0,
                &e.settings,
                e.credential_generation,
                &e,
                e.expires_at
            ),
            Err(AiError::VerificationExpired)
        );
    }

    #[test]
    fn publication_refuses_evidence_with_a_lifetime_longer_than_fifteen_minutes() {
        let mut e = evidence();
        e.expires_at = e.verified_at + chrono::Duration::minutes(15) + chrono::Duration::seconds(1);
        assert_eq!(
            publish_profile(
                "sdlc2",
                0,
                0,
                &e.settings,
                e.credential_generation,
                &e,
                e.verified_at
            ),
            Err(AiError::VerificationExpired)
        );
        e.expires_at = e.verified_at + chrono::Duration::minutes(15);
        assert!(
            publish_profile(
                "sdlc2",
                0,
                0,
                &e.settings,
                e.credential_generation,
                &e,
                e.verified_at
            )
            .is_ok()
        );
    }

    #[test]
    fn frozen_run_profile_survives_switch_and_contains_no_secret_fields() {
        let mut e = evidence();
        let original = publish_profile(
            "sdlc2",
            0,
            0,
            &e.settings,
            e.credential_generation,
            &e,
            Utc::now(),
        )
        .unwrap();
        let frozen = original.clone();
        e.settings.provider = ProviderId::Openrouter;
        e.settings.model = "deepseek/deepseek-v4.1-flash".into();
        e.capabilities.model = e.settings.model.clone();
        let next = publish_profile(
            "sdlc2",
            1,
            1,
            &e.settings,
            e.credential_generation,
            &e,
            Utc::now(),
        )
        .unwrap();
        assert_eq!(frozen.provider, ProviderId::Chatgpt);
        assert_eq!(frozen.revision, 1);
        assert_eq!(next.provider, ProviderId::Openrouter);
        assert_eq!(next.revision, 2);
        let payload = serde_json::to_value(frozen).unwrap();
        assert_eq!(payload["context_window_tokens"], 256_000);
        assert_eq!(payload["provider"], "chatgpt");
        for field in [
            "api_key",
            "access_token",
            "refresh_token",
            "credentials",
            "codex_home",
        ] {
            assert!(payload.get(field).is_none(), "secret field: {field}");
        }
    }
}
