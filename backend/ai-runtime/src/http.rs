//! Internal-only service routes. Admin owns human authorization and publication.
use crate::{
    codex::CodexClient,
    error::RuntimeError,
    openrouter::OpenRouter,
    vault::{
        Connection, DisconnectOperation, ManagedLogin, StoredOperation, Vault, check_private_path,
    },
};
use admin_panel_domain::ai::ProviderId;
use admin_panel_domain::ai::{
    RegisteredRevision, RegistrationStatus, RevisionRegistration, VerificationStatus,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path as RoutePath, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::{get, put},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use subtle::ConstantTimeEq;
use tokio::sync::Mutex;
use uuid::Uuid;
use zeroize::Zeroizing;

pub struct RuntimeState {
    pub vault: Mutex<Vault>,
    pub clients: Vec<ServiceClient>,
    pub grant_verifier: Option<crate::execution_grant::GrantVerifier>,
    pub openrouter: OpenRouter,
    pub codex: Option<CodexClient>,
    pub codex_error: Option<RuntimeError>,
    pub codex_home: std::path::PathBuf,
    pub auth_checkpoint_ready: AtomicBool,
    pub external_calls_enabled: bool,
}

pub struct ServiceClient {
    digest: [u8; 32],
    scopes: BTreeSet<String>,
    machine_subject: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientInput {
    token: String,
    scopes: BTreeSet<String>,
    #[serde(default)]
    machine_subject: Option<String>,
}

impl ServiceClient {
    pub fn uses_inference(&self) -> bool {
        self.scopes.contains("ai:infer")
    }

    pub fn load(path: &Path) -> Result<Vec<Self>, RuntimeError> {
        check_private_path(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if std::fs::metadata(path)
                .map_err(|_| RuntimeError::Configuration)?
                .permissions()
                .mode()
                & 0o077
                != 0
            {
                return Err(RuntimeError::Configuration);
            }
        }
        let bytes = Zeroizing::new(std::fs::read(path).map_err(|_| RuntimeError::Configuration)?);
        if bytes.len() > 65536 {
            return Err(RuntimeError::Configuration);
        }
        let inputs: Vec<ClientInput> =
            serde_json::from_slice(&bytes).map_err(|_| RuntimeError::Configuration)?;
        if inputs.is_empty() || inputs.len() > 32 {
            return Err(RuntimeError::Configuration);
        }
        let clients: Vec<Self> = inputs
            .into_iter()
            .map(|client| {
                let token = Zeroizing::new(client.token);
                if token.len() < 32
                    || token.len() > 256
                    || token.bytes().any(|b| b.is_ascii_whitespace())
                    || client.scopes.is_empty()
                    || client.scopes.iter().any(|scope| {
                        !matches!(scope.as_str(), "ai:admin" | "ai:catalog" | "ai:infer")
                    })
                    || (client.scopes.contains("ai:infer")
                        && (client.scopes.contains("ai:admin")
                            || !client
                                .machine_subject
                                .as_deref()
                                .is_some_and(crate::execution_grant::valid_machine_subject)))
                    || (client.machine_subject.is_some() && !client.scopes.contains("ai:infer"))
                {
                    return Err(RuntimeError::Configuration);
                }
                Ok(Self {
                    digest: Sha256::digest(token.as_bytes()).into(),
                    scopes: client.scopes,
                    machine_subject: client.machine_subject,
                })
            })
            .collect::<Result<_, _>>()?;
        for (index, client) in clients.iter().enumerate() {
            if clients[..index]
                .iter()
                .any(|other| bool::from(other.digest.ct_eq(&client.digest)))
            {
                return Err(RuntimeError::Configuration);
            }
        }
        Ok(clients)
    }
}

#[derive(Clone)]
pub(crate) struct Scopes(pub BTreeSet<String>, pub Option<String>);

async fn authenticate(
    State(state): State<Arc<RuntimeState>>,
    mut request: Request,
    next: Next,
) -> Result<Response, RuntimeError> {
    let token = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(RuntimeError::Unauthorized)?;
    if token.len() > 256 {
        return Err(RuntimeError::Unauthorized);
    }
    let digest: [u8; 32] = Sha256::digest(token.as_bytes()).into();
    let client = state
        .clients
        .iter()
        .find(|c| bool::from(c.digest.ct_eq(&digest)))
        .ok_or(RuntimeError::Unauthorized)?;
    if request
        .uri()
        .path()
        .starts_with("/internal/v1/providers/chatgpt")
        && state.codex.is_some()
        && !state.auth_checkpoint_ready.load(Ordering::Acquire)
    {
        return Err(RuntimeError::Unavailable);
    }
    request.extensions_mut().insert(Scopes(
        client.scopes.clone(),
        client.machine_subject.clone(),
    ));
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    Ok(response)
}

pub(crate) fn require(scopes: &Scopes, scope: &str) -> Result<(), RuntimeError> {
    if scopes.0.contains(scope) {
        Ok(())
    } else {
        Err(RuntimeError::Forbidden)
    }
}

pub fn router(state: Arc<RuntimeState>) -> Router {
    let protected = Router::new()
        .route(
            "/internal/v1/inference/{id}/status",
            axum::routing::post(crate::scoped_readback::status),
        )
        .route(
            "/internal/v1/inference/{id}/events",
            axum::routing::post(crate::scoped_readback::events),
        )
        .route(
            "/internal/v1/inference/{id}/cancel",
            axum::routing::post(crate::scoped_readback::cancel),
        )
        .route(
            "/internal/v1/profiles/registrations",
            axum::routing::post(register_profile),
        )
        .route(
            "/internal/v1/profiles/registrations/{id}",
            get(registration_status),
        )
        .route("/internal/v1/providers", get(providers))
        .route("/internal/v1/budget", get(budget_status))
        .route(
            "/internal/v1/providers/openrouter/connection",
            put(set_openrouter).delete(disconnect_openrouter),
        )
        .route("/internal/v1/providers/{provider}/models", get(models))
        .route(
            "/internal/v1/providers/{provider}/verifications/{id}",
            get(verification_status),
        )
        .route(
            "/internal/v1/providers/{provider}/operations/{id}",
            get(connection_operation),
        )
        .route(
            "/internal/v1/providers/chatgpt/account",
            get(chatgpt_account),
        )
        .route(
            "/internal/v1/providers/chatgpt/login",
            axum::routing::post(start_login),
        )
        .route(
            "/internal/v1/providers/chatgpt/login/{id}",
            get(login_status).delete(cancel_login),
        )
        .route(
            "/internal/v1/providers/chatgpt/connection",
            axum::routing::delete(disconnect_chatgpt),
        )
        .route_layer(middleware::from_fn_with_state(state.clone(), authenticate));
    Router::new()
        .route("/health/live", get(|| async { StatusCode::OK }))
        .merge(protected)
        .layer(DefaultBodyLimit::max(65536))
        .with_state(state)
}

async fn budget_status(
    State(state): State<Arc<RuntimeState>>,
    axum::Extension(scopes): axum::Extension<Scopes>,
) -> Result<Json<admin_panel_domain::ai::AcceptanceBudget>, RuntimeError> {
    require(&scopes, "ai:admin")?;
    let vault = state.vault.lock().await;
    Ok(Json(vault.state().budget.summary()?))
}

async fn verification_status(
    State(state): State<Arc<RuntimeState>>,
    axum::Extension(scopes): axum::Extension<Scopes>,
    RoutePath((provider, verification_id)): RoutePath<(String, Uuid)>,
) -> Result<Json<VerificationStatus>, RuntimeError> {
    require(&scopes, "ai:admin")?;
    let provider = match provider.as_str() {
        "chatgpt" => ProviderId::Chatgpt,
        "openrouter" => ProviderId::Openrouter,
        _ => return Err(RuntimeError::InvalidRequest),
    };
    if verification_id.is_nil() {
        return Err(RuntimeError::InvalidRequest);
    }
    let vault = state.vault.lock().await;
    let verified_adapter = vault
        .state()
        .verified_adapters
        .get(&verification_id)
        .filter(|proof| proof.evidence.settings.provider == provider)
        .cloned();
    // Readback is historical. Publication/registration recheck TTL and generation.
    Ok(Json(VerificationStatus {
        schema_version: 1,
        workspace: "sdlc2".into(),
        provider,
        verification_id,
        verified_adapter,
    }))
}

async fn register_profile(
    State(state): State<Arc<RuntimeState>>,
    axum::Extension(scopes): axum::Extension<Scopes>,
    Json(input): Json<RevisionRegistration>,
) -> Result<Json<RegisteredRevision>, RuntimeError> {
    require(&scopes, "ai:admin")?;
    // Restored evidence is readable, but restore mode must not activate profiles.
    if !state.external_calls_enabled {
        return Err(RuntimeError::ExternalCallsDisabled);
    }
    if input.profile.provider == ProviderId::Chatgpt {
        if state.codex.is_none() {
            return Err(RuntimeError::Unavailable);
        }
        if !state.auth_checkpoint_ready.load(Ordering::Acquire) {
            return Err(RuntimeError::Unavailable);
        }
    }
    let (adapter, accounting) = crate::publication::deployed_contract(input.profile.provider);
    let receipt = state.vault.lock().await.register_revision(
        input,
        adapter,
        accounting,
        chrono::Utc::now(),
    )?;
    Ok(Json(receipt))
}

async fn registration_status(
    State(state): State<Arc<RuntimeState>>,
    axum::Extension(scopes): axum::Extension<Scopes>,
    RoutePath(operation_id): RoutePath<Uuid>,
) -> Result<Json<RegistrationStatus>, RuntimeError> {
    require(&scopes, "ai:admin")?;
    if operation_id.is_nil() {
        return Err(RuntimeError::InvalidRequest);
    }
    let vault = state.vault.lock().await;
    Ok(Json(RegistrationStatus {
        schema_version: 1,
        workspace: "sdlc2".into(),
        operation_id,
        receipt: vault
            .state()
            .registered_revisions
            .get(&operation_id)
            .cloned(),
    }))
}

async fn providers(
    State(state): State<Arc<RuntimeState>>,
    axum::Extension(scopes): axum::Extension<Scopes>,
) -> Result<Json<Value>, RuntimeError> {
    require(&scopes, "ai:admin")?;
    let vault = state.vault.lock().await;
    Ok(Json(
        json!({"schema_version":1,"workspace":"sdlc2","providers":[
        {"id":"chatgpt","connected":vault.state().connections.contains_key("chatgpt"),"generation":vault.state().connections.get("chatgpt").map(|c|c.generation),"pending_login_operation":vault.state().logins.values().find(|login|matches!(login.status.as_str(),"starting"|"pending"|"uncertain"|"cancelling"|"expiring")).map(|login|login.operation_id),"runtime_available":state.codex.is_some(),"runtime_error":state.codex_error.map(|e|e.to_string()),"authorization_checkpoint_ready":state.auth_checkpoint_ready.load(Ordering::Acquire),"capabilities_verified":false},
            {"id":"openrouter","connected":vault.state().connections.contains_key("openrouter"),"generation":vault.state().connections.get("openrouter").map(|c| c.generation),"capabilities_verified":false}
        ]}),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CredentialInput {
    operation_id: Uuid,
    credential: String,
}

#[derive(Serialize)]
struct ConnectionOutput {
    schema_version: u32,
    provider: ProviderId,
    generation: Uuid,
}

async fn set_openrouter(
    State(state): State<Arc<RuntimeState>>,
    axum::Extension(scopes): axum::Extension<Scopes>,
    Json(input): Json<CredentialInput>,
) -> Result<Json<ConnectionOutput>, RuntimeError> {
    require(&scopes, "ai:admin")?;
    let credential = Zeroizing::new(input.credential);
    if credential.len() < 16
        || credential.len() > 4096
        || credential
            .bytes()
            .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
    {
        return Err(RuntimeError::InvalidRequest);
    }
    let mut fingerprint = Sha256::new();
    fingerprint.update(b"openrouter:set:");
    fingerprint.update(credential.as_bytes());
    let fingerprint = hex::encode(fingerprint.finalize());
    let mut vault = state.vault.lock().await;
    if vault.state().disconnects.contains_key(&input.operation_id)
        || vault.state().logins.contains_key(&input.operation_id)
    {
        return Err(RuntimeError::Conflict);
    }
    if let Some(operation) = vault.state().operations.get(&input.operation_id) {
        if operation.fingerprint != fingerprint {
            return Err(RuntimeError::Conflict);
        }
        return Ok(Json(ConnectionOutput {
            schema_version: 1,
            provider: ProviderId::Openrouter,
            generation: operation.generation,
        }));
    }
    let generation = Uuid::new_v4();
    let mut draft = vault.state().clone();
    draft.connections.insert(
        "openrouter".into(),
        Connection {
            provider: ProviderId::Openrouter,
            generation,
            credential: credential.to_string(),
        },
    );
    draft
        .verifications
        .retain(|_, evidence| evidence.settings.provider != ProviderId::Openrouter);
    draft.operations.insert(
        input.operation_id,
        StoredOperation {
            fingerprint,
            generation,
        },
    );
    vault.commit(draft)?;
    Ok(Json(ConnectionOutput {
        schema_version: 1,
        provider: ProviderId::Openrouter,
        generation,
    }))
}

async fn disconnect_openrouter(
    State(state): State<Arc<RuntimeState>>,
    axum::Extension(scopes): axum::Extension<Scopes>,
    headers: HeaderMap,
) -> Result<StatusCode, RuntimeError> {
    require(&scopes, "ai:admin")?;
    let operation = request_operation(&headers)?;
    let mut vault = state.vault.lock().await;
    if let Some(previous) = vault.state().disconnects.get(&operation) {
        return if previous.provider == ProviderId::Openrouter && previous.status == "completed" {
            Ok(StatusCode::NO_CONTENT)
        } else {
            Err(RuntimeError::Conflict)
        };
    }
    if vault.state().operations.contains_key(&operation)
        || vault.state().logins.contains_key(&operation)
    {
        return Err(RuntimeError::Conflict);
    }
    let mut draft = vault.state().clone();
    let generation = draft
        .connections
        .remove("openrouter")
        .map(|connection| connection.generation);
    draft.disconnects.insert(
        operation,
        DisconnectOperation {
            provider: ProviderId::Openrouter,
            generation,
            status: "completed".into(),
        },
    );
    draft
        .verifications
        .retain(|_, evidence| evidence.settings.provider != ProviderId::Openrouter);
    vault.commit(draft)?;
    Ok(StatusCode::NO_CONTENT)
}

fn request_operation(headers: &HeaderMap) -> Result<Uuid, RuntimeError> {
    headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| Uuid::parse_str(v).ok())
        .ok_or(RuntimeError::InvalidRequest)
}

async fn connection_operation(
    State(state): State<Arc<RuntimeState>>,
    axum::Extension(scopes): axum::Extension<Scopes>,
    RoutePath((provider, id)): RoutePath<(String, Uuid)>,
) -> Result<Json<Value>, RuntimeError> {
    require(&scopes, "ai:admin")?;
    let provider_id = match provider.as_str() {
        "chatgpt" => ProviderId::Chatgpt,
        "openrouter" => ProviderId::Openrouter,
        _ => return Err(RuntimeError::InvalidRequest),
    };
    let vault = state.vault.lock().await;
    if let Some(operation) = vault
        .state()
        .disconnects
        .get(&id)
        .filter(|op| op.provider == provider_id)
    {
        return Ok(Json(
            json!({"schema_version":1,"operation_id":id,"provider":provider_id,"kind":"disconnect","status":operation.status,"generation":operation.generation}),
        ));
    }
    if provider_id == ProviderId::Openrouter
        && let Some(operation) = vault.state().operations.get(&id)
    {
        return Ok(Json(
            json!({"schema_version":1,"operation_id":id,"provider":provider_id,"kind":"credentials","status":"completed","generation":operation.generation}),
        ));
    }
    Err(RuntimeError::InvalidRequest)
}

async fn models(
    State(state): State<Arc<RuntimeState>>,
    axum::Extension(scopes): axum::Extension<Scopes>,
    RoutePath(provider): RoutePath<String>,
) -> Result<Json<Value>, RuntimeError> {
    if !scopes.0.contains("ai:admin") {
        require(&scopes, "ai:catalog")?;
    }
    if !state.external_calls_enabled {
        return Err(RuntimeError::ExternalCallsDisabled);
    }
    match provider.as_str() {
        "openrouter" => {
            let connection = state
                .vault
                .lock()
                .await
                .state()
                .connections
                .get("openrouter")
                .cloned()
                .ok_or(RuntimeError::Disconnected)?;
            let models = state.openrouter.models(&connection.credential).await?;
            Ok(Json(
                json!({"schema_version":1,"provider":"openrouter","source":"provider_catalog","access_verified":false,"models":models}),
            ))
        }
        "chatgpt" => {
            let codex = state.codex.as_ref().ok_or(RuntimeError::Unavailable)?;
            let catalog = codex
                .call("model/list", json!({"includeHidden":false,"limit":100}))
                .await?;
            let list = catalog
                .get("data")
                .and_then(Value::as_array)
                .ok_or(RuntimeError::Protocol)?;
            // Whitelist metadata. Never forward arbitrary upstream JSON through Admin.
            let models: Vec<Value> = list.iter().filter(|m| m.get("hidden") != Some(&Value::Bool(true)))
                .map(|m| json!({"id":m["id"],"model":m["model"],"name":m["displayName"],"access_verified":false,"context_limit_tokens":null,"max_output_tokens":null})).collect();
            Ok(Json(
                json!({"schema_version":1,"provider":"chatgpt","source":"codex_model_list","access_verified":false,"models":models}),
            ))
        }
        _ => Err(RuntimeError::InvalidRequest),
    }
}

async fn chatgpt_account(
    State(state): State<Arc<RuntimeState>>,
    axum::Extension(scopes): axum::Extension<Scopes>,
) -> Result<Json<Value>, RuntimeError> {
    require(&scopes, "ai:admin")?;
    if !state.external_calls_enabled {
        return Err(RuntimeError::ExternalCallsDisabled);
    }
    let account = state
        .codex
        .as_ref()
        .ok_or(RuntimeError::Unavailable)?
        .call("account/read", json!({"refreshToken":false}))
        .await?;
    let kind = account["account"]["type"].as_str();
    // Only managed ChatGPT auth is accepted, never API-key substitution.
    if kind.is_some() && kind != Some("chatgpt") {
        return Err(RuntimeError::ProviderAuth);
    }
    Ok(Json(
        json!({"schema_version":1,"connected":kind == Some("chatgpt"),"plan_type":account["account"]["planType"],"capabilities_verified":false}),
    ))
}

async fn start_login(
    State(state): State<Arc<RuntimeState>>,
    axum::Extension(scopes): axum::Extension<Scopes>,
    headers: HeaderMap,
) -> Result<Json<Value>, RuntimeError> {
    require(&scopes, "ai:admin")?;
    if !state.external_calls_enabled {
        return Err(RuntimeError::ExternalCallsDisabled);
    }
    let operation_id = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| Uuid::parse_str(v).ok())
        .ok_or(RuntimeError::InvalidRequest)?;
    let codex = state.codex.as_ref().ok_or(RuntimeError::Unavailable)?;
    {
        let mut vault = state.vault.lock().await;
        if vault.state().disconnects.contains_key(&operation_id)
            || vault.state().operations.contains_key(&operation_id)
            || vault.state().disconnects.values().any(|operation| {
                operation.provider == ProviderId::Chatgpt && operation.status != "completed"
            })
        {
            return Err(RuntimeError::Conflict);
        }
        if let Some(login) = vault.state().logins.get(&operation_id) {
            return Ok(Json(login_projection(login)));
        }
        if vault.state().connections.contains_key("chatgpt") {
            return Err(RuntimeError::Conflict);
        }
        if vault.state().logins.values().any(|l| {
            matches!(
                l.status.as_str(),
                "starting" | "pending" | "uncertain" | "cancelling" | "expiring"
            )
        }) {
            return Err(RuntimeError::Conflict);
        }
        let mut next = vault.state().clone();
        next.logins.insert(
            operation_id,
            ManagedLogin {
                operation_id,
                login_id: None,
                status: "starting".into(),
                user_code: None,
                verification_url: None,
                expires_at: chrono::Utc::now() + chrono::Duration::minutes(15),
            },
        );
        vault.commit(next)?;
    }
    let result = codex
        .call("account/login/start", json!({"type":"chatgptDeviceCode"}))
        .await;
    let mut vault = state.vault.lock().await;
    let mut next = vault.state().clone();
    let login = next
        .logins
        .get_mut(&operation_id)
        .ok_or(RuntimeError::StateIntegrity)?;
    if login.status != "starting" {
        // Logout or a native completion may have won while login/start was in flight.
        return Ok(Json(login_projection(login)));
    }
    match result {
        Ok(value) if value["type"] == "chatgptDeviceCode" => {
            if login.status == "completed" {
                return Ok(Json(login_projection(login)));
            }
            let login_id = value["loginId"]
                .as_str()
                .and_then(|s| Uuid::parse_str(s).ok())
                .ok_or(RuntimeError::Protocol)?;
            let code = value["userCode"]
                .as_str()
                .filter(|s| s.len() <= 128 && !s.is_empty())
                .ok_or(RuntimeError::Protocol)?;
            let url = value["verificationUrl"]
                .as_str()
                .filter(|s| *s == "https://auth.openai.com/codex/device")
                .ok_or(RuntimeError::Protocol)?;
            login.login_id = Some(login_id);
            login.user_code = Some(code.into());
            login.verification_url = Some(url.into());
            login.status = "pending".into();
            let output = login_projection(login);
            vault.commit(next)?;
            Ok(Json(output))
        }
        _ => {
            login.status = "uncertain".into();
            vault.commit(next)?;
            Err(RuntimeError::Unavailable)
        }
    }
}

fn login_projection(login: &ManagedLogin) -> Value {
    json!({"schema_version":1,"operation_id":login.operation_id,"login_id":login.login_id,"status":login.status,
        "user_code":if login.status=="pending" {login.user_code.as_deref()} else {None},
        "verification_url":if login.status=="pending" {login.verification_url.as_deref()} else {None},"expires_at":login.expires_at})
}

async fn login_status(
    State(state): State<Arc<RuntimeState>>,
    axum::Extension(scopes): axum::Extension<Scopes>,
    RoutePath(id): RoutePath<Uuid>,
) -> Result<Json<Value>, RuntimeError> {
    require(&scopes, "ai:admin")?;
    let vault = state.vault.lock().await;
    let login = vault
        .state()
        .logins
        .get(&id)
        .or_else(|| {
            vault
                .state()
                .logins
                .values()
                .find(|l| l.login_id == Some(id))
        })
        .ok_or(RuntimeError::InvalidRequest)?;
    Ok(Json(login_projection(login)))
}

async fn cancel_login(
    State(state): State<Arc<RuntimeState>>,
    axum::Extension(scopes): axum::Extension<Scopes>,
    RoutePath(id): RoutePath<Uuid>,
) -> Result<StatusCode, RuntimeError> {
    require(&scopes, "ai:admin")?;
    if !state.external_calls_enabled {
        return Err(RuntimeError::ExternalCallsDisabled);
    }
    let codex = state.codex.as_ref().ok_or(RuntimeError::Unavailable)?;
    let (operation, native_id) = {
        let mut vault = state.vault.lock().await;
        let mut next = vault.state().clone();
        let login = next
            .logins
            .values_mut()
            .find(|l| l.operation_id == id || l.login_id == Some(id))
            .ok_or(RuntimeError::InvalidRequest)?;
        if login.status == "cancelled" {
            return Ok(StatusCode::NO_CONTENT);
        }
        if login.status != "pending" {
            return Err(RuntimeError::Conflict);
        }
        let native = login.login_id.ok_or(RuntimeError::StateIntegrity)?;
        let op = login.operation_id;
        login.status = "cancelling".into();
        vault.commit(next)?;
        (op, native)
    };
    let native_result = codex
        .call("account/login/cancel", json!({"loginId":native_id}))
        .await?;
    let mut vault = state.vault.lock().await;
    let mut next = vault.state().clone();
    let login = next
        .logins
        .get_mut(&operation)
        .ok_or(RuntimeError::StateIntegrity)?;
    if login.status == "completed" {
        return Err(RuntimeError::Conflict);
    }
    let confirmed = native_result["status"] == "canceled";
    login.status = if confirmed { "cancelled" } else { "uncertain" }.into();
    login.user_code = None;
    login.verification_url = None;
    vault.commit(next)?;
    if confirmed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(RuntimeError::Conflict)
    }
}

async fn disconnect_chatgpt(
    State(state): State<Arc<RuntimeState>>,
    axum::Extension(scopes): axum::Extension<Scopes>,
    headers: HeaderMap,
) -> Result<StatusCode, RuntimeError> {
    require(&scopes, "ai:admin")?;
    if !state.external_calls_enabled {
        return Err(RuntimeError::ExternalCallsDisabled);
    }
    let operation = request_operation(&headers)?;
    // Remove durable auth first. An unknown native logout is then reconciled,
    // never restored automatically from an old token checkpoint.
    {
        let mut vault = state.vault.lock().await;
        if let Some(previous) = vault.state().disconnects.get(&operation) {
            return if previous.provider == ProviderId::Chatgpt && previous.status == "completed" {
                Ok(StatusCode::NO_CONTENT)
            } else {
                Err(RuntimeError::Conflict)
            };
        }
        if vault.state().operations.contains_key(&operation)
            || vault.state().logins.contains_key(&operation)
            || vault
                .state()
                .disconnects
                .values()
                .any(|op| op.provider == ProviderId::Chatgpt && op.status != "completed")
        {
            return Err(RuntimeError::Conflict);
        }
        let mut next = vault.state().clone();
        let generation = next
            .connections
            .remove("chatgpt")
            .map(|connection| connection.generation);
        next.disconnects.insert(
            operation,
            DisconnectOperation {
                provider: ProviderId::Chatgpt,
                generation,
                status: "pending".into(),
            },
        );
        for login in next.logins.values_mut() {
            if matches!(
                login.status.as_str(),
                "pending" | "starting" | "uncertain" | "cancelling" | "expiring"
            ) {
                login.status = "revoked".into();
                login.user_code = None;
                login.verification_url = None;
            }
        }
        next.verifications
            .retain(|_, e| e.settings.provider != ProviderId::Chatgpt);
        vault.commit(next)?;
    }
    let result = state
        .codex
        .as_ref()
        .ok_or(RuntimeError::Unavailable)?
        .call("account/logout", json!({}))
        .await;
    let mut vault = state.vault.lock().await;
    let mut next = vault.state().clone();
    next.disconnects
        .get_mut(&operation)
        .ok_or(RuntimeError::StateIntegrity)?
        .status = if result.is_ok() {
        "completed"
    } else {
        "uncertain"
    }
    .into();
    vault.commit(next)?;
    result?;
    Ok(StatusCode::NO_CONTENT)
}

/// Persist native auth only from this process's own managed home.
pub fn spawn_auth_checkpoint(state: Arc<RuntimeState>) -> Result<(), RuntimeError> {
    let mut events = state
        .codex
        .as_ref()
        .ok_or(RuntimeError::Unavailable)?
        .subscribe();
    tokio::spawn(async move {
        let mut poll = tokio::time::interval(std::time::Duration::from_secs(2));
        loop {
            tokio::select! {
                event=events.recv() => {
                    let Ok(event)=event else { break; };
                    if event["method"]=="account/login/completed" {
                        let native_id=event["params"]["loginId"].as_str().and_then(|s|Uuid::parse_str(s).ok());
                        let success=event["params"]["success"].as_bool()==Some(true);
                        if checkpoint_login(&state,native_id,success).await.is_err() { break; }
                    } else if event["method"]=="runtime/disconnected" { break; }
                }
                _=poll.tick() => {
                    if expire_logins(&state).await.is_err() { break; }
                    if checkpoint_refresh(&state).await.is_err() { break; }
                }
            }
        }
        state.auth_checkpoint_ready.store(false, Ordering::Release);
    });
    Ok(())
}

/// A new native process cannot resume an old in-memory device-code attempt.
/// Readback reconciles withdrawals without dispatching another native logout.
pub async fn reconcile_native_startup(state: &RuntimeState) -> Result<(), RuntimeError> {
    let account = state
        .codex
        .as_ref()
        .ok_or(RuntimeError::Unavailable)?
        .call("account/read", json!({"refreshToken":false}))
        .await?;
    let mut vault = state.vault.lock().await;
    let mut next = vault.state().clone();
    let connected = next.connections.contains_key("chatgpt");
    if account.get("account").is_none()
        || (!connected && !account["account"].is_null())
        || (connected && account["account"]["type"] != "chatgpt")
    {
        return Err(RuntimeError::ProviderAuth);
    }
    let mut changed = false;
    for login in next.logins.values_mut() {
        if matches!(
            login.status.as_str(),
            "starting" | "pending" | "uncertain" | "cancelling" | "expiring"
        ) {
            login.status = if login.expires_at <= chrono::Utc::now() {
                "expired"
            } else {
                "interrupted"
            }
            .into();
            login.user_code = None;
            login.verification_url = None;
            changed = true;
        }
    }
    for operation in next.disconnects.values_mut() {
        if operation.provider == ProviderId::Chatgpt && operation.status != "completed" {
            if connected {
                return Err(RuntimeError::StateIntegrity);
            }
            operation.status = "completed".into();
            changed = true;
        }
    }
    if changed {
        vault.commit(next)?;
    }
    Ok(())
}

async fn expire_logins(state: &RuntimeState) -> Result<(), RuntimeError> {
    let pending = {
        let mut vault = state.vault.lock().await;
        let mut next = vault.state().clone();
        let pending = next
            .logins
            .values_mut()
            .filter(|login| login.status == "pending" && login.expires_at <= chrono::Utc::now())
            .map(|login| {
                login.status = "expiring".into();
                login.user_code = None;
                login.verification_url = None;
                (login.operation_id, login.login_id)
            })
            .collect::<Vec<_>>();
        if !pending.is_empty() {
            vault.commit(next)?;
        }
        pending
    };
    for (operation, id) in pending {
        let confirmed = if let (Some(id), Some(codex)) = (id, &state.codex) {
            codex
                .call("account/login/cancel", json!({"loginId":id}))
                .await
                .is_ok_and(|result| result["status"] == "canceled")
        } else {
            false
        };
        let mut vault = state.vault.lock().await;
        let mut next = vault.state().clone();
        let login = next
            .logins
            .get_mut(&operation)
            .ok_or(RuntimeError::StateIntegrity)?;
        if login.status == "expiring" {
            login.status = if confirmed { "expired" } else { "uncertain" }.into();
        }
        vault.commit(next)?;
    }
    Ok(())
}

fn read_managed_auth(home: &Path) -> Result<Zeroizing<String>, RuntimeError> {
    let path = home.join("auth.json");
    check_private_path(&path)?;
    if std::fs::metadata(&path)
        .map_err(|_| RuntimeError::StateIntegrity)?
        .len()
        > 65536
    {
        return Err(RuntimeError::StateIntegrity);
    }
    let text =
        Zeroizing::new(std::fs::read_to_string(path).map_err(|_| RuntimeError::StateIntegrity)?);
    let value: Value = serde_json::from_str(&text).map_err(|_| RuntimeError::StateIntegrity)?;
    if value.get("tokens").and_then(Value::as_object).is_none()
        || ["access_token", "refresh_token"]
            .iter()
            .any(|key| value["tokens"][key].as_str().is_none_or(|s| s.is_empty()))
        || value.get("OPENAI_API_KEY").is_some_and(|v| !v.is_null())
    {
        return Err(RuntimeError::ProviderAuth);
    }
    Ok(text)
}

/// Own decrypted auth exists only on an explicitly provisioned tmpfs mount.
/// Restoring a backup with external_calls=false never enters this function.
pub fn restore_managed_auth(home: &Path, vault: &Vault) -> Result<(), RuntimeError> {
    use std::io::Write;
    check_private_path(home)?;
    #[cfg(not(unix))]
    {
        let _ = (home, vault);
        return Err(RuntimeError::Configuration);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mountinfo = std::fs::read_to_string("/proc/self/mountinfo")
            .map_err(|_| RuntimeError::Configuration)?;
        let home = home
            .canonicalize()
            .map_err(|_| RuntimeError::Configuration)?;
        let own_mount = mountinfo
            .lines()
            .filter_map(|line| {
                let (left, right) = line.split_once(" - ")?;
                let path = std::path::PathBuf::from(left.split_whitespace().nth(4)?);
                if home.starts_with(&path) {
                    Some((path.components().count(), right.split_whitespace().next()?))
                } else {
                    None
                }
            })
            .max_by_key(|(depth, _)| *depth);
        if !matches!(own_mount, Some((_, "tmpfs"))) {
            return Err(RuntimeError::Configuration);
        }
        let path = home.join("auth.json");
        check_private_path(&path)?;
        if let Some(connection) = vault.state().connections.get("chatgpt") {
            // Only the runtime's encrypted native checkpoint can hydrate Codex.
            let temporary = home.join(format!("auth-{}.tmp", Uuid::new_v4()));
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)
                .map_err(|_| RuntimeError::StateWrite)?;
            file.write_all(connection.credential.as_bytes())
                .and_then(|_| file.sync_all())
                .map_err(|_| RuntimeError::StateWrite)?;
            std::fs::rename(temporary, path).map_err(|_| RuntimeError::StateWrite)?;
        } else if path.exists() {
            // Solely volatile runtime-owned auth, not working data or backups.
            std::fs::remove_file(path).map_err(|_| RuntimeError::StateWrite)?;
        }
        Ok(())
    }
}

async fn checkpoint_login(
    state: &RuntimeState,
    id: Option<Uuid>,
    success: bool,
) -> Result<(), RuntimeError> {
    let mut vault = state.vault.lock().await;
    let mut next = vault.state().clone();
    let Some(login) = next.logins.values_mut().find(|l| {
        id.is_some()
            && ((l.login_id == id && l.status == "pending")
                || (l.login_id.is_none() && l.status == "starting"))
    }) else {
        return Ok(());
    };
    login.login_id = id;
    if login.expires_at <= chrono::Utc::now() {
        // A late completion is never permission to resurrect an expired login.
        login.status = "uncertain".into();
        login.user_code = None;
        login.verification_url = None;
        return vault.commit(next);
    }
    if success {
        let auth = read_managed_auth(&state.codex_home)?;
        next.connections.insert(
            "chatgpt".into(),
            Connection {
                provider: ProviderId::Chatgpt,
                generation: Uuid::new_v4(),
                credential: auth.to_string(),
            },
        );
        next.verifications
            .retain(|_, e| e.settings.provider != ProviderId::Chatgpt);
        login.status = "completed".into();
    } else {
        login.status = "failed".into();
    }
    login.user_code = None;
    login.verification_url = None;
    vault.commit(next)
}

async fn checkpoint_refresh(state: &RuntimeState) -> Result<(), RuntimeError> {
    let mut vault = state.vault.lock().await;
    let mut next = vault.state().clone();
    // No connection means pending login or logout: never resurrect it from disk.
    if let Some(connection) = next.connections.get_mut("chatgpt") {
        let auth = read_managed_auth(&state.codex_home)?;
        if connection.credential != *auth {
            connection.credential = auth.to_string();
            vault.commit(next)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;

    fn test_state() -> (tempfile::TempDir, Arc<RuntimeState>) {
        let dir = tempfile::tempdir().unwrap();
        let storage = dir.path().join("state");
        let key = dir.path().join("key");
        Vault::initialize(&storage, &key, "sdlc2").unwrap();
        let admin = ServiceClient {
            digest: Sha256::digest(b"admin-test-token-0000000000000000").into(),
            scopes: BTreeSet::from(["ai:admin".into()]),
            machine_subject: None,
        };
        let agent = ServiceClient {
            digest: Sha256::digest(b"agent-test-token-0000000000000000").into(),
            scopes: BTreeSet::from(["ai:infer".into()]),
            machine_subject: Some("sdlc2:hermes:developer".into()),
        };
        let state = Arc::new(RuntimeState {
            vault: Mutex::new(Vault::open(&storage, &key, "sdlc2").unwrap()),
            clients: vec![admin, agent],
            grant_verifier: Some(crate::execution_grant::fixtures::verifier()),
            openrouter: OpenRouter::production().unwrap(),
            codex: None,
            codex_error: Some(RuntimeError::ExternalCallsDisabled),
            codex_home: dir.path().join("codex-home"),
            auth_checkpoint_ready: AtomicBool::new(true),
            external_calls_enabled: false,
        });
        (dir, state)
    }

    #[tokio::test]
    async fn budget_readback_is_admin_only_no_store_and_never_contains_reservations() {
        let (_directory, mut state) = test_state();
        Arc::get_mut(&mut state)
            .unwrap()
            .clients
            .push(ServiceClient {
                digest: Sha256::digest(b"catalog-test-token-00000000000000").into(),
                scopes: BTreeSet::from(["ai:catalog".into()]),
                machine_subject: None,
            });
        let before = serde_json::to_vec(&state.vault.lock().await.state().budget).unwrap();
        let app = router(state.clone());
        for (token, expected) in [
            (None, StatusCode::UNAUTHORIZED),
            (
                Some("catalog-test-token-00000000000000"),
                StatusCode::FORBIDDEN,
            ),
            (
                Some("agent-test-token-0000000000000000"),
                StatusCode::FORBIDDEN,
            ),
            (Some("admin-test-token-0000000000000000"), StatusCode::OK),
        ] {
            let mut request = Request::builder().uri("/internal/v1/budget");
            if let Some(token) = token {
                request = request.header("authorization", format!("Bearer {token}"));
            }
            let response = app
                .clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
            if expected == StatusCode::OK {
                assert_eq!(response.headers()["cache-control"], "no-store");
                let value: Value =
                    serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap())
                        .unwrap();
                assert_eq!(value["available_microdollars"], "30000000");
                for forbidden in [
                    "reservations",
                    "credential",
                    "operation_id",
                    "estimate",
                    "messages",
                ] {
                    assert!(value.get(forbidden).is_none());
                }
            }
        }
        assert_eq!(
            serde_json::to_vec(&state.vault.lock().await.state().budget).unwrap(),
            before
        );
    }

    #[tokio::test]
    async fn signed_readback_is_scoped_secret_free_and_cancellation_is_durable_without_io() {
        use crate::execution_grant::fixtures;
        let (_directory, state) = test_state();
        let (seed, claims, id) = crate::inference_journal::tests::prepared_fixture_state();
        state.vault.lock().await.commit(seed).unwrap();
        let app = router(state.clone());
        let signed = serde_json::to_value(fixtures::envelope(&claims)).unwrap();
        let request = |route: &str, id: Uuid, token: Option<&str>, body: Value| {
            let mut builder = Request::builder()
                .method("POST")
                .uri(format!("/internal/v1/inference/{id}/{route}"))
                .header("content-type", "application/json");
            if let Some(token) = token {
                builder = builder.header("authorization", format!("Bearer {token}"));
            }
            builder.body(Body::from(body.to_string())).unwrap()
        };
        let agent = Some("agent-test-token-0000000000000000");
        let response = app
            .clone()
            .oneshot(request("status", id, agent, signed.clone()))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(text.contains("prepared"));
        assert!(
            !text.contains("Private required evidence")
                && !text.contains("credential")
                && !text.contains("signature")
        );
        for (token, code) in [
            (None, StatusCode::UNAUTHORIZED),
            (
                Some("admin-test-token-0000000000000000"),
                StatusCode::FORBIDDEN,
            ),
        ] {
            assert_eq!(
                app.clone()
                    .oneshot(request("status", id, token, signed.clone()))
                    .await
                    .unwrap()
                    .status(),
                code
            );
        }
        for variation in 0..4 {
            let mut foreign = claims.clone();
            match variation {
                0 => foreign.execution.owner_subject = "other-owner".into(),
                1 => foreign.execution.project_id = Uuid::new_v4(),
                2 => foreign.machine_subject = "sdlc2:hermes:other-agent".into(),
                _ => {
                    foreign.issued_at -= chrono::Duration::minutes(1);
                    foreign.expires_at -= chrono::Duration::minutes(1);
                    foreign.lease_expires_at -= chrono::Duration::minutes(1);
                }
            }
            let body = serde_json::to_value(fixtures::envelope(&foreign)).unwrap();
            assert_eq!(
                app.clone()
                    .oneshot(request("status", id, agent, body))
                    .await
                    .unwrap()
                    .status(),
                StatusCode::FORBIDDEN
            );
        }
        let mut forged = signed.clone();
        forged["signature_hex"] = json!("00".repeat(64));
        assert_eq!(
            app.clone()
                .oneshot(request("cancel", id, agent, forged))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        let foreign = app
            .clone()
            .oneshot(request("status", Uuid::new_v4(), agent, signed.clone()))
            .await
            .unwrap();
        let mut wrong = claims.clone();
        wrong.execution.agent_id = Uuid::new_v4();
        let denied = app
            .clone()
            .oneshot(request(
                "status",
                id,
                agent,
                serde_json::to_value(fixtures::envelope(&wrong)).unwrap(),
            ))
            .await
            .unwrap();
        assert_eq!(foreign.status(), denied.status());
        assert_eq!(
            to_bytes(foreign.into_body(), 65536).await.unwrap(),
            to_bytes(denied.into_body(), 65536).await.unwrap()
        );
        let response = app
            .clone()
            .oneshot(request(
                "events",
                id,
                agent,
                json!({"grant":signed,"from_sequence":1,"limit":1}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let page: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
        assert_eq!(page["events"][0]["kind"]["type"], "prepared");
        assert_eq!(page["next_sequence"], 2);
        assert_eq!(
            app.clone()
                .oneshot(request(
                    "events",
                    id,
                    agent,
                    json!({"grant":signed,"from_sequence":0,"limit":201})
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        for _ in 0..2 {
            let response = app
                .clone()
                .oneshot(request("cancel", id, agent, signed.clone()))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let body: Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap())
                    .unwrap();
            assert_eq!(body["state"], "cancelled");
        }
        let vault = state.vault.lock().await;
        assert!(vault.state().budget.reservations.is_empty());
        assert_eq!(vault.state().inference_runs[&id].events.len(), 2);
        assert!(!state.external_calls_enabled);
    }

    #[test]
    fn service_credentials_reject_inference_identity_escalation_and_duplicate_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("clients.json");
        let write = |value: Value| {
            std::fs::write(&file, value.to_string()).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
            }
        };
        let base = json!({"token":"fixture-service-token-0000000000000000","scopes":["ai:infer"],"machine_subject":"sdlc2:hermes:developer"});
        write(json!([base]));
        assert!(ServiceClient::load(&file).unwrap()[0].uses_inference());
        for variant in 0..5 {
            let mut invalid = base.clone();
            match variant {
                0 => {
                    invalid.as_object_mut().unwrap().remove("machine_subject");
                }
                1 => invalid["machine_subject"] = json!("sdlc1:hermes:developer"),
                2 => invalid["scopes"] = json!(["ai:infer", "ai:admin"]),
                3 => invalid["scopes"] = json!(["ai:admin"]),
                _ => invalid["extra"] = json!(true),
            }
            write(json!([invalid]));
            assert!(matches!(
                ServiceClient::load(&file),
                Err(RuntimeError::Configuration)
            ));
        }
        write(json!([base, base]));
        assert!(matches!(
            ServiceClient::load(&file),
            Err(RuntimeError::Configuration)
        ));
        write(json!([{"token":"fixture-service-token-0000000000000000","scopes":["ai:admin"]}]));
        assert!(!ServiceClient::load(&file).unwrap()[0].uses_inference());
    }

    #[tokio::test]
    async fn profile_registration_requires_admin_runtime_proof_and_restore_mode_is_read_only() {
        use admin_panel_domain::ai::{
            ModelCapabilities, ProviderSettings, VerificationEvidence, publish_profile,
        };
        let (_dir, mut state) = test_state();
        Arc::get_mut(&mut state).unwrap().external_calls_enabled = true;
        let now = chrono::Utc::now();
        let generation = Uuid::new_v4();
        let evidence = VerificationEvidence {
            id: Uuid::new_v4(),
            settings: ProviderSettings {
                provider: ProviderId::Openrouter,
                model: "deepseek/deepseek-v4.1-flash".into(),
                context_window_tokens: 256000,
            },
            credential_generation: generation,
            capabilities: ModelCapabilities {
                model: "deepseek/deepseek-v4.1-flash".into(),
                context_limit_tokens: 1048576,
                max_output_tokens: 65536,
                tools: true,
                structured_output: true,
                streaming: true,
                cancellation: true,
            },
            verified_at: now,
            expires_at: now + chrono::Duration::minutes(15),
        };
        let profile = publish_profile(
            "sdlc2",
            0,
            0,
            &evidence.settings,
            generation,
            &evidence,
            now,
        )
        .unwrap();
        let (adapter, accounting) = crate::publication::deployed_contract(ProviderId::Openrouter);
        let registration = RevisionRegistration {
            draft_revision: 1,
            schema_version: 1,
            operation_id: Uuid::new_v4(),
            profile,
            credential_generation: generation,
            adapter_version: adapter.into(),
            accounting_policy: accounting.into(),
        };
        let body = serde_json::to_string(&registration).unwrap();
        let request = |token: Option<&str>, body: &str| {
            let mut request = Request::builder()
                .method("POST")
                .uri("/internal/v1/profiles/registrations")
                .header("content-type", "application/json");
            if let Some(token) = token {
                request = request.header("authorization", format!("Bearer {token}"));
            }
            request.body(Body::from(body.to_owned())).unwrap()
        };
        let app = router(state.clone());
        let proof_readback = |token: &str, provider: &str| {
            Request::builder()
                .uri(format!(
                    "/internal/v1/providers/{provider}/verifications/{}",
                    registration.profile.verification_id
                ))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap()
        };
        assert_eq!(
            app.clone()
                .oneshot(proof_readback(
                    "agent-test-token-0000000000000000",
                    "openrouter"
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        let missing = app
            .clone()
            .oneshot(proof_readback(
                "admin-test-token-0000000000000000",
                "openrouter",
            ))
            .await
            .unwrap();
        let missing: VerificationStatus =
            serde_json::from_slice(&to_bytes(missing.into_body(), 65536).await.unwrap()).unwrap();
        assert!(missing.verified_adapter.is_none());
        assert_eq!(
            app.clone()
                .oneshot(request(None, &body))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            app.clone()
                .oneshot(request(Some("agent-test-token-0000000000000000"), &body))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        let admin = "admin-test-token-0000000000000000";
        assert_eq!(
            app.clone()
                .oneshot(request(Some(admin), &body))
                .await
                .unwrap()
                .status(),
            StatusCode::FAILED_DEPENDENCY
        );
        {
            let mut vault = state.vault.lock().await;
            let mut next = vault.state().clone();
            next.connections.insert(
                "openrouter".into(),
                Connection {
                    provider: ProviderId::Openrouter,
                    generation,
                    credential: "fixture-private-do-not-return".into(),
                },
            );
            next.verified_adapters.insert(
                evidence.id,
                crate::publication::VerifiedAdapterEvidence {
                    draft_revision: Some(1),
                    evidence,
                    adapter_version: adapter.into(),
                    accounting_policy: accounting.into(),
                },
            );
            vault.commit(next).unwrap();
        }
        let proof = app
            .clone()
            .oneshot(proof_readback(admin, "openrouter"))
            .await
            .unwrap();
        let proof_bytes = to_bytes(proof.into_body(), 65536).await.unwrap();
        assert!(!String::from_utf8_lossy(&proof_bytes).contains("fixture-private-do-not-return"));
        let proof: VerificationStatus = serde_json::from_slice(&proof_bytes).unwrap();
        assert_eq!(proof.verification_id, registration.profile.verification_id);
        assert_eq!(
            proof
                .verified_adapter
                .unwrap()
                .evidence
                .credential_generation,
            generation
        );
        let foreign = app
            .clone()
            .oneshot(proof_readback(admin, "chatgpt"))
            .await
            .unwrap();
        let foreign: VerificationStatus =
            serde_json::from_slice(&to_bytes(foreign.into_body(), 65536).await.unwrap()).unwrap();
        assert!(foreign.verified_adapter.is_none());
        let first = app
            .clone()
            .oneshot(request(Some(admin), &body))
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::OK);
        let bytes = to_bytes(first.into_body(), 65536).await.unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("fixture-private-do-not-return"));
        let receipt: RegisteredRevision = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(receipt.registration, registration);
        let replay = app
            .clone()
            .oneshot(request(Some(admin), &body))
            .await
            .unwrap();
        assert_eq!(replay.status(), StatusCode::OK);
        assert_eq!(to_bytes(replay.into_body(), 65536).await.unwrap(), bytes);
        let readback = |token: &str| {
            Request::builder()
                .uri(format!(
                    "/internal/v1/profiles/registrations/{}",
                    registration.operation_id
                ))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap()
        };
        assert_eq!(
            app.clone()
                .oneshot(readback("agent-test-token-0000000000000000"))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        let status = app.clone().oneshot(readback(admin)).await.unwrap();
        let status: RegistrationStatus =
            serde_json::from_slice(&to_bytes(status.into_body(), 65536).await.unwrap()).unwrap();
        assert_eq!(status.receipt.unwrap().registration, registration);
        let mut altered = registration.clone();
        altered.profile.context_window_tokens = 128000;
        assert_eq!(
            app.clone()
                .oneshot(request(
                    Some(admin),
                    &serde_json::to_string(&altered).unwrap()
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::CONFLICT
        );
        altered.operation_id = Uuid::new_v4();
        altered.profile.workspace = "sdlc1".into();
        assert_eq!(
            app.oneshot(request(
                Some(admin),
                &serde_json::to_string(&altered).unwrap()
            ))
            .await
            .unwrap()
            .status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        drop(state);
        // A restored service can read receipts but cannot register even a verified profile.
        let (_restore_dir, restored) = test_state();
        assert_eq!(
            router(restored)
                .oneshot(request(Some(admin), &body))
                .await
                .unwrap()
                .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }

    #[tokio::test]
    async fn disconnect_replay_does_not_revoke_a_new_connection_and_readback_is_secret_free() {
        let (_dir, state) = test_state();
        let app = router(state.clone());
        let operation = Uuid::new_v4();
        let request = || {
            Request::builder()
                .method("DELETE")
                .uri("/internal/v1/providers/openrouter/connection")
                .header("authorization", "Bearer admin-test-token-0000000000000000")
                .header("idempotency-key", operation.to_string())
                .body(Body::empty())
                .unwrap()
        };
        assert_eq!(
            app.clone().oneshot(request()).await.unwrap().status(),
            StatusCode::NO_CONTENT
        );
        let generation = Uuid::new_v4();
        {
            let mut vault = state.vault.lock().await;
            let mut next = vault.state().clone();
            next.connections.insert(
                "openrouter".into(),
                Connection {
                    provider: ProviderId::Openrouter,
                    generation,
                    credential: "new-fictional-private-credential".into(),
                },
            );
            vault.commit(next).unwrap();
        }
        assert_eq!(
            app.clone().oneshot(request()).await.unwrap().status(),
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            state.vault.lock().await.state().connections["openrouter"].generation,
            generation
        );
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/internal/v1/providers/openrouter/operations/{operation}"
                    ))
                    .header("authorization", "Bearer admin-test-token-0000000000000000")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(text.contains("completed"));
        assert!(!text.contains("credential") && !text.contains("fingerprint"));
    }

    #[tokio::test]
    async fn expired_native_completion_cannot_persist_auth_or_resurrect_login() {
        let (_dir, state) = test_state();
        let operation = Uuid::new_v4();
        let native = Uuid::new_v4();
        {
            let mut vault = state.vault.lock().await;
            let mut next = vault.state().clone();
            next.logins.insert(
                operation,
                ManagedLogin {
                    operation_id: operation,
                    login_id: Some(native),
                    status: "pending".into(),
                    user_code: Some("fictional-device-code".into()),
                    verification_url: Some("https://auth.openai.com/codex/device".into()),
                    expires_at: chrono::Utc::now() - chrono::Duration::seconds(1),
                },
            );
            vault.commit(next).unwrap();
        }
        checkpoint_login(&state, Some(native), true).await.unwrap();
        let vault = state.vault.lock().await;
        assert!(!vault.state().connections.contains_key("chatgpt"));
        let login = &vault.state().logins[&operation];
        assert_eq!(login.status, "uncertain");
        assert!(login.user_code.is_none() && login.verification_url.is_none());
    }

    #[tokio::test]
    async fn write_only_credentials_replay_conflict_scope_and_restore_mode() {
        let (_dir, state) = test_state();
        let app = router(state.clone());
        let id = Uuid::new_v4();
        let body = json!({"operation_id":id,"credential":"fictional-secret-000000000"}).to_string();
        let request = |token: &str, body: &str| {
            Request::builder()
                .method("PUT")
                .uri("/internal/v1/providers/openrouter/connection")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_owned()))
                .unwrap()
        };
        assert_eq!(
            app.clone()
                .oneshot(request("agent-test-token-0000000000000000", &body))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            app.clone()
                .oneshot(request("foreign-workspace-token-000000000", &body))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let first = app
            .clone()
            .oneshot(request("admin-test-token-0000000000000000", &body))
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::OK);
        let first_body = to_bytes(first.into_body(), 65536).await.unwrap();
        assert!(!String::from_utf8_lossy(&first_body).contains("fictional-secret"));
        let replay = app
            .clone()
            .oneshot(request("admin-test-token-0000000000000000", &body))
            .await
            .unwrap();
        assert_eq!(
            to_bytes(replay.into_body(), 65536).await.unwrap(),
            first_body
        );
        let changed =
            json!({"operation_id":id,"credential":"different-secret-000000000"}).to_string();
        assert_eq!(
            app.clone()
                .oneshot(request("admin-test-token-0000000000000000", &changed))
                .await
                .unwrap()
                .status(),
            StatusCode::CONFLICT
        );
        let models = Request::builder()
            .uri("/internal/v1/providers/openrouter/models")
            .header("authorization", "Bearer admin-test-token-0000000000000000")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            app.oneshot(models).await.unwrap().status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }
}
