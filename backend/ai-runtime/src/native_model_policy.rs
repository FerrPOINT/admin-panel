//! Model-owned tool selection precedes feature flags in the pinned native SDK.
//! This exact read-only catalog controls tools, never proves model entitlement.
use crate::{error::RuntimeError, vault::check_private_path};
use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "sdlc2-model-policy.json";
const CONTENT: &[u8] = include_bytes!("../policy/sdlc2-model-policy.json");

pub(crate) fn path_for(binary: &Path) -> Result<PathBuf, RuntimeError> {
    Ok(binary
        .parent()
        .ok_or(RuntimeError::Configuration)?
        .join(FILE_NAME))
}

pub(crate) fn validated_path(binary: &Path) -> Result<PathBuf, RuntimeError> {
    let path = path_for(binary)?;
    check_private_path(&path)?;
    let metadata = std::fs::metadata(&path).map_err(|_| RuntimeError::NativeToolPolicy)?;
    if !metadata.is_file() || metadata.len() != CONTENT.len() as u64 {
        return Err(RuntimeError::NativeToolPolicy);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o222 != 0 {
            return Err(RuntimeError::NativeToolPolicy);
        }
    }
    if std::fs::read(&path).map_err(|_| RuntimeError::NativeToolPolicy)? != CONTENT {
        return Err(RuntimeError::NativeToolPolicy);
    }
    Ok(path)
}

#[cfg(test)]
pub(crate) fn provision_fixture(binary: &Path) -> PathBuf {
    let path = path_for(binary).unwrap();
    std::fs::write(&path, CONTENT).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn catalog_keeps_exact_model_without_enabling_builtin_tools() {
        let catalog: serde_json::Value = serde_json::from_slice(CONTENT).unwrap();
        let models = catalog["models"].as_array().unwrap();
        assert_eq!(models.len(), 1);
        let model = &models[0];
        assert_eq!(model["slug"], "gpt-6-luna");
        for (key, value) in [
            ("tool_mode", json!("direct")),
            ("multi_agent_version", json!("disabled")),
            ("shell_type", json!("disabled")),
            ("apply_patch_tool_type", json!(null)),
            ("experimental_supported_tools", json!([])),
            ("node_repl_disabled", json!(true)),
            ("include_skills_usage_instructions", json!(false)),
            ("include_apps_usage_instructions", json!(false)),
            ("include_plugin_usage_instructions", json!(false)),
        ] {
            assert_eq!(model[key], value, "{key}");
        }
        // Advertised metadata is preserved; it is not runtime capability evidence.
        assert_eq!(model["context_window"], 272000);
        assert_eq!(model["max_context_window"], 872000);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn missing_writable_changed_or_linked_policy_is_rejected() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let directory = tempfile::tempdir().unwrap();
        let binary = directory.path().join("codex");
        assert_eq!(validated_path(&binary), Err(RuntimeError::NativeToolPolicy));
        let path = provision_fixture(&binary);
        assert_eq!(validated_path(&binary).unwrap(), path);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(validated_path(&binary), Err(RuntimeError::NativeToolPolicy));
        let mut changed = CONTENT.to_vec();
        changed[0] = b' ';
        std::fs::write(&path, changed).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
        assert_eq!(validated_path(&binary), Err(RuntimeError::NativeToolPolicy));
        std::fs::remove_file(&path).unwrap();
        let target = directory.path().join("foreign-policy");
        std::fs::write(&target, CONTENT).unwrap();
        symlink(&target, &path).unwrap();
        assert_eq!(validated_path(&binary), Err(RuntimeError::Configuration));
    }
}
