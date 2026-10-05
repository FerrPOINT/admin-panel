//! Safe product projection. Inbox and feed effects share the SDK transaction.

use chrono::{DateTime, Utc};
use sdlc_messaging::{
    Envelope,
    contracts::{PipelineFinished, PipelineStatus},
    postgres::consumer::{EventHandler, HandlerError, HandlerFuture},
};
use serde::Serialize;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Default, Clone, Serialize)]
pub struct ConsumerObservation {
    pub observed_at: Option<DateTime<Utc>>,
    pub connection: Option<sdlc_messaging::ConnectionStatus>,
    pub metrics: Option<sdlc_messaging::ConsumerMetrics>,
    pub stale: bool,
    pub stream: Option<sdlc_messaging::StreamMetrics>,
}

#[derive(Default, Clone, Serialize)]
pub struct ProducerObservation {
    pub last_success_at: Option<DateTime<Utc>>,
    pub status: Option<sdlc_messaging::contracts::ProducerDiagnostics>,
    pub stale: bool,
}

pub struct MessagingRuntime {
    pub enabled: bool,
    pub consumer: tokio::sync::RwLock<ConsumerObservation>,
    pub producer: tokio::sync::RwLock<ProducerObservation>,
}
impl MessagingRuntime {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            consumer: tokio::sync::RwLock::new(ConsumerObservation {
                stale: enabled,
                ..Default::default()
            }),
            producer: tokio::sync::RwLock::new(ProducerObservation {
                stale: true,
                ..Default::default()
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct PlatformEvent {
    pub id: Uuid,
    pub source: String,
    pub event_type: String,
    pub schema_version: i32,
    pub occurred_at: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
    pub correlation_id: Uuid,
    pub project_id: Uuid,
    pub pipeline_id: Uuid,
    pub completion_seq: i64,
    pub status: String,
    pub finished_at: DateTime<Utc>,
}

#[derive(Default)]
pub struct FeedFilter {
    pub source: Option<String>,
    pub status: Option<String>,
    pub occurred_from: Option<DateTime<Utc>>,
    pub occurred_to: Option<DateTime<Utc>>,
    pub correlation_id: Option<Uuid>,
}

#[derive(Serialize)]
pub struct FeedPage {
    pub items: Vec<PlatformEvent>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

const FILTER: &str = " WHERE ($1::text IS NULL OR source=$1) AND ($2::text IS NULL OR status=$2) AND ($3::timestamptz IS NULL OR occurred_at >= $3) AND ($4::timestamptz IS NULL OR occurred_at < $4) AND ($5::uuid IS NULL OR correlation_id=$5)";

#[derive(Clone)]
pub struct FeedStore(pub PgPool);
impl FeedStore {
    pub async fn diagnostics(&self) -> Result<serde_json::Value, sqlx::Error> {
        let last_received: Option<DateTime<Utc>> =
            sqlx::query_scalar("SELECT max(received_at) FROM platform_events")
                .fetch_one(&self.0)
                .await?;
        let total: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM messaging_quarantine WHERE handler_id='admin-platform-feed-v1'",
        )
        .fetch_one(&self.0)
        .await?;
        let last: Option<(String, DateTime<Utc>)> = sqlx::query_as("SELECT reason_code,observed_at FROM messaging_quarantine WHERE handler_id='admin-platform-feed-v1' ORDER BY observed_at DESC LIMIT 1").fetch_optional(&self.0).await?;
        Ok(
            serde_json::json!({ "last_received_at": last_received, "quarantine": { "total": total, "last": last.map(|(reason, at)| serde_json::json!({"reason_code":reason,"observed_at":at})) } }),
        )
    }
    pub async fn list(
        &self,
        filter: &FeedFilter,
        limit: i64,
        offset: i64,
    ) -> Result<FeedPage, sqlx::Error> {
        let mut tx = self.0.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *tx)
            .await?;
        let total =
            sqlx::query_scalar::<_, i64>(&format!("SELECT count(*) FROM platform_events{FILTER}"))
                .bind(&filter.source)
                .bind(&filter.status)
                .bind(filter.occurred_from)
                .bind(filter.occurred_to)
                .bind(filter.correlation_id)
                .fetch_one(&mut *tx)
                .await?;
        let items = sqlx::query_as::<_, PlatformEvent>(&format!("SELECT id,source,event_type,schema_version,occurred_at,received_at,correlation_id,project_id,pipeline_id,completion_seq,status,finished_at FROM platform_events{FILTER} ORDER BY received_at DESC, id DESC LIMIT $6 OFFSET $7"))
            .bind(&filter.source).bind(&filter.status).bind(filter.occurred_from).bind(filter.occurred_to).bind(filter.correlation_id).bind(limit).bind(offset)
            .fetch_all(&mut *tx).await?;
        tx.commit().await?;
        Ok(FeedPage {
            items,
            total,
            limit,
            offset,
        })
    }

    pub async fn detail(&self, id: Uuid) -> Result<Option<PlatformEvent>, sqlx::Error> {
        sqlx::query_as("SELECT id,source,event_type,schema_version,occurred_at,received_at,correlation_id,project_id,pipeline_id,completion_seq,status,finished_at FROM platform_events WHERE id=$1")
            .bind(id)
            .fetch_optional(&self.0)
            .await
    }

    /// Product projection retention; SDK inbox/quarantine maintenance is independent.
    pub async fn cleanup(&self) -> Result<u64, sqlx::Error> {
        let mut tx = self.0.begin().await?;
        sqlx::query("SET LOCAL lock_timeout='2s'")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SET LOCAL statement_timeout='5s'")
            .execute(&mut *tx)
            .await?;
        let removed=sqlx::query("DELETE FROM platform_events WHERE id IN (SELECT id FROM platform_events WHERE received_at<now()-make_interval(days=>$1) ORDER BY received_at,id LIMIT 500 FOR UPDATE SKIP LOCKED)")
            .bind(sdlc_messaging::policy::RETENTION.feed_days as i32).execute(&mut *tx).await?.rows_affected();
        let result = serde_json::to_value(sdlc_messaging::postgres::maintenance::CleanupResult {
            product_rows: removed,
            ..Default::default()
        })
        .map_err(|error| sqlx::Error::Decode(Box::new(error)))?;
        sqlx::query("INSERT INTO messaging_maintenance_state(scope,last_attempt_at,last_success_at,last_result) VALUES('feed:platform_events',now(),now(),$1) ON CONFLICT(scope) DO UPDATE SET last_attempt_at=EXCLUDED.last_attempt_at,last_success_at=EXCLUDED.last_success_at,last_result=EXCLUDED.last_result,last_error_code=NULL").bind(result).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(removed)
    }
}

pub struct PlatformFeedHandler;
impl EventHandler<PipelineFinished> for PlatformFeedHandler {
    const HANDLER_ID: &'static str = "admin-platform-feed-v1";
    fn handle<'a, 'tx: 'a>(
        &'a self,
        tx: &'a mut Transaction<'tx, Postgres>,
        event: &'a Envelope<PipelineFinished>,
    ) -> HandlerFuture<'a> {
        Box::pin(async move {
            let status = match event.data.status {
                PipelineStatus::Success => "success",
                PipelineStatus::Failed => "failed",
                PipelineStatus::Canceled => "canceled",
            };
            let inserted: Option<Uuid> = sqlx::query_scalar("INSERT INTO platform_events (id,source,event_type,schema_version,occurred_at,correlation_id,project_id,pipeline_id,completion_seq,status,finished_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) ON CONFLICT (source,pipeline_id,completion_seq) DO NOTHING RETURNING id")
                .bind(event.id).bind(&event.source).bind(&event.event_type).bind(event.schema_version as i32).bind(event.occurred_at).bind(event.correlation_id)
                .bind(event.data.project_id).bind(event.data.pipeline_id).bind(event.data.completion_seq).bind(status).bind(event.data.finished_at)
                .fetch_optional(&mut **tx).await?;
            if inserted.is_none() {
                return Err(HandlerError::Business);
            }
            Ok(())
        })
    }
}
