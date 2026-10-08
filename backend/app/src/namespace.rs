//! Durable intent precedes owner HTTP. Unknown outcomes are reconciled with the same key.
use admin_panel_domain::{DomainResult, namespace::*};
use async_trait::async_trait;
use std::sync::Arc;
use uuid::Uuid;

#[async_trait]
pub trait NamespaceRepository: Send + Sync {
    async fn list(&self, limit: i64, offset: i64) -> DomainResult<Vec<Namespace>>;
    async fn create(&self, command: &CreateNamespace, actor: &str) -> DomainResult<Namespace>;
    async fn context(&self, id: Uuid) -> DomainResult<NamespaceContext>;
    async fn update(
        &self,
        id: Uuid,
        command: &UpdateNamespace,
        actor: &str,
    ) -> DomainResult<Namespace>;
    async fn reserve(
        &self,
        id: Uuid,
        command: &NamespaceCommand,
        actor: &str,
    ) -> DomainResult<Operation>;
    async fn operation(&self, id: Uuid) -> DomainResult<Operation>;
    async fn acknowledge(&self, binding: &Binding, ack: &OwnerReadback) -> DomainResult<()>;
    async fn record_error(&self, binding: &Binding, code: &str) -> DomainResult<()>;
}

#[async_trait]
pub trait NamespaceOwner: Send + Sync {
    fn instances(&self) -> Vec<OwnerInstance>;
    fn accepts(&self, resource: &ResourceRef) -> bool;
    async fn apply(&self, command: &OwnerCommand) -> Result<OwnerReadback, &'static str>;
}

pub struct NamespaceService {
    pub repository: Arc<dyn NamespaceRepository>,
    owners: Arc<dyn NamespaceOwner>,
}
impl NamespaceService {
    pub fn instances(&self) -> Vec<OwnerInstance> {
        self.owners.instances()
    }
    pub fn new(repository: Arc<dyn NamespaceRepository>, owners: Arc<dyn NamespaceOwner>) -> Self {
        Self { repository, owners }
    }

    pub async fn execute(
        &self,
        id: Uuid,
        command: &NamespaceCommand,
        actor: &str,
    ) -> DomainResult<Operation> {
        command.validate()?;
        for intent in command.intents() {
            if !self.owners.accepts(&intent.resource) {
                return Err(admin_panel_domain::DomainError::Validation(
                    "unregistered_resource_instance".into(),
                ));
            }
        }
        let operation = self.repository.reserve(id, command, actor).await?;
        if operation.state == "completed" {
            return Ok(operation);
        }
        self.reconcile(operation.id).await
    }

    pub async fn reconcile(&self, operation_id: Uuid) -> DomainResult<Operation> {
        let operation = self.repository.operation(operation_id).await?;
        if operation.state == "completed" {
            return Ok(operation);
        }
        let context = self.repository.context(operation.namespace_id).await?;
        for binding in context
            .bindings
            .iter()
            .filter(|b| b.operation_id == operation_id && !b.confirmed)
        {
            // No compensation delete and no replacement operation on timeout.
            match self.owners.apply(&binding.command()).await {
                Ok(ack) if ack.matches(&binding.command()) => {
                    self.repository.acknowledge(binding, &ack).await?
                }
                Ok(_) => {
                    self.repository
                        .record_error(binding, "owner_readback_mismatch")
                        .await?
                }
                Err(code) => self.repository.record_error(binding, code).await?,
            }
        }
        self.repository.operation(operation_id).await
    }
}
