//! Signed execution readback only: no admission, provider I/O or checkout access.
use crate::{
    error::RuntimeError,
    execution_grant::{AuthorizedExecution, GrantEnvelope},
    http::{RuntimeState, Scopes, require},
    inference_journal::{InferenceEventsPage, InferenceStatus, RunState},
};
use axum::{
    Extension, Json,
    extract::{Path, State},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

fn authorize(
    state: &RuntimeState,
    client: &Scopes,
    grant: &GrantEnvelope,
    now: DateTime<Utc>,
) -> Result<AuthorizedExecution, RuntimeError> {
    require(client, "ai:infer")?;
    let subject = client.1.as_deref().ok_or(RuntimeError::Forbidden)?;
    state
        .grant_verifier
        .as_ref()
        .ok_or(RuntimeError::Forbidden)?
        .verify(grant, subject, now)
}

pub(crate) async fn status(
    State(state): State<Arc<RuntimeState>>,
    Extension(client): Extension<Scopes>,
    Path(id): Path<Uuid>,
    Json(grant): Json<GrantEnvelope>,
) -> Result<Json<InferenceStatus>, RuntimeError> {
    let now = Utc::now();
    let authorized = authorize(&state, &client, &grant, now)?;
    let vault = state.vault.lock().await;
    Ok(Json(vault.inference_status(id, &authorized, Utc::now())?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EventsRequest {
    grant: GrantEnvelope,
    from_sequence: u64,
    limit: usize,
}

pub(crate) async fn events(
    State(state): State<Arc<RuntimeState>>,
    Extension(client): Extension<Scopes>,
    Path(id): Path<Uuid>,
    Json(request): Json<EventsRequest>,
) -> Result<Json<InferenceEventsPage>, RuntimeError> {
    let authorized = authorize(&state, &client, &request.grant, Utc::now())?;
    let vault = state.vault.lock().await;
    Ok(Json(vault.inference_events(
        id,
        &authorized,
        request.from_sequence,
        request.limit,
        Utc::now(),
    )?))
}

#[derive(Serialize)]
pub(crate) struct Cancellation {
    schema_version: u32,
    request_id: Uuid,
    state: RunState,
}

pub(crate) async fn cancel(
    State(state): State<Arc<RuntimeState>>,
    Extension(client): Extension<Scopes>,
    Path(id): Path<Uuid>,
    Json(grant): Json<GrantEnvelope>,
) -> Result<Json<Cancellation>, RuntimeError> {
    let authorized = authorize(&state, &client, &grant, Utc::now())?;
    let state = state
        .vault
        .lock()
        .await
        .cancel_inference(id, &authorized, Utc::now())?;
    Ok(Json(Cancellation {
        schema_version: 1,
        request_id: id,
        state,
    }))
}
