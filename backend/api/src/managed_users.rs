use crate::{Caller, SharedState};
use axum::Json;
use axum::body::{Body, to_bytes};
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

pub(super) async fn list_personal_token_services(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> Response {
    forward(
        &state,
        &headers,
        reqwest::Method::GET,
        "/auth/token-services",
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
    if !response.status().is_success() {
        return response;
    }
    let (parts, body) = response.into_parts();
    // forward already buffered and serialized the upstream JSON; preserve its exact reply.
    let Ok(bytes) = to_bytes(body, usize::MAX).await else {
        return StatusCode::BAD_GATEWAY.into_response();
    };
    #[derive(Deserialize)]
    struct CreatedUser {
        id: uuid::Uuid,
    }
    let Ok(created) = serde_json::from_slice::<CreatedUser>(&bytes) else {
        return (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error": {"code": "CENTRAL_AUTH_RESPONSE", "message": "Central Auth returned an invalid user identity"}})),
        )
            .into_response();
    };
    audit(&state, &caller, "central_user.created", Some(created.id)).await;
    Response::from_parts(parts, Body::from(bytes))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppState;
    use axum::Router;
    use axum::routing::post;
    use sqlx::{PgPool, postgres::PgPoolOptions};
    use std::sync::Arc;

    #[tokio::test]
    async fn creation_audit_links_upstream_identity_and_preserves_reply() {
        let Ok(url) = std::env::var("ADMIN_PANEL_AUDIT_TEST_DATABASE_URL") else {
            eprintln!("skipping managed-user audit database test: test URL is not set");
            return;
        };
        let admin_pool = PgPool::connect(&url)
            .await
            .expect("connect to isolated database");
        let database: String = sqlx::query_scalar("SELECT current_database()")
            .fetch_one(&admin_pool)
            .await
            .unwrap();
        assert_eq!(database, "admin_panel_audit_test");
        let schema = format!("managed_user_audit_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin_pool)
            .await
            .unwrap();
        let search_path = schema.clone();
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .after_connect(move |connection, _| {
                let statement = format!("SET search_path TO {search_path}");
                Box::pin(async move {
                    sqlx::query(&statement).execute(connection).await?;
                    Ok(())
                })
            })
            .connect(&url)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE audit_events (id uuid PRIMARY KEY, occurred_at timestamptz NOT NULL, \
             request_id uuid NOT NULL, actor_subject text, actor_role text, action text NOT NULL, \
             entity_type text NOT NULL, entity_id uuid, metadata jsonb NOT NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let user_id = uuid::Uuid::new_v4();
        let caller = Caller {
            subject: "qa-audit-operator".into(),
            email: None,
            central_role: None,
            role: admin_panel_domain::PanelRole::PlatformAdmin,
            can_mutate: true,
            can_manage_bindings: false,
        };
        for (upstream_status, upstream_body, expected_status, expected_events) in [
            (
                StatusCode::CREATED,
                json!({"id": user_id, "display_name": "QA audit user", "status": "pending"}),
                StatusCode::CREATED,
                1,
            ),
            (
                StatusCode::CREATED,
                json!({"id": "invalid-uuid"}),
                StatusCode::BAD_GATEWAY,
                0,
            ),
            (
                StatusCode::CREATED,
                json!({"display_name": "Missing identity"}),
                StatusCode::BAD_GATEWAY,
                0,
            ),
            (
                StatusCode::CONFLICT,
                json!({"error": {"message": "already exists"}}),
                StatusCode::CONFLICT,
                0,
            ),
        ] {
            sqlx::query("DELETE FROM audit_events")
                .execute(&pool)
                .await
                .unwrap();
            let reply = upstream_body.clone();
            let upstream = Router::new().route("/auth/users", post(move |headers: HeaderMap, Json(body): Json<Value>| {
                let reply = reply.clone();
                async move {
                    assert_eq!(bearer(&headers), Some("qa-test-bearer"));
                    assert_eq!(body, json!({"email": "qa-audit@example.test", "display_name": "QA audit user"}));
                    (upstream_status, Json(reply))
                }
            }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server =
                tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
            let config = serde_json::from_value(json!({
                "database": {"url": url, "max_connections": 1},
                "server": {"address": "127.0.0.1", "port": 0, "cors_allowed_origins": []},
                "auth": {"jwks_uri": "", "issuer": "", "audience": "", "central_api_url": format!("http://{address}")},
            })).unwrap();
            let state = Arc::new(AppState {
                namespaces: None,
                ai: None,
                ai_runtime: None,
                messaging: Arc::new(admin_panel_infra::messaging::MessagingRuntime::new(false)),
                registry: admin_panel_infra::registry::RegistryStore::new(pool.clone()),
                branding: admin_panel_infra::branding::BrandingStore::new(pool.clone()),
                access: admin_panel_infra::access::AccessStore::new(pool.clone()),
                audit: admin_panel_infra::audit::AuditStore::new(pool.clone()),
                config,
            });
            let mut headers = HeaderMap::new();
            headers.insert("authorization", "Bearer qa-test-bearer".parse().unwrap());
            let response = create_managed_user(
                State(state),
                Extension(caller.clone()),
                headers,
                Json(json!({"email": "qa-audit@example.test", "display_name": "QA audit user"})),
            )
            .await;
            assert_eq!(response.status(), expected_status);
            if expected_status != StatusCode::BAD_GATEWAY {
                assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
                assert_eq!(
                    response.headers().get("content-type").unwrap(),
                    "application/json"
                );
                let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
                assert_eq!(
                    serde_json::from_slice::<Value>(&bytes).unwrap(),
                    upstream_body
                );
            }
            let events = admin_panel_infra::audit::AuditStore::new(pool.clone())
                .list(None, None, 20, 0)
                .await
                .unwrap();
            assert_eq!(events.total, expected_events);
            if expected_events == 1 {
                assert_eq!(events.events[0].entity_id, Some(user_id));
                assert_eq!(events.events[0].entity_type, "central_user");
                assert_eq!(events.events[0].action, "central_user.created");
                assert_eq!(
                    events.events[0].actor_subject.as_deref(),
                    Some("qa-audit-operator")
                );
                assert_eq!(events.events[0].metadata, json!({}));
            }
            server.abort();
            let _ = server.await;
        }
        pool.close().await;
        sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
            .execute(&admin_pool)
            .await
            .unwrap();
        admin_pool.close().await;
    }
}
