//! Admin Panel server entrypoint.

use std::sync::Arc;

use admin_panel_api::{AppState, SharedState};
use admin_panel_shared::AppConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = Arc::new(AppConfig::from_env().expect("failed to load config"));

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(config.database.max_connections)
        .connect(&config.database.url)
        .await?;
    let migrations = sqlx::migrate::Migrator::new(std::path::Path::new(
        &std::env::var("ADMINP_MIGRATIONS_DIR").unwrap_or_else(|_| "migration/migrations".into()),
    ))
    .await?;
    migrations.run(&pool).await?;
    tracing::info!("migrations applied");

    let state: SharedState = Arc::new(AppState {
        registry: admin_panel_infra::registry::RegistryStore::new(pool.clone()),
        branding: admin_panel_infra::branding::BrandingStore::new(pool.clone()),
        access: admin_panel_infra::access::AccessStore::new(pool.clone()),
        audit: admin_panel_infra::audit::AuditStore::new(pool.clone()),
        config: (*config).clone(),
    });

    let allow_origins: Vec<&str> = state
        .config
        .server
        .cors_allowed_origins
        .iter()
        .map(String::as_str)
        .collect();
    let cors = tower_http::cors::CorsLayer::new()
        .allow_origin(
            allow_origins
                .iter()
                .filter_map(|origin| origin.parse::<axum::http::HeaderValue>().ok())
                .collect::<Vec<_>>(),
        )
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PATCH,
            axum::http::Method::DELETE,
        ])
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::header::IF_NONE_MATCH,
            axum::http::header::IF_MATCH,
        ]);

    let app = admin_panel_api::router(state)
        .layer(cors)
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let addr = format!("{}:{}", config.server.address, config.server.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(bind = %addr, "admin-panel api listening");
    axum::serve(listener, app).await?;
    Ok(())
}
