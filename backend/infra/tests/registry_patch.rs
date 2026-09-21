use admin_panel_domain::{ApprovalStatus, Declaration, DomainError, RegistryEntry, ServiceStatus};
use admin_panel_infra::registry::RegistryStore;
use chrono::Utc;
use sqlx::{PgPool, postgres::PgPoolOptions};
use uuid::Uuid;

#[tokio::test]
async fn patch_entry_is_atomic_and_increments_version_once() {
    let Ok(url) = std::env::var("ADMIN_PANEL_AUDIT_TEST_DATABASE_URL") else {
        eprintln!("skipping isolated registry database test: test URL is not set");
        return;
    };
    let admin_pool = PgPool::connect(&url)
        .await
        .expect("connect to test database");
    let database: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&admin_pool)
        .await
        .expect("read database name");
    assert_eq!(database, "admin_panel_audit_test");
    sqlx::query("CREATE SCHEMA IF NOT EXISTS registry_patch_test")
        .execute(&admin_pool)
        .await
        .expect("create isolated schema");
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET search_path TO registry_patch_test")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .expect("connect to isolated schema");
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS service_registry_entries (\
         id uuid PRIMARY KEY, service_key text UNIQUE NOT NULL, display_name text NOT NULL, \
         owner_team text NOT NULL, status text NOT NULL, active_declaration_id uuid, \
         created_at timestamptz NOT NULL, updated_at timestamptz NOT NULL, version bigint NOT NULL, \
         health_status text, health_checked_at timestamptz, health_detail text)",
    )
    .execute(&pool)
    .await
    .expect("create registry table");
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS service_declarations (\
         id uuid PRIMARY KEY, registry_entry_id uuid NOT NULL REFERENCES service_registry_entries(id), \
         declaration_version integer NOT NULL, integration_base_url text NOT NULL, \
         public_ui_url text, capabilities jsonb NOT NULL, service_contract_version text NOT NULL, \
         declared_by_subject text NOT NULL, declared_at timestamptz NOT NULL, \
         approval_status text NOT NULL, content_hash text NOT NULL, \
         UNIQUE (registry_entry_id, content_hash))",
    )
    .execute(&pool)
    .await
    .expect("create declaration table");

    let store = RegistryStore::new(pool.clone());
    let entry = RegistryEntry {
        id: Uuid::now_v7(),
        service_key: format!("test-{}", Uuid::new_v4().simple()),
        display_name: "Original".into(),
        owner_team: "QA".into(),
        status: ServiceStatus::Pending,
        active_declaration_id: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        version: 1,
        health_status: None,
        health_checked_at: None,
        health_detail: None,
    };
    let initial = declaration(entry.id, "initial");
    store
        .insert_entry(&entry, &initial)
        .await
        .expect("insert initial entry");
    sqlx::query("UPDATE service_registry_entries SET status = 'active' WHERE id = $1")
        .bind(entry.id)
        .execute(&pool)
        .await
        .expect("activate entry");

    let changed = declaration(entry.id, "changed");
    let updated = store
        .patch_entry(&entry.service_key, "Updated", "QA", 1, Some(&changed))
        .await
        .expect("patch entry with new declaration");
    assert_eq!(updated.version, 2);
    assert_eq!(updated.status, ServiceStatus::Pending);
    assert_eq!(updated.display_name, "Updated");
    sqlx::query("UPDATE service_registry_entries SET status = 'active' WHERE id = $1")
        .bind(entry.id)
        .execute(&pool)
        .await
        .expect("reactivate entry");

    let repeated = store
        .patch_entry(&entry.service_key, "Repeated", "QA", 2, Some(&changed))
        .await
        .expect("repeat same declaration");
    assert_eq!(repeated.version, 3);
    assert_eq!(repeated.status, ServiceStatus::Active);
    let declarations: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM service_declarations WHERE registry_entry_id = $1",
    )
    .bind(entry.id)
    .fetch_one(&pool)
    .await
    .expect("count declarations");
    assert_eq!(declarations, 2);

    let next = declaration(entry.id, "next");
    assert!(matches!(
        store
            .patch_entry(&entry.service_key, "Stale", "QA", 2, Some(&next))
            .await,
        Err(DomainError::PreconditionFailed(_))
    ));
    let mut invalid = next.clone();
    invalid.id = changed.id;
    assert!(matches!(
        store
            .patch_entry(&entry.service_key, "Rolled back", "QA", 3, Some(&invalid))
            .await,
        Err(DomainError::Conflict(_))
    ));
    let persisted = store
        .find_by_key(&entry.service_key)
        .await
        .expect("read persisted entry")
        .expect("entry exists");
    assert_eq!(persisted.version, 3);
    assert_eq!(persisted.display_name, "Repeated");
    let declarations_after: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM service_declarations WHERE registry_entry_id = $1",
    )
    .bind(entry.id)
    .fetch_one(&pool)
    .await
    .expect("count after failures");
    assert_eq!(declarations_after, 2);
}

fn declaration(entry_id: Uuid, content: &str) -> Declaration {
    Declaration {
        id: Uuid::now_v7(),
        registry_entry_id: entry_id,
        declaration_version: 1,
        integration_base_url: "http://localhost:8080".into(),
        public_ui_url: None,
        capabilities: vec!["health.read".into()],
        service_contract_version: "v1".into(),
        declared_by_subject: "qa-user".into(),
        declared_at: Utc::now(),
        approval_status: ApprovalStatus::Pending,
        approved_by_subject: None,
        approved_at: None,
        content_hash: format!("{content:0<64}"),
    }
}
