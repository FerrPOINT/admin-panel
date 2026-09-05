//! HTTP API of the Admin Panel (docs/API.md).

use std::sync::Arc;

pub mod auth;

use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
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
    State(state): State<SharedState>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    use admin_panel_domain::PanelRole;

    let token = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer ").map(str::to_string))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let resolved: PanelRole = match auth::check_token(&token).await {
        auth::CentralCheck::Validated(ctx) => {
            let central = auth::panel_role_for(&ctx);
            // Local role_bindings may elevate the central claim (claim_name=user_id).
            let mut best = central;
            if let Ok(Some(local)) = resolve_local_role(&state, &ctx).await {
                if local > best {
                    best = local;
                }
            }
            best
        }
        auth::CentralCheck::Expired => return Err(StatusCode::UNAUTHORIZED),
        auth::CentralCheck::FallThrough => return Err(StatusCode::UNAUTHORIZED),
    };
    req.extensions_mut().insert(CallerRole(resolved));
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
            "/api/v1/branding/revisions",
            get(list_revisions).post(create_draft),
        )
        .route(
            "/api/v1/branding/revisions/{revision}/publish",
            post(publish_revision),
        )
        .route("/api/v1/audit-events", get(list_audit))
        .with_state(state.clone())
        .route_layer(middleware::from_fn(require_operator))
        .route_layer(middleware::from_fn_with_state(state.clone(), bearer_auth));

    let admin_gated = Router::new()
        .route("/api/v1/role-bindings", get(list_role_bindings))
        .with_state(state.clone())
        .route_layer(middleware::from_fn(require_admin))
        .route_layer(middleware::from_fn_with_state(state.clone(), bearer_auth));

    public.merge(operator_gated).merge(admin_gated)
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
        list_revisions,
        create_draft,
        publish_revision,
        list_role_bindings,
        list_audit,
    ),
    tags(
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
            "capabilities": decl.capabilities,
            "contract_version": decl.service_contract_version,
        }));
    }
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
    Json(req): Json<CreateServiceRequest>,
) -> Response {
    if !admin_panel_domain::valid_service_key(&req.service_key) {
        return validation("service_key", "invalid_format");
    }
    if !admin_panel_domain::valid_integration_base_url(&req.declaration.integration_base_url) {
        return validation("integration_base_url", "must_be_https_origin");
    }
    if let Err(err) = admin_panel_domain::validate_capabilities(&req.declaration.capabilities) {
        return validation("capabilities", &err.to_string());
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
    };
    let mut capabilities = req.declaration.capabilities.clone();
    capabilities.sort();
    let content_hash = content_hash(&[
        &req.declaration.integration_base_url,
        &serde_json::to_string(&capabilities).unwrap(),
        &req.declaration.service_contract_version,
    ]);
    let declaration = admin_panel_domain::Declaration {
        id: uuid::Uuid::now_v7(),
        registry_entry_id: entry.id,
        declaration_version: req.declaration.declaration_version,
        integration_base_url: req.declaration.integration_base_url.clone(),
        capabilities,
        service_contract_version: req.declaration.service_contract_version.clone(),
        declared_by_subject: "api".into(),
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
    if let Some(decl) = req.declaration {
        if !admin_panel_domain::valid_integration_base_url(&decl.integration_base_url) {
            return validation("integration_base_url", "must_be_https_origin");
        }
        if let Err(err) = admin_panel_domain::validate_capabilities(&decl.capabilities) {
            return validation("capabilities", &err.to_string());
        }
        let mut capabilities = decl.capabilities.clone();
        capabilities.sort();
        let content_hash = content_hash(&[
            &decl.integration_base_url,
            &serde_json::to_string(&capabilities).unwrap(),
            &decl.service_contract_version,
        ]);
        let declaration = admin_panel_domain::Declaration {
            id: uuid::Uuid::now_v7(),
            registry_entry_id: current.id,
            declaration_version: decl.declaration_version,
            integration_base_url: decl.integration_base_url,
            capabilities,
            service_contract_version: decl.service_contract_version,
            declared_by_subject: decl.requested_by.unwrap_or_else(|| "api".to_string()),
            declared_at: chrono::Utc::now(),
            approval_status: admin_panel_domain::ApprovalStatus::Pending,
            approved_by_subject: None,
            approved_at: None,
            content_hash,
        };
        match state.registry.insert_declaration(&declaration).await {
            Ok(()) => {}
            // Same content already declared (idempotent PATCH): reuse it.
            Err(admin_panel_domain::DomainError::Conflict(_)) => {}
            Err(err) => return internal(err),
        }
    }
    match state
        .registry
        .update_metadata(
            &service_key,
            req.display_name.as_deref().unwrap_or(&current.display_name),
            req.owner_team.as_deref().unwrap_or(&current.owner_team),
            expected_version,
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
        .approve_declaration(&service_key, req.declaration_id, "admin", expected_version)
        .await
    {
        Ok((entry, declaration)) => {
            let _ = state
                .audit
                .append(&admin_panel_domain::AuditEvent {
                    id: uuid::Uuid::now_v7(),
                    occurred_at: chrono::Utc::now(),
                    request_id: uuid::Uuid::now_v7(),
                    actor_subject: Some("admin".into()),
                    actor_role: Some(admin_panel_domain::PanelRole::PlatformAdmin),
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
    Json(req): Json<admin_panel_domain::BrandingDocument>,
) -> Response {
    if let Err(err) = req.validate() {
        return validation("document", &err.to_string());
    }
    let Ok(revision_number) = state.branding.next_revision_number().await else {
        return internal("cannot allocate revision");
    };
    let document_hash = content_hash(&[&serde_json::to_string(&req).unwrap()]);
    let revision = admin_panel_domain::BrandingRevision {
        id: uuid::Uuid::now_v7(),
        revision: revision_number,
        state: admin_panel_domain::RevisionState::Draft,
        document: req,
        document_hash: document_hash.clone(),
        etag: format!("draft-{document_hash}"),
        created_by_subject: "operator".into(),
        created_at: chrono::Utc::now(),
        published_by_subject: None,
        published_at: None,
        based_on_revision: None,
    };
    match state.branding.insert_draft(&revision).await {
        Ok(()) => (StatusCode::CREATED, Json(json!({ "revision": revision }))).into_response(),
        Err(err) => internal(err),
    }
}

#[utoipa::path(post, path = "/api/v1/branding/revisions/{id}/publish",
    tag = "branding",
    params(("id" = uuid::Uuid, Path), ("If-Match" = String, Header)),
    responses((status = 200, description = "published"),
              (status = 409, description = "not a draft / already published")))]
async fn publish_revision(
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
    match state.branding.publish(revision, "admin", &etag).await {
        Ok(published) => {
            let _ = state
                .audit
                .append(&admin_panel_domain::AuditEvent {
                    id: uuid::Uuid::now_v7(),
                    occurred_at: chrono::Utc::now(),
                    request_id: uuid::Uuid::now_v7(),
                    actor_subject: Some("admin".into()),
                    actor_role: Some(admin_panel_domain::PanelRole::PlatformAdmin),
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

// ─── Audit ───────────────────────────────────────────────────────────────────

#[derive(Deserialize, utoipa::ToSchema)]
struct ListAuditQuery {
    action: Option<String>,
    entity_type: Option<String>,
    limit: Option<i64>,
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

#[utoipa::path(get, path = "/api/v1/audit",
    tag = "audit",
    params(("limit" = Option<u32>, Query, description = "max events (default 100)")),
    responses((status = 200, description = "audit events")))]
async fn list_audit(
    State(state): State<SharedState>,
    axum::extract::Query(query): axum::extract::Query<ListAuditQuery>,
) -> Response {
    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    match state
        .audit
        .list(query.action.as_deref(), query.entity_type.as_deref(), limit)
        .await
    {
        Ok(events) => (
            StatusCode::OK,
            Json(json!({ "events": events, "total": events.len() })),
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
