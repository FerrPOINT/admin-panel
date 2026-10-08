//! Namespace identity and the bounded resource-owner protocol (v1).
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use sdlc_shared::resource_context::{
    NamespaceRef, OwnerCommand, OwnerReadback, ResourceKind, ResourceRef,
};

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Namespace {
    pub id: Uuid,
    pub registry_instance_id: Uuid,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub responsible_subject: String,
    pub state: String,
    pub revision: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
impl Namespace {
    pub fn reference(&self) -> NamespaceRef {
        NamespaceRef {
            registry_instance_id: self.registry_instance_id,
            namespace_id: self.id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateNamespace {
    pub operation_id: Uuid,
    pub slug: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub responsible_subject: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateNamespace {
    pub expected_revision: i64,
    pub name: String,
    pub description: String,
    pub responsible_subject: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum NamespaceCommand {
    Provision {
        operation_id: Uuid,
        expected_revision: i64,
        resources: Vec<ResourceIntent>,
    },
    Attach {
        operation_id: Uuid,
        expected_revision: i64,
        resource: ResourceRef,
        create_spec: Option<serde_json::Value>,
    },
    Archive {
        operation_id: Uuid,
        expected_revision: i64,
    },
    Restore {
        operation_id: Uuid,
        expected_revision: i64,
    },
}
impl NamespaceCommand {
    pub fn operation_id(&self) -> Uuid {
        match self {
            Self::Provision { operation_id, .. }
            | Self::Attach { operation_id, .. }
            | Self::Archive { operation_id, .. }
            | Self::Restore { operation_id, .. } => *operation_id,
        }
    }
    pub fn expected_revision(&self) -> i64 {
        match self {
            Self::Provision {
                expected_revision, ..
            }
            | Self::Attach {
                expected_revision, ..
            }
            | Self::Archive {
                expected_revision, ..
            }
            | Self::Restore {
                expected_revision, ..
            } => *expected_revision,
        }
    }
    pub fn intents(&self) -> Vec<ResourceIntent> {
        match self {
            Self::Provision { resources, .. } => resources.clone(),
            Self::Attach {
                resource,
                create_spec,
                ..
            } => vec![ResourceIntent {
                resource: resource.clone(),
                create_spec: create_spec.clone(),
            }],
            _ => vec![],
        }
    }
    pub fn validate(&self) -> super::DomainResult<()> {
        if self.operation_id().is_nil() || self.expected_revision() < 1 {
            return Err(super::DomainError::Validation(
                "invalid_namespace_command".into(),
            ));
        }
        let intents = self.intents();
        if matches!(self, Self::Provision { .. })
            && (intents.len() != 3
                || !ResourceKind::ALL
                    .iter()
                    .all(|kind| intents.iter().filter(|i| i.resource.kind == *kind).count() == 1))
        {
            return Err(super::DomainError::Validation(
                "three_distinct_resource_owners_required".into(),
            ));
        }
        for intent in intents {
            if intent.resource.instance_id.is_nil()
                || intent.resource.resource_id.is_nil()
                || intent
                    .create_spec
                    .as_ref()
                    .is_some_and(|v| !v.is_object() || v.to_string().len() > 16_384)
            {
                return Err(super::DomainError::Validation(
                    "invalid_resource_intent".into(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ResourceIntent {
    pub resource: ResourceRef,
    pub create_spec: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct OwnerInstance {
    pub kind: ResourceKind,
    pub instance_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Binding {
    pub namespace: NamespaceRef,
    pub resource: ResourceRef,
    pub operation_id: Uuid,
    pub generation: i64,
    pub desired_state: String,
    pub confirmed: bool,
    pub create_spec: Option<serde_json::Value>,
    pub last_error: Option<String>,
}

impl Binding {
    pub fn command(&self) -> OwnerCommand {
        OwnerCommand {
            schema_version: 1,
            namespace: self.namespace.clone(),
            resource: self.resource.clone(),
            operation_id: self.operation_id,
            generation: self.generation,
            state: self.desired_state.clone(),
            create_spec: self.create_spec.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct NamespaceContext {
    pub namespace: Namespace,
    pub bindings: Vec<Binding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Operation {
    pub id: Uuid,
    pub namespace_id: Uuid,
    pub state: String,
    pub command: serde_json::Value,
}

pub fn validate_properties(
    name: &str,
    description: &str,
    subject: &str,
) -> super::DomainResult<()> {
    if name.trim().is_empty()
        || name.chars().count() > 200
        || description.len() > 16_384
        || subject.trim().is_empty()
        || subject.len() > 255
    {
        return Err(super::DomainError::Validation(
            "invalid_namespace_properties".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_requires_exact_owner_generation_and_drain() {
        let binding = Binding {
            namespace: NamespaceRef {
                registry_instance_id: Uuid::new_v4(),
                namespace_id: Uuid::new_v4(),
            },
            resource: ResourceRef {
                kind: ResourceKind::WikiSpace,
                instance_id: Uuid::new_v4(),
                resource_id: Uuid::new_v4(),
            },
            operation_id: Uuid::new_v4(),
            generation: 2,
            desired_state: "archived".into(),
            confirmed: false,
            create_spec: None,
            last_error: None,
        };
        let mut ack = OwnerReadback {
            schema_version: 1,
            namespace: binding.namespace.clone(),
            resource: binding.resource.clone(),
            operation_id: binding.operation_id,
            generation: 2,
            state: "archived".into(),
            drained: false,
        };
        assert!(!ack.matches(&binding.command()));
        ack.drained = true;
        assert!(ack.matches(&binding.command()));
        ack.resource.instance_id = Uuid::new_v4();
        assert!(!ack.matches(&binding.command()));
        ack.resource = binding.resource.clone();
        ack.generation = 1;
        assert!(!ack.matches(&binding.command()));
    }
    #[test]
    fn slug_is_not_updateable_and_v1_is_strict() {
        let update = serde_json::json!({"expected_revision":1,"name":"Project","description":"","responsible_subject":"owner","slug":"other"});
        assert!(serde_json::from_value::<UpdateNamespace>(update).is_err());
    }
}
