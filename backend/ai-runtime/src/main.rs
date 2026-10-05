use admin_panel_ai_runtime::{
    codex::CodexClient,
    config::RuntimeConfig,
    error::RuntimeError,
    http::{RuntimeState, ServiceClient},
    openrouter::OpenRouter,
    vault::Vault,
};
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        // No upstream body, configuration value, path or credentials in stderr.
        eprintln!("ai-runtime: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), RuntimeError> {
    let arguments: Vec<String> = std::env::args().collect();
    if arguments.get(1).map(String::as_str) == Some("inspect-schema") {
        let path = arguments.get(2).ok_or(RuntimeError::InvalidRequest)?;
        println!(
            "{}",
            admin_panel_ai_runtime::codex::inspect_schema(std::path::Path::new(path))?
        );
        return Ok(());
    }
    let config = RuntimeConfig::from_env()?;
    if arguments.get(1).map(String::as_str) == Some("init") {
        return Vault::initialize(&config.state_dir, &config.key_file, &config.workspace);
    }
    if arguments.len() != 1 && !(arguments.len() == 2 && arguments[1] == "check-state") {
        return Err(RuntimeError::InvalidRequest);
    }
    let mut vault = Vault::open(&config.state_dir, &config.key_file, &config.workspace)?;
    let clients = ServiceClient::load(&config.clients_file)?;
    let grant_verifier = config
        .execution_trust_file
        .as_deref()
        .map(admin_panel_ai_runtime::execution_grant::GrantVerifier::load)
        .transpose()?;
    if clients.iter().any(ServiceClient::uses_inference) && grant_verifier.is_none() {
        return Err(RuntimeError::Configuration);
    }
    if arguments.get(1).map(String::as_str) == Some("check-state") {
        return Ok(());
    }
    // A persisted intent cannot be repeated after restart, including restore mode.
    vault.reconcile_inference_restart(chrono::Utc::now())?;
    let (codex, codex_error) = if config.external_calls_enabled {
        let result =
            match admin_panel_ai_runtime::http::restore_managed_auth(&config.codex_home, &vault) {
                Ok(()) => CodexClient::start(&config).await,
                Err(error) => Err(error),
            };
        match result {
            Ok(client) => (Some(client), None),
            Err(error) => (None, Some(error)),
        }
    } else {
        (None, Some(RuntimeError::ExternalCallsDisabled))
    };
    let state = Arc::new(RuntimeState {
        vault: Mutex::new(vault),
        clients,
        grant_verifier,
        openrouter: OpenRouter::production()?,
        codex,
        codex_error,
        codex_home: config.codex_home.clone(),
        auth_checkpoint_ready: std::sync::atomic::AtomicBool::new(true),
        external_calls_enabled: config.external_calls_enabled,
    });
    if state.codex.is_some() {
        admin_panel_ai_runtime::http::reconcile_native_startup(&state).await?;
        admin_panel_ai_runtime::http::spawn_auth_checkpoint(state.clone())?;
    }
    let listener = tokio::net::TcpListener::bind(config.listen)
        .await
        .map_err(|_| RuntimeError::Configuration)?;
    axum::serve(listener, admin_panel_ai_runtime::http::router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|_| RuntimeError::Unavailable)
}
