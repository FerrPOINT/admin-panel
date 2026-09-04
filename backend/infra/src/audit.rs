//! Append-only audit events.

use admin_panel_domain::AuditEvent;
use sqlx::PgPool;

#[derive(Clone)]
pub struct AuditStore {
    pool: PgPool,
}

impl AuditStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn append(&self, event: &AuditEvent) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO audit_events \
             (id, occurred_at, request_id, actor_subject, actor_role, action, entity_type, \
             entity_id, metadata) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(event.id)
        .bind(event.occurred_at)
        .bind(event.request_id)
        .bind(&event.actor_subject)
        .bind(event.actor_role.map(|r| r.as_str()))
        .bind(&event.action)
        .bind(&event.entity_type)
        .bind(event.entity_id)
        .bind(&event.metadata)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list(
        &self,
        action: Option<&str>,
        entity_type: Option<&str>,
        limit: i64,
    ) -> Result<Vec<AuditEvent>, sqlx::Error> {
        let rows = sqlx::query_as::<_, AuditRow>(
            "SELECT id, occurred_at, request_id, actor_subject, actor_role, action, \
             entity_type, entity_id, metadata FROM audit_events \
             WHERE ($1::text IS NULL OR action = $1) \
             AND ($2::text IS NULL OR entity_type = $2) \
             ORDER BY occurred_at DESC LIMIT $3",
        )
        .bind(action)
        .bind(entity_type)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }
}

#[derive(sqlx::FromRow)]
struct AuditRow {
    id: uuid::Uuid,
    occurred_at: chrono::DateTime<chrono::Utc>,
    request_id: uuid::Uuid,
    actor_subject: Option<String>,
    actor_role: Option<String>,
    action: String,
    entity_type: String,
    entity_id: Option<uuid::Uuid>,
    metadata: serde_json::Value,
}

impl From<AuditRow> for AuditEvent {
    fn from(row: AuditRow) -> Self {
        Self {
            id: row.id,
            occurred_at: row.occurred_at,
            request_id: row.request_id,
            actor_subject: row.actor_subject,
            actor_role: row
                .actor_role
                .as_deref()
                .and_then(admin_panel_domain::PanelRole::parse),
            action: row.action,
            entity_type: row.entity_type,
            entity_id: row.entity_id,
            metadata: row.metadata,
        }
    }
}
