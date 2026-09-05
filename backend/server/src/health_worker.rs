//! Background health probe for registered services.
//!
//! For every active registry entry whose approved declaration grants
//! `health.read`, periodically GET `<base_url>/health/live` and persist the
//! outcome. The dashboard badge reads these columns; nothing here mutates
//! service state.

use std::time::Duration;

use admin_panel_infra::registry::RegistryStore;

/// Probe interval; short for the local stand, still gentle upstream.
const INTERVAL: Duration = Duration::from_secs(60);
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

pub fn spawn(store: RegistryStore) {
    tokio::spawn(async move {
        loop {
            probe_all(&store).await;
            tokio::time::sleep(INTERVAL).await;
        }
    });
}

async fn probe_all(store: &RegistryStore) {
    let Ok(targets) = store.list_health_targets().await else {
        tracing::warn!("health worker: cannot list targets");
        return;
    };
    if targets.is_empty() {
        return;
    }
    let count = targets.len();
    let client = match reqwest::Client::builder().timeout(PROBE_TIMEOUT).build() {
        Ok(client) => client,
        Err(error) => {
            tracing::warn!(%error, "health worker: cannot build client");
            return;
        }
    };
    for (key, base_url) in targets {
        // Registry URLs are user-facing (localhost). From inside the container
        // the host frontends are reachable via host.docker.internal.
        let base = base_url
            .trim_end_matches('/')
            .replace("://localhost:", "://host.docker.internal:")
            .replace("://127.0.0.1:", "://host.docker.internal:");
        let url = format!("{base}/health/live");
        let (status, detail) = match client.get(&url).send().await {
            Ok(response) => {
                let code = response.status().as_u16();
                if response.status().is_success() {
                    ("healthy", Some(format!("HTTP {code}")))
                } else {
                    ("unreachable", Some(format!("HTTP {code}")))
                }
            }
            Err(error) => ("unreachable", Some(error.to_string())),
        };
        if let Err(error) = store.set_health(&key, status, detail.as_deref()).await {
            tracing::warn!(%error, service = %key, "health worker: cannot persist result");
        }
    }
    tracing::debug!(count, "health worker pass complete");
}
