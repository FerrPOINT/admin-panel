//! Role bindings persistence.

use admin_panel_domain::{DomainError, PanelRole, RoleBinding};
use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Clone)]
pub struct AccessStore {
    pool: PgPool,
}

impl AccessStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn list(&self) -> Result<Vec<RoleBinding>, sqlx::Error> {
        let rows = sqlx::query_as::<_, BindingRow>(
            "SELECT id, claim_name, claim_value, panel_role::text, created_by_subject, created_at \
             FROM role_bindings ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn insert(&self, binding: &RoleBinding) -> Result<(), DomainError> {
        let result = sqlx::query(
            "INSERT INTO role_bindings (id, claim_name, claim_value, panel_role, created_by_subject) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(binding.id)
        .bind(&binding.claim_name)
        .bind(&binding.claim_value)
        .bind(binding.panel_role.as_str())
        .bind(&binding.created_by_subject)
        .execute(&self.pool)
        .await
        .map_err(|err| {
            if err
                .as_database_error()
                .map(|e| e.is_unique_violation())
                .unwrap_or(false)
            {
                DomainError::Conflict("duplicate role binding".into())
            } else {
                DomainError::Conflict(err.to_string())
            }
        })?;
        if result.rows_affected() != 1 {
            return Err(DomainError::Conflict("binding not created".into()));
        }
        Ok(())
    }

    pub async fn delete(&self, id: Uuid) -> Result<(), DomainError> {
        let result = sqlx::query("DELETE FROM role_bindings WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|err| DomainError::Conflict(err.to_string()))?;
        if result.rows_affected() != 1 {
            return Err(DomainError::NotFound("role binding".into()));
        }
        Ok(())
    }

    /// Resolves the strongest panel role for a set of claim values.
    pub async fn resolve_role(
        &self,
        claims: &[(String, Vec<String>)],
    ) -> Result<Option<PanelRole>, sqlx::Error> {
        let mut best: Option<PanelRole> = None;
        for (name, values) in claims {
            for value in values {
                let role = sqlx::query_scalar::<_, String>(
                    "SELECT panel_role::text FROM role_bindings WHERE claim_name = $1 AND claim_value = $2",
                )
                .bind(name)
                .bind(value)
                .fetch_optional(&self.pool)
                .await?;
                if let Some(role) = role {
                    let parsed =
                        PanelRole::parse(&role).unwrap_or(PanelRole::PlatformViewer);
                    if best.map(|b| parsed > b).unwrap_or(true) {
                        best = Some(parsed);
                    }
                }
            }
        }
        Ok(best)
    }
}

#[derive(sqlx::FromRow)]
struct BindingRow {
    id: Uuid,
    claim_name: String,
    claim_value: String,
    #[sqlx(rename = "panel_role")]
    role_text: String,
    created_by_subject: String,
    created_at: chrono::DateTime<Utc>,
}

impl From<BindingRow> for RoleBinding {
    fn from(row: BindingRow) -> Self {
        Self {
            id: row.id,
            claim_name: row.claim_name,
            claim_value: row.claim_value,
            panel_role: PanelRole::parse(&row.role_text)
                .unwrap_or(PanelRole::PlatformViewer),
            created_by_subject: row.created_by_subject,
            created_at: row.created_at,
        }
    }
}

