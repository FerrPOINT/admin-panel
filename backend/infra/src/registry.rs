//! Service registry + declarations persistence.

use admin_panel_domain::{ApprovalStatus, Declaration, DomainError, RegistryEntry, ServiceStatus};
use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Clone)]
pub struct RegistryStore {
    pool: PgPool,
}

impl RegistryStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Raw pool access for health checks only.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn migrate(&self) -> Result<(), sqlx::migrate::MigrateError> {
        // Migration files live in the migration crate; resolved via env at runtime.
        let migrations = sqlx::migrate::Migrator::new(std::path::Path::new(
            &std::env::var("ADMINP_MIGRATIONS_DIR")
                .unwrap_or_else(|_| "migration/migrations".into()),
        ))
        .await?;
        migrations.run(&self.pool).await
    }

    pub async fn list(&self) -> Result<Vec<RegistryEntry>, sqlx::Error> {
        let rows = sqlx::query_as::<_, RegistryEntryRow>(
            "SELECT id, service_key, display_name, owner_team, status::text, \
             active_declaration_id, created_at, updated_at, version, \
             health_status, health_checked_at, health_detail \
             FROM service_registry_entries ORDER BY updated_at DESC, service_key ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn find_by_key(&self, key: &str) -> Result<Option<RegistryEntry>, sqlx::Error> {
        sqlx::query_as::<_, RegistryEntryRow>(
            "SELECT id, service_key, display_name, owner_team, status::text, \
             active_declaration_id, created_at, updated_at, version, \
             health_status, health_checked_at, health_detail \
             FROM service_registry_entries WHERE service_key = $1",
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(Into::into))
    }

    /// Persist the outcome of a background health probe.
    pub async fn set_health(
        &self,
        key: &str,
        status: &str,
        detail: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE service_registry_entries \
             SET health_status = $1, health_checked_at = now(), health_detail = $2 \
             WHERE service_key = $3",
        )
        .bind(status)
        .bind(detail)
        .bind(key)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Active entries with an approved declaration that declares health.read.
    pub async fn list_health_targets(&self) -> Result<Vec<(String, String)>, sqlx::Error> {
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT e.service_key, d.integration_base_url \
             FROM service_registry_entries e \
             JOIN service_declarations d ON d.id = e.active_declaration_id \
             WHERE e.status = 'active' AND d.approval_status = 'approved' \
             AND d.capabilities @> '\"health.read\"'::jsonb \
             ORDER BY e.service_key",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn insert_entry(
        &self,
        entry: &RegistryEntry,
        declaration: &Declaration,
    ) -> Result<(), DomainError> {
        let mut tx = self.pool.begin().await.map_err(db)?;
        sqlx::query(
            "INSERT INTO service_registry_entries \
             (id, service_key, display_name, owner_team, status, created_at, updated_at, version) \
             VALUES ($1, $2, $3, $4, $5, $6, $6, 1)",
        )
        .bind(entry.id)
        .bind(&entry.service_key)
        .bind(&entry.display_name)
        .bind(&entry.owner_team)
        .bind(entry.status.as_str())
        .bind(entry.created_at)
        .execute(&mut *tx)
        .await
        .map_err(conflict_as_dup)?;
        insert_declaration_tx(&mut tx, declaration).await?;
        tx.commit().await.map_err(db)?;
        Ok(())
    }

    pub async fn update_metadata(
        &self,
        key: &str,
        display_name: &str,
        owner_team: &str,
        expected_version: i64,
    ) -> Result<RegistryEntry, DomainError> {
        let row = sqlx::query_as::<_, RegistryEntryRow>(
            "UPDATE service_registry_entries \
             SET display_name = $2, owner_team = $3, updated_at = now(), version = version + 1 \
             WHERE service_key = $1 AND version = $4 \
             RETURNING id, service_key, display_name, owner_team, status::text, \
             active_declaration_id, created_at, updated_at, version",
        )
        .bind(key)
        .bind(display_name)
        .bind(owner_team)
        .bind(expected_version)
        .fetch_optional(&self.pool)
        .await
        .map_err(db)?;
        row.map(Into::into)
            .ok_or(DomainError::PreconditionFailed("version mismatch".into()))
    }

    pub async fn approve_declaration(
        &self,
        service_key: &str,
        declaration_id: Uuid,
        approver: &str,
        expected_version: i64,
    ) -> Result<(RegistryEntry, Declaration), DomainError> {
        let mut tx = self.pool.begin().await.map_err(db)?;
        let entry = sqlx::query_as::<_, RegistryEntryRow>(
            "SELECT id, service_key, display_name, owner_team, status::text, \
             active_declaration_id, created_at, updated_at, version, \
             health_status, health_checked_at, health_detail \
             FROM service_registry_entries WHERE service_key = $1 FOR UPDATE",
        )
        .bind(service_key)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?
        .ok_or(DomainError::NotFound(service_key.into()))?;
        if entry.version != expected_version {
            return Err(DomainError::PreconditionFailed("version mismatch".into()));
        }
        let decl = sqlx::query_as::<_, DeclarationRow>(
            "UPDATE service_declarations SET approval_status = 'approved', \
             approved_by_subject = $2, approved_at = $3 \
             WHERE id = $1 AND registry_entry_id = $4 AND approval_status = 'pending' \
             RETURNING id, registry_entry_id, declaration_version, integration_base_url, \
             capabilities, service_contract_version, declared_by_subject, declared_at, \
             approval_status::text, approved_by_subject, approved_at, content_hash",
        )
        .bind(declaration_id)
        .bind(approver)
        .bind(Utc::now())
        .bind(entry.id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?
        .ok_or(DomainError::Conflict("declaration is not pending".into()))?;
        // Supersede previous approved declarations of the same entry.
        sqlx::query(
            "UPDATE service_declarations SET approval_status = 'superseded' \
             WHERE registry_entry_id = $1 AND id <> $2 AND approval_status = 'approved'",
        )
        .bind(entry.id)
        .bind(declaration_id)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
        let updated = sqlx::query_as::<_, RegistryEntryRow>(
            "UPDATE service_registry_entries \
             SET status = 'active', active_declaration_id = $2, updated_at = $3, version = version + 1 \
             WHERE id = $1 \
             RETURNING id, service_key, display_name, owner_team, status::text, \
             active_declaration_id, created_at, updated_at, version",
        )
        .bind(entry.id)
        .bind(declaration_id)
        .bind(Utc::now())
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
        tx.commit().await.map_err(db)?;
        Ok((updated.into(), decl.into()))
    }

    pub async fn set_status(
        &self,
        service_key: &str,
        status: ServiceStatus,
        expected_version: i64,
    ) -> Result<RegistryEntry, DomainError> {
        let row = sqlx::query_as::<_, RegistryEntryRow>(
            "UPDATE service_registry_entries SET status = $2, updated_at = $3, version = version + 1 \
             WHERE service_key = $1 AND version = $4 \
             RETURNING id, service_key, display_name, owner_team, status::text, \
             active_declaration_id, created_at, updated_at, version",
        )
        .bind(service_key)
        .bind(status.as_str())
        .bind(Utc::now())
        .bind(expected_version)
        .fetch_optional(&self.pool)
        .await
        .map_err(db)?;
        row.map(Into::into)
            .ok_or(DomainError::PreconditionFailed("version mismatch".into()))
    }

    pub async fn insert_declaration(&self, declaration: &Declaration) -> Result<(), DomainError> {
        let mut tx = self.pool.begin().await.map_err(db)?;
        insert_declaration_tx(&mut tx, declaration).await?;
        // A changed declaration always moves the entry back to pending.
        sqlx::query(
            "UPDATE service_registry_entries SET status = 'pending', updated_at = $2, version = version + 1 \
             WHERE id = $1",
        )
        .bind(declaration.registry_entry_id)
        .bind(Utc::now())
        .execute(&mut *tx)
        .await
        .map_err(db)?;
        tx.commit().await.map_err(db)?;
        Ok(())
    }

    pub async fn find_declaration(&self, id: Uuid) -> Result<Option<Declaration>, sqlx::Error> {
        sqlx::query_as::<_, DeclarationRow>(
            "SELECT id, registry_entry_id, declaration_version, integration_base_url, \
             capabilities, service_contract_version, declared_by_subject, declared_at, \
             approval_status::text, approved_by_subject, approved_at, content_hash \
             FROM service_declarations WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(Into::into))
    }

    pub async fn list_declarations(&self, entry_id: Uuid) -> Result<Vec<Declaration>, sqlx::Error> {
        let rows = sqlx::query_as::<_, DeclarationRow>(
            "SELECT id, registry_entry_id, declaration_version, integration_base_url, \
             capabilities, service_contract_version, declared_by_subject, declared_at, \
             approval_status::text, approved_by_subject, approved_at, content_hash \
             FROM service_declarations WHERE registry_entry_id = $1 \
             ORDER BY declared_at DESC",
        )
        .bind(entry_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }
}

pub async fn insert_declaration_tx(
    tx: &mut sqlx::PgConnection,
    declaration: &Declaration,
) -> Result<(), DomainError> {
    sqlx::query(
        "INSERT INTO service_declarations \
         (id, registry_entry_id, declaration_version, integration_base_url, capabilities, \
         service_contract_version, declared_by_subject, declared_at, approval_status, content_hash) \
         VALUES ($1, $2, $3, $4, $5::jsonb, $6, $7, $8, $9, $10)",
    )
    .bind(declaration.id)
    .bind(declaration.registry_entry_id)
    .bind(declaration.declaration_version)
    .bind(&declaration.integration_base_url)
    .bind(serde_json::to_string(&declaration.capabilities).unwrap())
    .bind(&declaration.service_contract_version)
    .bind(&declaration.declared_by_subject)
    .bind(declaration.declared_at)
    .bind(declaration.approval_status.as_str())
    .bind(&declaration.content_hash)
    .execute(&mut *tx)
    .await
    .map_err(conflict_as_dup)?;
    Ok(())
}

fn db(err: sqlx::Error) -> DomainError {
    DomainError::Conflict(err.to_string())
}

fn conflict_as_dup(err: sqlx::Error) -> DomainError {
    if err
        .as_database_error()
        .map(|e| e.is_unique_violation())
        .unwrap_or(false)
    {
        DomainError::Conflict("duplicate entry".into())
    } else {
        db(err)
    }
}

#[derive(sqlx::FromRow)]
struct RegistryEntryRow {
    id: Uuid,
    service_key: String,
    display_name: String,
    owner_team: String,
    #[sqlx(rename = "status")]
    status_text: String,
    active_declaration_id: Option<Uuid>,
    created_at: chrono::DateTime<Utc>,
    updated_at: chrono::DateTime<Utc>,
    version: i64,
    health_status: Option<String>,
    health_checked_at: Option<chrono::DateTime<Utc>>,
    health_detail: Option<String>,
}

impl From<RegistryEntryRow> for RegistryEntry {
    fn from(row: RegistryEntryRow) -> Self {
        Self {
            id: row.id,
            service_key: row.service_key,
            display_name: row.display_name,
            owner_team: row.owner_team,
            status: ServiceStatus::parse(&row.status_text).unwrap_or(ServiceStatus::Pending),
            active_declaration_id: row.active_declaration_id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            version: row.version,
            health_status: row.health_status,
            health_checked_at: row.health_checked_at,
            health_detail: row.health_detail,
        }
    }
}

#[derive(sqlx::FromRow)]
struct DeclarationRow {
    id: Uuid,
    registry_entry_id: Uuid,
    declaration_version: i32,
    integration_base_url: String,
    capabilities: serde_json::Value,
    service_contract_version: String,
    declared_by_subject: String,
    declared_at: chrono::DateTime<Utc>,
    #[sqlx(rename = "approval_status")]
    approval_text: String,
    approved_by_subject: Option<String>,
    approved_at: Option<chrono::DateTime<Utc>>,
    content_hash: String,
}

impl From<DeclarationRow> for Declaration {
    fn from(row: DeclarationRow) -> Self {
        Self {
            id: row.id,
            registry_entry_id: row.registry_entry_id,
            declaration_version: row.declaration_version,
            integration_base_url: row.integration_base_url,
            capabilities: serde_json::from_value(row.capabilities).unwrap_or_default(),
            service_contract_version: row.service_contract_version,
            declared_by_subject: row.declared_by_subject,
            declared_at: row.declared_at,
            approval_status: ApprovalStatus::parse(&row.approval_text)
                .unwrap_or(ApprovalStatus::Pending),
            approved_by_subject: row.approved_by_subject,
            approved_at: row.approved_at,
            content_hash: row.content_hash,
        }
    }
}
