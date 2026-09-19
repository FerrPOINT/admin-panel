use crate::{Caller, SharedState};
use axum::Json;
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Deserialize)]
pub(super) struct Search {
    #[serde(default)]
    q: String,
    #[serde(default)]
    offset: i64,
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

async fn forward(
    state: &SharedState,
    headers: &HeaderMap,
    method: reqwest::Method,
    path: &str,
    query: Option<&Search>,
    body: Option<Value>,
) -> Response {
    let Some(token) = bearer(headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let Ok(mut url) = reqwest::Url::parse(&state.config.auth.central_api_url) else {
        return (StatusCode::BAD_GATEWAY, Json(json!({"error": {"code": "CENTRAL_AUTH_CONFIG", "message": "Central Auth is not configured"}}))).into_response();
    };
    url.set_path(path);
    if let Some(search) = query {
        url.query_pairs_mut()
            .append_pair("q", &search.q)
            .append_pair("offset", &search.offset.max(0).to_string());
    }
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
    {
        Ok(client) => client,
        Err(_) => return StatusCode::BAD_GATEWAY.into_response(),
    };
    let mut request = client.request(method, url).bearer_auth(token);
    if let Some(body) = body {
        request = request.json(&body);
    }
    let Ok(response) = request.send().await else {
        return (StatusCode::BAD_GATEWAY, Json(json!({"error": {"code": "CENTRAL_AUTH_UNAVAILABLE", "message": "Central Auth is unreachable"}}))).into_response();
    };
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    if status == StatusCode::NO_CONTENT {
        return status.into_response();
    }
    match response.json::<Value>().await {
        Ok(body) => {
            let mut reply = (status, Json(body)).into_response();
            reply.headers_mut().insert(
                axum::http::header::CACHE_CONTROL,
                axum::http::HeaderValue::from_static("no-store"),
            );
            reply
        }
        Err(_) => StatusCode::BAD_GATEWAY.into_response(),
    }
}

pub(super) async fn list_personal_tokens(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> Response {
    forward(
        &state,
        &headers,
        reqwest::Method::GET,
        "/auth/tokens",
        None,
        None,
    )
    .await
}

pub(super) async fn create_personal_token(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    forward(
        &state,
        &headers,
        reqwest::Method::POST,
        "/auth/tokens",
        None,
        Some(body),
    )
    .await
}

pub(super) async fn revoke_personal_token(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(id): Path<uuid::Uuid>,
) -> Response {
    forward(
        &state,
        &headers,
        reqwest::Method::DELETE,
        &format!("/auth/tokens/{id}"),
        None,
        None,
    )
    .await
}

async fn audit(state: &SharedState, caller: &Caller, action: &str, user_id: Option<uuid::Uuid>) {
    let _ = state
        .audit
        .append(&admin_panel_domain::AuditEvent {
            id: uuid::Uuid::now_v7(),
            occurred_at: chrono::Utc::now(),
            request_id: uuid::Uuid::now_v7(),
            actor_subject: Some(caller.subject.clone()),
            actor_role: Some(caller.role),
            action: action.into(),
            entity_type: "central_user".into(),
            entity_id: user_id,
            metadata: json!({}),
        })
        .await;
}

pub(super) async fn list_managed_users(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(search): Query<Search>,
) -> Response {
    forward(
        &state,
        &headers,
        reqwest::Method::GET,
        "/auth/users",
        Some(&search),
        None,
    )
    .await
}

pub(super) async fn create_managed_user(
    State(state): State<SharedState>,
    Extension(caller): Extension<Caller>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let response = forward(
        &state,
        &headers,
        reqwest::Method::POST,
        "/auth/users",
        None,
        Some(body),
    )
    .await;
    if response.status().is_success() {
        audit(&state, &caller, "central_user.created", None).await;
    }
    response
}

pub(super) async fn update_managed_user(
    State(state): State<SharedState>,
    Extension(caller): Extension<Caller>,
    headers: HeaderMap,
    Path(id): Path<uuid::Uuid>,
    Json(body): Json<Value>,
) -> Response {
    let response = forward(
        &state,
        &headers,
        reqwest::Method::PATCH,
        &format!("/auth/users/{id}"),
        None,
        Some(body),
    )
    .await;
    if response.status().is_success() {
        audit(&state, &caller, "central_user.updated", Some(id)).await;
    }
    response
}

pub(super) async fn set_managed_user_status(
    State(state): State<SharedState>,
    Extension(caller): Extension<Caller>,
    headers: HeaderMap,
    Path(id): Path<uuid::Uuid>,
    Json(body): Json<Value>,
) -> Response {
    let response = forward(
        &state,
        &headers,
        reqwest::Method::POST,
        &format!("/auth/users/{id}/status"),
        None,
        Some(body),
    )
    .await;
    if response.status().is_success() {
        audit(&state, &caller, "central_user.status_changed", Some(id)).await;
    }
    response
}

pub(super) async fn resend_managed_user_link(
    State(state): State<SharedState>,
    Extension(caller): Extension<Caller>,
    headers: HeaderMap,
    Path(id): Path<uuid::Uuid>,
) -> Response {
    let response = forward(
        &state,
        &headers,
        reqwest::Method::POST,
        &format!("/auth/users/{id}/password-link"),
        None,
        None,
    )
    .await;
    if response.status().is_success() {
        audit(&state, &caller, "central_user.password_link_sent", Some(id)).await;
    }
    response
}
