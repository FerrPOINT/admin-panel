//! Independent consumer, retention and protected producer observations.

use admin_panel_infra::messaging::{
    ConsumerObservation, FeedStore, MessagingRuntime, PlatformFeedHandler,
};
use sdlc_messaging::{
    ADMIN_FEED_CONSUMER, Bus, NatsConfig, PLATFORM_STREAM,
    contracts::{PipelineFinished, ProducerDiagnostics},
    postgres::consumer,
};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tokio::{sync::watch, task::JoinHandle};

pub fn spawn(
    pool: PgPool,
    state: Arc<MessagingRuntime>,
    config: Option<NatsConfig>,
    shutdown: watch::Receiver<bool>,
) -> Vec<JoinHandle<()>> {
    let mut tasks = vec![];
    tasks.push(tokio::spawn(sdlc_messaging::postgres::maintenance::run(
        pool.clone(),
        sdlc_messaging::postgres::maintenance::Scope::Inbox {
            handler: "admin-platform-feed-v1".into(),
            source: "ci-cd".into(),
        },
        shutdown.clone(),
    )));
    if let Some(config) = config {
        tasks.push(tokio::spawn(run_consumer(
            pool.clone(),
            state.clone(),
            config,
            shutdown.clone(),
        )));
    }
    tasks.push(tokio::spawn(retention(pool, shutdown.clone())));
    tasks.push(tokio::spawn(observe_producer(state, shutdown)));
    tasks
}

async fn pause(shutdown: &mut watch::Receiver<bool>, seconds: u64) -> bool {
    if *shutdown.borrow() {
        return true;
    }
    tokio::select! { _=shutdown.changed()=>true, _=tokio::time::sleep(Duration::from_secs(seconds))=>false }
}

async fn run_consumer(
    pool: PgPool,
    state: Arc<MessagingRuntime>,
    config: NatsConfig,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        if *shutdown.borrow() {
            break;
        }
        let bus = match Bus::connect(config.clone()).await {
            Ok(bus) => bus,
            Err(_) => {
                state.consumer.write().await.stale = true;
                if pause(&mut shutdown, 5).await {
                    break;
                }
                continue;
            }
        };
        let receiver = match bus.consumer(PLATFORM_STREAM, ADMIN_FEED_CONSUMER).await {
            Ok(receiver) => receiver,
            Err(_) => {
                state.consumer.write().await.stale = true;
                let _ = bus.drain().await;
                if pause(&mut shutdown, 5).await {
                    break;
                }
                continue;
            }
        };
        let work = consumer::run::<PipelineFinished, _>(
            receiver.clone(),
            pool.clone(),
            Arc::new(PlatformFeedHandler),
            shutdown.clone(),
        );
        tokio::pin!(work);
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        loop {
            tokio::select! {
                _=&mut work=>break,
                _=interval.tick()=>{
                    let metrics=receiver.metrics().await;
                    let stream=bus.stream_metrics(PLATFORM_STREAM).await.ok();
                    let mut observation=state.consumer.write().await;
                    observation.connection=Some(bus.status());
                    match metrics {
                        Ok(metrics)=>*observation=ConsumerObservation {observed_at:Some(metrics.observed_at),connection:Some(bus.status()),metrics:Some(metrics),stream,stale:bus.status()!=sdlc_messaging::ConnectionStatus::Connected},
                        Err(_)=>observation.stale=true,
                    }
                }
            }
        }
        let _ = bus.drain().await;
        state.consumer.write().await.stale = true;
        if pause(&mut shutdown, 5).await {
            break;
        }
    }
}

async fn retention(pool: PgPool, mut shutdown: watch::Receiver<bool>) {
    let store = FeedStore(pool);
    loop {
        if *shutdown.borrow() || shutdown.has_changed().is_err() {
            break;
        }
        let removed = match tokio::time::timeout(Duration::from_secs(10), store.cleanup()).await {
            Ok(Ok(n)) => n,
            failed => {
                let code = if failed.is_err() {
                    "operation_timeout"
                } else {
                    "storage_unavailable"
                };
                let save=sqlx::query("INSERT INTO messaging_maintenance_state(scope,last_attempt_at,last_error_code) VALUES('feed:platform_events',now(),$1) ON CONFLICT(scope) DO UPDATE SET last_attempt_at=EXCLUDED.last_attempt_at,last_error_code=EXCLUDED.last_error_code").bind(code).execute(&store.0);
                let _ = tokio::time::timeout(Duration::from_secs(5), save).await;
                tracing::warn!("messaging retention unavailable");
                0
            }
        };
        if pause(&mut shutdown, if removed >= 500 { 1 } else { 60 }).await {
            break;
        }
    }
}

async fn observe_producer(state: Arc<MessagingRuntime>, mut shutdown: watch::Receiver<bool>) {
    let Ok(url) = std::env::var("ADMINP_MESSAGING_PRODUCER_STATUS_URL") else {
        return;
    };
    let Ok(secret_path) = std::env::var("ADMINP_MESSAGING_PRODUCER_STATUS_TOKEN_FILE") else {
        return;
    };
    let Ok(client) = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(2))
        .build()
    else {
        return;
    };
    loop {
        let result = async {
            let secret = tokio::fs::read_to_string(&secret_path).await.ok()?;
            if secret.trim().is_empty() {
                return None;
            }
            let response = client
                .get(&url)
                .bearer_auth(secret.trim())
                .send()
                .await
                .ok()?;
            if !response.status().is_success() {
                return None;
            }
            // Bounded read even if the trusted producer accidentally returns a large body.
            if response.content_length().is_some_and(|n| n > 16_384) {
                return None;
            }
            let mut response = response;
            let mut body = vec![];
            while let Some(chunk) = response.chunk().await.ok()? {
                if body.len() + chunk.len() > 16_384 {
                    return None;
                }
                body.extend_from_slice(&chunk);
            }
            let status: ProducerDiagnostics = serde_json::from_slice(&body).ok()?;
            let age = chrono::Utc::now()
                .signed_duration_since(status.observed_at)
                .num_seconds();
            if !(-5..=30).contains(&age) {
                return None;
            }
            Some(status)
        }
        .await;
        let mut observation = state.producer.write().await;
        match result {
            Some(status) => {
                observation.last_success_at = Some(chrono::Utc::now());
                observation.status = Some(status);
                observation.stale = false;
            }
            None => observation.stale = true,
        }
        drop(observation);
        if pause(&mut shutdown, 10).await {
            break;
        }
    }
}
