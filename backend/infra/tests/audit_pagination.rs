use admin_panel_domain::AuditEvent;
use admin_panel_infra::audit::AuditStore;
use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

#[tokio::test]
async fn audit_total_counts_filtered_events_before_pagination() {
    let Ok(url) = std::env::var("ADMIN_PANEL_AUDIT_TEST_DATABASE_URL") else {
        eprintln!("skipping isolated audit database test: test URL is not set");
        return;
    };
    let pool = PgPool::connect(&url)
        .await
        .expect("connect to isolated test database");
    let database: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .expect("read database name");
    assert_eq!(database, "admin_panel_audit_test");
    let table_is_absent: bool = sqlx::query_scalar(
        "SELECT to_regclass('public.audit_events') IS NULL \
             AND to_regclass('audit_events') IS NULL",
    )
    .fetch_one(&pool)
    .await
    .expect("check whether an audit table already exists");
    assert!(
        table_is_absent,
        "refusing to run: public.audit_events already exists in the test database"
    );
    sqlx::query(
        "CREATE TABLE public.audit_events (\
         id uuid PRIMARY KEY, occurred_at timestamptz NOT NULL, request_id uuid NOT NULL, \
         actor_subject varchar(255), actor_role varchar(32), action varchar(100) NOT NULL, \
         entity_type varchar(64) NOT NULL, entity_id uuid, metadata jsonb NOT NULL)",
    )
    .execute(&pool)
    .await
    .expect("create isolated audit table");

    let store = AuditStore::new(pool);
    let empty = store.list(None, None, 20, 0).await.expect("empty page");
    assert_eq!((empty.events.len(), empty.total), (0, 0));

    for index in 0..40 {
        let (action, entity_type) = if index < 20 {
            ("central_user.created", "central_user")
        } else {
            ("branding.published", "branding_revision")
        };
        store
            .append(&AuditEvent {
                id: Uuid::now_v7(),
                occurred_at: Utc::now(),
                request_id: Uuid::now_v7(),
                actor_subject: None,
                actor_role: None,
                action: action.into(),
                entity_type: entity_type.into(),
                entity_id: None,
                metadata: serde_json::json!({}),
            })
            .await
            .expect("append isolated audit event");
        if index == 19 {
            let first = store
                .list(None, None, 20, 0)
                .await
                .expect("full first page");
            let after = store.list(None, None, 20, 20).await.expect("page after 20");
            assert_eq!((first.events.len(), first.total), (20, 20));
            assert_eq!((after.events.len(), after.total), (0, 20));
        }
    }

    let first = store
        .list(None, None, 20, 0)
        .await
        .expect("first page of 40");
    let second = store
        .list(None, None, 20, 20)
        .await
        .expect("second page of 40");
    let after = store.list(None, None, 20, 40).await.expect("page after 40");
    assert_eq!((first.events.len(), first.total), (20, 40));
    assert_eq!((second.events.len(), second.total), (20, 40));
    assert_eq!((after.events.len(), after.total), (0, 40));

    let users = store
        .list(Some("central_user.created"), Some("central_user"), 20, 0)
        .await
        .expect("filtered users");
    let mismatch = store
        .list(
            Some("central_user.created"),
            Some("branding_revision"),
            20,
            0,
        )
        .await
        .expect("empty filtered page");
    assert_eq!((users.events.len(), users.total), (20, 20));
    assert_eq!((mismatch.events.len(), mismatch.total), (0, 0));
}
