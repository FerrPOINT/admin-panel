//! HTTP API of the Admin Panel (docs/API.md).

use std::sync::Arc;

pub mod auth;
mod managed_users;
use managed_users::{
    create_managed_user, list_managed_users, resend_managed_user_link, set_managed_user_status,
    update_managed_user,
};

use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub struct AppState {
    pub registry: admin_panel_infra::registry::RegistryStore,
    pub branding: admin_panel_infra::branding::BrandingStore,
    pub access: admin_panel_infra::access::AccessStore,
    pub audit: admin_panel_infra::audit::AuditStore,
    pub config: admin_panel_shared::AppConfig,
}

pub type SharedState = Arc<AppState>;

/// Effective caller role resolved by the auth middleware.
#[derive(Clone)]
pub struct CallerRole(pub admin_panel_domain::PanelRole);

/// Identity + effective role inserted by `bearer_auth`.
#[derive(Clone)]
pub struct Caller {
    pub subject: String,
    pub email: Option<String>,
    pub central_role: Option<String>,
    pub role: admin_panel_domain::PanelRole,
}

async fn require_role(
    required: admin_panel_domain::PanelRole,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let Some(role) = req.extensions().get::<CallerRole>() else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    if role.0.allows(required) {
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

async fn require_operator(req: Request, next: Next) -> Result<Response, StatusCode> {
    require_role(admin_panel_domain::PanelRole::PlatformOperator, req, next).await
}

async fn require_admin(req: Request, next: Next) -> Result<Response, StatusCode> {
    require_role(admin_panel_domain::PanelRole::PlatformAdmin, req, next).await
}

/// Bearer validation via the central auth-server (JWKS). Public runtime
/// routes skip the gate; everything else requires a valid central token
/// whose role maps onto the panel role ladder. When central auth is not
/// configured, mutations stay closed (fail-closed).
async fn bearer_auth(
    State(_state): State<SharedState>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let token = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer ").map(str::to_string))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let caller = match auth::check_token(&token).await {
        auth::CentralCheck::Validated(ctx) => {
            if !ctx.allows_service("admin-panel", req.method().as_str()) {
                return Err(StatusCode::FORBIDDEN);
            }
            Caller {
                subject: ctx.user_id.clone(),
                email: ctx.email.clone(),
                central_role: ctx.role.clone(),
                role: admin_panel_domain::PanelRole::PlatformAdmin,
            }
        }
        auth::CentralCheck::Expired => return Err(StatusCode::UNAUTHORIZED),
        auth::CentralCheck::Unavailable => return Err(StatusCode::SERVICE_UNAVAILABLE),
        auth::CentralCheck::FallThrough => return Err(StatusCode::UNAUTHORIZED),
    };
    req.extensions_mut().insert(CallerRole(caller.role));
    req.extensions_mut().insert(caller);
    Ok(next.run(req).await)
}

async fn resolve_local_role(
    state: &AppState,
    ctx: &sdlc_auth_core::AuthContext,
) -> Result<Option<admin_panel_domain::PanelRole>, StatusCode> {
    let claims = vec![("user_id".to_string(), vec![ctx.user_id.clone()])];
    state
        .access
        .resolve_role(&claims)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

pub fn router(state: SharedState) -> Router {
    let public = Router::new()
        .route("/api/v1/auth/login", post(auth_login))
        .route("/health", get(health_live))
        .route("/health/live", get(health_live))
        .route("/health/ready", get(health_ready))
        .route("/api/v1/runtime/branding", get(runtime_branding))
        .route("/api/v1/runtime/services", get(runtime_services))
        .with_state(state.clone());

    let operator_gated = Router::new()
        .route("/api/v1/services", get(list_services).post(create_service))
        .route(
            "/api/v1/services/{service_key}",
            get(get_service).patch(patch_service),
        )
        .route(
            "/api/v1/services/{service_key}/approve",
            post(approve_service),
        )
        .route(
            "/api/v1/services/{service_key}/disable",
            post(disable_service),
        )
        .route(
            "/api/v1/services/{service_key}/retire",
            post(retire_service),
        )
        .route(
            "/api/v1/services/{service_key}/checks",
            get(list_service_checks).post(run_service_check),
        )
        .route(
            "/api/v1/branding/revisions",
            get(list_revisions).post(create_draft),
        )
        .route(
            "/api/v1/branding/revisions/{revision}/publish",
            post(publish_revision),
        )
        .route(
            "/api/v1/branding/revisions/{revision}/withdraw",
            post(withdraw_revision),
        )
        .route("/api/v1/audit-events", get(list_audit))
        .with_state(state.clone())
        .route_layer(middleware::from_fn(require_operator))
        .route_layer(middleware::from_fn_with_state(state.clone(), bearer_auth));

    let authenticated = Router::new()
        .route("/api/v1/auth/me", get(auth_me))
        .route(
            "/api/v1/tokens",
            get(managed_users::list_personal_tokens).post(managed_users::create_personal_token),
        )
        .route(
            "/api/v1/tokens/{id}",
            axum::routing::delete(managed_users::revoke_personal_token),
        )
        .route(
            "/api/v1/users",
            get(list_managed_users).post(create_managed_user),
        )
        .route(
            "/api/v1/users/{id}",
            axum::routing::patch(update_managed_user),
        )
        .route("/api/v1/users/{id}/status", post(set_managed_user_status))
        .route(
            "/api/v1/users/{id}/password-link",
            post(resend_managed_user_link),
        )
        .with_state(state.clone())
        .route_layer(middleware::from_fn_with_state(state.clone(), bearer_auth));

    let admin_gated = Router::new()
        .route(
            "/api/v1/role-bindings",
            get(list_role_bindings).post(create_role_binding),
        )
        .route(
            "/api/v1/role-bindings/{id}",
            axum::routing::delete(delete_role_binding),
        )
        .with_state(state.clone())
        .route_layer(middleware::from_fn(require_admin))
        .route_layer(middleware::from_fn_with_state(state.clone(), bearer_auth));

    public
        .merge(authenticated)
        .merge(operator_gated)
        .merge(admin_gated)
}

/// OpenAPI contract for the Base Admin Panel API (v1).
#[derive(utoipa::OpenApi)]
#[openapi(
    info(
        title = "Base Admin Panel API",
        version = "1.0.0",
        description = "Platform control plane: branding revisions, service registry, runtime catalog, roles, audit."
    ),
    paths(
        auth_login,
        auth_me,
        health_live,
        health_ready,
        runtime_branding,
        runtime_services,
        list_services,
        get_service,
        create_service,
        patch_service,
        approve_service,
        disable_service,
        retire_service,
        run_service_check,
        list_service_checks,
        list_revisions,
        create_draft,
        withdraw_revision,
        publish_revision,
        list_role_bindings,
        create_role_binding,
        delete_role_binding,
        list_audit,
    ),
    tags(
        (name = "auth", description = "Login proxy and caller identity"),
        (name = "health", description = "Liveness/readiness"),
        (name = "runtime", description = "Public runtime endpoints (no auth)"),
        (name = "services", description = "Service registry management (auth required)"),
        (name = "branding", description = "Branding revisions (auth required)"),
        (name = "access", description = "Role bindings (admin only)"),
        (name = "audit", description = "Audit events (operator+)"),
    )
)]
pub struct ApiDoc;

#[utoipa::path(get, path = "/health/live",
    tag = "health",
    responses((status = 200, description = "alive")))]
async fn health_live() -> StatusCode {
    StatusCode::OK
}

#[utoipa::path(get, path = "/health/ready",
    tag = "health",
    responses((status = 200, description = "ready"), (status = 503, description = "not ready")))]
async fn health_ready(State(state): State<SharedState>) -> Response {
    match sqlx::query("SELECT 1").execute(state.registry.pool()).await {
        Ok(_) => StatusCode::OK.into_response(),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": {"code": "NOT_READY"}})),
        )
            .into_response(),
    }
}

// ─── Runtime branding ────────────────────────────────────────────────────────

#[utoipa::path(get, path = "/api/v1/runtime/branding",
    tag = "runtime",
    params(("If-None-Match" = Option<String>, Header, description = "ETag for conditional GET")),
    responses((status = 200, description = "published branding document"),
              (status = 304, description = "not modified"),
              (status = 404, description = "no published revision")))]
async fn runtime_branding(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    let Ok(Some(published)) = state.branding.find_published().await else {
        return error_response(
            StatusCode::NOT_FOUND,
            "BRANDING_NOT_PUBLISHED",
            "no published branding revision",
        );
    };
    let etag = format!("\"{}\"", published.etag);
    if let Some(if_none_match) = headers.get("if-none-match").and_then(|v| v.to_str().ok())
        && if_none_match == etag
    {
        return Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header("ETag", etag)
            .header("Cache-Control", "public, max-age=60, must-revalidate")
            .body(axum::body::Body::empty())
            .unwrap();
    }
    let body = json!({
        "revision": published.revision,
        "updated_at": published.published_at.map(|t| t.to_rfc3339()).unwrap_or_else(|| published.created_at.to_rfc3339()),
        "branding": published.document,
    });
    Response::builder()
        .status(StatusCode::OK)
        .header("ETag", etag)
        .header("Cache-Control", "public, max-age=60, must-revalidate")
        .header("Content-Type", "application/json")
        .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

// ─── Services ────────────────────────────────────────────────────────────────

// ─── Public runtime catalog ──────────────────────────────────────────────────

/// Public, cacheable service catalog for fleet consumers (switcher UIs).
/// Only active services with an approved declaration are exposed.
#[utoipa::path(get, path = "/api/v1/runtime/services",
    tag = "runtime",
    params(("If-None-Match" = Option<String>, Header, description = "ETag for conditional GET")),
    responses((status = 200, description = "public service catalog")))]
async fn runtime_services(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    let entries = match state.registry.list().await {
        Ok(entries) => entries,
        Err(err) => return internal(err),
    };
    let mut catalog: Vec<serde_json::Value> = Vec::new();
    let mut max_version: i64 = 0;
    for entry in entries {
        if !matches!(entry.status, admin_panel_domain::ServiceStatus::Active) {
            continue;
        }
        max_version = max_version.max(entry.version);
        let Some(decl_id) = entry.active_declaration_id else {
            continue;
        };
        let Ok(Some(decl)) = state.registry.find_declaration(decl_id).await else {
            continue;
        };
        if !matches!(
            decl.approval_status,
            admin_panel_domain::ApprovalStatus::Approved
        ) {
            continue;
        }
        catalog.push(json!({
            "key": entry.service_key,
            "label": entry.display_name,
            "url": decl.integration_base_url,
            "ui_url": if decl.capabilities.iter().any(|c| c == "ui.render") {
                json!(decl
                    .public_ui_url
                    .as_deref()
                    .unwrap_or(&decl.integration_base_url))
            } else {
                json!(null)
            },
            "health": entry
                .health_status
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
            "capabilities": decl.capabilities,
            "contract_version": decl.service_contract_version,
        }));
    }
    catalog.sort_by_key(|service| {
        let key = service["key"].as_str().unwrap_or_default();
        let order = match key {
            "admin-panel" => 0,
            "ci-cd" => 1,
            "task-tracker" => 2,
            "wiki" => 3,
            "fleet-control" => 4,
            "project-workflow" => 5,
            _ => 100,
        };
        (order, key.to_owned())
    });
    let etag = format!("\"services-v{max_version}-{}\"", catalog.len());
    if let Some(if_none_match) = headers.get("if-none-match").and_then(|v| v.to_str().ok())
        && if_none_match == etag
    {
        return Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header("ETag", etag)
            .header("Cache-Control", "public, max-age=60, must-revalidate")
            .body(axum::body::Body::empty())
            .unwrap();
    }
    let headers = [
        ("ETag", etag),
        (
            "Cache-Control",
            "public, max-age=60, must-revalidate".to_string(),
        ),
    ];
    (
        StatusCode::OK,
        headers,
        Json(json!({ "services": catalog })),
    )
        .into_response()
}

#[utoipa::path(get, path = "/api/v1/services",
    tag = "services",
    responses((status = 200, description = "registered services"),
              (status = 401, description = "missing/invalid bearer")))]
async fn list_services(State(state): State<SharedState>) -> Response {
    match state.registry.list().await {
        Ok(entries) => (
            StatusCode::OK,
            Json(json!({ "services": entries, "total": entries.len() })),
        )
            .into_response(),
        Err(err) => internal(err),
    }
}

#[utoipa::path(get, path = "/api/v1/services/{service_key}",
    tag = "services",
    params(("service_key" = String, Path, description = "kebab-case service key")),
    responses((status = 200, description = "service with declarations"),
              (status = 404, description = "unknown service")))]
async fn get_service(
    State(state): State<SharedState>,
    axum::extract::Path(service_key): axum::extract::Path<String>,
) -> Response {
    match state.registry.find_by_key(&service_key).await {
        Ok(Some(entry)) => {
            let declarations = state
                .registry
                .list_declarations(entry.id)
                .await
                .unwrap_or_default();
            let version = entry.version;
            (
                StatusCode::OK,
                [("ETag", format!("\"service-v{version}\""))],
                Json(json!({ "service": entry, "declarations": declarations })),
            )
                .into_response()
        }
        Ok(None) => not_found(&service_key),
        Err(err) => internal(err),
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
struct CreateServiceRequest {
    service_key: String,
    display_name: String,
    owner_team: String,
    declaration: DeclarationInput,
}

#[derive(Deserialize, utoipa::ToSchema)]
struct DeclarationInput {
    declaration_version: i32,
    integration_base_url: String,
    #[serde(default)]
    public_ui_url: Option<String>,
    service_contract_version: String,
    capabilities: Vec<String>,
    requested_by: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/services",
    tag = "services",
    request_body = CreateServiceRequest,
    responses((status = 201, description = "created"),
              (status = 422, description = "validation error"),
              (status = 409, description = "duplicate")))]
async fn create_service(
    State(state): State<SharedState>,
    axum::extract::Extension(caller): axum::extract::Extension<Caller>,
    Json(req): Json<CreateServiceRequest>,
) -> Response {
    if !admin_panel_domain::valid_service_key(&req.service_key) {
        return validation("service_key", "invalid_format");
    }
    if !admin_panel_domain::valid_integration_base_url(&req.declaration.integration_base_url) {
        return validation("integration_base_url", "must_be_https_origin");
    }
    if let Err(msg) =
        admin_panel_domain::validate_public_ui_url(req.declaration.public_ui_url.as_ref())
    {
        return validation("public_ui_url", &msg);
    }
    if let Err(err) = admin_panel_domain::validate_capabilities(&req.declaration.capabilities) {
        return validation("capabilities", &err.to_string());
    }
    if !requested_by_matches_caller(req.declaration.requested_by.as_deref(), &caller.subject) {
        return validation("requested_by", "must_match_authenticated_subject");
    }
    let entry = admin_panel_domain::RegistryEntry {
        id: uuid::Uuid::now_v7(),
        service_key: req.service_key.clone(),
        display_name: req.display_name.clone(),
        owner_team: req.owner_team.clone(),
        status: admin_panel_domain::ServiceStatus::Pending,
        active_declaration_id: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        version: 1,
        health_status: None,
        health_checked_at: None,
        health_detail: None,
    };
    let mut capabilities = req.declaration.capabilities.clone();
    capabilities.sort();
    let content_hash = declaration_content_hash(
        &req.declaration.integration_base_url,
        req.declaration.public_ui_url.as_deref(),
        &capabilities,
        &req.declaration.service_contract_version,
    );
    let declaration = admin_panel_domain::Declaration {
        id: uuid::Uuid::now_v7(),
        registry_entry_id: entry.id,
        declaration_version: req.declaration.declaration_version,
        integration_base_url: req.declaration.integration_base_url.clone(),
        public_ui_url: req
            .declaration
            .public_ui_url
            .clone()
            .filter(|u| !u.is_empty()),
        capabilities,
        service_contract_version: req.declaration.service_contract_version.clone(),
        declared_by_subject: caller.subject.clone(),
        declared_at: chrono::Utc::now(),
        approval_status: admin_panel_domain::ApprovalStatus::Pending,
        approved_by_subject: None,
        approved_at: None,
        content_hash,
    };
    match state.registry.insert_entry(&entry, &declaration).await {
        Ok(()) => (
            StatusCode::CREATED,
            [(
                "Location",
                format!("/api/v1/services/{}", entry.service_key),
            )],
            Json(json!({ "service": entry, "declaration": declaration })),
        )
            .into_response(),
        Err(admin_panel_domain::DomainError::Conflict(msg)) => conflict(&msg),
        Err(err) => internal(err),
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
struct PatchServiceRequest {
    display_name: Option<String>,
    owner_team: Option<String>,
    declaration: Option<DeclarationInput>,
}

#[utoipa::path(patch, path = "/api/v1/services/{service_key}",
    tag = "services",
    request_body = PatchServiceRequest,
    params(("service_key" = String, Path), ("If-Match" = String, Header, description = "expected version ETag")),
    responses((status = 200, description = "updated"), (status = 412, description = "version mismatch")))]
async fn patch_service(
    State(state): State<SharedState>,
    axum::extract::Extension(caller): axum::extract::Extension<Caller>,
    axum::extract::Path(service_key): axum::extract::Path<String>,
    headers: HeaderMap,
    Json(req): Json<PatchServiceRequest>,
) -> Response {
    let Some(current) = state
        .registry
        .find_by_key(&service_key)
        .await
        .ok()
        .flatten()
    else {
        return not_found(&service_key);
    };
    let Some(expected_version) = match_if_match(&headers, current.version) else {
        return precondition_failed();
    };
    let declaration = if let Some(decl) = req.declaration {
        if !admin_panel_domain::valid_integration_base_url(&decl.integration_base_url) {
            return validation("integration_base_url", "must_be_https_origin");
        }
        if let Err(msg) = admin_panel_domain::validate_public_ui_url(decl.public_ui_url.as_ref()) {
            return validation("public_ui_url", &msg);
        }
        if let Err(err) = admin_panel_domain::validate_capabilities(&decl.capabilities) {
            return validation("capabilities", &err.to_string());
        }
        if !requested_by_matches_caller(decl.requested_by.as_deref(), &caller.subject) {
            return validation("requested_by", "must_match_authenticated_subject");
        }
        let mut capabilities = decl.capabilities.clone();
        capabilities.sort();
        let content_hash = declaration_content_hash(
            &decl.integration_base_url,
            decl.public_ui_url.as_deref(),
            &capabilities,
            &decl.service_contract_version,
        );
        Some(admin_panel_domain::Declaration {
            id: uuid::Uuid::now_v7(),
            registry_entry_id: current.id,
            declaration_version: decl.declaration_version,
            integration_base_url: decl.integration_base_url,
            public_ui_url: decl.public_ui_url.clone().filter(|u| !u.is_empty()),
            capabilities,
            service_contract_version: decl.service_contract_version,
            declared_by_subject: caller.subject.clone(),
            declared_at: chrono::Utc::now(),
            approval_status: admin_panel_domain::ApprovalStatus::Pending,
            approved_by_subject: None,
            approved_at: None,
            content_hash,
        })
    } else {
        None
    };
    match state
        .registry
        .patch_entry(
            &service_key,
            req.display_name.as_deref().unwrap_or(&current.display_name),
            req.owner_team.as_deref().unwrap_or(&current.owner_team),
            expected_version,
            declaration.as_ref(),
        )
        .await
    {
        Ok(entry) => (
            StatusCode::OK,
            [("ETag", format!("\"service-v{}\"", entry.version))],
            Json(json!({ "service": entry })),
        )
            .into_response(),
        Err(admin_panel_domain::DomainError::PreconditionFailed(_)) => precondition_failed(),
        Err(admin_panel_domain::DomainError::Conflict(msg)) => conflict(&msg),
        Err(err) => internal(err),
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
struct ApproveRequest {
    declaration_id: uuid::Uuid,
}

#[utoipa::path(post, path = "/api/v1/services/{service_key}/approve",
    tag = "services",
    request_body = ApproveRequest,
    params(("service_key" = String, Path), ("If-Match" = String, Header)),
    responses((status = 200, description = "declaration approved and activated"),
              (status = 409, description = "already approved / conflict")))]
async fn approve_service(
    State(state): State<SharedState>,
    axum::extract::Extension(caller): axum::extract::Extension<Caller>,
    axum::extract::Path(service_key): axum::extract::Path<String>,
    headers: HeaderMap,
    Json(req): Json<ApproveRequest>,
) -> Response {
    let Some(current) = state
        .registry
        .find_by_key(&service_key)
        .await
        .ok()
        .flatten()
    else {
        return not_found(&service_key);
    };
    let Some(expected_version) = match_if_match(&headers, current.version) else {
        return precondition_failed();
    };
    match state
        .registry
        .approve_declaration(
            &service_key,
            req.declaration_id,
            &caller.subject,
            expected_version,
        )
        .await
    {
        Ok((entry, declaration)) => {
            let _ = state
                .audit
                .append(&admin_panel_domain::AuditEvent {
                    id: uuid::Uuid::now_v7(),
                    occurred_at: chrono::Utc::now(),
                    request_id: uuid::Uuid::now_v7(),
                    actor_subject: Some(caller.subject.clone()),
                    actor_role: Some(caller.role),
                    action: "service.approved".into(),
                    entity_type: "service".into(),
                    entity_id: Some(entry.id),
                    metadata: json!({ "declaration_id": declaration.id }),
                })
                .await;
            (
                StatusCode::OK,
                [("ETag", format!("\"service-v{}\"", entry.version))],
                Json(json!({ "service": entry, "declaration": declaration })),
            )
                .into_response()
        }
        Err(admin_panel_domain::DomainError::PreconditionFailed(_)) => precondition_failed(),
        Err(admin_panel_domain::DomainError::Conflict(msg)) => conflict(&msg),
        Err(err) => internal(err),
    }
}

#[utoipa::path(post, path = "/api/v1/services/{service_key}/disable",
    tag = "services",
    params(("service_key" = String, Path), ("If-Match" = String, Header)),
    responses((status = 200, description = "disabled")))]
async fn disable_service(
    State(state): State<SharedState>,
    axum::extract::Path(service_key): axum::extract::Path<String>,
    headers: HeaderMap,
) -> Response {
    change_status(
        state,
        service_key,
        headers,
        admin_panel_domain::ServiceStatus::Disabled,
    )
    .await
}

#[utoipa::path(post, path = "/api/v1/services/{service_key}/retire",
    tag = "services",
    params(("service_key" = String, Path), ("If-Match" = String, Header)),
    responses((status = 200, description = "retired")))]
async fn retire_service(
    State(state): State<SharedState>,
    axum::extract::Path(service_key): axum::extract::Path<String>,
    headers: HeaderMap,
) -> Response {
    change_status(
        state,
        service_key,
        headers,
        admin_panel_domain::ServiceStatus::Retired,
    )
    .await
}

async fn change_status(
    state: SharedState,
    service_key: String,
    headers: HeaderMap,
    target: admin_panel_domain::ServiceStatus,
) -> Response {
    let Some(current) = state
        .registry
        .find_by_key(&service_key)
        .await
        .ok()
        .flatten()
    else {
        return not_found(&service_key);
    };
    let Some(expected_version) = match_if_match(&headers, current.version) else {
        return precondition_failed();
    };
    match state
        .registry
        .set_status(&service_key, target, expected_version)
        .await
    {
        Ok(entry) => (
            StatusCode::OK,
            [("ETag", format!("\"service-v{}\"", entry.version))],
            Json(json!({ "service": entry })),
        )
            .into_response(),
        Err(admin_panel_domain::DomainError::PreconditionFailed(_)) => precondition_failed(),
        Err(err) => internal(err),
    }
}

// ─── Capability checks (docs/API.md §5.5) ────────────────────────────────────

#[derive(Deserialize, utoipa::ToSchema)]
pub struct RunCheckRequest {
    capability: String,
}

#[utoipa::path(post, path = "/api/v1/services/{service_key}/checks",
    tag = "services",
    request_body = RunCheckRequest,
    params(("service_key" = String, Path)),
    responses(
        (status = 202, description = "check run accepted and executed"),
        (status = 404, description = "unknown service"),
        (status = 409, description = "not active or capability not declared"),
        (status = 422, description = "unknown capability"),
    ))]
async fn run_service_check(
    State(state): State<SharedState>,
    axum::extract::Extension(caller): axum::extract::Extension<Caller>,
    axum::extract::Path(service_key): axum::extract::Path<String>,
    Json(req): Json<RunCheckRequest>,
) -> Response {
    let Some(entry) = state
        .registry
        .find_by_key(&service_key)
        .await
        .ok()
        .flatten()
    else {
        return not_found(&service_key);
    };
    if entry.status != admin_panel_domain::ServiceStatus::Active {
        return conflict("service is not active");
    }
    // Capability must be declared by the active approved declaration.
    let Ok(Some(declaration)) = state.registry.active_declaration(entry.id).await else {
        return conflict("no active approved declaration");
    };
    let declared = declaration
        .capabilities
        .iter()
        .any(|c| c == &req.capability);
    if !declared {
        return conflict(&format!("capability {} is not declared", req.capability));
    }
    // The server builds the request from the local capability catalog.
    let Ok(Some((method, fixed_path))) = state.registry.capability(&req.capability).await else {
        return validation("capability", "unknown capability");
    };
    let base = declaration
        .integration_base_url
        .trim_end_matches('/')
        .replace("://localhost:", "://host.docker.internal:")
        .replace("://127.0.0.1:", "://host.docker.internal:");
    let path = if fixed_path.starts_with('/') {
        fixed_path.clone()
    } else {
        format!("/{fixed_path}")
    };
    let url = format!("{base}{path}");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build();
    let run_id = uuid::Uuid::now_v7();
    let (outcome, http_status, summary) = match client {
        Ok(client) if method.eq_ignore_ascii_case("GET") => match client.get(&url).send().await {
            Ok(response) => {
                let code = response.status().as_u16() as i16;
                if response.status().is_success() {
                    ("success", Some(code), format!("HTTP {code}"))
                } else {
                    (
                        "invalid_response",
                        Some(code),
                        format!("HTTP {code} from {path}"),
                    )
                }
            }
            Err(err) => ("unreachable", None, err.to_string()),
        },
        Ok(_) => (
            "invalid_response",
            None,
            format!("catalog method {method} is not supported by the checker"),
        ),
        Err(err) => ("internal_error", None, err.to_string()),
    };
    let summary: String = summary.chars().take(500).collect();
    let _ = state
        .registry
        .insert_check_run(admin_panel_infra::registry::CheckRunParams {
            id: run_id,
            registry_entry_id: entry.id,
            declaration_id: declaration.id,
            capability_key: &req.capability,
            triggered_by_subject: &caller.subject,
            outcome,
            http_status,
            summary: &summary,
        })
        .await;
    let _ = state
        .audit
        .append(&admin_panel_domain::AuditEvent {
            id: uuid::Uuid::now_v7(),
            occurred_at: chrono::Utc::now(),
            request_id: uuid::Uuid::now_v7(),
            actor_subject: Some(caller.subject.clone()),
            actor_role: Some(caller.role),
            action: "service.checked".into(),
            entity_type: "service".into(),
            entity_id: Some(entry.id),
            metadata: json!({
                "capability": req.capability,
                "outcome": outcome,
                "check_run_id": run_id,
            }),
        })
        .await;
    (
        StatusCode::ACCEPTED,
        Json(json!({
            "check_run": {
                "id": run_id,
                "service_key": service_key,
                "capability": req.capability,
                "outcome": outcome,
                "http_status": http_status,
                "summary": summary,
            }
        })),
    )
        .into_response()
}

#[utoipa::path(get, path = "/api/v1/services/{service_key}/checks",
    tag = "services",
    params(("service_key" = String, Path)),
    responses((status = 200, description = "check run history")))]
async fn list_service_checks(
    State(state): State<SharedState>,
    axum::extract::Path(service_key): axum::extract::Path<String>,
) -> Response {
    let Some(entry) = state
        .registry
        .find_by_key(&service_key)
        .await
        .ok()
        .flatten()
    else {
        return not_found(&service_key);
    };
    match state.registry.list_check_runs(entry.id, 50).await {
        Ok(runs) => (
            StatusCode::OK,
            Json(json!({ "checks": runs, "total": runs.len() })),
        )
            .into_response(),
        Err(err) => internal(err),
    }
}

// ─── Branding revisions ──────────────────────────────────────────────────────

#[utoipa::path(get, path = "/api/v1/branding/revisions",
    tag = "branding",
    responses((status = 200, description = "revisions")))]
async fn list_revisions(State(state): State<SharedState>) -> Response {
    match state.branding.list().await {
        Ok(revisions) => (
            StatusCode::OK,
            Json(json!({ "revisions": revisions, "total": revisions.len() })),
        )
            .into_response(),
        Err(err) => internal(err),
    }
}

#[utoipa::path(post, path = "/api/v1/branding/revisions",
    tag = "branding",
    request_body = Object,
    responses((status = 201, description = "draft created"),
              (status = 422, description = "validation error")))]
async fn create_draft(
    State(state): State<SharedState>,
    axum::extract::Extension(caller): axum::extract::Extension<Caller>,
    Json(req): Json<admin_panel_domain::BrandingDocument>,
) -> Response {
    if let Err(err) = req.validate() {
        return validation("document", &err.to_string());
    }
    let Ok(revision_number) = state.branding.next_revision_number().await else {
        return internal("cannot allocate revision");
    };
    let document_hash = content_hash(&[&serde_json::to_string(&req).unwrap()]);
    let based_on = state
        .branding
        .find_published()
        .await
        .ok()
        .flatten()
        .map(|published| published.revision);
    let revision = admin_panel_domain::BrandingRevision {
        id: uuid::Uuid::now_v7(),
        revision: revision_number,
        state: admin_panel_domain::RevisionState::Draft,
        document: req,
        document_hash: document_hash.clone(),
        etag: format!("draft-{document_hash}"),
        created_by_subject: caller.subject.clone(),
        created_at: chrono::Utc::now(),
        published_by_subject: None,
        published_at: None,
        based_on_revision: based_on,
    };
    match state.branding.insert_draft(&revision).await {
        Ok(()) => (StatusCode::CREATED, Json(json!({ "revision": revision }))).into_response(),
        Err(admin_panel_domain::DomainError::Conflict(msg)) => conflict(&msg),
        Err(err) => internal(err),
    }
}

#[utoipa::path(post, path = "/api/v1/branding/revisions/{revision}/publish",
    tag = "branding",
    params(("id" = uuid::Uuid, Path), ("If-Match" = String, Header)),
    responses((status = 200, description = "published"),
              (status = 409, description = "not a draft / already published")))]
async fn publish_revision(
    axum::extract::Extension(caller): axum::extract::Extension<Caller>,
    State(state): State<SharedState>,
    axum::extract::Path(revision): axum::extract::Path<i64>,
) -> Response {
    let Ok(Some(draft)) = state.branding.find_by_revision(revision).await else {
        return not_found(&revision.to_string());
    };
    if !draft.can_publish() {
        return conflict("revision is not a draft");
    }
    let etag = format!(
        "branding-r{}-{}",
        draft.revision,
        &draft.document_hash[..12.min(draft.document_hash.len())]
    );
    match state
        .branding
        .publish(revision, &caller.subject, &etag)
        .await
    {
        Ok(published) => {
            let _ = state
                .audit
                .append(&admin_panel_domain::AuditEvent {
                    id: uuid::Uuid::now_v7(),
                    occurred_at: chrono::Utc::now(),
                    request_id: uuid::Uuid::now_v7(),
                    actor_subject: Some(caller.subject.clone()),
                    actor_role: Some(caller.role),
                    action: "branding.published".into(),
                    entity_type: "branding_revision".into(),
                    entity_id: Some(published.id),
                    metadata: json!({ "revision": published.revision }),
                })
                .await;
            (StatusCode::OK, Json(json!({ "revision": published }))).into_response()
        }
        Err(admin_panel_domain::DomainError::Conflict(msg)) => conflict(&msg),
        Err(err) => internal(err),
    }
}

#[utoipa::path(post, path = "/api/v1/branding/revisions/{revision}/withdraw",
    tag = "branding",
    params(("id" = i64, Path)),
    responses((status = 200, description = "withdrawn"),
              (status = 409, description = "not a draft")))]
async fn withdraw_revision(
    axum::extract::Extension(caller): axum::extract::Extension<Caller>,
    State(state): State<SharedState>,
    axum::extract::Path(revision): axum::extract::Path<i64>,
) -> Response {
    match state.branding.withdraw_revision(revision).await {
        Ok(()) => {
            let _ = state
                .audit
                .append(&admin_panel_domain::AuditEvent {
                    id: uuid::Uuid::now_v7(),
                    occurred_at: chrono::Utc::now(),
                    request_id: uuid::Uuid::now_v7(),
                    actor_subject: Some(caller.subject.clone()),
                    actor_role: Some(caller.role),
                    action: "branding.withdrawn".into(),
                    entity_type: "branding_revision".into(),
                    entity_id: Some(uuid::Uuid::now_v7()),
                    metadata: json!({ "revision": revision }),
                })
                .await;
            (
                StatusCode::OK,
                Json(json!({ "revision": revision, "state": "withdrawn" })),
            )
                .into_response()
        }
        Err(admin_panel_domain::DomainError::Conflict(msg)) => conflict(&msg),
        Err(err) => internal(err),
    }
}

// ─── Audit ───────────────────────────────────────────────────────────────────

#[derive(Deserialize, utoipa::ToSchema)]
struct ListAuditQuery {
    action: Option<String>,
    entity_type: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

fn audit_page_bounds(query: &ListAuditQuery) -> (i64, i64) {
    (
        query.limit.unwrap_or(50).clamp(1, 100),
        query.offset.unwrap_or(0).max(0),
    )
}

// ─── Auth session endpoints ──────────────────────────────────────────────────

#[derive(Deserialize, utoipa::ToSchema)]
struct LoginRequest {
    email: String,
    password: String,
}

#[derive(Serialize)]
struct LoginSession {
    access_token: String,
    token_type: String,
    expires_in: Option<u64>,
    subject: String,
    central_role: Option<String>,
    panel_role: String,
}

/// Proxies credentials to the central auth-server. The panel never stores
/// passwords; a central rejection maps onto a uniform 401 without details.
#[utoipa::path(
    post,
    path = "/api/v1/auth/login",
    tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Central token issued", description = "session issued"),
        (status = 401, description = "Rejected by central auth or not configured"),
    )
)]
async fn auth_login(State(state): State<SharedState>, Json(req): Json<LoginRequest>) -> Response {
    if std::env::var_os("ADMINP_AUTH__CENTRAL_JWKS_URI").is_some() {
        return error_response(
            StatusCode::GONE,
            "SSO_REQUIRED",
            "use Central Auth browser login",
        );
    }
    if req.email.trim().is_empty() || req.password.is_empty() {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "VALIDATION_ERROR",
            "email and password are required",
        );
    }
    match auth::BRIDGE.try_login(&req.email, &req.password).await {
        Ok(Some(pair)) => {
            // Validate the fresh token through the same bridge so the session
            // carries the effective panel role (central + local bindings).
            match auth::check_token(&pair.access_token).await {
                auth::CentralCheck::Validated(ctx) => {
                    let central = auth::panel_role_for(&ctx);
                    let mut role = central;
                    if let Ok(Some(local)) = resolve_local_role(&state, &ctx).await
                        && local > role
                    {
                        role = local;
                    }
                    (
                        StatusCode::OK,
                        Json(LoginSession {
                            access_token: pair.access_token,
                            token_type: pair.token_type.unwrap_or_else(|| "Bearer".into()),
                            expires_in: pair.expires_in,
                            subject: ctx.user_id.clone(),
                            central_role: ctx.role.clone(),
                            panel_role: role.as_str().to_string(),
                        }),
                    )
                        .into_response()
                }
                _ => error_response(
                    StatusCode::UNAUTHORIZED,
                    "INVALID_CREDENTIALS",
                    "central auth rejected the credentials",
                ),
            }
        }
        Ok(None) => error_response(
            StatusCode::UNAUTHORIZED,
            "INVALID_CREDENTIALS",
            "central auth rejected the credentials",
        ),
        Err(error) => {
            tracing::warn!(%error, "central login proxy failed");
            error_response(
                StatusCode::BAD_GATEWAY,
                "CENTRAL_AUTH_UNAVAILABLE",
                "central auth is unreachable",
            )
        }
    }
}

/// Identity snapshot for the SPA: who the caller is and what the panel
/// allows. Local `role_bindings` may elevate the central claim.
#[utoipa::path(
    get,
    path = "/api/v1/auth/me",
    tag = "auth",
    responses(
        (status = 200, description = "Caller identity", description = "caller identity"),
        (status = 401, description = "Missing or invalid bearer"),
    )
)]
async fn auth_me(axum::extract::Extension(caller): axum::extract::Extension<Caller>) -> Response {
    (
        StatusCode::OK,
        Json(json!({
            "subject": caller.subject,
            "email": caller.email,
            "central_role": caller.central_role,
            "panel_role": caller.role.as_str(),
            "capabilities": {
                "mutate": caller.role.allows(admin_panel_domain::PanelRole::PlatformOperator),
                "manage_bindings": caller.role.allows(admin_panel_domain::PanelRole::PlatformAdmin),
            },
        })),
    )
        .into_response()
}

#[utoipa::path(get, path = "/api/v1/role-bindings",
    tag = "access",
    responses((status = 200, description = "role bindings"),
              (status = 403, description = "admin role required")))]
async fn list_role_bindings(State(state): State<SharedState>) -> Result<Response, StatusCode> {
    let bindings = state
        .access
        .list()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({ "bindings": bindings })).into_response())
}

#[derive(Deserialize, utoipa::ToSchema)]
struct CreateRoleBindingRequest {
    claim_name: String,
    claim_value: String,
    panel_role: String,
}

#[utoipa::path(post,
    path = "/api/v1/role-bindings",
    tag = "access",
    request_body = CreateRoleBindingRequest,
    responses(
        (status = 201, description = "binding created"),
        (status = 422, description = "validation error"),
        (status = 409, description = "duplicate binding"),
    )
)]
async fn create_role_binding(
    State(state): State<SharedState>,
    axum::extract::Extension(caller): axum::extract::Extension<Caller>,
    Json(req): Json<CreateRoleBindingRequest>,
) -> Response {
    if std::env::var_os("ADMINP_AUTH__CENTRAL_JWKS_URI").is_some() {
        return error_response(
            StatusCode::GONE,
            "ROLES_DISABLED",
            "user role bindings are disabled",
        );
    }
    let role = match req.panel_role.as_str() {
        "platform_viewer" => admin_panel_domain::PanelRole::PlatformViewer,
        "platform_operator" => admin_panel_domain::PanelRole::PlatformOperator,
        "platform_admin" => admin_panel_domain::PanelRole::PlatformAdmin,
        other => {
            return validation("panel_role", &format!("unknown panel role: {other}"));
        }
    };
    if !matches!(req.claim_name.as_str(), "user_id" | "email" | "role") {
        return validation("claim_name", "must be one of user_id, email, role");
    }
    if req.claim_value.trim().is_empty() {
        return validation("claim_value", "must not be empty");
    }
    let binding = admin_panel_domain::RoleBinding {
        id: uuid::Uuid::now_v7(),
        claim_name: req.claim_name,
        claim_value: req.claim_value.trim().to_string(),
        panel_role: role,
        created_by_subject: caller.subject.clone(),
        created_at: chrono::Utc::now(),
    };
    match state.access.insert(&binding).await {
        Ok(()) => {
            let _ = state.audit.append(&admin_panel_domain::AuditEvent {
                id: uuid::Uuid::now_v7(),
                occurred_at: chrono::Utc::now(),
                request_id: uuid::Uuid::now_v7(),
                actor_subject: Some(caller.subject),
                actor_role: Some(caller.role),
                action: "role_binding.created".into(),
                entity_type: "role_binding".into(),
                entity_id: Some(binding.id),
                metadata: json!({ "claim_name": &binding.claim_name, "panel_role": binding.panel_role.as_str() }),
            }).await;
            (StatusCode::CREATED, Json(json!({ "binding": binding }))).into_response()
        }
        Err(admin_panel_domain::DomainError::Conflict(msg)) => conflict(&msg),
        Err(_) => internal("cannot insert role binding"),
    }
}

#[utoipa::path(delete,
    path = "/api/v1/role-bindings/{id}",
    tag = "access",
    params(("id" = Uuid, Path, description = "binding id")),
    responses((status = 204, description = "binding deleted"), (status = 404, description = "not found"))
)]
async fn delete_role_binding(
    axum::extract::Extension(caller): axum::extract::Extension<Caller>,
    State(state): State<SharedState>,
    axum::extract::Path(id): axum::extract::Path<uuid::Uuid>,
) -> Response {
    if std::env::var_os("ADMINP_AUTH__CENTRAL_JWKS_URI").is_some() {
        return error_response(
            StatusCode::GONE,
            "ROLES_DISABLED",
            "user role bindings are disabled",
        );
    }
    match state.access.delete(id).await {
        Ok(()) => {
            let _ = state
                .audit
                .append(&admin_panel_domain::AuditEvent {
                    id: uuid::Uuid::now_v7(),
                    occurred_at: chrono::Utc::now(),
                    request_id: uuid::Uuid::now_v7(),
                    actor_subject: Some(caller.subject),
                    actor_role: Some(caller.role),
                    action: "role_binding.deleted".into(),
                    entity_type: "role_binding".into(),
                    entity_id: Some(id),
                    metadata: json!({}),
                })
                .await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(admin_panel_domain::DomainError::NotFound(_)) => not_found("role binding"),
        Err(_) => internal("cannot delete role binding"),
    }
}

#[utoipa::path(get, path = "/api/v1/audit-events",
    tag = "audit",
    params(
        ("action" = Option<String>, Query, description = "exact action code"),
        ("entity_type" = Option<String>, Query, description = "exact entity type"),
        ("limit" = Option<i64>, Query, description = "page size (default 50, 1..100)"),
        ("offset" = Option<i64>, Query, description = "zero-based offset (default 0)")
    ),
    responses((status = 200, description = "page of audit events; total is the filtered count before limit and offset")))]
async fn list_audit(
    State(state): State<SharedState>,
    axum::extract::Query(query): axum::extract::Query<ListAuditQuery>,
) -> Response {
    let (limit, offset) = audit_page_bounds(&query);
    match state
        .audit
        .list(
            query.action.as_deref(),
            query.entity_type.as_deref(),
            limit,
            offset,
        )
        .await
    {
        Ok(page) => (
            StatusCode::OK,
            Json(json!({ "events": page.events, "total": page.total })),
        )
            .into_response(),
        Err(err) => internal(err),
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn match_if_match(headers: &HeaderMap, current_version: i64) -> Option<i64> {
    let value = headers
        .get("if-match")
        .and_then(|v| v.to_str().ok())?
        .trim_matches('"');
    value
        .trim_start_matches("service-v")
        .parse::<i64>()
        .ok()
        .filter(|v| *v == current_version)
}

fn content_hash(parts: &[&str]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update(b"\0");
    }
    hex::encode(hasher.finalize())
}

fn declaration_content_hash(
    integration_base_url: &str,
    public_ui_url: Option<&str>,
    capabilities: &[String],
    service_contract_version: &str,
) -> String {
    content_hash(&[
        integration_base_url,
        public_ui_url.unwrap_or_default(),
        &serde_json::to_string(capabilities).unwrap(),
        service_contract_version,
    ])
}

fn requested_by_matches_caller(requested_by: Option<&str>, caller_subject: &str) -> bool {
    requested_by.is_none_or(|requested_by| requested_by == caller_subject)
}

fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message } })),
    )
        .into_response()
}

fn internal(err: impl std::fmt::Display) -> Response {
    tracing::error!(error = %err, "internal error");
    error_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        "INTERNAL_ERROR",
        "internal server error",
    )
}

fn not_found(entity: &str) -> Response {
    error_response(
        StatusCode::NOT_FOUND,
        "NOT_FOUND",
        &format!("{entity} not found"),
    )
}

fn conflict(message: &str) -> Response {
    error_response(StatusCode::CONFLICT, "CONFLICT", message)
}

fn precondition_failed() -> Response {
    error_response(
        StatusCode::PRECONDITION_FAILED,
        "PRECONDITION_FAILED",
        "If-Match does not match current version",
    )
}

fn validation(field: &str, reason: &str) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({
            "error": {
                "code": "VALIDATION_ERROR",
                "message": "Значение не прошло проверку",
                "details": [{ "field": field, "reason": reason }]
            }
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::{
        ListAuditQuery, audit_page_bounds, declaration_content_hash, requested_by_matches_caller,
    };

    #[test]
    fn declaration_author_cannot_be_spoofed() {
        assert!(requested_by_matches_caller(None, "real-user"));
        assert!(requested_by_matches_caller(Some("real-user"), "real-user"));
        assert!(!requested_by_matches_caller(
            Some("local-bootstrap"),
            "real-user"
        ));
    }

    #[test]
    fn declaration_hash_includes_public_ui_url() {
        let capabilities = vec!["health.read".to_owned(), "ui.render".to_owned()];
        let base = declaration_content_hash("http://localhost:8080", None, &capabilities, "v1");
        let first = declaration_content_hash(
            "http://localhost:8080",
            Some("http://localhost:7772"),
            &capabilities,
            "v1",
        );
        let second = declaration_content_hash(
            "http://localhost:8080",
            Some("http://localhost:7722"),
            &capabilities,
            "v1",
        );
        assert_ne!(base, first);
        assert_ne!(first, second);
    }

    #[test]
    fn audit_page_bounds_clamp_invalid_input() {
        let query = ListAuditQuery {
            action: None,
            entity_type: None,
            limit: Some(-4),
            offset: Some(-20),
        };
        assert_eq!(audit_page_bounds(&query), (1, 0));
        let query = ListAuditQuery {
            limit: Some(200),
            offset: Some(40),
            ..query
        };
        assert_eq!(audit_page_bounds(&query), (100, 40));
        let query = ListAuditQuery {
            limit: None,
            offset: None,
            ..query
        };
        assert_eq!(audit_page_bounds(&query), (50, 0));
    }
}
