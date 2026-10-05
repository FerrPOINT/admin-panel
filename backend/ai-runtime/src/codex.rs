//! Bounded, bidirectional stdio RPC; no checkout or inherited provider credentials.
use crate::{config::RuntimeConfig, error::RuntimeError, vault::check_private_path};
use serde_json::{Value, json};
use std::{collections::HashMap, path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
    sync::{broadcast, mpsc, oneshot},
};

const MAX_FRAME: usize = 2 * 1024 * 1024;
pub const NATIVE_PROVIDER_ID: &str = "sdlc2_chatgpt";
const DISABLED_FEATURES: [&str; 40] = [
    "shell_tool",
    "view_image",
    "unified_exec",
    "apply_patch_freeform",
    "multi_agent",
    "apps",
    "plugins",
    "remote_plugin",
    "tool_suggest",
    "hooks",
    "memories",
    "auth_elicitation",
    "mentions_v2",
    "browser_use",
    "browser_use_external",
    "browser_use_full_cdp_access",
    "code_mode",
    "code_mode_host",
    "computer_use",
    "daemon_auto_start",
    "default_mode_request_user_input",
    "enable_mcp_apps",
    "goals",
    "image_generation",
    "in_app_browser",
    "in_app_local_automation",
    "multi_agent_v2",
    "recommended_plugins",
    "request_permissions_tool",
    "realtime_conversation",
    "shell_snapshot",
    "skill_mcp_dependency_install",
    "skill_search",
    "sleep_tool",
    "step_model_switching",
    "system_proxy_fallback",
    "tool_call_mcp_elicitation",
    "unbounded_connection_retries",
    "unified_exec_tty",
    "workspace_dependencies",
];

#[derive(Clone)]
pub struct CodexClient {
    commands: mpsc::Sender<Request>,
    events: broadcast::Sender<Value>,
}

enum Request {
    Call {
        method: String,
        params: Value,
        response: oneshot::Sender<Result<Value, RuntimeError>>,
    },
    Reject {
        id: Value,
    },
}

pub fn isolated_command(
    binary: &Path,
    home: &Path,
    workdir: &Path,
) -> Result<Command, RuntimeError> {
    let mut command = Command::new(binary);
    command
        .env_clear()
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("CODEX_HOME", home)
        .env("TMPDIR", home)
        .env("TMP", home)
        .env("TEMP", home)
        .env("PATH", binary.parent().unwrap_or(Path::new("/usr/bin")))
        .current_dir(workdir)
        .kill_on_drop(true);
    // Windows process startup and trust store; never inherit the user's env wholesale.
    #[cfg(windows)]
    {
        if let Some(root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", root);
        }
        command.creation_flags(0x08000000);
    }
    crate::native_filesystem::attach(&mut command, binary, home, workdir)?;
    Ok(command)
}

impl CodexClient {
    pub async fn start(config: &RuntimeConfig) -> Result<Self, RuntimeError> {
        config.validate()?;
        let model_policy = crate::native_model_policy::validated_path(&config.codex_binary)?;
        for path in [
            &config.codex_binary,
            &config.codex_home,
            &config.codex_workdir,
        ] {
            check_private_path(path)?;
        }
        // Both directories must be explicitly provisioned, isolated from source trees.
        if !config.codex_home.is_dir()
            || !config.codex_workdir.is_dir()
            || config
                .codex_workdir
                .read_dir()
                .map_err(|_| RuntimeError::Configuration)?
                .next()
                .is_some()
        {
            return Err(RuntimeError::Configuration);
        }
        let output = isolated_command(
            &config.codex_binary,
            &config.codex_home,
            &config.codex_workdir,
        )?
        .arg("--version")
        .output()
        .await
        .map_err(|_| RuntimeError::NativeFilesystemIsolation)?;
        let version = String::from_utf8(output.stdout).map_err(|_| RuntimeError::CodexVersion)?;
        if !output.status.success()
            || version.trim() != format!("codex-cli {}", config.codex_version)
        {
            return Err(RuntimeError::CodexVersion);
        }
        let mut command = isolated_command(
            &config.codex_binary,
            &config.codex_home,
            &config.codex_workdir,
        )?;
        command.args([
            "app-server",
            "--listen",
            "stdio://",
            "-c",
            "model_provider=\"sdlc2_chatgpt\"",
            "-c",
            "model_providers.sdlc2_chatgpt.name=\"SDLC2 ChatGPT\"",
            "-c",
            "model_providers.sdlc2_chatgpt.wire_api=\"responses\"",
            "-c",
            "model_providers.sdlc2_chatgpt.requires_openai_auth=true",
            "-c",
            "model_providers.sdlc2_chatgpt.request_max_retries=0",
            "-c",
            "model_providers.sdlc2_chatgpt.stream_max_retries=0",
            "-c",
            "cli_auth_credentials_store=\"file\"",
            "-c",
            "features.skip_host_skill_discovery=true",
            "-c",
            "apps._default.enabled=false",
            "-c",
            "forced_login_method=\"chatgpt\"",
            "-c",
            "web_search=\"disabled\"",
            "-c",
            "sandbox_mode=\"read-only\"",
            "-c",
            "approval_policy=\"never\"",
            "-c",
            "agents.enabled=false",
            "-c",
            "tools.experimental_request_user_input.enabled=false",
            "-c",
            "tools.update_plan.enabled=false",
        ]);
        command.arg("-c").arg(format!(
            "model_catalog_json={}",
            json!(model_policy.to_str().ok_or(RuntimeError::Configuration)?)
        ));
        for flag in DISABLED_FEATURES {
            command.arg("-c").arg(format!("features.{flag}=false"));
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| RuntimeError::NativeFilesystemIsolation)?;
        let stdin = child.stdin.take().ok_or(RuntimeError::Protocol)?;
        let stdout = child.stdout.take().ok_or(RuntimeError::Protocol)?;
        let (commands, incoming) = mpsc::channel(32);
        let (events, _) = broadcast::channel(256);
        tokio::spawn(rpc_loop(child, stdin, stdout, incoming, events.clone()));
        let client = Self { commands, events };
        client.call("initialize", json!({"clientInfo":{"name":"sdlc2-ai-runtime","version":"0.1.0"},"capabilities":{"experimentalApi":true}})).await?;
        // initialized is a notification, not a call expecting a result.
        client
            .commands
            .send(Request::Call {
                method: "initialized".into(),
                params: json!({}),
                response: oneshot::channel().0,
            })
            .await
            .map_err(|_| RuntimeError::Unavailable)?;
        let readback = client
            .call("config/read", json!({"includeLayers":true}))
            .await?;
        check_provider_config(&readback)?;
        check_model_policy_path(&readback, &model_policy)?;
        Ok(client)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.events.subscribe()
    }

    pub async fn call(&self, method: &str, params: Value) -> Result<Value, RuntimeError> {
        let (response, receiver) = oneshot::channel();
        self.commands
            .send(Request::Call {
                method: method.into(),
                params,
                response,
            })
            .await
            .map_err(|_| RuntimeError::Unavailable)?;
        tokio::time::timeout(Duration::from_secs(45), receiver)
            .await
            .map_err(|_| RuntimeError::Unavailable)?
            .map_err(|_| RuntimeError::Unavailable)?
    }

    /// Until a verified caller tool continuation is installed, decline every tool.
    pub async fn reject_tool(&self, id: Value) -> Result<(), RuntimeError> {
        self.commands
            .send(Request::Reject { id })
            .await
            .map_err(|_| RuntimeError::Unavailable)
    }
}

/// Readback is mandatory: ignored CLI settings cannot silently enable replay.
fn check_provider_config(response: &Value) -> Result<(), RuntimeError> {
    let config = &response["config"];
    if config["agents"]["enabled"].as_bool() != Some(false) {
        return Err(RuntimeError::NativeToolPolicy);
    }
    // The pinned Config projection drops these nested tools fields. Read their
    // exact session-layer values and effective origins instead of guessing defaults.
    let mut sessions = response["layers"]
        .as_array()
        .ok_or(RuntimeError::NativeToolPolicy)?
        .iter()
        .filter(|layer| layer["name"]["type"] == "sessionFlags");
    let session = sessions.next().ok_or(RuntimeError::NativeToolPolicy)?;
    if sessions.next().is_some() || !session["version"].is_string() {
        return Err(RuntimeError::NativeToolPolicy);
    }
    for (key, pointer) in [
        ("agents.enabled", "/agents/enabled"),
        (
            "tools.experimental_request_user_input.enabled",
            "/tools/experimental_request_user_input/enabled",
        ),
        ("tools.update_plan.enabled", "/tools/update_plan/enabled"),
    ] {
        let origin = &response["origins"][key];
        if session["config"].pointer(pointer).and_then(Value::as_bool) != Some(false)
            || origin["name"] != session["name"]
            || origin["version"] != session["version"]
        {
            return Err(RuntimeError::NativeToolPolicy);
        }
    }
    let provider = &config["model_providers"][NATIVE_PROVIDER_ID];
    if config["model_provider"].as_str() != Some(NATIVE_PROVIDER_ID)
        || provider["request_max_retries"].as_u64() != Some(0)
        || provider["stream_max_retries"].as_u64() != Some(0)
        || provider["requires_openai_auth"].as_bool() != Some(true)
        || provider["wire_api"].as_str() != Some("responses")
        || !provider["base_url"].is_null()
        || !provider["env_key"].is_null()
        || !provider["experimental_bearer_token"].is_null()
        || config["forced_login_method"].as_str() != Some("chatgpt")
        || config["cli_auth_credentials_store"].as_str() != Some("file")
        || config["web_search"].as_str() != Some("disabled")
        || config["sandbox_mode"].as_str() != Some("read-only")
        || config["approval_policy"].as_str() != Some("never")
        || config["apps"]["_default"]["enabled"].as_bool() != Some(false)
        || config["features"]["skip_host_skill_discovery"].as_bool() != Some(true)
        || DISABLED_FEATURES
            .iter()
            .any(|flag| config["features"][flag].as_bool() != Some(false))
    {
        return Err(RuntimeError::Configuration);
    }
    Ok(())
}

fn check_model_policy_path(response: &Value, policy: &Path) -> Result<(), RuntimeError> {
    if response["config"]["model_catalog_json"].as_str() != policy.to_str() {
        return Err(RuntimeError::NativeToolPolicy);
    }
    Ok(())
}

async fn rpc_loop(
    mut child: Child,
    mut stdin: tokio::process::ChildStdin,
    stdout: tokio::process::ChildStdout,
    mut incoming: mpsc::Receiver<Request>,
    events: broadcast::Sender<Value>,
) {
    let mut reader = BufReader::new(stdout);
    let mut line = Vec::new();
    let mut pending: HashMap<u64, oneshot::Sender<Result<Value, RuntimeError>>> = HashMap::new();
    let mut sequence: u64 = 0;
    loop {
        tokio::select! {
            request = incoming.recv() => {
                let Some(request) = request else { break; };
                // Prune timed-out waiters; none of their commands are retried.
                pending.retain(|_, sender| !sender.is_closed());
                let message = match request {
                    Request::Call { method, params, response } if method == "initialized" => {
                        drop(response); json!({"method":method,"params":params})
                    }
                    Request::Call { method, params, response } => {
                        if pending.len() >= 32 {
                            let _ = response.send(Err(RuntimeError::Unavailable));
                            continue;
                        }
                        sequence += 1; pending.insert(sequence, response);
                        json!({"id":sequence,"method":method,"params":params})
                    }
                    Request::Reject { id } => json!({"id":id,"error":{"code":-32601,"message":"tool_execution_disabled"}}),
                };
                let Ok(mut bytes) = serde_json::to_vec(&message) else { break; };
                if bytes.len() > MAX_FRAME { break; }
                bytes.push(b'\n');
                if stdin.write_all(&bytes).await.is_err() { break; }
            }
            read = bounded_frame(&mut reader, &mut line) => {
                if !matches!(read, Ok(n) if n > 0) || line.len() > MAX_FRAME { break; }
                let Ok(message) = serde_json::from_slice::<Value>(&line) else { break; };
                line.clear();
                if message.get("method").is_some() {
                    // Server requests require a response. No implicit execution or approval.
                    if message.get("id").is_some() {
                        let reply = json!({"id":message["id"],"error":{"code":-32601,"message":"tool_execution_disabled"}});
                        if stdin.write_all(format!("{reply}\n").as_bytes()).await.is_err() { break; }
                    }
                    let _ = events.send(message);
                } else if let Some(id) = message.get("id").and_then(Value::as_u64)
                    && let Some(sender) = pending.remove(&id) {
                    let result = if message.get("error").is_some() { Err(RuntimeError::Protocol) }
                        else { message.get("result").cloned().ok_or(RuntimeError::Protocol) };
                    let _ = sender.send(result);
                }
            }
        }
    }
    for (_, sender) in pending {
        let _ = sender.send(Err(RuntimeError::Unavailable));
    }
    let _ = child.kill().await;
    let _ = events.send(json!({"method":"runtime/disconnected","params":{}}));
}

/// Version-specific feasibility is independent of account or model availability.
pub fn inspect_schema(directory: &Path) -> Result<Value, RuntimeError> {
    let read = |name: &str| -> Result<Value, RuntimeError> {
        serde_json::from_slice(
            &std::fs::read(directory.join("v2").join(name)).map_err(|_| RuntimeError::Protocol)?,
        )
        .map_err(|_| RuntimeError::Protocol)
    };
    let thread = read("ThreadStartParams.json")?;
    let turn = read("TurnStartParams.json")?;
    Ok(
        json!({"schema_version":1,"dynamic_tools_declared":thread["properties"].get("dynamicTools").is_some(),
        "structured_output_declared":turn["properties"].get("outputSchema").is_some(),
        "model_override_declared":thread["properties"].get("model").is_some(),
        "live_capabilities_verified":false}),
    )
}

async fn bounded_frame<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    line: &mut Vec<u8>,
) -> Result<usize, RuntimeError> {
    loop {
        let available = reader
            .fill_buf()
            .await
            .map_err(|_| RuntimeError::Protocol)?;
        if available.is_empty() {
            return Ok(0);
        }
        let end = available.iter().position(|b| *b == b'\n').map(|n| n + 1);
        let count = end.unwrap_or(available.len());
        if line.len() + count > MAX_FRAME {
            return Err(RuntimeError::Protocol);
        }
        line.extend_from_slice(&available[..count]);
        reader.consume(count);
        if end.is_some() {
            return Ok(line.len());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture_config() -> Value {
        let mut good = json!({"config":{"model_provider":"sdlc2_chatgpt","model_providers":{"sdlc2_chatgpt":{
            "wire_api":"responses","requires_openai_auth":true,"request_max_retries":0,"stream_max_retries":0}}}});
        good["config"]["features"] = json!({});
        for flag in DISABLED_FEATURES {
            good["config"]["features"][flag] = json!(false);
        }
        good["config"]["forced_login_method"] = json!("chatgpt");
        good["config"]["cli_auth_credentials_store"] = json!("file");
        good["config"]["web_search"] = json!("disabled");
        good["config"]["sandbox_mode"] = json!("read-only");
        good["config"]["approval_policy"] = json!("never");
        good["config"]["apps"] = json!({"_default":{"enabled":false}});
        good["config"]["features"]["view_image"] = json!(false);
        good["config"]["features"]["skip_host_skill_discovery"] = json!(true);
        good["config"]["agents"] = json!({"enabled":false});
        good["config"]["tools"] = json!({"web_search":null});
        let metadata = json!({"name":{"type":"sessionFlags"},"version":"sha256:fixture"});
        good["layers"] = json!([{"name":metadata["name"],"version":metadata["version"],"config":{
            "agents":{"enabled":false},"tools":{"experimental_request_user_input":{"enabled":false},"update_plan":{"enabled":false}}}}]);
        good["origins"] = json!({"agents.enabled":metadata,"tools.experimental_request_user_input.enabled":metadata,"tools.update_plan.enabled":metadata});
        good
    }

    #[test]
    fn config_readback_rejects_implicit_retries_and_alternate_credentials() {
        let good = fixture_config();
        assert!(check_provider_config(&good).is_ok());
        for value in [json!(false), json!(null)] {
            let mut bad = good.clone();
            bad["config"]["features"]["skip_host_skill_discovery"] = value;
            assert_eq!(
                check_provider_config(&bad),
                Err(RuntimeError::Configuration)
            );
        }
        for value in [json!(true), json!(null)] {
            let mut bad = good.clone();
            bad["config"]["features"]["view_image"] = value;
            assert_eq!(
                check_provider_config(&bad),
                Err(RuntimeError::Configuration)
            );
        }
        for flag in DISABLED_FEATURES {
            for value in [json!(true), json!(null)] {
                let mut bad = good.clone();
                bad["config"]["features"][flag] = value;
                assert_eq!(
                    check_provider_config(&bad),
                    Err(RuntimeError::Configuration)
                );
            }
        }
        for (key, value) in [
            ("forced_login_method", "api"),
            ("cli_auth_credentials_store", "keyring"),
            ("web_search", "live"),
            ("sandbox_mode", "danger-full-access"),
            ("approval_policy", "on-request"),
        ] {
            let mut bad = good.clone();
            bad["config"][key] = json!(value);
            assert_eq!(
                check_provider_config(&bad),
                Err(RuntimeError::Configuration)
            );
        }
        let mut apps = good.clone();
        apps["config"]["apps"]["_default"]["enabled"] = json!(true);
        assert_eq!(
            check_provider_config(&apps),
            Err(RuntimeError::Configuration)
        );
        for field in ["request_max_retries", "stream_max_retries"] {
            let mut bad = good.clone();
            bad["config"]["model_providers"][NATIVE_PROVIDER_ID][field] = json!(1);
            assert_eq!(
                check_provider_config(&bad),
                Err(RuntimeError::Configuration)
            );
        }
        for field in ["env_key", "base_url", "experimental_bearer_token"] {
            let mut bad = good.clone();
            bad["config"]["model_providers"][NATIVE_PROVIDER_ID][field] = json!("unapproved");
            assert_eq!(
                check_provider_config(&bad),
                Err(RuntimeError::Configuration)
            );
        }
        assert_eq!(
            check_provider_config(&json!({"config":{}})),
            Err(RuntimeError::NativeToolPolicy)
        );
    }

    #[test]
    fn native_tool_policy_requires_disabled_controls_and_exact_catalog_readback() {
        let good = fixture_config();
        for field in ["agents", "experimental_request_user_input", "update_plan"] {
            for value in [json!(true), json!(null)] {
                let mut bad = good.clone();
                if field == "agents" {
                    bad["config"][field]["enabled"] = value;
                } else {
                    bad["layers"][0]["config"]["tools"][field]["enabled"] = value;
                }
                assert_eq!(
                    check_provider_config(&bad),
                    Err(RuntimeError::NativeToolPolicy)
                );
            }
        }
        let mut response = good;
        let path = Path::new("/opt/codex/bin/sdlc2-model-policy.json");
        assert_eq!(
            check_model_policy_path(&response, path),
            Err(RuntimeError::NativeToolPolicy)
        );
        response["config"]["model_catalog_json"] = json!(path);
        assert_eq!(check_model_policy_path(&response, path), Ok(()));
        response["config"]["model_catalog_json"] = json!("/run/codex-home/foreign.json");
        assert_eq!(
            check_model_policy_path(&response, path),
            Err(RuntimeError::NativeToolPolicy)
        );
    }

    #[test]
    fn tool_controls_require_session_layer_and_matching_effective_origins() {
        let good = fixture_config();
        for field in ["layers", "origins"] {
            let mut bad = good.clone();
            bad[field] = json!(null);
            assert_eq!(
                check_provider_config(&bad),
                Err(RuntimeError::NativeToolPolicy)
            );
        }
        for key in [
            "agents.enabled",
            "tools.experimental_request_user_input.enabled",
            "tools.update_plan.enabled",
        ] {
            for field in ["name", "version"] {
                let mut bad = good.clone();
                bad["origins"][key][field] = json!("foreign");
                assert_eq!(
                    check_provider_config(&bad),
                    Err(RuntimeError::NativeToolPolicy)
                );
            }
        }
        let mut duplicate = good.clone();
        duplicate["layers"]
            .as_array_mut()
            .unwrap()
            .push(good["layers"][0].clone());
        assert_eq!(
            check_provider_config(&duplicate),
            Err(RuntimeError::NativeToolPolicy)
        );
    }

    #[tokio::test]
    async fn reader_limits_frames_before_allocation_and_preserves_fragmented_input() {
        let mut reader = BufReader::new(&b"{\"id\":1}\n{\"id\":2}\n"[..]);
        let mut line = Vec::new();
        assert_eq!(bounded_frame(&mut reader, &mut line).await.unwrap(), 9);
        assert_eq!(line, b"{\"id\":1}\n");
        line.clear();
        assert_eq!(bounded_frame(&mut reader, &mut line).await.unwrap(), 9);
        let over = vec![b'x'; MAX_FRAME + 1];
        let mut reader = BufReader::new(&over[..]);
        line.clear();
        assert_eq!(
            bounded_frame(&mut reader, &mut line).await,
            Err(RuntimeError::Protocol)
        );
        assert!(line.len() <= MAX_FRAME);
    }

    #[test]
    fn schema_requires_actual_dynamic_tools_and_does_not_verify_live_access() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("v2")).unwrap();
        std::fs::write(
            dir.path().join("v2/ThreadStartParams.json"),
            r#"{"properties":{"model":{},"dynamicTools":{}}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("v2/TurnStartParams.json"),
            r#"{"properties":{"outputSchema":{}}}"#,
        )
        .unwrap();
        let result = inspect_schema(dir.path()).unwrap();
        assert_eq!(result["dynamic_tools_declared"], true);
        assert_eq!(result["live_capabilities_verified"], false);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn subprocess_handshake_denies_server_tools_and_requires_pinned_version() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let binary = directory.path().join("codex-fixture");
        let model_policy = crate::native_model_policy::provision_fixture(&binary);
        let fixture_script = r#"#!/bin/bash
if [ "$1" = "--version" ]; then printf 'codex-cli fixture-1\n'; exit; fi
while IFS= read -r line; do
  if [[ "$line" =~ \"id\":([0-9]+) ]]; then id=${BASH_REMATCH[1]}; else continue; fi
  if [[ "$line" == *'"method":"initialize"'* ]]; then
    printf '{"id":%s,"result":{"platformFamily":"unix"}}\n' "$id"
  elif [[ "$line" == *'"method":"account/read"'* ]]; then
    printf '{"id":404,"method":"item/tool/call","params":{"threadId":"fixture","tool":"unsafe_tool"}}\n'
    printf '{"id":%s,"result":{"account":null}}\n' "$id"
  elif [[ "$line" == *'"method":"config/read"'* ]]; then
    printf '{"id":%s,"result":{"config":{"model_provider":"sdlc2_chatgpt","model_providers":{"sdlc2_chatgpt":{"wire_api":"responses","requires_openai_auth":true,"request_max_retries":0,"stream_max_retries":0}},"features":{"shell_tool":false,"view_image":false,"unified_exec":false,"apply_patch_freeform":false,"multi_agent":false,"apps":false,"plugins":false,"remote_plugin":false,"tool_suggest":false,"hooks":false,"memories":false},"apps":{"_default":{"enabled":false}},"forced_login_method":"chatgpt","cli_auth_credentials_store":"file","web_search":"disabled","sandbox_mode":"read-only","approval_policy":"never"}}}\n' "$id"
  elif [[ "$line" == *'"id":404'* && "$line" == *'tool_execution_disabled'* ]]; then
    printf '{"method":"fixture/tool_denied","params":{}}\n'
  fi
done
"#;
        let config_line = fixture_script
            .lines()
            .find(|line| line.contains("model_provider"))
            .unwrap();
        let mut fixture = fixture_config();
        fixture["config"]["model_catalog_json"] = json!(model_policy);
        let replacement = format!("    printf '{{\"id\":%s,\"result\":{fixture}}}\\n' \"$id\"");
        std::fs::write(&binary, fixture_script.replace(config_line, &replacement)).unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let home = directory.path().join("home");
        let work = directory.path().join("work");
        std::fs::create_dir(&home).unwrap();
        std::fs::create_dir(&work).unwrap();
        let mut config = RuntimeConfig {
            workspace: "sdlc2".into(),
            listen: "127.0.0.1:0".parse().unwrap(),
            state_dir: directory.path().join("state"),
            key_file: directory.path().join("key"),
            clients_file: directory.path().join("clients"),
            execution_trust_file: None,
            codex_binary: binary,
            codex_version: "wrong".into(),
            codex_home: home,
            codex_workdir: work,
            external_calls_enabled: false,
        };
        assert!(matches!(
            CodexClient::start(&config).await,
            Err(RuntimeError::CodexVersion)
        ));
        config.codex_version = "fixture-1".into();
        let client = CodexClient::start(&config).await.unwrap();
        let mut events = client.subscribe();
        assert_eq!(
            client
                .call("account/read", json!({"refreshToken":false}))
                .await
                .unwrap()["account"],
            Value::Null
        );
        let result = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if events.recv().await.unwrap()["method"] == "fixture/tool_denied" {
                    break;
                }
            }
        })
        .await;
        assert!(result.is_ok());
    }
}
