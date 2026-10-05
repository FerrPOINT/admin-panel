//! Exact-model HTTPS adapter. Catalog metadata is never verification evidence.
use crate::error::RuntimeError;
use admin_panel_domain::ai::ModelCapabilities;
use futures_util::StreamExt;
use reqwest::{Client, Response, StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

const RESPONSE_LIMIT: usize = 4 * 1024 * 1024;

#[derive(Clone)]
pub struct OpenRouter {
    pub(crate) client: Client,
    pub(crate) endpoint: Url,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CatalogModel {
    pub id: String,
    pub name: String,
    pub context_limit_tokens: Option<u32>,
    pub max_output_tokens: Option<u32>,
    pub supported_parameters: Vec<String>,
    pub access_verified: bool,
}

impl CatalogModel {
    pub fn advertised_capabilities(&self) -> Result<ModelCapabilities, RuntimeError> {
        Ok(ModelCapabilities {
            model: self.id.clone(),
            context_limit_tokens: self
                .context_limit_tokens
                .ok_or(RuntimeError::CapabilityNotVerified)?,
            max_output_tokens: self
                .max_output_tokens
                .ok_or(RuntimeError::CapabilityNotVerified)?,
            tools: self.supported_parameters.iter().any(|p| p == "tools"),
            structured_output: self
                .supported_parameters
                .iter()
                .any(|p| p == "structured_outputs"),
            // These require live verification; declarations do not enable a profile.
            streaming: false,
            cancellation: false,
        })
    }
}

impl OpenRouter {
    pub fn production() -> Result<Self, RuntimeError> {
        Self::new("https://openrouter.ai/api/v1/")
    }

    fn new(endpoint: &str) -> Result<Self, RuntimeError> {
        let endpoint = Url::parse(endpoint).map_err(|_| RuntimeError::Configuration)?;
        if endpoint.as_str() != "https://openrouter.ai/api/v1/" {
            return Err(RuntimeError::Configuration);
        }
        let client = Client::builder()
            .use_rustls_tls()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|_| RuntimeError::Configuration)?;
        Ok(Self { client, endpoint })
    }

    #[cfg(test)]
    pub(crate) fn fixture(endpoint: &str) -> Self {
        let endpoint = Url::parse(endpoint).unwrap();
        assert_eq!(endpoint.scheme(), "http");
        assert_eq!(endpoint.host_str(), Some("127.0.0.1"));
        let mut adapter = Self::production().unwrap();
        adapter.endpoint = endpoint;
        adapter
    }

    pub async fn models(&self, credential: &str) -> Result<Vec<CatalogModel>, RuntimeError> {
        let response = self
            .client
            .get(
                self.endpoint
                    .join("models")
                    .map_err(|_| RuntimeError::Configuration)?,
            )
            .bearer_auth(credential)
            .send()
            .await
            .map_err(|_| RuntimeError::Unavailable)?;
        let data = bounded_json(response).await?;
        parse_models(&data)
    }
}

pub fn provider_status(status: StatusCode) -> Result<(), RuntimeError> {
    match status.as_u16() {
        200..=299 => Ok(()),
        401 | 403 => Err(RuntimeError::ProviderAuth),
        402 | 429 => Err(RuntimeError::Quota),
        400 | 404 | 422 => Err(RuntimeError::Protocol),
        _ => Err(RuntimeError::Unavailable),
    }
}

pub(crate) async fn bounded_json(response: Response) -> Result<Value, RuntimeError> {
    provider_status(response.status())?;
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| RuntimeError::Unavailable)?;
        if body.len() + chunk.len() > RESPONSE_LIMIT {
            return Err(RuntimeError::Protocol);
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| RuntimeError::Protocol)
}

pub fn parse_models(data: &Value) -> Result<Vec<CatalogModel>, RuntimeError> {
    let list = data
        .get("data")
        .and_then(Value::as_array)
        .ok_or(RuntimeError::Protocol)?;
    list.iter()
        .map(|model| {
            let id = model
                .get("id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty() && s.len() <= 256)
                .ok_or(RuntimeError::Protocol)?;
            let number = |value: &Value| {
                value
                    .as_u64()
                    .and_then(|v| u32::try_from(v).ok())
                    .filter(|v| *v > 0)
            };
            Ok(CatalogModel {
                id: id.into(),
                name: model
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(id)
                    .into(),
                context_limit_tokens: number(&model["context_length"]),
                max_output_tokens: number(&model["top_provider"]["max_completion_tokens"]),
                supported_parameters: model
                    .get("supported_parameters")
                    .and_then(Value::as_array)
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(|v| v.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default(),
                access_verified: false,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn rejects_custom_endpoints_and_never_converts_catalog_into_access() {
        assert!(matches!(
            OpenRouter::new("http://localhost:4000/v1/"),
            Err(RuntimeError::Configuration)
        ));
        let models = parse_models(&json!({"data":[{"id":"deepseek/deepseek-v4.1-flash","context_length":1048576,"top_provider":{"max_completion_tokens":65536},"supported_parameters":["tools","structured_outputs"]}]})).unwrap();
        assert!(!models[0].access_verified);
        let caps = models[0].advertised_capabilities().unwrap();
        assert!(caps.tools && caps.structured_output);
        assert!(!caps.streaming && !caps.cancellation);
        assert!(parse_models(&json!({"error":"secret"})).is_err());
    }
    #[test]
    fn quota_auth_and_transient_errors_are_distinct() {
        assert_eq!(
            provider_status(StatusCode::PAYMENT_REQUIRED),
            Err(RuntimeError::Quota)
        );
        assert_eq!(
            provider_status(StatusCode::TOO_MANY_REQUESTS),
            Err(RuntimeError::Quota)
        );
        assert_eq!(
            provider_status(StatusCode::UNAUTHORIZED),
            Err(RuntimeError::ProviderAuth)
        );
        assert_eq!(
            provider_status(StatusCode::FOUND),
            Err(RuntimeError::Unavailable)
        );
    }
}
