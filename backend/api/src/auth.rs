//! Admin Panel wiring of the shared central-auth bridge.
//!
//! All JWKS mechanics live in `sdlc_auth_core::service_bridge`; this module
//! only maps bridge outcomes to admin-panel middleware decisions and binds
//! the central role to the local `PanelRole`.

use sdlc_auth_core::AuthContext;
use sdlc_auth_core::service_bridge::{BridgeOutcome, ServiceBridge};

use admin_panel_domain::PanelRole;

/// Env prefix: ADMINP_AUTH__CENTRAL_{JWKS_URI,ISSUER,LOGIN_URL,TIMEOUT_SECS}.
pub static BRIDGE: ServiceBridge = ServiceBridge::new("ADMINP_AUTH__CENTRAL");

/// Outcome of central bearer validation for the admin API.
pub enum CentralCheck {
    /// Validated centrally; carries the verified context.
    Validated(AuthContext),
    /// Not a central token or central auth not configured.
    FallThrough,
    /// Central token, expired.
    Expired,
}

/// Map the central role claim onto the local panel role ladder.
/// Unknown/absent role → PlatformViewer (least privilege).
pub fn panel_role_for(ctx: &AuthContext) -> PanelRole {
    match ctx.role.as_deref() {
        Some("platform_admin") | Some("admin") => PanelRole::PlatformAdmin,
        Some("platform_operator") | Some("operator") | Some("platform_editor") | Some("editor") => {
            PanelRole::PlatformOperator
        }
        _ => PanelRole::PlatformViewer,
    }
}

pub async fn check_token(token: &str) -> CentralCheck {
    match BRIDGE.try_token(token).await {
        BridgeOutcome::Validated(ctx) => CentralCheck::Validated(ctx),
        BridgeOutcome::NotOurs | BridgeOutcome::NotConfigured => CentralCheck::FallThrough,
        BridgeOutcome::Expired => CentralCheck::Expired,
        BridgeOutcome::Invalid(reason) => {
            tracing::debug!(reason, "bearer is not a valid central token");
            CentralCheck::FallThrough
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn ctx_with_role(role: Option<&str>) -> AuthContext {
        AuthContext {
            user_id: "u-1".into(),
            role: role.map(str::to_string),
            scopes: HashSet::new(),
            session_id: None,
            email: None,
            token: "raw".into(),
        }
    }

    #[test]
    fn platform_admin_maps_to_admin() {
        assert!(matches!(
            panel_role_for(&ctx_with_role(Some("platform_admin"))),
            PanelRole::PlatformAdmin
        ));
    }

    #[test]
    fn operator_and_editor_map_to_operator() {
        assert!(matches!(
            panel_role_for(&ctx_with_role(Some("operator"))),
            PanelRole::PlatformOperator
        ));
        assert!(matches!(
            panel_role_for(&ctx_with_role(Some("editor"))),
            PanelRole::PlatformOperator
        ));
    }

    #[test]
    fn unknown_or_missing_role_is_least_privilege() {
        assert!(matches!(
            panel_role_for(&ctx_with_role(Some("whatever"))),
            PanelRole::PlatformViewer
        ));
        assert!(matches!(
            panel_role_for(&ctx_with_role(None)),
            PanelRole::PlatformViewer
        ));
    }
}
