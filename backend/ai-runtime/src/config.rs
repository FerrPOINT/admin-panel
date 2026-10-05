use crate::error::RuntimeError;
use std::{net::SocketAddr, path::PathBuf};

#[derive(Clone)]
pub struct RuntimeConfig {
    pub workspace: String,
    pub listen: SocketAddr,
    pub state_dir: PathBuf,
    pub key_file: PathBuf,
    pub clients_file: PathBuf,
    pub execution_trust_file: Option<PathBuf>,
    pub codex_binary: PathBuf,
    pub codex_version: String,
    pub codex_home: PathBuf,
    pub codex_workdir: PathBuf,
    pub external_calls_enabled: bool,
}

impl RuntimeConfig {
    pub fn from_env() -> Result<Self, RuntimeError> {
        let required = |name| std::env::var(name).map_err(|_| RuntimeError::Configuration);
        let config = Self {
            workspace: required("AI_RUNTIME_WORKSPACE")?,
            listen: required("AI_RUNTIME_LISTEN")?
                .parse()
                .map_err(|_| RuntimeError::Configuration)?,
            state_dir: required("AI_RUNTIME_STATE_DIR")?.into(),
            key_file: required("AI_RUNTIME_KEY_FILE")?.into(),
            clients_file: required("AI_RUNTIME_CLIENTS_FILE")?.into(),
            execution_trust_file: std::env::var_os("AI_RUNTIME_EXECUTION_TRUST_FILE")
                .map(PathBuf::from),
            codex_binary: required("AI_RUNTIME_CODEX_BINARY")?.into(),
            codex_version: required("AI_RUNTIME_CODEX_VERSION")?,
            codex_home: required("AI_RUNTIME_CODEX_HOME")?.into(),
            codex_workdir: required("AI_RUNTIME_CODEX_WORKDIR")?.into(),
            external_calls_enabled: matches!(
                std::env::var("AI_RUNTIME_EXTERNAL_CALLS").as_deref(),
                Ok("true")
            ),
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), RuntimeError> {
        // First rollout cannot accidentally start against the neighbour's state.
        if self.workspace != "sdlc2"
            || self.execution_trust_file.as_ref().is_some_and(|p| {
                !p.is_absolute()
                    || p.starts_with(&self.state_dir)
                    || p.starts_with(&self.codex_home)
                    || p.starts_with(&self.codex_workdir)
            })
            || self.codex_version.is_empty()
            || [
                &self.state_dir,
                &self.key_file,
                &self.clients_file,
                &self.codex_binary,
                &self.codex_home,
                &self.codex_workdir,
            ]
            .iter()
            .any(|p| !p.is_absolute())
            || self.codex_home == self.state_dir
            || self.codex_workdir == self.state_dir
            || self.codex_home == self.codex_workdir
            || self.key_file.starts_with(&self.state_dir)
            || self.clients_file.starts_with(&self.state_dir)
            || self.codex_home.starts_with(&self.state_dir)
            || self.state_dir.starts_with(&self.codex_home)
            || self.codex_workdir.starts_with(&self.state_dir)
            || self.state_dir.starts_with(&self.codex_workdir)
            || self.codex_home.starts_with(&self.codex_workdir)
            || self.codex_workdir.starts_with(&self.codex_home)
            || [&self.key_file, &self.clients_file, &self.codex_binary]
                .iter()
                .any(|path| {
                    path.starts_with(&self.codex_home) || path.starts_with(&self.codex_workdir)
                })
        {
            return Err(RuntimeError::Configuration);
        }
        Ok(())
    }
}
