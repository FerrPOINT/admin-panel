//! Canonical registry and reservations. All writes lock the namespace row first.
use admin_panel_app::namespace::NamespaceRepository;
use admin_panel_domain::{DomainError, DomainResult, namespace::*};
use async_trait::async_trait;
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub struct NamespaceStore {
    pool: PgPool,
    instance: Uuid,
}
impl NamespaceStore {
    pub fn new(pool: PgPool, instance: Uuid) -> Self {
        Self { pool, instance }
    }
    pub async fn verify_instance(&self) -> DomainResult<()> {
        let foreign: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM namespaces WHERE registry_instance_id <> $1)",
        )
        .bind(self.instance)
        .fetch_one(&self.pool)
        .await
        .map_err(db)?;
        if foreign {
            return Err(DomainError::Conflict(
                "namespace_registry_instance_changed".into(),
            ));
        }
        Ok(())
    }
}
fn db(error: sqlx::Error) -> DomainError {
    if error
        .as_database_error()
        .is_some_and(|e| e.is_unique_violation())
    {
        return DomainError::Conflict("namespace_or_resource_reserved".into());
    }
    tracing::error!(error = %error, "namespace storage failure");
    DomainError::InvalidTransition("namespace_storage_unavailable".into())
}
fn namespace(row: sqlx::postgres::PgRow) -> DomainResult<Namespace> {
    serde_json::from_value(row.try_get("document").map_err(db)?)
        .map_err(|_| DomainError::InvalidTransition("invalid_namespace_record".into()))
}
fn operation(row: sqlx::postgres::PgRow) -> DomainResult<Operation> {
    Ok(Operation {
        id: row.try_get("id").map_err(db)?,
        namespace_id: row.try_get("namespace_id").map_err(db)?,
        state: row.try_get("state").map_err(db)?,
        command: row.try_get("command").map_err(db)?,
    })
}
async fn audit(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: Uuid,
    actor: &str,
    action: &str,
    request_id: Uuid,
) -> DomainResult<()> {
    sqlx::query("INSERT INTO audit_events (id,request_id,actor_subject,action,entity_type,entity_id) VALUES ($1,$2,$3,$4,'namespace',$5)")
        .bind(Uuid::new_v4()).bind(request_id).bind(actor).bind(action).bind(id).execute(&mut **tx).await.map_err(db)?;
    Ok(())
}
async fn lock(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, id: Uuid) -> DomainResult<Namespace> {
    let row =
        sqlx::query("SELECT to_jsonb(n) AS document FROM namespaces n WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db)?
            .ok_or_else(|| DomainError::NotFound("namespace".into()))?;
    namespace(row)
}

#[async_trait]
impl NamespaceRepository for NamespaceStore {
    async fn list(&self, limit: i64, offset: i64) -> DomainResult<Vec<Namespace>> {
        sqlx::query("SELECT to_jsonb(n) AS document FROM namespaces n ORDER BY created_at DESC,id LIMIT $1 OFFSET $2")
            .bind(limit.clamp(1,100)).bind(offset.max(0)).fetch_all(&self.pool).await.map_err(db)?.into_iter().map(namespace).collect()
    }
    async fn create(&self, cmd: &CreateNamespace, actor: &str) -> DomainResult<Namespace> {
        if cmd.operation_id.is_nil() {
            return Err(DomainError::Validation("invalid_operation_id".into()));
        }
        validate_properties(&cmd.name, &cmd.description, &cmd.responsible_subject)?;
        if !admin_panel_domain::valid_service_key(&cmd.slug) {
            return Err(DomainError::Validation("invalid_namespace_slug".into()));
        }
        let mut tx = self.pool.begin().await.map_err(db)?;
        // A transaction-scoped lock serializes only this idempotency key, including concurrent first attempts.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(cmd.operation_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        let payload = serde_json::to_value(cmd)
            .map_err(|_| DomainError::Validation("invalid_command".into()))?;
        if let Some(row) = sqlx::query("SELECT * FROM namespace_operations WHERE id=$1")
            .bind(cmd.operation_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db)?
        {
            let original_actor: String = row.try_get("actor_subject").map_err(db)?;
            let op = operation(row)?;
            if op.command != payload || original_actor != actor {
                return Err(DomainError::Conflict("operation_payload_conflict".into()));
            }
            let result = lock(&mut tx, op.namespace_id).await?;
            tx.commit().await.map_err(db)?;
            return Ok(result);
        }
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO namespaces (id,registry_instance_id,slug,name,description,responsible_subject,state) VALUES ($1,$2,$3,$4,$5,$6,'provisioning')")
            .bind(id).bind(self.instance).bind(&cmd.slug).bind(cmd.name.trim()).bind(&cmd.description).bind(&cmd.responsible_subject).execute(&mut *tx).await.map_err(db)?;
        sqlx::query("INSERT INTO namespace_operations (id,namespace_id,actor_subject,command,state) VALUES ($1,$2,$3,$4,'completed')").bind(cmd.operation_id).bind(id).bind(actor).bind(payload).execute(&mut *tx).await.map_err(db)?;
        audit(&mut tx, id, actor, "namespace.create", cmd.operation_id).await?;
        let result = lock(&mut tx, id).await?;
        tx.commit().await.map_err(db)?;
        Ok(result)
    }
    async fn context(&self, id: Uuid) -> DomainResult<NamespaceContext> {
        let mut tx = self.pool.begin().await.map_err(db)?;
        // Coherent metadata+bindings snapshot without blocking owner work.
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        let row = sqlx::query("SELECT to_jsonb(n) AS document FROM namespaces n WHERE id=$1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db)?
            .ok_or_else(|| DomainError::NotFound("namespace".into()))?;
        let ns = namespace(row)?;
        let rows =
            sqlx::query("SELECT * FROM namespace_bindings WHERE namespace_id=$1 ORDER BY kind")
                .bind(id)
                .fetch_all(&mut *tx)
                .await
                .map_err(db)?;
        let mut bindings = Vec::new();
        for row in rows {
            let key: String = row.try_get("kind").map_err(db)?;
            let kind = ResourceKind::ALL
                .into_iter()
                .find(|k| k.as_str() == key)
                .ok_or_else(|| DomainError::InvalidTransition("invalid_resource_kind".into()))?;
            bindings.push(Binding {
                namespace: ns.reference(),
                resource: ResourceRef {
                    kind,
                    instance_id: row.try_get("resource_instance_id").map_err(db)?,
                    resource_id: row.try_get("resource_id").map_err(db)?,
                },
                operation_id: row.try_get("operation_id").map_err(db)?,
                generation: row.try_get("generation").map_err(db)?,
                desired_state: row.try_get("desired_state").map_err(db)?,
                confirmed: row.try_get("confirmed").map_err(db)?,
                create_spec: row.try_get("create_spec").map_err(db)?,
                last_error: row.try_get("last_error").map_err(db)?,
            });
        }
        tx.commit().await.map_err(db)?;
        Ok(NamespaceContext {
            namespace: ns,
            bindings,
        })
    }
    async fn update(
        &self,
        id: Uuid,
        cmd: &UpdateNamespace,
        actor: &str,
    ) -> DomainResult<Namespace> {
        validate_properties(&cmd.name, &cmd.description, &cmd.responsible_subject)?;
        let mut tx = self.pool.begin().await.map_err(db)?;
        let ns = lock(&mut tx, id).await?;
        if ns.revision != cmd.expected_revision {
            return Err(DomainError::PreconditionFailed("revision_conflict".into()));
        }
        sqlx::query("UPDATE namespaces SET name=$2,description=$3,responsible_subject=$4,revision=revision+1,updated_at=now() WHERE id=$1")
            .bind(id).bind(cmd.name.trim()).bind(&cmd.description).bind(&cmd.responsible_subject).execute(&mut *tx).await.map_err(db)?;
        audit(&mut tx, id, actor, "namespace.update", Uuid::new_v4()).await?;
        let result = lock(&mut tx, id).await?;
        tx.commit().await.map_err(db)?;
        Ok(result)
    }
    async fn reserve(
        &self,
        id: Uuid,
        cmd: &NamespaceCommand,
        actor: &str,
    ) -> DomainResult<Operation> {
        cmd.validate()?;
        let mut tx = self.pool.begin().await.map_err(db)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(cmd.operation_id().to_string())
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        let ns = lock(&mut tx, id).await?;
        let payload = serde_json::to_value(cmd)
            .map_err(|_| DomainError::Validation("invalid_command".into()))?;
        if let Some(row) = sqlx::query("SELECT * FROM namespace_operations WHERE id=$1")
            .bind(cmd.operation_id())
            .fetch_optional(&mut *tx)
            .await
            .map_err(db)?
        {
            let original_actor: String = row.try_get("actor_subject").map_err(db)?;
            let op = operation(row)?;
            if op.namespace_id != id || op.command != payload || original_actor != actor {
                return Err(DomainError::Conflict("operation_payload_conflict".into()));
            }
            tx.commit().await.map_err(db)?;
            return Ok(op);
        }
        if ns.revision != cmd.expected_revision() {
            return Err(DomainError::PreconditionFailed("revision_conflict".into()));
        }
        let pending: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM namespace_bindings WHERE namespace_id=$1 AND NOT confirmed",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
        if pending != 0 {
            return Err(DomainError::Conflict(
                "original_operation_readback_required".into(),
            ));
        }
        let (new_state, desired) = match cmd {
            NamespaceCommand::Provision { .. } if ns.state == "provisioning" => {
                ("provisioning", "active")
            }
            NamespaceCommand::Attach { .. } if ns.state == "provisioning" => {
                ("provisioning", "active")
            }
            NamespaceCommand::Archive { .. } if ns.state == "active" => ("archiving", "archived"),
            NamespaceCommand::Restore { .. } if ns.state == "archived" => ("restoring", "active"),
            _ => return Err(DomainError::Conflict("invalid_namespace_transition".into())),
        };
        sqlx::query("INSERT INTO namespace_operations (id,namespace_id,actor_subject,command,state) VALUES ($1,$2,$3,$4,'pending')").bind(cmd.operation_id()).bind(id).bind(actor).bind(&payload).execute(&mut *tx).await.map_err(db)?;
        match cmd {
            NamespaceCommand::Provision { .. } | NamespaceCommand::Attach { .. } => {
                for intent in cmd.intents() {
                    let resource = intent.resource;
                    sqlx::query("INSERT INTO namespace_bindings (namespace_id,kind,resource_instance_id,resource_id,operation_id,generation,desired_state,create_spec) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)")
                    .bind(id).bind(resource.kind.as_str()).bind(resource.instance_id).bind(resource.resource_id).bind(cmd.operation_id()).bind(ns.revision+1).bind(desired).bind(intent.create_spec).execute(&mut *tx).await.map_err(db)?;
                }
            }
            _ => {
                sqlx::query("UPDATE namespace_bindings SET operation_id=$2,generation=$3,desired_state=$4,confirmed=false,last_error=NULL,create_spec=NULL,updated_at=now() WHERE namespace_id=$1")
                    .bind(id).bind(cmd.operation_id()).bind(ns.revision+1).bind(desired).execute(&mut *tx).await.map_err(db)?;
            }
        }
        sqlx::query(
            "UPDATE namespaces SET state=$2,revision=revision+1,updated_at=now() WHERE id=$1",
        )
        .bind(id)
        .bind(new_state)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
        audit(&mut tx, id, actor, "namespace.reserve", cmd.operation_id()).await?;
        tx.commit().await.map_err(db)?;
        Ok(Operation {
            id: cmd.operation_id(),
            namespace_id: id,
            state: "pending".into(),
            command: payload,
        })
    }
    async fn operation(&self, id: Uuid) -> DomainResult<Operation> {
        operation(
            sqlx::query("SELECT * FROM namespace_operations WHERE id=$1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db)?
                .ok_or_else(|| DomainError::NotFound("namespace_operation".into()))?,
        )
    }
    async fn acknowledge(&self, binding: &Binding, ack: &OwnerReadback) -> DomainResult<()> {
        if !ack.matches(&binding.command()) {
            return Err(DomainError::Conflict("owner_readback_mismatch".into()));
        }
        let id = binding.namespace.namespace_id;
        let mut tx = self.pool.begin().await.map_err(db)?;
        let ns = lock(&mut tx, id).await?;
        let confirmed: Option<bool> = sqlx::query_scalar("SELECT confirmed FROM namespace_bindings WHERE namespace_id=$1 AND kind=$2 AND operation_id=$3 AND generation=$4 AND desired_state=$5 AND resource_instance_id=$6 AND resource_id=$7")
            .bind(id).bind(binding.resource.kind.as_str()).bind(binding.operation_id).bind(binding.generation).bind(&binding.desired_state).bind(binding.resource.instance_id).bind(binding.resource.resource_id).fetch_optional(&mut *tx).await.map_err(db)?;
        match confirmed {
            Some(true) => {
                tx.commit().await.map_err(db)?;
                return Ok(());
            }
            Some(false) => {}
            None => return Err(DomainError::Conflict("stale_owner_readback".into())),
        }
        let affected = sqlx::query("UPDATE namespace_bindings SET confirmed=true,last_error=NULL,updated_at=now() WHERE namespace_id=$1 AND kind=$2 AND operation_id=$3 AND generation=$4 AND desired_state=$5 AND resource_instance_id=$6 AND resource_id=$7")
            .bind(id).bind(binding.resource.kind.as_str()).bind(binding.operation_id).bind(binding.generation).bind(&binding.desired_state).bind(binding.resource.instance_id).bind(binding.resource.resource_id).execute(&mut *tx).await.map_err(db)?.rows_affected();
        if affected != 1 {
            return Err(DomainError::Conflict("stale_owner_readback".into()));
        }
        let (count,pending): (i64,i64) = sqlx::query_as("SELECT count(*),count(*) FILTER (WHERE NOT confirmed) FROM namespace_bindings WHERE namespace_id=$1").bind(id).fetch_one(&mut *tx).await.map_err(db)?;
        let operation_pending: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM namespace_bindings WHERE operation_id=$1 AND NOT confirmed",
        )
        .bind(binding.operation_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
        if operation_pending == 0 {
            sqlx::query(
                "UPDATE namespace_operations SET state='completed',updated_at=now() WHERE id=$1",
            )
            .bind(binding.operation_id)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        }
        if count == 3 && pending == 0 {
            let state = if ns.state == "archiving" {
                "archived"
            } else {
                "active"
            };
            sqlx::query(
                "UPDATE namespaces SET state=$2,revision=revision+1,updated_at=now() WHERE id=$1",
            )
            .bind(id)
            .bind(state)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        }
        audit(
            &mut tx,
            id,
            "namespace-owner",
            "namespace.owner_ack",
            binding.operation_id,
        )
        .await?;
        tx.commit().await.map_err(db)?;
        Ok(())
    }
    async fn record_error(&self, binding: &Binding, code: &str) -> DomainResult<()> {
        sqlx::query("UPDATE namespace_bindings SET last_error=$3,updated_at=now() WHERE operation_id=$1 AND generation=$2 AND NOT confirmed")
            .bind(binding.operation_id).bind(binding.generation).bind(code).execute(&self.pool).await.map_err(db)?;
        Ok(())
    }
}
