//! Real database tests; the disposable harness supplies an isolated database.
use admin_panel_infra::messaging::{FeedFilter, FeedStore, PlatformFeedHandler};
use chrono::{Duration, Utc};
use sdlc_messaging::{
    Envelope,
    contracts::{PipelineFinished, PipelineStatus},
    postgres::consumer::EventHandler,
    postgres::{InboxDisposition, InboxKey, begin_inbox, mark_processed},
};
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

#[tokio::test]
#[ignore = "disposable product messaging capacity measurement only"]
async fn product_capacity_sample() {
    let url = std::env::var("MESSAGING_PRODUCT_TEST_DATABASE_URL").unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap();
    let name: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(name, "admin_messaging_test");
    for i in 0..1000i64 {
        let event = Envelope::new(
            Uuid::new_v4(),
            Utc::now(),
            None,
            PipelineFinished {
                project_id: Uuid::new_v4(),
                pipeline_id: Uuid::new_v4(),
                completion_seq: i64::MAX,
                status: PipelineStatus::Canceled,
                finished_at: Utc::now(),
            },
        );
        let mut tx = pool.begin().await.unwrap();
        let key = InboxKey {
            handler_id: PlatformFeedHandler::HANDLER_ID,
            source: "ci-cd",
            event_id: event.id,
        };
        let fingerprint = event.fingerprint().unwrap();
        begin_inbox(&mut tx, &key, &fingerprint).await.unwrap();
        PlatformFeedHandler.handle(&mut tx, &event).await.unwrap();
        mark_processed(&mut tx, &key, &fingerprint).await.unwrap();
        sqlx::query("INSERT INTO messaging_quarantine(id,handler_id,source,event_id,stream,stream_sequence,fingerprint,reason_code) VALUES($1,'admin-platform-feed-v1','ci-cd',$2,'PLATFORM_EVENTS',$3,$4,'invalid_schema')").bind(Uuid::new_v4()).bind(event.id).bind(i+100_000).bind(fingerprint.as_slice()).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
    }
    for (table, days) in [
        ("platform_events", 90),
        ("messaging_inbox", 180),
        ("messaging_quarantine", 180),
    ] {
        let (rows, bytes): (i64, i64) = sqlx::query_as(&format!(
            "SELECT count(*),pg_total_relation_size('{table}') FROM {table}"
        ))
        .fetch_one(&pool)
        .await
        .unwrap();
        println!(
            "CAPACITY_RELATION={}",
            serde_json::json!([format!("admin.{table}"), rows, bytes, days])
        );
    }
    pool.close().await;
}

#[tokio::test]
#[ignore = "run with the disposable product integration harness"]
async fn feed_atomicity_pagination_and_retention() {
    let url = std::env::var("MESSAGING_PRODUCT_TEST_DATABASE_URL").expect("isolated test URL");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap();
    let name: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(name, "admin_messaging_test");
    sqlx::raw_sql(include_str!(
        "../../migration/migrations/0006_platform_messaging.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../../migration/migrations/0008_messaging_maintenance.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    let store = FeedStore(pool.clone());
    let now = Utc::now();
    let correlation = Uuid::new_v4();
    let mut events = vec![];
    for i in 0..43 {
        events.push(Envelope::new(
            Uuid::new_v4(),
            now + Duration::seconds(i),
            Some(correlation),
            PipelineFinished {
                project_id: Uuid::new_v4(),
                pipeline_id: Uuid::new_v4(),
                completion_seq: 1,
                status: if i % 2 == 0 {
                    PipelineStatus::Success
                } else {
                    PipelineStatus::Failed
                },
                finished_at: now + Duration::seconds(i),
            },
        ));
    }
    let handler = PlatformFeedHandler;
    let event = &events[0];
    let fingerprint = event.fingerprint().unwrap();
    let key = InboxKey {
        handler_id: PlatformFeedHandler::HANDLER_ID,
        source: &event.source,
        event_id: event.id,
    };
    let mut tx = pool.begin().await.unwrap();
    assert_eq!(
        begin_inbox(&mut tx, &key, &fingerprint).await.unwrap(),
        InboxDisposition::Process
    );
    handler.handle(&mut tx, event).await.unwrap();
    tx.rollback().await.unwrap();
    assert!(store.detail(event.id).await.unwrap().is_none());
    for event in &events {
        let key = InboxKey {
            handler_id: PlatformFeedHandler::HANDLER_ID,
            source: &event.source,
            event_id: event.id,
        };
        let fingerprint = event.fingerprint().unwrap();
        let mut tx = pool.begin().await.unwrap();
        assert_eq!(
            begin_inbox(&mut tx, &key, &fingerprint).await.unwrap(),
            InboxDisposition::Process
        );
        handler.handle(&mut tx, event).await.unwrap();
        assert!(mark_processed(&mut tx, &key, &fingerprint).await.unwrap());
        tx.commit().await.unwrap();
    }
    let mut tx = pool.begin().await.unwrap();
    assert_eq!(
        begin_inbox(&mut tx, &key, &fingerprint).await.unwrap(),
        InboxDisposition::Duplicate
    );
    tx.commit().await.unwrap();
    let page = store.list(&FeedFilter::default(), 20, 40).await.unwrap();
    assert_eq!(page.total, 43);
    assert_eq!(page.items.len(), 3);
    let filtered = store
        .list(
            &FeedFilter {
                status: Some("failed".into()),
                correlation_id: Some(correlation),
                ..Default::default()
            },
            20,
            20,
        )
        .await
        .unwrap();
    assert_eq!(filtered.total, 21);
    assert_eq!(filtered.items.len(), 1);
    let time = store
        .list(
            &FeedFilter {
                occurred_from: Some(now + Duration::seconds(40)),
                occurred_to: Some(now + Duration::seconds(42)),
                ..Default::default()
            },
            50,
            0,
        )
        .await
        .unwrap();
    assert_eq!(time.total, 2);
    let mut conflict = events[0].clone();
    conflict.id = Uuid::new_v4();
    let mut tx = pool.begin().await.unwrap();
    assert!(handler.handle(&mut tx, &conflict).await.is_err());
    tx.rollback().await.unwrap();
    sqlx::query("UPDATE platform_events SET received_at=now()-interval '91 days' WHERE id=$1")
        .bind(event.id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE messaging_inbox SET updated_at=now()-interval '181 days' WHERE event_id=$1",
    )
    .bind(event.id)
    .execute(&pool)
    .await
    .unwrap();
    let pending = Uuid::new_v4();
    sqlx::query("INSERT INTO messaging_inbox (handler_id,source,event_id,fingerprint,updated_at) VALUES ('test','ci-cd',$1,$2,now()-interval '91 days')").bind(pending).bind(fingerprint.as_slice()).execute(&pool).await.unwrap();
    assert_eq!(store.cleanup().await.unwrap(), 1);
    let result = sdlc_messaging::postgres::maintenance::cleanup(
        &pool,
        &sdlc_messaging::postgres::maintenance::Scope::Inbox {
            handler: PlatformFeedHandler::HANDLER_ID.into(),
            source: "ci-cd".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(result.inbox, 1);
    assert_eq!(
        store
            .list(&FeedFilter::default(), 50, 0)
            .await
            .unwrap()
            .total,
        42
    );
    let pending_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM messaging_inbox WHERE event_id=$1)")
            .bind(pending)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(pending_exists);
    pool.close().await;
}

#[tokio::test]
#[ignore = "disposable product messaging harness only"]
async fn product_feed_transport() {
    product_feed_transport_source(false).await;
}

#[tokio::test]
#[ignore = "owned standalone PostgreSQL/NATS harness; SDK publisher fixtures"]
async fn standalone_product_feed_transport() {
    product_feed_transport_source(true).await;
}

async fn product_feed_transport_source(standalone: bool) {
    use sdlc_messaging::{
        ADMIN_FEED_CONSUMER, Bus, NatsConfig, PLATFORM_STREAM,
        postgres::consumer::{self, DeliveryOutcome},
        provisioning::{ProvisionMode, provision_qa as provision},
    };
    use std::{sync::Arc, time::Duration as Wait};
    let url = std::env::var("MESSAGING_PRODUCT_TEST_DATABASE_URL").unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&url)
        .await
        .unwrap();
    let name: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(name, "admin_messaging_transport_test");
    sqlx::raw_sql(include_str!(
        "../../migration/migrations/0006_platform_messaging.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../../migration/migrations/0008_messaging_maintenance.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    let connect = |role: &str| NatsConfig {
        servers: vec!["nats://broker:4222".into()],
        username: Some(role.into()),
        password_file: Some(format!("/secrets/{role}").into()),
        client_name: role.into(),
        require_tls: false,
    };
    let operator = Bus::connect(connect("operator")).await.unwrap();
    provision(&operator, ProvisionMode::Apply).await.unwrap();
    operator.drain().await.unwrap();
    let ids: Vec<Uuid> = if standalone {
        let publisher = Bus::connect(connect("cicd")).await.unwrap();
        let mut ids = Vec::new();
        for completion_seq in 1..=7 {
            let event = Envelope::new(
                Uuid::new_v4(),
                Utc::now(),
                None,
                PipelineFinished {
                    project_id: Uuid::new_v4(),
                    pipeline_id: Uuid::new_v4(),
                    completion_seq,
                    status: PipelineStatus::Success,
                    finished_at: Utc::now(),
                },
            );
            publisher.publish(&event).await.unwrap();
            ids.push(event.id);
        }
        publisher.drain().await.unwrap();
        ids
    } else {
        tokio::fs::write("/control/provisioned", "ready")
            .await
            .unwrap();
        serde_json::from_slice(&tokio::fs::read("/control/events.json").await.unwrap()).unwrap()
    };
    assert_eq!(ids.len(), 7);
    let bus = Bus::connect(connect("admin")).await.unwrap();
    let receiver = bus
        .consumer(PLATFORM_STREAM, ADMIN_FEED_CONSUMER)
        .await
        .unwrap();
    // The stand pauses the broker before the CI/CD publisher starts.
    if !standalone {
        tokio::time::timeout(Wait::from_secs(90), async {
            while !std::path::Path::new("/control/broker-resumed").exists() {
                tokio::time::sleep(Wait::from_millis(100)).await;
            }
        })
        .await
        .unwrap();
    }
    let handler = Arc::new(PlatformFeedHandler);
    let first = tokio::time::timeout(Wait::from_secs(30), async {
        loop {
            if let Some(delivery) = receiver.fetch(1).await.unwrap().pop() {
                break delivery;
            }
        }
    })
    .await
    .unwrap();
    let event = first.decode::<PipelineFinished>().unwrap();
    // Simulate consumer death after product commit, before ACK. The original
    // delivery is dropped; JetStream must naturally redeliver after AckWait.
    let key = InboxKey {
        handler_id: PlatformFeedHandler::HANDLER_ID,
        source: &event.source,
        event_id: event.id,
    };
    let fingerprint = event.fingerprint().unwrap();
    let mut tx = pool.begin().await.unwrap();
    assert_eq!(
        begin_inbox(&mut tx, &key, &fingerprint).await.unwrap(),
        InboxDisposition::Process
    );
    handler.handle(&mut tx, &event).await.unwrap();
    mark_processed(&mut tx, &key, &fingerprint).await.unwrap();
    tx.commit().await.unwrap();
    drop(first);
    let mut duplicates = 0;
    let mut processed = 1;
    tokio::time::timeout(Wait::from_secs(55), async {
        while processed < 7 || duplicates < 1 {
            for delivery in receiver.fetch(8).await.unwrap() {
                match consumer::process_delivery::<PipelineFinished, _>(&pool, &*handler, &delivery)
                    .await
                    .unwrap()
                {
                    DeliveryOutcome::Processed => processed += 1,
                    DeliveryOutcome::Duplicate => duplicates += 1,
                    other => panic!("unexpected {other:?}"),
                }
            }
        }
    })
    .await
    .unwrap();
    let rows: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM platform_events")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(rows.len(), 7);
    for id in ids {
        assert!(rows.contains(&id));
    }
    let metrics = receiver.metrics().await.unwrap();
    assert_eq!(metrics.unacknowledged_messages, 0);
    bus.drain().await.unwrap();
    pool.close().await;
    println!(
        "Product feed: seven typed facts received (standalone SDK fixtures={standalone}), real redelivery after commit-before-ACK crash produces one effect, all messages ACKed"
    );
}
