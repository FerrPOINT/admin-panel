//! Authenticated feed endpoints; broker credentials never enter HTTP responses.

use crate::{SharedState, error_response};
use admin_panel_infra::messaging::{FeedFilter, FeedStore, PlatformEvent};
use axum::{
    Json,
    extract::{Path, Query, State, rejection::QueryRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

#[derive(Default, Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
pub struct FeedQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub source: Option<String>,
    pub status: Option<String>,
    pub occurred_from: Option<DateTime<Utc>>,
    pub occurred_to: Option<DateTime<Utc>>,
    pub correlation_id: Option<Uuid>,
}
impl FeedQuery {
    pub fn validate(self) -> Result<(FeedFilter, i64, i64), &'static str> {
        let limit = self.limit.unwrap_or(50);
        let offset = self.offset.unwrap_or(0);
        if !(1..=100).contains(&limit) || offset < 0 {
            return Err("invalid page range");
        }
        if self.source.as_deref().is_some_and(|s| s != "ci-cd")
            || self
                .status
                .as_deref()
                .is_some_and(|s| !matches!(s, "success" | "failed" | "canceled"))
        {
            return Err("unknown event filter");
        }
        if matches!((self.occurred_from, self.occurred_to), (Some(from), Some(to)) if from >= to) {
            return Err("invalid time range");
        }
        Ok((
            FeedFilter {
                source: self.source,
                status: self.status,
                occurred_from: self.occurred_from,
                occurred_to: self.occurred_to,
                correlation_id: self.correlation_id,
            },
            limit,
            offset,
        ))
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct EventDataView {
    pub project_id: Uuid,
    pub pipeline_id: Uuid,
    pub completion_seq: i64,
    pub status: String,
    pub finished_at: DateTime<Utc>,
}
#[derive(Serialize, utoipa::ToSchema)]
pub struct EventView {
    pub id: Uuid,
    pub source: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub schema_version: i32,
    pub occurred_at: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
    pub correlation_id: Uuid,
    pub data: EventDataView,
}
impl From<PlatformEvent> for EventView {
    fn from(e: PlatformEvent) -> Self {
        Self {
            id: e.id,
            source: e.source,
            event_type: e.event_type,
            schema_version: e.schema_version,
            occurred_at: e.occurred_at,
            received_at: e.received_at,
            correlation_id: e.correlation_id,
            data: EventDataView {
                project_id: e.project_id,
                pipeline_id: e.pipeline_id,
                completion_seq: e.completion_seq,
                status: e.status,
                finished_at: e.finished_at,
            },
        }
    }
}
#[derive(Serialize, utoipa::ToSchema)]
pub struct EventPageView {
    pub items: Vec<EventView>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[utoipa::path(get, path="/api/v1/platform-events", tag="messaging", params(FeedQuery), responses((status=200, body=EventPageView), (status=400), (status=401), (status=403), (status=503)))]
pub async fn list(
    State(state): State<SharedState>,
    query: Result<Query<FeedQuery>, QueryRejection>,
) -> Response {
    let Ok(Query(query)) = query else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "INVALID_FILTER",
            "invalid event filters",
        );
    };
    let (filter, limit, offset) = match query.validate() {
        Ok(v) => v,
        Err(message) => return error_response(StatusCode::BAD_REQUEST, "INVALID_FILTER", message),
    };
    match FeedStore(state.registry.pool().clone())
        .list(&filter, limit, offset)
        .await
    {
        Ok(page) => Json(EventPageView {
            items: page.items.into_iter().map(Into::into).collect(),
            total: page.total,
            limit,
            offset,
        })
        .into_response(),
        Err(_) => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "FEED_UNAVAILABLE",
            "event feed unavailable",
        ),
    }
}
#[utoipa::path(get, path="/api/v1/platform-events/{id}", tag="messaging", params(("id"=Uuid,Path)), responses((status=200, body=EventView), (status=404), (status=401), (status=403), (status=503)))]
pub async fn detail(State(state): State<SharedState>, Path(id): Path<Uuid>) -> Response {
    match FeedStore(state.registry.pool().clone()).detail(id).await {
        Ok(Some(event)) => Json(EventView::from(event)).into_response(),
        Ok(None) => error_response(StatusCode::NOT_FOUND, "EVENT_NOT_FOUND", "event not found"),
        Err(_) => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "FEED_UNAVAILABLE",
            "event feed unavailable",
        ),
    }
}
#[utoipa::path(get, path="/api/v1/messaging/status", tag="messaging", responses((status=200, description="Independent observations, stale flags and safe aggregates"),(status=401),(status=403)))]
pub async fn status(State(state): State<SharedState>) -> Json<serde_json::Value> {
    let configured_capacity = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        let path = std::env::var("MESSAGING_CAPACITY_REPORT").ok()?;
        let metadata = tokio::fs::metadata(&path).await.ok()?;
        if metadata.len() > 65_536 {
            return None;
        }
        let bytes = tokio::fs::read(path).await.ok()?;
        let capacity: sdlc_messaging::policy::CapacityReport =
            serde_json::from_slice(&bytes).ok()?;
        capacity.validate().ok()?;
        Some(capacity)
    })
    .await
    .ok()
    .flatten();
    let mut consumer = state.messaging.consumer.read().await.clone();
    let mut producer = state.messaging.producer.read().await.clone();
    consumer.stale |= state.messaging.enabled
        && consumer
            .observed_at
            .is_none_or(|at| Utc::now().signed_duration_since(at).num_seconds() > 30);
    producer.stale |= producer
        .last_success_at
        .is_none_or(|at| Utc::now().signed_duration_since(at).num_seconds() > 30);
    let feed = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        FeedStore(state.registry.pool().clone()).diagnostics(),
    )
    .await
    .ok()
    .and_then(Result::ok);
    type MaintenanceRow = (
        String,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
        Option<String>,
        Option<String>,
    );
    let storage=tokio::time::timeout(std::time::Duration::from_secs(2),async {
        let pool=state.registry.pool();
        let bytes:i64=sqlx::query_scalar("SELECT pg_total_relation_size('platform_events')+pg_total_relation_size('messaging_inbox')+pg_total_relation_size('messaging_quarantine')").fetch_one(pool).await?;
        let counts:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM platform_events),(SELECT count(*) FROM messaging_inbox WHERE handler_id='admin-platform-feed-v1' AND source='ci-cd'),(SELECT count(*) FROM messaging_quarantine WHERE handler_id='admin-platform-feed-v1' AND source='ci-cd')").fetch_one(pool).await?;
        let maintenance:Vec<MaintenanceRow>=sqlx::query_as("SELECT scope,started_at,last_success_at,last_result::text,last_error_code FROM messaging_maintenance_state WHERE scope IN ('feed:platform_events','inbox:admin-platform-feed-v1:ci-cd')").fetch_all(pool).await?;
        Ok::<_,sqlx::Error>(json!({"bytes":bytes,"feed_count":counts.0,"inbox_count":counts.1,"quarantine_count":counts.2,"maintenance":maintenance.into_iter().map(|(scope,started_at,at,result,code)|json!({"scope":scope,"started_at":started_at,"last_success_at":at,"last_result":result.and_then(|r|serde_json::from_str::<serde_json::Value>(&r).ok()),"last_error_code":code})).collect::<Vec<_>>()}))
    }).await.ok().and_then(Result::ok);
    let stream_matches_profile = consumer
        .stream
        .as_ref()
        .filter(|_| !consumer.stale)
        .and_then(|stream| {
            if stream.max_age_seconds
                != u64::from(sdlc_messaging::policy::RETENTION.broker_days)
                    * sdlc_messaging::policy::DAY_SECONDS
            {
                return Some(false);
            }
            configured_capacity
                .as_ref()
                .map(|capacity| stream.max_bytes == capacity.stream_max_bytes as i64)
        });
    Json(
        json!({"enabled":state.messaging.enabled,"observed_at":Utc::now(),"consumer":consumer,"producer":producer,"feed":feed,"feed_available":feed.is_some(),"retention":sdlc_messaging::policy::RETENTION,"configured_capacity":configured_capacity.map(|capacity|json!({"stream_max_bytes":capacity.stream_max_bytes,"server_file_store_bytes":capacity.server_file_store_bytes})),"storage":storage,"stream_matches_profile":stream_matches_profile}),
    )
}
#[utoipa::path(get, path="/api/v1/messaging/contracts", tag="messaging", responses((status=200, description="Pilot contract metadata"),(status=401),(status=403)))]
pub async fn contracts() -> Json<serde_json::Value> {
    Json(
        json!({"items":[{"source":"ci-cd","type":"platform.cicd.pipeline.finished.v1","schema_version":1,"retention_days":sdlc_messaging::policy::RETENTION.feed_days,"broker_retention_days":sdlc_messaging::policy::RETENTION.broker_days,"published_outbox_retention_days":sdlc_messaging::policy::RETENTION.published_outbox_days,"failed_outbox_retention_days":sdlc_messaging::policy::RETENTION.failed_outbox_days,"inbox_retention_days":sdlc_messaging::policy::RETENTION.inbox_days,"quarantine_retention_days":sdlc_messaging::policy::RETENTION.quarantine_days}]}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_invalid_filters_and_preserves_page_contract() {
        let (_, limit, offset) = FeedQuery::default().validate().unwrap();
        assert_eq!((limit, offset), (50, 0));
        for limit in [0, -1, 101] {
            assert!(
                FeedQuery {
                    limit: Some(limit),
                    ..Default::default()
                }
                .validate()
                .is_err()
            );
        }
        assert!(
            FeedQuery {
                offset: Some(-1),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            FeedQuery {
                source: Some("unknown".into()),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            FeedQuery {
                status: Some("running".into()),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        let now = Utc::now();
        assert!(
            FeedQuery {
                occurred_from: Some(now),
                occurred_to: Some(now),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
}
