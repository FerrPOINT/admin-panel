//! Real product API and central PAT authorization against the disposable stand.
use axum::{
    Json, Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
    routing::get,
};
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use tower::ServiceExt;

async fn request(app: &Router, path: &str, token: Option<&str>) -> (StatusCode, Vec<u8>) {
    let mut request = Request::get(path);
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    (
        response.status(),
        to_bytes(response.into_body(), 65_536)
            .await
            .unwrap()
            .to_vec(),
    )
}
#[tokio::test]
#[ignore = "disposable product messaging harness only"]
async fn product_feed_api() {
    let central = Router::new()
        .route("/oidc/jwks", get(|| async { Json(json!({"keys":[]})) }))
        .route(
            "/auth/tokens/introspect",
            get(|headers: HeaderMap| async move {
                let token = headers
                    .get("authorization")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                if token == "Bearer sdlc_pat_revoked_qa" {
                    return (StatusCode::UNAUTHORIZED, Json(json!({})));
                }
                let scopes = if token == "Bearer sdlc_pat_read_qa" {
                    vec!["admin-panel:read"]
                } else {
                    vec!["wiki:read"]
                };
                (
                    StatusCode::OK,
                    Json(json!({"sub":"qa-user","email":"qa@example.invalid","scopes":scopes})),
                )
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:18771")
        .await
        .unwrap();
    let auth = tokio::spawn(async move {
        axum::serve(listener, central).await.unwrap();
    });
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&std::env::var("MESSAGING_PRODUCT_TEST_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let name: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(name, "admin_messaging_transport_test");
    let state = Arc::new(admin_panel_api::AppState {
        ai_runtime: None,
        ai: None,
        registry: admin_panel_infra::registry::RegistryStore::new(pool.clone()),
        branding: admin_panel_infra::branding::BrandingStore::new(pool.clone()),
        access: admin_panel_infra::access::AccessStore::new(pool.clone()),
        audit: admin_panel_infra::audit::AuditStore::new(pool.clone()),
        messaging: Arc::new(admin_panel_infra::messaging::MessagingRuntime::new(false)),
        config: admin_panel_shared::AppConfig::from_env().unwrap(),
    });
    let app = admin_panel_api::router(state);
    for path in [
        "/api/v1/platform-events",
        "/api/v1/messaging/status",
        "/api/v1/messaging/contracts",
    ] {
        assert_eq!(request(&app, path, None).await.0, StatusCode::UNAUTHORIZED);
        assert_eq!(
            request(&app, path, Some("sdlc_pat_wrong_scope_qa")).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            request(&app, path, Some("sdlc_pat_revoked_qa")).await.0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            request(&app, path, Some("sdlc_pat_read_qa")).await.0,
            StatusCode::OK
        );
    }
    let (_, contracts) = request(
        &app,
        "/api/v1/messaging/contracts",
        Some("sdlc_pat_read_qa"),
    )
    .await;
    let contracts: Value = serde_json::from_slice(&contracts).unwrap();
    let contract = &contracts["items"][0];
    for key in [
        "retention_days",
        "broker_retention_days",
        "published_outbox_retention_days",
    ] {
        assert_eq!(contract[key], 90);
    }
    for key in [
        "failed_outbox_retention_days",
        "inbox_retention_days",
        "quarantine_retention_days",
    ] {
        assert_eq!(contract[key], 180);
    }
    let (_, diagnostics) =
        request(&app, "/api/v1/messaging/status", Some("sdlc_pat_read_qa")).await;
    let diagnostics: Value = serde_json::from_slice(&diagnostics).unwrap();
    assert_eq!(diagnostics["retention"]["broker_days"], 90);
    assert!(diagnostics["stream_matches_profile"].is_null());
    assert!(diagnostics["configured_capacity"].is_null());
    assert!(diagnostics["storage"]["bytes"].as_i64().unwrap() > 0);
    assert!(diagnostics["storage"]["maintenance"].is_array());
    let serialized = diagnostics.to_string();
    for forbidden in ["password", "postgres://", "payload", "SQLSTATE"] {
        assert!(!serialized.contains(forbidden));
    }
    let (_, bytes) = request(
        &app,
        "/api/v1/platform-events?limit=2&offset=6",
        Some("sdlc_pat_read_qa"),
    )
    .await;
    let page: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(page["total"], 7);
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert_eq!(page["offset"], 6);
    let id = page["items"][0]["id"].as_str().unwrap();
    assert_eq!(
        request(&app, &format!("/api/v1/platform-events/{id}"), None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &app,
            &format!("/api/v1/platform-events/{id}"),
            Some("sdlc_pat_wrong_scope_qa")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (code, detail) = request(
        &app,
        &format!("/api/v1/platform-events/{id}"),
        Some("sdlc_pat_read_qa"),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let detail: Value = serde_json::from_slice(&detail).unwrap();
    assert_eq!(detail.as_object().unwrap().len(), 8);
    assert_eq!(detail["data"].as_object().unwrap().len(), 5);
    for query in [
        "limit=0",
        "limit=101",
        "offset=-1",
        "source=private",
        "status=running",
        "correlation_id=invalid",
        "payload=true",
        "occurred_from=invalid",
        "occurred_from=2026-10-02T12:00:00Z&occurred_to=2026-10-01T12:00:00Z",
    ] {
        assert_eq!(
            request(
                &app,
                &format!("/api/v1/platform-events?{query}"),
                Some("sdlc_pat_read_qa")
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        request(
            &app,
            "/api/v1/platform-events/10000000-0000-4000-8000-000000000000",
            Some("sdlc_pat_read_qa")
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    pool.close().await;
    assert_eq!(
        request(&app, "/api/v1/platform-events", Some("sdlc_pat_read_qa"))
            .await
            .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    let (code, bytes) = request(&app, "/api/v1/messaging/status", Some("sdlc_pat_read_qa")).await;
    assert_eq!(code, StatusCode::OK);
    let state: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(state["feed_available"], false);
    assert_eq!(state["enabled"], false);
    assert_eq!(state["consumer"]["stale"], false);
    auth.abort();
    println!(
        "Product API: real central PAT read scope, 401/403, paging, validation, safe DTO, detail 404 and independent diagnostics with DB 503 verified"
    );
}
