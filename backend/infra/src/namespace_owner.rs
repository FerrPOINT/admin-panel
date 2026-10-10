//! Deployment-owned endpoints and credentials; never accepts a URL or bearer from UI.
use admin_panel_app::namespace::NamespaceOwner;
use admin_panel_domain::namespace::*;
use async_trait::async_trait;
use reqwest::{Client, Url};
use serde::Deserialize;
use std::{collections::HashMap, time::Duration};
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeploymentOwner {
    kind: ResourceKind,
    instance_id: Uuid,
    endpoint: String,
    token_file: std::path::PathBuf,
}
struct Target {
    instance_id: Uuid,
    endpoint: Url,
    token: Zeroizing<String>,
}
pub struct OwnerClient {
    client: Client,
    targets: HashMap<&'static str, Target>,
}
impl OwnerClient {
    pub fn from_deployment(raw: &str) -> Result<Self, &'static str> {
        let owners: Vec<DeploymentOwner> =
            serde_json::from_str(raw).map_err(|_| "invalid_namespace_owner_config")?;
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| "namespace_http_client_failed")?;
        let mut targets = HashMap::new();
        for owner in owners {
            let endpoint =
                Url::parse(&owner.endpoint).map_err(|_| "invalid_namespace_owner_endpoint")?;
            if owner.instance_id.is_nil()
                || !matches!(endpoint.scheme(), "http" | "https")
                || endpoint.host_str().is_none()
                || !endpoint.username().is_empty()
                || endpoint.password().is_some()
                || endpoint.query().is_some()
                || endpoint.fragment().is_some()
                || endpoint.path() != "/"
            {
                return Err("invalid_namespace_owner_endpoint");
            }
            let token = std::fs::read_to_string(&owner.token_file)
                .map_err(|_| "namespace_owner_credential_unavailable")?;
            let token = Zeroizing::new(token.trim().to_string());
            if token.is_empty()
                || token.contains(['\r', '\n'])
                || targets
                    .insert(
                        owner.kind.as_str(),
                        Target {
                            instance_id: owner.instance_id,
                            endpoint,
                            token,
                        },
                    )
                    .is_some()
            {
                return Err("invalid_namespace_owner_config");
            }
        }
        if targets.len() != 3 {
            return Err("three_namespace_owners_required");
        }
        Ok(Self { client, targets })
    }
    pub fn resources(&self) -> Vec<(ResourceKind, Uuid)> {
        ResourceKind::ALL
            .into_iter()
            .filter_map(|k| self.targets.get(k.as_str()).map(|t| (k, t.instance_id)))
            .collect()
    }
}
#[async_trait]
impl NamespaceOwner for OwnerClient {
    fn instances(&self) -> Vec<OwnerInstance> {
        self.resources()
            .into_iter()
            .map(|(kind, instance_id)| OwnerInstance { kind, instance_id })
            .collect()
    }
    fn accepts(&self, resource: &ResourceRef) -> bool {
        !resource.resource_id.is_nil()
            && self
                .targets
                .get(resource.kind.as_str())
                .is_some_and(|t| t.instance_id == resource.instance_id)
    }
    async fn apply(&self, command: &OwnerCommand) -> Result<OwnerReadback, &'static str> {
        if !self.accepts(&command.resource) {
            return Err("unregistered_resource_instance");
        }
        let target = &self.targets[command.resource.kind.as_str()];
        let endpoint = target
            .endpoint
            .join(&format!(
                "api/v1/namespace-resources/{}/{}",
                command.resource.kind.as_str(),
                command.resource.resource_id
            ))
            .map_err(|_| "invalid_owner_endpoint")?;
        // PUT is replayed with the original command. The owner returns its persisted readback.
        let mut response = self
            .client
            .put(endpoint)
            .bearer_auth(target.token.as_str())
            .json(command)
            .send()
            .await
            .map_err(|_| "owner_unavailable_readback_required")?;
        if !response.status().is_success() {
            return Err(if response.status().as_u16() == 409 {
                "owner_binding_conflict"
            } else {
                "owner_command_rejected"
            });
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "owner_unavailable_readback_required")?
        {
            if bytes.len() + chunk.len() > 65_536 {
                return Err("owner_readback_too_large");
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| "invalid_owner_readback")
    }
}
