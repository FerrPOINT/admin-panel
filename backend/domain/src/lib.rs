//! Domain model of the Admin Panel: registry, branding, roles, audit.
//!
//! Ownership boundaries (docs/ARCHITECTURE.md): this crate owns platform
//! configuration only. auth-server owns identity; integrated services own
//! their domains.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─── Errors ──────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("validation failed: {0}")]
    Validation(String),
    #[error("invalid state transition: {0}")]
    InvalidTransition(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("precondition failed: {0}")]
    PreconditionFailed(String),
}

pub type DomainResult<T> = Result<T, DomainError>;

// ─── Registry ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceStatus {
    Pending,
    Active,
    Disabled,
    Retired,
}

impl ServiceStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ServiceStatus::Pending => "pending",
            ServiceStatus::Active => "active",
            ServiceStatus::Disabled => "disabled",
            ServiceStatus::Retired => "retired",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(ServiceStatus::Pending),
            "active" => Some(ServiceStatus::Active),
            "disabled" => Some(ServiceStatus::Disabled),
            "retired" => Some(ServiceStatus::Retired),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub id: Uuid,
    pub service_key: String,
    pub display_name: String,
    pub owner_team: String,
    pub status: ServiceStatus,
    pub active_declaration_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i64,
}

/// kebab-case key, e.g. `task-tracker`.
pub fn valid_service_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 64
        && key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && key.starts_with(|c: char| c.is_ascii_lowercase())
        && !key.ends_with('-')
        && !key.contains("--")
}

/// HTTPS origin without userinfo, path, query or fragment.
pub fn valid_integration_base_url(url: &str) -> bool {
    let rest = url.strip_prefix("https://").unwrap_or("");
    !rest.is_empty()
        && !rest.contains('/')
        && !rest.contains('?')
        && !rest.contains('#')
        && !rest.contains('@')
        && !rest.contains('\\')
        && rest.split('.').count() >= 1
        && !rest.starts_with('.')
        && !rest.ends_with('.')
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Rejected,
    Superseded,
}

impl ApprovalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ApprovalStatus::Pending => "pending",
            ApprovalStatus::Approved => "approved",
            ApprovalStatus::Rejected => "rejected",
            ApprovalStatus::Superseded => "superseded",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(ApprovalStatus::Pending),
            "approved" => Some(ApprovalStatus::Approved),
            "rejected" => Some(ApprovalStatus::Rejected),
            "superseded" => Some(ApprovalStatus::Superseded),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Declaration {
    pub id: Uuid,
    pub registry_entry_id: Uuid,
    pub declaration_version: i32,
    pub integration_base_url: String,
    pub capabilities: Vec<String>,
    pub service_contract_version: String,
    pub declared_by_subject: String,
    pub declared_at: DateTime<Utc>,
    pub approval_status: ApprovalStatus,
    pub approved_by_subject: Option<String>,
    pub approved_at: Option<DateTime<Utc>>,
    pub content_hash: String,
}

/// v1 capability allowlist (docs/ARCHITECTURE.md 4.2).
pub const CAPABILITY_CATALOG: &[(&str, &str, &str)] = &[
    ("health.read", "GET", "/health"),
    ("integration.status.read", "GET", "/integration/status"),
    ("branding.runtime.read", "GET", "/branding/contract"),
];

pub fn known_capability(key: &str) -> bool {
    CAPABILITY_CATALOG.iter().any(|(k, _, _)| *k == key)
}

pub fn validate_capabilities(caps: &[String]) -> DomainResult<()> {
    if caps.is_empty() {
        return Err(DomainError::Validation(
            "capabilities must not be empty".into(),
        ));
    }
    for cap in caps {
        if !known_capability(cap) {
            return Err(DomainError::Validation(format!(
                "unknown capability: {cap}"
            )));
        }
    }
    let unique: std::collections::HashSet<&String> = caps.iter().collect();
    if unique.len() != caps.len() {
        return Err(DomainError::Validation("duplicate capability".into()));
    }
    Ok(())
}

// ─── Branding ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct BrandingDocument {
    pub product_name: String,
    pub product_short_name: String,
    #[serde(default)]
    pub logo_url: Option<String>,
    #[serde(default)]
    pub favicon_url: Option<String>,
    #[serde(default)]
    pub support_url: Option<String>,
    pub primary_color: String,
    pub accent_color: String,
    #[serde(default)]
    pub surface_color: Option<String>,
}

pub fn valid_hex_color(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(|b| b.is_ascii_hexdigit())
}

/// https public URL without credentials.
pub fn valid_public_url(value: &str) -> bool {
    let rest = value.strip_prefix("https://").unwrap_or("");
    !rest.is_empty() && !rest.contains('@') && !rest.contains('\\')
}

impl BrandingDocument {
    pub fn validate(&self) -> DomainResult<()> {
        if self.product_name.trim().is_empty() || self.product_name.len() > 80 {
            return Err(DomainError::Validation(
                "product_name must be 1..=80 chars".into(),
            ));
        }
        if self.product_short_name.trim().is_empty() || self.product_short_name.len() > 24 {
            return Err(DomainError::Validation(
                "product_short_name must be 1..=24 chars".into(),
            ));
        }
        if !valid_hex_color(&self.primary_color) {
            return Err(DomainError::Validation(
                "primary_color must be #RRGGBB".into(),
            ));
        }
        if !valid_hex_color(&self.accent_color) {
            return Err(DomainError::Validation(
                "accent_color must be #RRGGBB".into(),
            ));
        }
        if let Some(surface) = &self.surface_color {
            if !valid_hex_color(surface) {
                return Err(DomainError::Validation(
                    "surface_color must be #RRGGBB".into(),
                ));
            }
        }
        for (field, url) in [
            ("logo_url", &self.logo_url),
            ("favicon_url", &self.favicon_url),
            ("support_url", &self.support_url),
        ] {
            if let Some(url) = url {
                if !valid_public_url(url) {
                    return Err(DomainError::Validation(format!(
                        "{field} must be a public https URL"
                    )));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevisionState {
    Draft,
    Published,
    Superseded,
    Withdrawn,
}

impl RevisionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            RevisionState::Draft => "draft",
            RevisionState::Published => "published",
            RevisionState::Superseded => "superseded",
            RevisionState::Withdrawn => "withdrawn",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "draft" => Some(RevisionState::Draft),
            "published" => Some(RevisionState::Published),
            "superseded" => Some(RevisionState::Superseded),
            "withdrawn" => Some(RevisionState::Withdrawn),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrandingRevision {
    pub id: Uuid,
    pub revision: i64,
    pub state: RevisionState,
    pub document: BrandingDocument,
    pub document_hash: String,
    pub etag: String,
    pub created_by_subject: String,
    pub created_at: DateTime<Utc>,
    pub published_by_subject: Option<String>,
    pub published_at: Option<DateTime<Utc>>,
    pub based_on_revision: Option<i64>,
}

impl BrandingRevision {
    /// Only a draft may transition towards publication.
    pub fn can_publish(&self) -> bool {
        self.state == RevisionState::Draft
    }
}

// ─── Panel roles ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelRole {
    PlatformViewer,
    PlatformOperator,
    PlatformAdmin,
}

impl PanelRole {
    pub fn allows(&self, required: PanelRole) -> bool {
        self >= &required
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            PanelRole::PlatformViewer => "platform_viewer",
            PanelRole::PlatformOperator => "platform_operator",
            PanelRole::PlatformAdmin => "platform_admin",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "platform_viewer" => Some(PanelRole::PlatformViewer),
            "platform_operator" => Some(PanelRole::PlatformOperator),
            "platform_admin" => Some(PanelRole::PlatformAdmin),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleBinding {
    pub id: Uuid,
    pub claim_name: String,
    pub claim_value: String,
    pub panel_role: PanelRole,
    pub created_by_subject: String,
    pub created_at: DateTime<Utc>,
}

// ─── Audit ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub request_id: Uuid,
    pub actor_subject: Option<String>,
    pub actor_role: Option<PanelRole>,
    pub action: String,
    pub entity_type: String,
    pub entity_id: Option<Uuid>,
    pub metadata: serde_json::Value,
}

// ─── Checks ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckOutcome {
    Success,
    Unreachable,
    Timeout,
    Rejected,
    InvalidResponse,
    InternalError,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckRun {
    pub id: Uuid,
    pub registry_entry_id: Uuid,
    pub declaration_id: Uuid,
    pub capability_key: String,
    pub triggered_by_subject: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub outcome: CheckOutcome,
    pub http_status: Option<i16>,
    pub summary: String,
    pub request_id: Uuid,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_key_rules() {
        assert!(valid_service_key("task-tracker"));
        assert!(!valid_service_key("Task-Tracker"));
        assert!(!valid_service_key("-lead"));
        assert!(!valid_service_key("double--dash"));
        assert!(!valid_service_key(""));
    }

    #[test]
    fn integration_url_requires_https_origin_only() {
        assert!(valid_integration_base_url(
            "https://service.example.internal"
        ));
        assert!(!valid_integration_base_url("http://insecure.example"));
        assert!(!valid_integration_base_url("https://user:pass@example.com"));
        assert!(!valid_integration_base_url("https://example.com/path"));
    }

    #[test]
    fn capabilities_are_allowlisted_and_unique() {
        assert!(validate_capabilities(&["health.read".into()]).is_ok());
        assert!(validate_capabilities(&["execute".into()]).is_err());
        assert!(validate_capabilities(&[]).is_err());
        assert!(validate_capabilities(&["health.read".into(), "health.read".into()]).is_err());
    }

    #[test]
    fn branding_document_validation() {
        let doc = BrandingDocument {
            product_name: "SDLC".into(),
            product_short_name: "SDLC".into(),
            logo_url: Some("https://public.example/logo.svg".into()),
            favicon_url: None,
            support_url: None,
            primary_color: "#123456".into(),
            accent_color: "#234567".into(),
            surface_color: Some("#f5f5f0".into()),
        };
        assert!(doc.validate().is_ok());

        let mut bad = doc.clone();
        bad.primary_color = "123456".into();
        assert!(bad.validate().is_err());

        let mut credentialed = doc.clone();
        credentialed.logo_url = Some("https://token@cdn.example/logo.svg".into());
        assert!(credentialed.validate().is_err());
    }

    #[test]
    fn roles_are_ordered() {
        assert!(PanelRole::PlatformAdmin.allows(PanelRole::PlatformViewer));
        assert!(!PanelRole::PlatformViewer.allows(PanelRole::PlatformOperator));
    }
}
