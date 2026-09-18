//! Admin Panel server entrypoint.

use std::sync::Arc;

use admin_panel_api::{AppState, SharedState};
use admin_panel_shared::AppConfig;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

mod health_worker;

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
    bootstrap_services(&pool).await?;

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

    health_worker::spawn(state.registry.clone());

    let app = admin_panel_api::router(state)
        .layer(cors)
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let addr = format!("{}:{}", config.server.address, config.server.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(bind = %addr, "admin-panel api listening");
    axum::serve(listener, app).await?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct BootstrapService {
    key: String,
    label: String,
    api_url: String,
    #[serde(default)]
    ui_url: Option<String>,
}

fn valid_bootstrap_api_url(url: &str) -> bool {
    if admin_panel_domain::valid_integration_base_url(url) {
        return true;
    }

    let Some(origin) = url.strip_prefix("http://") else {
        return false;
    };
    let authority = origin.split('/').next().unwrap_or_default();
    let (host, valid_port) = match authority.rsplit_once(':') {
        Some((host, port)) => (
            host,
            !host.contains(':') && port.parse::<u16>().is_ok_and(|port| port > 0),
        ),
        None => (authority, true),
    };
    !authority.is_empty()
        && authority == origin
        && !authority.contains('@')
        && valid_port
        && !host.is_empty()
        && host.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '-'
                || character == '.'
        })
        && !host.starts_with(['-', '.'])
        && !host.ends_with(['-', '.'])
}

async fn bootstrap_services(pool: &PgPool) -> Result<(), Box<dyn std::error::Error>> {
    let Some(raw) = std::env::var("ADMINP_BOOTSTRAP_SERVICES")
        .ok()
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(());
    };
    let services: Vec<BootstrapService> = serde_json::from_str(&raw)?;
    for service in services {
        if !admin_panel_domain::valid_service_key(&service.key)
            || !valid_bootstrap_api_url(&service.api_url)
            || admin_panel_domain::validate_public_ui_url(service.ui_url.as_ref()).is_err()
        {
            return Err(format!("invalid bootstrap service declaration: {}", service.key).into());
        }
        let existing = sqlx::query_as::<_, (Uuid, Option<Uuid>, Option<String>, Option<String>)>(
            "SELECT e.id, e.active_declaration_id, d.declared_by_subject, d.content_hash \
             FROM service_registry_entries e \
             LEFT JOIN service_declarations d ON d.id = e.active_declaration_id \
             WHERE e.service_key = $1",
        )
        .bind(&service.key)
        .fetch_optional(pool)
        .await?;
        let entry_id = existing
            .as_ref()
            .map(|(entry_id, _, _, _)| *entry_id)
            .unwrap_or_else(Uuid::now_v7);
        let declaration_id = Uuid::now_v7();
        let capabilities = if service.ui_url.is_some() {
            serde_json::json!(["health.read", "ui.render"])
        } else {
            serde_json::json!(["health.read"])
        };
        let mut hasher = Sha256::new();
        hasher.update(service.api_url.as_bytes());
        hasher.update(service.ui_url.as_deref().unwrap_or_default().as_bytes());
        hasher.update(capabilities.to_string().as_bytes());
        let content_hash = hex::encode(hasher.finalize());
        if existing
            .as_ref()
            .is_some_and(|(_, active_id, author, hash)| {
                !should_bootstrap(
                    *active_id,
                    author.as_deref(),
                    hash.as_deref(),
                    &content_hash,
                )
            })
        {
            continue;
        }

        let mut tx = pool.begin().await?;
        if existing.is_none() {
            sqlx::query(
                "INSERT INTO service_registry_entries \
                 (id, service_key, display_name, owner_team, status, created_at, updated_at, version) \
                 VALUES ($1, $2, $3, 'platform', 'active', now(), now(), 1)",
            )
            .bind(entry_id)
            .bind(&service.key)
            .bind(&service.label)
            .execute(&mut *tx)
            .await?;
        }
        let declaration_version: i32 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(declaration_version), 0) + 1 \
             FROM service_declarations WHERE registry_entry_id = $1",
        )
        .bind(entry_id)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO service_declarations \
             (id, registry_entry_id, declaration_version, integration_base_url, public_ui_url, \
              capabilities, service_contract_version, declared_by_subject, declared_at, \
              approval_status, approved_by_subject, approved_at, content_hash) \
             VALUES ($1, $2, $3, $4, $5, $6, '1.0.0', 'local-bootstrap', now(), \
                     'approved', 'local-bootstrap', now(), $7)",
        )
        .bind(declaration_id)
        .bind(entry_id)
        .bind(declaration_version)
        .bind(&service.api_url)
        .bind(&service.ui_url)
        .bind(capabilities)
        .bind(content_hash)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE service_registry_entries SET active_declaration_id = $2 WHERE id = $1")
            .bind(entry_id)
            .bind(declaration_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        tracing::info!(service_key = %service.key, "bootstrapped service catalog entry");
    }
    Ok(())
}

fn should_bootstrap(
    active_id: Option<Uuid>,
    author: Option<&str>,
    current_hash: Option<&str>,
    desired_hash: &str,
) -> bool {
    active_id.is_none() || (author == Some("local-bootstrap") && current_hash != Some(desired_hash))
}

#[cfg(test)]
mod tests {
    use super::{should_bootstrap, valid_bootstrap_api_url};
    use uuid::Uuid;

    #[test]
    fn bootstrap_accepts_compose_origins_without_weakening_url_shape() {
        assert!(valid_bootstrap_api_url("http://task-backend:7721"));
        assert!(valid_bootstrap_api_url("http://host.docker.internal:8812"));
        assert!(valid_bootstrap_api_url("https://services.example.test"));
        assert!(!valid_bootstrap_api_url("http://task-backend:7721/health"));
        assert!(!valid_bootstrap_api_url("http://user@task-backend:7721"));
        assert!(!valid_bootstrap_api_url("http://Task_Backend:7721"));
        assert!(!valid_bootstrap_api_url("http://task-backend:invalid"));
    }

    #[test]
    fn bootstrap_reconciles_only_changed_local_declarations() {
        let active = Some(Uuid::now_v7());
        assert!(should_bootstrap(None, None, None, "new"));
        assert!(should_bootstrap(
            active,
            Some("local-bootstrap"),
            Some("old"),
            "new"
        ));
        assert!(!should_bootstrap(
            active,
            Some("local-bootstrap"),
            Some("same"),
            "same"
        ));
        assert!(!should_bootstrap(
            active,
            Some("admin-user"),
            Some("old"),
            "new"
        ));
    }
}
