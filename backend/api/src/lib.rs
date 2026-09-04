//! HTTP API of the Admin Panel (docs/API.md).

use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
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

pub fn router(state: SharedState) -> Router {
    Router::new()
        .route("/health/live", get(health_live))
        .route("/health/ready", get(health_ready))
        .route("/api/v1/runtime/branding", get(runtime_branding))
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
        .with_state(state)
}

async fn health_live() -> StatusCode {
    StatusCode::OK
}

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

async fn runtime_branding(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    let Ok(Some(published)) = state.branding.find_published().await else {
        return error_response(
            StatusCode::NOT_FOUND,
            "BRANDING_NOT_PUBLISHED",
            "no published branding revision",
        );
    };
    let etag = format!("\"{}\"", published.etag);
    if let Some(if_none_match) = headers.get("if-none-match").and_then(|v| v.to_str().ok()) {
        if if_none_match == etag {
            return Response::builder()
                .status(StatusCode::NOT_MODIFIED)
                .header("ETag", etag)
                .header("Cache-Control", "public, max-age=60, must-revalidate")
                .body(axum::body::Body::empty())
                .unwrap();
        }
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

#[derive(Deserialize)]
struct CreateServiceRequest {
    service_key: String,
    display_name: String,
    owner_team: String,
    declaration: DeclarationInput,
}

#[derive(Deserialize)]
struct DeclarationInput {
    declaration_version: i32,
    integration_base_url: String,
    service_contract_version: String,
    capabilities: Vec<String>,
}

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

#[derive(Deserialize)]
struct PatchServiceRequest {
    display_name: Option<String>,
    owner_team: Option<String>,
}

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

#[derive(Deserialize)]
struct ApproveRequest {
    declaration_id: uuid::Uuid,
}

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

#[derive(Deserialize)]
struct ListAuditQuery {
    action: Option<String>,
    entity_type: Option<String>,
    limit: Option<i64>,
}

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
