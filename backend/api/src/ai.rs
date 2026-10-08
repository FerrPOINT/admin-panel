//! Authenticated AI registry; no human-supplied verification evidence.
use crate::{Caller, SharedState};
use admin_panel_domain::ai::ProviderId;
use admin_panel_domain::ai::{AiError, ProviderSettings};
use admin_panel_infra::ai::{PendingPublication, Publication, StoreError};
use admin_panel_infra::ai_runtime::{BridgeError, CredentialInput};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::json;
use uuid::Uuid;

fn unavailable() -> Response {
    failure(StatusCode::SERVICE_UNAVAILABLE, "ai_not_configured")
}

fn failure(status: StatusCode, code: &str) -> Response {
    (status, Json(json!({"error":{"code":code}}))).into_response()
}

fn store_error(error: StoreError) -> Response {
    match error {
        StoreError::Domain(AiError::RevisionConflict) => {
            failure(StatusCode::PRECONDITION_FAILED, "revision_conflict")
        }
        StoreError::Domain(error) => failure(StatusCode::UNPROCESSABLE_ENTITY, &error.to_string()),
        StoreError::OperationConflict => {
            failure(StatusCode::CONFLICT, "operation_payload_conflict")
        }
        StoreError::PublicationPending => failure(
            StatusCode::CONFLICT,
            "publication_pending_readback_required",
        ),
        _ => failure(StatusCode::INTERNAL_SERVER_ERROR, "ai_storage_error"),
    }
}

#[utoipa::path(get,path="/api/v1/ai/providers",tag="ai",responses((status=200,description="Saved provider drafts; connection state requires runtime"),(status=401,description="Authentication required"),(status=503,description="AI not configured")))]
pub async fn providers(State(state): State<SharedState>) -> Response {
    let Some(store) = &state.ai else {
        return unavailable();
    };
    match store.drafts().await {
        Ok(drafts) => {
            let (runtime, error) = match &state.ai_runtime {
                Some(client) => match client.registry().await {
                    Ok(registry) => (Some(registry), None),
                    Err(error) => (None, Some(error.to_string())),
                },
                None => (None, Some("ai_runtime_not_configured".into())),
            };
            no_store(Json(json!({"schema_version":1,"providers":drafts,"runtime":runtime,"runtime_error":error})).into_response())
        }
        Err(error) => store_error(error),
    }
}

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response
}

fn bridge_error(error: BridgeError) -> Response {
    let status = match error {
        BridgeError::Configuration | BridgeError::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        BridgeError::Protocol => StatusCode::BAD_GATEWAY,
        BridgeError::Conflict => StatusCode::CONFLICT,
        BridgeError::Disconnected => StatusCode::FAILED_DEPENDENCY,
        BridgeError::Quota => StatusCode::TOO_MANY_REQUESTS,
        BridgeError::Rejected => StatusCode::UNPROCESSABLE_ENTITY,
    };
    no_store(failure(status, &error.to_string()))
}

#[utoipa::path(get,path="/api/v1/ai/budget",tag="ai",responses((status=200,description="Secret-free aggregate acceptance ledger; decimal microdollars"),(status=401,description="Active central session required"),(status=502,description="Invalid runtime projection"),(status=503,description="Runtime unavailable; no empty-ledger fallback")))]
pub async fn acceptance_budget(State(state): State<SharedState>) -> Response {
    let Some(client) = &state.ai_runtime else {
        return no_store(unavailable());
    };
    match client.acceptance_budget().await {
        Ok(budget) => no_store(Json(budget).into_response()),
        Err(error) => bridge_error(error),
    }
}

fn provider_id(name: &str) -> Option<ProviderId> {
    match name {
        "chatgpt" => Some(ProviderId::Chatgpt),
        "openrouter" => Some(ProviderId::Openrouter),
        _ => None,
    }
}

#[utoipa::path(get,path="/api/v1/ai/providers/{provider}/models",tag="ai",params(("provider"=String,Path)),responses((status=200,description="Catalog; access remains unverified"),(status=401,description="Authentication required"),(status=503,description="Runtime unavailable")))]
pub async fn models(State(state): State<SharedState>, Path(provider): Path<String>) -> Response {
    let Some(provider) = provider_id(&provider) else {
        return failure(StatusCode::NOT_FOUND, "unknown_provider");
    };
    let Some(client) = &state.ai_runtime else {
        return unavailable();
    };
    match client.models(provider).await {
        Ok(catalog) => no_store(Json(catalog).into_response()),
        Err(error) => bridge_error(error),
    }
}

#[utoipa::path(get,path="/api/v1/ai/providers/chatgpt/account",tag="ai",responses((status=200,description="Secret-free managed account state"),(status=401,description="Authentication required"),(status=503,description="Runtime unavailable")))]
pub async fn account(State(state): State<SharedState>) -> Response {
    let Some(client) = &state.ai_runtime else {
        return unavailable();
    };
    match client.account().await {
        Ok(account) => no_store(Json(account).into_response()),
        Err(error) => bridge_error(error),
    }
}

async fn audit_intent(
    state: &SharedState,
    caller: &Caller,
    operation: Uuid,
    provider: ProviderId,
    action: &str,
) -> Result<(), Box<Response>> {
    if !caller.can_mutate {
        return Err(Box::new(failure(
            StatusCode::FORBIDDEN,
            "write_capability_required",
        )));
    }
    if state.ai.is_none() || state.ai_runtime.is_none() {
        return Err(Box::new(unavailable()));
    }
    state
        .audit
        .append(&admin_panel_domain::AuditEvent {
            id: Uuid::new_v4(),
            occurred_at: chrono::Utc::now(),
            request_id: operation,
            actor_subject: Some(caller.subject.clone()),
            actor_role: Some(caller.role),
            action: action.into(),
            entity_type: "ai_provider".into(),
            entity_id: None,
            metadata: json!({"provider":provider,"operation_id":operation,"phase":"intent"}),
        })
        .await
        .map_err(|_| {
            Box::new(failure(
                StatusCode::INTERNAL_SERVER_ERROR,
                "ai_audit_unavailable",
            ))
        })
}

#[utoipa::path(put,path="/api/v1/ai/providers/openrouter/credentials",tag="ai",request_body=serde_json::Value,responses((status=200,description="Credential generation; secret is write-only"),(status=403,description="Write capability required"),(status=409,description="Operation conflict"),(status=422,description="Invalid credential"),(status=503,description="Runtime unavailable; read back operation before retry")))]
pub async fn credentials(
    State(state): State<SharedState>,
    axum::Extension(caller): axum::Extension<Caller>,
    Json(input): Json<CredentialInput>,
) -> Response {
    if !caller.can_mutate {
        return failure(StatusCode::FORBIDDEN, "write_capability_required");
    }
    if input.credential.len() < 16
        || input.credential.len() > 4096
        || input
            .credential
            .bytes()
            .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
    {
        return failure(StatusCode::UNPROCESSABLE_ENTITY, "invalid_credential");
    }
    if let Err(response) = audit_intent(
        &state,
        &caller,
        input.operation_id,
        ProviderId::Openrouter,
        "ai.credentials.set",
    )
    .await
    {
        return *response;
    }
    match state.ai_runtime.as_ref().unwrap().credentials(&input).await {
        Ok(connection) => no_store(Json(connection).into_response()),
        Err(error) => bridge_error(error),
    }
}

fn operation_id(headers: &HeaderMap) -> Option<Uuid> {
    headers
        .get("idempotency-key")?
        .to_str()
        .ok()
        .and_then(|s| Uuid::parse_str(s).ok())
}

#[utoipa::path(post,path="/api/v1/ai/providers/chatgpt/login",tag="ai",params(("Idempotency-Key"=String,Header,description="Operation UUID")),responses((status=200,description="Device authorization attempt"),(status=403,description="Write capability required"),(status=409,description="An unresolved operation prevents login"),(status=428,description="Operation UUID required")))]
pub async fn start_login(
    State(state): State<SharedState>,
    axum::Extension(caller): axum::Extension<Caller>,
    headers: HeaderMap,
) -> Response {
    if !caller.can_mutate {
        return failure(StatusCode::FORBIDDEN, "write_capability_required");
    }
    let Some(operation) = operation_id(&headers) else {
        return failure(
            StatusCode::PRECONDITION_REQUIRED,
            "idempotency_key_required",
        );
    };
    if let Err(response) = audit_intent(
        &state,
        &caller,
        operation,
        ProviderId::Chatgpt,
        "ai.login.start",
    )
    .await
    {
        return *response;
    }
    match state
        .ai_runtime
        .as_ref()
        .unwrap()
        .login(operation, true)
        .await
    {
        Ok(login) => no_store(Json(login).into_response()),
        Err(error) => bridge_error(error),
    }
}

#[utoipa::path(get,path="/api/v1/ai/providers/chatgpt/login/{operation}",tag="ai",params(("operation"=Uuid,Path)),responses((status=200,description="Authorization status readback"),(status=401,description="Authentication required")))]
pub async fn login_status(
    State(state): State<SharedState>,
    Path(operation): Path<Uuid>,
) -> Response {
    let Some(client) = &state.ai_runtime else {
        return unavailable();
    };
    match client.login(operation, false).await {
        Ok(login) => no_store(Json(login).into_response()),
        Err(error) => bridge_error(error),
    }
}

#[utoipa::path(delete,path="/api/v1/ai/providers/chatgpt/login/{operation}",tag="ai",params(("operation"=Uuid,Path)),responses((status=204,description="Cancellation confirmed"),(status=403,description="Write capability required"),(status=503,description="Unknown cancellation; read back operation")))]
pub async fn cancel_login(
    State(state): State<SharedState>,
    axum::Extension(caller): axum::Extension<Caller>,
    Path(operation): Path<Uuid>,
) -> Response {
    if let Err(response) = audit_intent(
        &state,
        &caller,
        operation,
        ProviderId::Chatgpt,
        "ai.login.cancel",
    )
    .await
    {
        return *response;
    }
    match state
        .ai_runtime
        .as_ref()
        .unwrap()
        .cancel_login(operation)
        .await
    {
        Ok(()) => no_store(StatusCode::NO_CONTENT.into_response()),
        Err(error) => bridge_error(error),
    }
}

#[utoipa::path(delete,path="/api/v1/ai/providers/{provider}/connection",tag="ai",params(("provider"=String,Path),("Idempotency-Key"=String,Header,description="Operation UUID")),responses((status=204,description="Connection withdrawal confirmed"),(status=403,description="Write capability required"),(status=409,description="Operation conflict"),(status=428,description="Operation UUID required"),(status=503,description="Unknown withdrawal; read back operation")))]
pub async fn disconnect(
    State(state): State<SharedState>,
    axum::Extension(caller): axum::Extension<Caller>,
    Path(provider): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !caller.can_mutate {
        return failure(StatusCode::FORBIDDEN, "write_capability_required");
    }
    let Some(provider) = provider_id(&provider) else {
        return failure(StatusCode::NOT_FOUND, "unknown_provider");
    };
    let Some(operation) = operation_id(&headers) else {
        return failure(
            StatusCode::PRECONDITION_REQUIRED,
            "idempotency_key_required",
        );
    };
    if let Err(response) = audit_intent(
        &state,
        &caller,
        operation,
        provider,
        "ai.connection.disconnect",
    )
    .await
    {
        return *response;
    }
    match state
        .ai_runtime
        .as_ref()
        .unwrap()
        .disconnect(provider, operation)
        .await
    {
        Ok(()) => no_store(StatusCode::NO_CONTENT.into_response()),
        Err(error) => bridge_error(error),
    }
}

#[utoipa::path(put,path="/api/v1/ai/providers/{provider}",tag="ai",params(("provider"=String,Path),("If-Match"=String,Header,description="Quoted draft revision")),request_body=serde_json::Value,responses((status=200,description="Saved draft"),(status=403,description="Write capability required"),(status=412,description="Stale draft"),(status=428,description="If-Match required")))]
pub async fn save_settings(
    State(state): State<SharedState>,
    axum::Extension(caller): axum::Extension<Caller>,
    Path(provider): Path<String>,
    headers: HeaderMap,
    Json(settings): Json<ProviderSettings>,
) -> Response {
    if !caller.can_mutate {
        return failure(StatusCode::FORBIDDEN, "write_capability_required");
    }
    let Some(store) = &state.ai else {
        return unavailable();
    };
    if provider != admin_panel_infra::ai::provider_key(settings.provider) {
        return failure(StatusCode::UNPROCESSABLE_ENTITY, "provider_mismatch");
    }
    let Some(revision) = parse_match(&headers) else {
        return failure(StatusCode::PRECONDITION_REQUIRED, "if_match_required");
    };
    match store.save_draft(&settings, revision, &caller.subject).await {
        Ok(draft) => (
            [
                ("etag", format!("\"{}\"", draft.draft_revision)),
                ("cache-control", "no-store".into()),
            ],
            Json(draft),
        )
            .into_response(),
        Err(error) => store_error(error),
    }
}

#[utoipa::path(get,path="/api/v1/ai/providers/{provider}/operations/{operation}",tag="ai",params(("provider"=String,Path),("operation"=Uuid,Path)),responses((status=200,description="Secret-free credential or withdrawal receipt"),(status=401,description="Authentication required"),(status=409,description="Operation is unknown or belongs to another provider")))]
pub async fn connection_operation(
    State(state): State<SharedState>,
    Path((provider, operation)): Path<(String, Uuid)>,
) -> Response {
    let Some(provider) = provider_id(&provider) else {
        return failure(StatusCode::NOT_FOUND, "unknown_provider");
    };
    let Some(client) = &state.ai_runtime else {
        return unavailable();
    };
    match client.connection_operation(provider, operation).await {
        Ok(operation) => no_store(Json(operation).into_response()),
        Err(error) => bridge_error(error),
    }
}

fn parse_match(headers: &HeaderMap) -> Option<i64> {
    let value = headers.get("if-match")?.to_str().ok()?;
    let number = value
        .strip_prefix('"')?
        .strip_suffix('"')?
        .parse::<i64>()
        .ok()?;
    (number > 0).then_some(number)
}

#[utoipa::path(get,path="/api/v1/ai/selection",tag="ai",responses((status=200,description="Published immutable profile or explicit unconfigured state"),(status=503,description="AI disabled")))]
pub async fn selection(State(state): State<SharedState>) -> Response {
    let Some(store) = &state.ai else {
        return unavailable();
    };
    match store.selected().await {
        Ok(profile) => {
            let revision = profile.as_ref().map_or(0, |p| p.revision);
            (
                [
                    ("etag", format!("\"ai-sdlc2-{revision}\"")),
                    ("cache-control", "no-store".into()),
                ],
                Json(json!({"schema_version":1,"configured":profile.is_some(),"profile":profile})),
            )
                .into_response()
        }
        Err(error) => store_error(error),
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishInput {
    pub schema_version: u32,
    pub operation_id: Uuid,
    pub expected_draft_revision: i64,
    pub verification_id: Uuid,
    pub settings: ProviderSettings,
}

fn publication_match(headers: &HeaderMap) -> Option<u64> {
    let value = headers.get("if-match")?.to_str().ok()?;
    let number = value.strip_prefix("\"ai-sdlc2-")?.strip_suffix('"')?;
    let revision = number.parse::<u64>().ok()?;
    (revision < i64::MAX as u64 && number == revision.to_string()).then_some(revision)
}

fn publication_projection(pending: PendingPublication) -> serde_json::Value {
    json!({"schema_version":1,"operation_id":pending.operation_id,"state":pending.state,
        "profile":pending.profile,"rejection_code":pending.rejection_code})
}

#[utoipa::path(put,path="/api/v1/ai/selection",tag="ai",request_body=serde_json::Value,responses((status=202,description="Immutable candidate pending authenticated runtime acknowledgement"),(status=401,description="Authentication required"),(status=403,description="Write capability required"),(status=412,description="Stale revision"),(status=424,description="Runtime-owned verification missing"),(status=428,description="Strong If-Match required")))]
pub async fn publish_selection(
    State(state): State<SharedState>,
    axum::Extension(caller): axum::Extension<Caller>,
    headers: HeaderMap,
    Json(input): Json<PublishInput>,
) -> Response {
    if !caller.can_mutate {
        return failure(StatusCode::FORBIDDEN, "write_capability_required");
    }
    let (Some(store), Some(client)) = (&state.ai, &state.ai_runtime) else {
        return unavailable();
    };
    let Some(expected_revision) = publication_match(&headers) else {
        return failure(StatusCode::PRECONDITION_REQUIRED, "if_match_required");
    };
    if input.schema_version != 1
        || input.operation_id.is_nil()
        || input.verification_id.is_nil()
        || input.expected_draft_revision <= 0
    {
        return failure(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_publication_request",
        );
    }
    let status = match client
        .verification_status(input.settings.provider, input.verification_id)
        .await
    {
        Ok(status) => status,
        Err(error) => return bridge_error(error),
    };
    let Some(proof) = status.verified_adapter else {
        return failure(StatusCode::FAILED_DEPENDENCY, "configuration_not_verified");
    };
    if proof.draft_revision != Some(input.expected_draft_revision) {
        return failure(
            StatusCode::PRECONDITION_FAILED,
            "verification_draft_changed",
        );
    }
    match store
        .prepare_publication(Publication {
            expected_revision,
            expected_draft_revision: input.expected_draft_revision,
            operation_id: input.operation_id,
            actor_subject: &caller.subject,
            settings: &input.settings,
            credential_generation: proof.evidence.credential_generation,
            evidence: &proof.evidence,
            adapter_version: &proof.adapter_version,
            accounting_policy: &proof.accounting_policy,
        })
        .await
    {
        Ok(_) => match store.publication_status(input.operation_id).await {
            Ok(Some(pending)) => {
                let code = if pending.state == "pending" {
                    StatusCode::ACCEPTED
                } else {
                    StatusCode::OK
                };
                no_store((code, Json(publication_projection(pending))).into_response())
            }
            _ => failure(StatusCode::INTERNAL_SERVER_ERROR, "ai_storage_error"),
        },
        Err(error) => store_error(error),
    }
}

#[utoipa::path(get,path="/api/v1/ai/publications/{operation}",tag="ai",params(("operation"=Uuid,Path)),responses((status=200,description="Secret-free durable pending/published/rejected outcome"),(status=401,description="Authentication required"),(status=404,description="Unknown own-workspace operation")))]
pub async fn publication_status(
    State(state): State<SharedState>,
    Path(operation): Path<Uuid>,
) -> Response {
    let Some(store) = &state.ai else {
        return unavailable();
    };
    if operation.is_nil() {
        return failure(StatusCode::UNPROCESSABLE_ENTITY, "invalid_operation_id");
    }
    match store.publication_status(operation).await {
        Ok(Some(pending)) => no_store(Json(publication_projection(pending)).into_response()),
        Ok(None) => failure(StatusCode::NOT_FOUND, "publication_not_found"),
        Err(error) => store_error(error),
    }
}

#[utoipa::path(get,path="/api/v1/runtime/ai",tag="ai",responses((status=200,description="Authenticated secret-free frozen profile"),(status=304,description="Unchanged"),(status=404,description="No published profile"),(status=401,description="Authentication required")))]
pub async fn runtime_profile(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    let Some(store) = &state.ai else {
        return unavailable();
    };
    match store.selected().await {
        Ok(Some(profile)) => {
            let etag = format!("\"ai-{}-{}\"", profile.workspace, profile.revision);
            if headers.get("if-none-match").and_then(|v| v.to_str().ok()) == Some(&etag) {
                return (
                    StatusCode::NOT_MODIFIED,
                    [("etag", etag), ("cache-control", "no-store".into())],
                )
                    .into_response();
            }
            (
                [("etag", etag), ("cache-control", "no-store".into())],
                Json(profile),
            )
                .into_response()
        }
        Ok(None) => failure(StatusCode::NOT_FOUND, "ai_profile_not_published"),
        Err(error) => store_error(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requires_exact_strong_draft_revision() {
        let mut headers = HeaderMap::new();
        for invalid in ["*", "W/\"1\"", "1", "\"0\"", "\"-1\"", "\"1\", \"2\""] {
            headers.insert("if-match", invalid.parse().unwrap());
            assert_eq!(parse_match(&headers), None);
        }
        headers.insert("if-match", "\"12\"".parse().unwrap());
        assert_eq!(parse_match(&headers), Some(12));
    }

    #[test]
    fn publication_requires_own_strong_revision_and_never_accepts_browser_evidence() {
        let mut headers = HeaderMap::new();
        for bad in [
            "*",
            "W/\"ai-sdlc2-1\"",
            "\"ai-sdlc1-1\"",
            "\"ai-sdlc2-01\"",
            "\"ai-sdlc2--1\"",
            "\"ai-sdlc2-9223372036854775807\"",
        ] {
            headers.insert("if-match", bad.parse().unwrap());
            assert!(publication_match(&headers).is_none());
        }
        for revision in [0, 1, 19] {
            headers.insert(
                "if-match",
                format!("\"ai-sdlc2-{revision}\"").parse().unwrap(),
            );
            assert_eq!(publication_match(&headers), Some(revision));
        }
        let mut input = json!({"schema_version":1,"operation_id":Uuid::new_v4(),"expected_draft_revision":1,
            "verification_id":Uuid::new_v4(),"settings":{"provider":"chatgpt","model":"gpt-6-luna","context_window_tokens":256000}});
        assert!(serde_json::from_value::<PublishInput>(input.clone()).is_ok());
        input["evidence"] = json!({"tools":true,"context_limit_tokens":256000});
        assert!(serde_json::from_value::<PublishInput>(input).is_err());
    }

    #[tokio::test]
    async fn ai_projection_and_registry_are_never_public_branding_routes() {
        use axum::{body::Body, http::Request};
        use std::sync::Arc;
        use tower::ServiceExt;
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://fixture:fixture@127.0.0.1:1/unused")
            .unwrap();
        let state = Arc::new(crate::AppState {
            namespaces: None,
            ai: None,
            ai_runtime: None,
            messaging: Arc::new(admin_panel_infra::messaging::MessagingRuntime::new(false)),
            registry: admin_panel_infra::registry::RegistryStore::new(pool.clone()),
            branding: admin_panel_infra::branding::BrandingStore::new(pool.clone()),
            access: admin_panel_infra::access::AccessStore::new(pool.clone()),
            audit: admin_panel_infra::audit::AuditStore::new(pool),
            config: admin_panel_shared::AppConfig {
                database: admin_panel_shared::DatabaseConfig {
                    url: "unused".into(),
                    max_connections: 1,
                },
                server: admin_panel_shared::ServerConfig {
                    address: "127.0.0.1".into(),
                    port: 8771,
                    cors_allowed_origins: vec![],
                },
                auth: admin_panel_shared::AuthConfig {
                    jwks_uri: "http://localhost:8701/oidc/jwks".into(),
                    issuer: "http://localhost:8701".into(),
                    audience: "sdlc".into(),
                    central_api_url: "http://localhost:8701".into(),
                },
            },
        });
        let app = crate::router(state.clone());
        let unavailable_budget = acceptance_budget(State(state.clone())).await;
        assert_eq!(unavailable_budget.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(unavailable_budget.headers()["cache-control"], "no-store");
        for path in [
            "/api/v1/ai/budget",
            "/api/v1/ai/providers",
            "/api/v1/ai/selection",
            "/api/v1/runtime/ai",
            "/api/v1/ai/providers/chatgpt/account",
            "/api/v1/ai/providers/openrouter/models",
            "/api/v1/ai/providers/chatgpt/login/00000000-0000-0000-0000-000000000001",
            "/api/v1/ai/providers/openrouter/operations/00000000-0000-0000-0000-000000000001",
            "/api/v1/ai/publications/00000000-0000-0000-0000-000000000001",
        ] {
            let request = Request::builder().uri(path).body(Body::empty()).unwrap();
            assert_eq!(
                app.clone().oneshot(request).await.unwrap().status(),
                StatusCode::UNAUTHORIZED
            );
        }
        let input = json!({"schema_version":1,"operation_id":Uuid::new_v4(),"expected_draft_revision":1,
            "verification_id":Uuid::new_v4(),"settings":{"provider":"chatgpt","model":"gpt-6-luna","context_window_tokens":256000}});
        assert_eq!(
            app.clone()
                .oneshot(
                    Request::builder()
                        .method("PUT")
                        .uri("/api/v1/ai/selection")
                        .header("content-type", "application/json")
                        .header("if-match", "\"ai-sdlc2-0\"")
                        .body(Body::from(input.to_string()))
                        .unwrap()
                )
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let readonly = Caller {
            subject: "readonly-pat".into(),
            email: None,
            central_role: None,
            role: admin_panel_domain::PanelRole::PlatformAdmin,
            can_mutate: false,
            can_manage_bindings: false,
        };
        let mut headers = HeaderMap::new();
        headers.insert("if-match", "\"ai-sdlc2-0\"".parse().unwrap());
        assert_eq!(
            publish_selection(
                State(state),
                axum::Extension(readonly),
                headers,
                Json(serde_json::from_value(input).unwrap())
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            app.oneshot(
                Request::builder()
                    .uri("/health/live")
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
            StatusCode::OK
        );
    }
}
