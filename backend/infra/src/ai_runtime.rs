//! Explicit internal contract; no arbitrary proxy, redirects or retries.
use admin_panel_domain::ai::{
    ProviderId, RegisteredRevision, RegistrationStatus, RevisionRegistration, VerificationStatus,
};
use chrono::{DateTime, Utc};
use reqwest::{Client, Method, Url};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{path::Path, time::Duration};
use uuid::Uuid;
use zeroize::Zeroizing;

const ENDPOINTS: [&str; 2] = ["http://ai-backend:8760/", "http://ai-runtime:8760/"];
const MAX_BODY: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("ai_runtime_configuration_invalid")]
    Configuration,
    #[error("ai_runtime_unavailable")]
    Unavailable,
    #[error("ai_runtime_protocol_error")]
    Protocol,
    #[error("ai_runtime_operation_conflict")]
    Conflict,
    #[error("ai_provider_not_connected")]
    Disconnected,
    #[error("ai_provider_quota_exceeded")]
    Quota,
    #[error("ai_runtime_request_rejected")]
    Rejected,
}

// Deliberately no Debug implementation.
pub struct RuntimeClient {
    client: Client,
    endpoint: Url,
    token: Zeroizing<String>,
}

#[derive(Deserialize, Serialize)]
pub struct ProviderStatus {
    pub id: ProviderId,
    pub connected: bool,
    pub generation: Option<Uuid>,
    pub pending_login_operation: Option<Uuid>,
    pub runtime_available: Option<bool>,
    pub authorization_checkpoint_ready: Option<bool>,
    pub capabilities_verified: bool,
}

#[derive(Deserialize, Serialize)]
pub struct Registry {
    pub schema_version: u32,
    pub workspace: String,
    pub providers: Vec<ProviderStatus>,
}

#[derive(Deserialize, Serialize)]
pub struct Model {
    pub id: String,
    pub model: Option<String>,
    pub name: String,
    pub context_limit_tokens: Option<u32>,
    pub max_output_tokens: Option<u32>,
    pub access_verified: bool,
}

#[derive(Deserialize, Serialize)]
pub struct Catalog {
    pub schema_version: u32,
    pub provider: ProviderId,
    pub source: String,
    pub access_verified: bool,
    pub models: Vec<Model>,
}

#[derive(Deserialize, Serialize)]
pub struct Account {
    pub schema_version: u32,
    pub connected: bool,
    pub plan_type: Option<String>,
    pub capabilities_verified: bool,
}

#[derive(Deserialize, Serialize)]
pub struct Login {
    pub schema_version: u32,
    pub operation_id: Uuid,
    pub login_id: Option<Uuid>,
    pub status: String,
    pub user_code: Option<String>,
    pub verification_url: Option<String>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Deserialize, Serialize)]
pub struct Connection {
    pub schema_version: u32,
    pub provider: ProviderId,
    pub generation: Uuid,
}

#[derive(Deserialize, Serialize)]
pub struct ConnectionOperation {
    pub schema_version: u32,
    pub operation_id: Uuid,
    pub provider: ProviderId,
    pub kind: String,
    pub status: String,
    pub generation: Option<Uuid>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialInput {
    pub operation_id: Uuid,
    pub credential: Zeroizing<String>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum RequestBody<'a> {
    Credentials(&'a CredentialInput),
    Registration(&'a RevisionRegistration),
}

impl RuntimeClient {
    pub async fn acceptance_budget(
        &self,
    ) -> Result<admin_panel_domain::ai::AcceptanceBudget, BridgeError> {
        let budget: admin_panel_domain::ai::AcceptanceBudget = self
            .call(Method::GET, "internal/v1/budget", None, None)
            .await?
            .ok_or(BridgeError::Protocol)?;
        if !budget.valid_for("sdlc2") {
            return Err(BridgeError::Protocol);
        }
        Ok(budget)
    }

    pub fn from_deployment(endpoint: &str, token_file: &Path) -> Result<Self, BridgeError> {
        if !ENDPOINTS.contains(&endpoint) || !token_file.is_absolute() {
            return Err(BridgeError::Configuration);
        }
        let mut current = std::path::PathBuf::new();
        for part in token_file.components() {
            if matches!(part, std::path::Component::ParentDir) {
                return Err(BridgeError::Configuration);
            }
            current.push(part);
            let metadata =
                std::fs::symlink_metadata(&current).map_err(|_| BridgeError::Configuration)?;
            if metadata.file_type().is_symlink() {
                return Err(BridgeError::Configuration);
            }
        }
        let metadata = std::fs::metadata(token_file).map_err(|_| BridgeError::Configuration)?;
        if !metadata.is_file() || metadata.len() > 256 {
            return Err(BridgeError::Configuration);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o077 != 0 {
                return Err(BridgeError::Configuration);
            }
        }
        let token = Zeroizing::new(
            std::fs::read_to_string(token_file).map_err(|_| BridgeError::Configuration)?,
        );
        if token.len() < 32 || token.bytes().any(|b| !b.is_ascii_graphic()) {
            return Err(BridgeError::Configuration);
        }
        Self::new(endpoint, token)
    }

    fn new(endpoint: &str, token: Zeroizing<String>) -> Result<Self, BridgeError> {
        let client = Client::builder()
            .no_proxy()
            .retry(reqwest::retry::never())
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(55))
            .build()
            .map_err(|_| BridgeError::Configuration)?;
        Ok(Self {
            client,
            endpoint: Url::parse(endpoint).map_err(|_| BridgeError::Configuration)?,
            token,
        })
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<RequestBody<'_>>,
        operation: Option<Uuid>,
    ) -> Result<Option<T>, BridgeError> {
        let url = self
            .endpoint
            .join(path)
            .map_err(|_| BridgeError::Configuration)?;
        let mut request = self
            .client
            .request(method, url)
            .bearer_auth(self.token.as_str());
        if let Some(body) = body {
            request = request.json(&body);
        }
        if let Some(operation) = operation {
            request = request.header("idempotency-key", operation.to_string());
        }
        let mut response = request.send().await.map_err(|_| BridgeError::Unavailable)?;
        match response.status().as_u16() {
            204 => return Ok(None),
            200 => {}
            409 => return Err(BridgeError::Conflict),
            422 => return Err(BridgeError::Rejected),
            424 => return Err(BridgeError::Disconnected),
            429 => return Err(BridgeError::Quota),
            503 => return Err(BridgeError::Unavailable),
            _ => return Err(BridgeError::Protocol),
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| BridgeError::Protocol)? {
            if bytes.len() + chunk.len() > MAX_BODY {
                return Err(BridgeError::Protocol);
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| BridgeError::Protocol)
    }

    pub async fn registry(&self) -> Result<Registry, BridgeError> {
        let registry: Registry = self
            .call(Method::GET, "internal/v1/providers", None, None)
            .await?
            .ok_or(BridgeError::Protocol)?;
        if registry.schema_version != 1
            || registry.workspace != "sdlc2"
            || registry.providers.len() != 2
        {
            return Err(BridgeError::Protocol);
        }
        Ok(registry)
    }

    pub async fn models(&self, provider: ProviderId) -> Result<Catalog, BridgeError> {
        let path = format!(
            "internal/v1/providers/{}/models",
            crate::ai::provider_key(provider)
        );
        let mut catalog: Catalog = self
            .call(Method::GET, &path, None, None)
            .await?
            .ok_or(BridgeError::Protocol)?;
        if catalog.schema_version != 1
            || catalog.provider != provider
            || catalog.models.len() > 8192
            || catalog
                .models
                .iter()
                .any(|m| m.id.is_empty() || m.id.len() > 256 || m.name.len() > 512)
        {
            return Err(BridgeError::Protocol);
        }
        catalog.access_verified = false;
        for model in &mut catalog.models {
            model.access_verified = false;
        }
        Ok(catalog)
    }

    pub async fn account(&self) -> Result<Account, BridgeError> {
        let account: Account = self
            .call(
                Method::GET,
                "internal/v1/providers/chatgpt/account",
                None,
                None,
            )
            .await?
            .ok_or(BridgeError::Protocol)?;
        if account.schema_version != 1 || account.plan_type.as_ref().is_some_and(|p| p.len() > 128)
        {
            return Err(BridgeError::Protocol);
        }
        Ok(account)
    }

    pub async fn credentials(&self, input: &CredentialInput) -> Result<Connection, BridgeError> {
        let connection: Connection = self
            .call(
                Method::PUT,
                "internal/v1/providers/openrouter/connection",
                Some(RequestBody::Credentials(input)),
                None,
            )
            .await?
            .ok_or(BridgeError::Protocol)?;
        if connection.schema_version != 1 || connection.provider != ProviderId::Openrouter {
            return Err(BridgeError::Protocol);
        }
        Ok(connection)
    }

    /// Registration is idempotent and performs no inference. Unknown outcomes require readback.
    pub async fn register_profile(
        &self,
        input: &RevisionRegistration,
    ) -> Result<RegisteredRevision, BridgeError> {
        let receipt: RegisteredRevision = self
            .call(
                Method::POST,
                "internal/v1/profiles/registrations",
                Some(RequestBody::Registration(input)),
                None,
            )
            .await?
            .ok_or(BridgeError::Protocol)?;
        if receipt.registration != *input
            || receipt.registered_at > Utc::now() + chrono::Duration::seconds(30)
        {
            return Err(BridgeError::Protocol);
        }
        Ok(receipt)
    }

    pub async fn registration_status(
        &self,
        operation: Uuid,
    ) -> Result<RegistrationStatus, BridgeError> {
        let path = format!("internal/v1/profiles/registrations/{operation}");
        let status: RegistrationStatus = self
            .call(Method::GET, &path, None, None)
            .await?
            .ok_or(BridgeError::Protocol)?;
        validate_registration_status(&status, operation)?;
        Ok(status)
    }

    pub async fn verification_status(
        &self,
        provider: ProviderId,
        verification: Uuid,
    ) -> Result<VerificationStatus, BridgeError> {
        if verification.is_nil() {
            return Err(BridgeError::Protocol);
        }
        let path = format!(
            "internal/v1/providers/{}/verifications/{verification}",
            crate::ai::provider_key(provider)
        );
        let status: VerificationStatus = self
            .call(Method::GET, &path, None, None)
            .await?
            .ok_or(BridgeError::Protocol)?;
        validate_verification_status(&status, provider, verification)?;
        Ok(status)
    }

    pub async fn login(&self, operation: Uuid, start: bool) -> Result<Login, BridgeError> {
        let path = if start {
            "internal/v1/providers/chatgpt/login".into()
        } else {
            format!("internal/v1/providers/chatgpt/login/{operation}")
        };
        let login: Login = self
            .call(
                if start { Method::POST } else { Method::GET },
                &path,
                None,
                Some(operation),
            )
            .await?
            .ok_or(BridgeError::Protocol)?;
        if login.schema_version != 1
            || login.operation_id != operation
            || login
                .user_code
                .as_ref()
                .is_some_and(|code| code.is_empty() || code.len() > 128)
            || login
                .verification_url
                .as_deref()
                .is_some_and(|url| url != "https://auth.openai.com/codex/device")
        {
            return Err(BridgeError::Protocol);
        }
        Ok(login)
    }

    pub async fn cancel_login(&self, operation: Uuid) -> Result<(), BridgeError> {
        self.delete(&format!("internal/v1/providers/chatgpt/login/{operation}"))
            .await
    }

    pub async fn disconnect(
        &self,
        provider: ProviderId,
        operation: Uuid,
    ) -> Result<(), BridgeError> {
        let path = format!(
            "internal/v1/providers/{}/connection",
            crate::ai::provider_key(provider)
        );
        match self
            .call::<serde_json::Value>(Method::DELETE, &path, None, Some(operation))
            .await?
        {
            None => Ok(()),
            Some(_) => Err(BridgeError::Protocol),
        }
    }

    pub async fn connection_operation(
        &self,
        provider: ProviderId,
        operation: Uuid,
    ) -> Result<ConnectionOperation, BridgeError> {
        let path = format!(
            "internal/v1/providers/{}/operations/{operation}",
            crate::ai::provider_key(provider)
        );
        let response: ConnectionOperation = self
            .call(Method::GET, &path, None, None)
            .await?
            .ok_or(BridgeError::Protocol)?;
        if response.schema_version != 1
            || response.operation_id != operation
            || response.provider != provider
            || !matches!(response.kind.as_str(), "credentials" | "disconnect")
            || !matches!(
                response.status.as_str(),
                "pending" | "uncertain" | "completed"
            )
        {
            return Err(BridgeError::Protocol);
        }
        Ok(response)
    }

    async fn delete(&self, path: &str) -> Result<(), BridgeError> {
        match self
            .call::<serde_json::Value>(Method::DELETE, path, None, None)
            .await?
        {
            None => Ok(()),
            Some(_) => Err(BridgeError::Protocol),
        }
    }
}

fn validate_verification_status(
    status: &VerificationStatus,
    provider: ProviderId,
    verification: Uuid,
) -> Result<(), BridgeError> {
    if status.schema_version != 1
        || status.workspace != "sdlc2"
        || status.provider != provider
        || status.verification_id != verification
    {
        return Err(BridgeError::Protocol);
    }
    if let Some(proof) = &status.verified_adapter {
        let evidence = &proof.evidence;
        if evidence.id != verification
            || evidence.credential_generation.is_nil()
            || proof.draft_revision.is_none_or(|revision| revision <= 0)
            || evidence.settings.provider != provider
            || evidence.expires_at <= evidence.verified_at
            || evidence.expires_at - evidence.verified_at > chrono::Duration::minutes(15)
            || evidence.verified_at > Utc::now() + chrono::Duration::seconds(30)
            || evidence.settings.validate(&evidence.capabilities).is_err()
            || [&proof.adapter_version, &proof.accounting_policy]
                .iter()
                .any(|v| v.is_empty() || v.len() > 256 || v.chars().any(char::is_control))
        {
            return Err(BridgeError::Protocol);
        }
    }
    Ok(())
}

fn validate_registration_status(
    status: &RegistrationStatus,
    operation: Uuid,
) -> Result<(), BridgeError> {
    if operation.is_nil()
        || status.schema_version != 1
        || status.workspace != "sdlc2"
        || status.operation_id != operation
    {
        return Err(BridgeError::Protocol);
    }
    if let Some(receipt) = &status.receipt {
        let registration = &receipt.registration;
        if registration.schema_version != 1
            || registration.operation_id != operation
            || registration.profile.workspace != "sdlc2"
            || registration.profile.schema_version != 1
            || registration.profile.revision == 0
            || registration.draft_revision <= 0
            || registration.credential_generation.is_nil()
            || receipt.registered_at > Utc::now() + chrono::Duration::seconds(30)
        {
            return Err(BridgeError::Protocol);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn verification_readback_rejects_foreign_proof_and_unbounded_evidence() {
        use admin_panel_domain::ai::{
            ModelCapabilities, ProviderSettings, VerificationEvidence, VerifiedAdapterEvidence,
        };
        let id = Uuid::new_v4();
        let settings = ProviderSettings {
            provider: ProviderId::Openrouter,
            model: "deepseek/deepseek-v4.1-flash".into(),
            context_window_tokens: 256000,
        };
        let now = Utc::now();
        let proof = VerifiedAdapterEvidence {
            draft_revision: Some(2),
            evidence: VerificationEvidence {
                id,
                settings: settings.clone(),
                credential_generation: Uuid::new_v4(),
                capabilities: ModelCapabilities {
                    model: settings.model,
                    context_limit_tokens: 1048576,
                    max_output_tokens: 65536,
                    tools: true,
                    structured_output: true,
                    streaming: true,
                    cancellation: true,
                },
                verified_at: now,
                expires_at: now + chrono::Duration::minutes(15),
            },
            adapter_version: "fixture-v1".into(),
            accounting_policy: "fixture-v1".into(),
        };
        let mut status = VerificationStatus {
            schema_version: 1,
            workspace: "sdlc2".into(),
            provider: ProviderId::Openrouter,
            verification_id: id,
            verified_adapter: Some(proof),
        };
        assert!(validate_verification_status(&status, ProviderId::Openrouter, id).is_ok());
        assert!(validate_verification_status(&status, ProviderId::Chatgpt, id).is_err());
        assert!(
            validate_verification_status(&status, ProviderId::Openrouter, Uuid::new_v4()).is_err()
        );
        status.workspace = "sdlc1".into();
        assert!(validate_verification_status(&status, ProviderId::Openrouter, id).is_err());
        status.workspace = "sdlc2".into();
        status.verified_adapter.as_mut().unwrap().draft_revision = None;
        assert!(validate_verification_status(&status, ProviderId::Openrouter, id).is_err());
        status.verified_adapter.as_mut().unwrap().draft_revision = Some(2);
        status
            .verified_adapter
            .as_mut()
            .unwrap()
            .evidence
            .expires_at = now + chrono::Duration::minutes(16);
        assert!(validate_verification_status(&status, ProviderId::Openrouter, id).is_err());
    }

    #[tokio::test]
    async fn lost_registration_ack_is_read_back_without_automatic_post_replay() {
        use admin_panel_domain::ai::RuntimeProfile;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let request = RevisionRegistration {
            schema_version: 1,
            operation_id: Uuid::new_v4(),
            draft_revision: 2,
            profile: RuntimeProfile {
                schema_version: 1,
                workspace: "sdlc2".into(),
                revision: 3,
                provider: ProviderId::Openrouter,
                model: "deepseek/deepseek-v4.1-flash".into(),
                context_window_tokens: 256000,
                verification_id: Uuid::new_v4(),
            },
            credential_generation: Uuid::new_v4(),
            adapter_version: "fixture-v1".into(),
            accounting_policy: "fixture-v1".into(),
        };
        let registered = request.clone();
        let server = tokio::spawn(async move {
            for index in 0..2 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let header_end = loop {
                    let mut chunk = [0u8; 1024];
                    let count = stream.read(&mut chunk).await.unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&chunk[..count]);
                    assert!(bytes.len() < 65536);
                    if let Some(position) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                        break position + 4;
                    }
                };
                let headers = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
                assert!(headers.contains("Bearer fixture-token-00000000000000000000"));
                if index == 0 {
                    assert!(
                        headers.starts_with("POST /internal/v1/profiles/registrations HTTP/1.1")
                    );
                    let length: usize = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .map(str::to_owned)
                        })
                        .unwrap()
                        .parse()
                        .unwrap();
                    while bytes.len() < header_end + length {
                        let mut chunk = [0u8; 1024];
                        let count = stream.read(&mut chunk).await.unwrap();
                        assert!(count > 0);
                        bytes.extend_from_slice(&chunk[..count]);
                        assert!(bytes.len() < 65536);
                    }
                    let received: RevisionRegistration =
                        serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap();
                    assert_eq!(received, registered);
                    // Runtime committed the registration, but its HTTP ACK was lost.
                } else {
                    assert!(headers.starts_with(&format!(
                        "GET /internal/v1/profiles/registrations/{} HTTP/1.1",
                        registered.operation_id
                    )));
                    let status = RegistrationStatus {
                        schema_version: 1,
                        workspace: "sdlc2".into(),
                        operation_id: registered.operation_id,
                        receipt: Some(RegisteredRevision {
                            registration: registered.clone(),
                            registered_at: Utc::now(),
                        }),
                    };
                    let body = serde_json::to_string(&status).unwrap();
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    stream.write_all(response.as_bytes()).await.unwrap();
                }
            }
        });
        let client = RuntimeClient::new(
            &endpoint,
            Zeroizing::new("fixture-token-00000000000000000000".into()),
        )
        .unwrap();
        let lost = tokio::time::timeout(Duration::from_secs(3), client.register_profile(&request))
            .await
            .unwrap();
        assert!(matches!(lost, Err(BridgeError::Unavailable)));
        let status = tokio::time::timeout(
            Duration::from_secs(3),
            client.registration_status(request.operation_id),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(status.receipt.unwrap().registration, request);
        server.await.unwrap();
    }
    #[test]
    fn registration_readback_is_bound_to_the_exact_workspace_and_operation() {
        let operation = Uuid::new_v4();
        let mut status = RegistrationStatus {
            schema_version: 1,
            workspace: "sdlc2".into(),
            operation_id: operation,
            receipt: None,
        };
        assert!(validate_registration_status(&status, operation).is_ok());
        assert!(validate_registration_status(&status, Uuid::new_v4()).is_err());
        status.workspace = "sdlc1".into();
        assert!(validate_registration_status(&status, operation).is_err());
        status.workspace = "sdlc2".into();
        status.schema_version = 2;
        assert!(validate_registration_status(&status, operation).is_err());
        let unknown = serde_json::json!({"schema_version":1,"workspace":"sdlc2","operation_id":operation,
            "receipt":null,"credential":"foreign-secret"});
        assert!(serde_json::from_value::<RegistrationStatus>(unknown).is_err());
    }
    #[test]
    fn deployment_endpoint_is_not_browser_configurable() {
        for endpoint in [
            "http://localhost:4000/",
            "http://sdlc1-ai:8760/",
            "https://openrouter.ai/api/v1/",
            "http://ai-runtime:8760/@secret",
        ] {
            assert!(matches!(
                RuntimeClient::from_deployment(endpoint, Path::new("relative")),
                Err(BridgeError::Configuration)
            ));
        }
    }
    #[test]
    fn deployment_accepts_owned_process_names_and_rejects_foreign_origins_with_a_valid_token_file()
    {
        let file = std::env::temp_dir().join(format!("sdlc2-ai-bridge-{}", Uuid::new_v4()));
        std::fs::write(&file, "fixture-token-00000000000000000000").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let accepted = ["http://ai-backend:8760/", "http://ai-runtime:8760/"]
            .map(|endpoint| RuntimeClient::from_deployment(endpoint, &file).is_ok());
        let rejected = [
            "http://sdlc1-ai:8760/",
            "http://localhost:4000/",
            "https://openrouter.ai/api/v1/",
            "http://ai-backend:8761/",
            "http://ai-backend:8760/@secret",
            "http://ai-backend:8760/?token=fixture",
            "http://ai-backend:8760/#fragment",
            "http://foreign@ai-backend:8760/",
        ]
        .map(|endpoint| {
            matches!(
                RuntimeClient::from_deployment(endpoint, &file),
                Err(BridgeError::Configuration)
            )
        });
        std::fs::remove_file(file).unwrap();
        assert!(accepted.into_iter().all(|value| value));
        assert!(rejected.into_iter().all(|value| value));
    }
    #[test]
    fn response_projection_discards_secrets_and_catalog_is_not_evidence() {
        let source = serde_json::json!({"schema_version":1,"connected":true,"plan_type":"plus","capabilities_verified":false,"credential":"must-not-return","tokens":{"access_token":"private"}});
        let account: Account = serde_json::from_value(source).unwrap();
        let result = serde_json::to_string(&account).unwrap();
        assert!(!result.contains("must-not-return") && !result.contains("tokens"));
    }

    async fn fixture_response(status: &str, extra_headers: &str, body: &str) -> RuntimeClient {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra_headers}\r\n{body}",
            body.len()
        );
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 8192];
            let length = stream.read(&mut request).await.unwrap();
            let headers = std::str::from_utf8(&request[..length]).unwrap();
            assert!(headers.starts_with("GET /internal/v1/providers/chatgpt/account HTTP/1.1"));
            assert!(headers.contains("Bearer fixture-token-00000000000000000000"));
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        RuntimeClient::new(
            &endpoint,
            Zeroizing::new("fixture-token-00000000000000000000".into()),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn bridge_refuses_redirects_raw_errors_and_incompatible_success() {
        let redirect = fixture_response(
            "302 Found",
            "Location: http://127.0.0.1:1/foreign\r\n",
            "raw-private-error",
        )
        .await;
        assert!(matches!(
            redirect.account().await,
            Err(BridgeError::Protocol)
        ));
        let quota =
            fixture_response("429 Too Many Requests", "", "raw-provider-token-private").await;
        let error = quota.account().await.err().unwrap();
        assert!(matches!(error, BridgeError::Quota));
        assert_eq!(error.to_string(), "ai_provider_quota_exceeded");
        let incompatible = fixture_response("200 OK","",r#"{"schema_version":2,"connected":true,"plan_type":"plus","capabilities_verified":false}"#).await;
        assert!(matches!(
            incompatible.account().await,
            Err(BridgeError::Protocol)
        ));
    }
}
