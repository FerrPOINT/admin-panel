//! Authenticated encryption, atomic durability, and explicit initialization.
use crate::error::RuntimeError;
use admin_panel_domain::ai::{ProviderId, VerificationEvidence};
use ring::{
    aead,
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

const HEADER: &[u8] = b"SDLC-AI-V1\0";
const MAX_VAULT_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub generation: Uuid,
    pub provider: ProviderId,
    // Deliberately no Debug implementation or public serialization endpoint.
    pub credential: String,
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.credential.zeroize();
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultState {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inference_runs: BTreeMap<Uuid, crate::inference_journal::StoredInference>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub root_profile_bindings: BTreeMap<Uuid, crate::inference_journal::RootProfileBinding>,
    #[serde(default)]
    pub verified_adapters: BTreeMap<Uuid, crate::publication::VerifiedAdapterEvidence>,
    #[serde(default)]
    pub registered_revisions: BTreeMap<Uuid, crate::publication::RegisteredRevision>,
    #[serde(default)]
    pub budget: crate::budget::BudgetLedger,
    pub connections: BTreeMap<String, Connection>,
    pub verifications: BTreeMap<Uuid, VerificationEvidence>,
    pub operations: BTreeMap<Uuid, StoredOperation>,
    #[serde(default)]
    pub logins: BTreeMap<Uuid, ManagedLogin>,
    #[serde(default)]
    pub disconnects: BTreeMap<Uuid, DisconnectOperation>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedLogin {
    pub operation_id: Uuid,
    pub login_id: Option<Uuid>,
    pub status: String,
    pub user_code: Option<String>,
    pub verification_url: Option<String>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredOperation {
    pub fingerprint: String,
    pub generation: Uuid,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisconnectOperation {
    pub provider: ProviderId,
    pub generation: Option<Uuid>,
    pub status: String,
}

pub struct Vault {
    _lock: File,
    file: PathBuf,
    workspace: String,
    key: aead::LessSafeKey,
    state: VaultState,
}

/// Reject links in every existing path component before handling secrets.
pub fn check_private_path(path: &Path) -> Result<(), RuntimeError> {
    if !path.is_absolute() {
        return Err(RuntimeError::Configuration);
    }
    let mut current = PathBuf::new();
    for part in path.components() {
        if matches!(part, Component::ParentDir) {
            return Err(RuntimeError::Configuration);
        }
        current.push(part);
        if let Ok(metadata) = fs::symlink_metadata(&current) {
            if metadata.file_type().is_symlink() {
                return Err(RuntimeError::Configuration);
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    return Err(RuntimeError::Configuration);
                }
            }
        }
    }
    Ok(())
}

fn create_private(path: &Path) -> Result<File, RuntimeError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).map_err(|_| RuntimeError::StateWrite)
}

fn load_key(path: &Path) -> Result<aead::LessSafeKey, RuntimeError> {
    check_private_path(path)?;
    let key = Zeroizing::new(fs::read(path).map_err(|_| RuntimeError::NotInitialized)?);
    if key.len() != 32 {
        return Err(RuntimeError::StateIntegrity);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if fs::metadata(path)
            .map_err(|_| RuntimeError::NotInitialized)?
            .permissions()
            .mode()
            & 0o077
            != 0
        {
            return Err(RuntimeError::Configuration);
        }
    }
    aead::UnboundKey::new(&aead::AES_256_GCM, &key)
        .map(aead::LessSafeKey::new)
        .map_err(|_| RuntimeError::StateIntegrity)
}

impl Drop for Vault {
    fn drop(&mut self) {
        // flock belongs to the open file description. A concurrent fork can
        // retain its CLOEXEC duplicate briefly until exec; closing only our fd
        // would then keep a retired writer locked. Explicitly release on owner
        // lifetime end, while preserving exclusivity for the entire live Vault.
        let _ = fs2::FileExt::unlock(&self._lock);
    }
}

impl Vault {
    pub fn initialize(
        directory: &Path,
        key_file: &Path,
        workspace: &str,
    ) -> Result<(), RuntimeError> {
        if workspace != "sdlc2" {
            return Err(RuntimeError::Configuration);
        }
        check_private_path(directory)?;
        check_private_path(key_file)?;
        fs::create_dir_all(directory).map_err(|_| RuntimeError::StateWrite)?;
        if key_file.starts_with(directory)
            || directory
                .read_dir()
                .map_err(|_| RuntimeError::StateWrite)?
                .next()
                .is_some()
            || key_file.exists()
        {
            return Err(RuntimeError::Conflict);
        }
        let mut raw_key = Zeroizing::new([0u8; 32]);
        SystemRandom::new()
            .fill(&mut *raw_key)
            .map_err(|_| RuntimeError::StateWrite)?;
        let mut key = create_private(key_file)?;
        key.write_all(&*raw_key)
            .and_then(|_| key.sync_all())
            .map_err(|_| RuntimeError::StateWrite)?;
        let lock = create_private(&directory.join("writer.lock"))?;
        fs2::FileExt::try_lock_exclusive(&lock).map_err(|_| RuntimeError::Conflict)?;
        let vault = Self {
            _lock: lock,
            file: directory.join("vault.aead"),
            workspace: workspace.into(),
            key: load_key(key_file)?,
            state: VaultState::default(),
        };
        vault.write(&vault.state)?;
        let mut owner = create_private(&directory.join("owner.json"))?;
        owner
            .write_all(format!("{{\"schema_version\":1,\"workspace\":\"{workspace}\"}}").as_bytes())
            .and_then(|_| owner.sync_all())
            .map_err(|_| RuntimeError::StateWrite)
    }

    pub fn open(directory: &Path, key_file: &Path, workspace: &str) -> Result<Self, RuntimeError> {
        check_private_path(directory)?;
        check_private_path(&directory.join("vault.aead"))?;
        check_private_path(&directory.join("owner.json"))?;
        check_private_path(&directory.join("writer.lock"))?;
        let owner: serde_json::Value = serde_json::from_slice(
            &fs::read(directory.join("owner.json")).map_err(|_| RuntimeError::NotInitialized)?,
        )
        .map_err(|_| RuntimeError::StateIntegrity)?;
        if workspace != "sdlc2"
            || owner != serde_json::json!({"schema_version":1,"workspace":workspace})
        {
            return Err(RuntimeError::StateIntegrity);
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .open(directory.join("writer.lock"))
            .map_err(|_| RuntimeError::NotInitialized)?;
        fs2::FileExt::try_lock_exclusive(&lock).map_err(|_| RuntimeError::Conflict)?;
        let file = directory.join("vault.aead");
        if fs::metadata(&file)
            .map_err(|_| RuntimeError::NotInitialized)?
            .len()
            > MAX_VAULT_BYTES
        {
            return Err(RuntimeError::StateIntegrity);
        }
        let key = load_key(key_file)?;
        let bytes = fs::read(&file).map_err(|_| RuntimeError::NotInitialized)?;
        if bytes.len() < HEADER.len() + 12 + 16 || !bytes.starts_with(HEADER) {
            return Err(RuntimeError::StateIntegrity);
        }
        let nonce_bytes: [u8; 12] = bytes[HEADER.len()..HEADER.len() + 12]
            .try_into()
            .map_err(|_| RuntimeError::StateIntegrity)?;
        let mut encrypted = Zeroizing::new(bytes[HEADER.len() + 12..].to_vec());
        let clear = key
            .open_in_place(
                aead::Nonce::assume_unique_for_key(nonce_bytes),
                aead::Aad::from(workspace.as_bytes()),
                &mut encrypted,
            )
            .map_err(|_| RuntimeError::StateIntegrity)?;
        let state = serde_json::from_slice(clear).map_err(|_| RuntimeError::StateIntegrity)?;
        Ok(Self {
            _lock: lock,
            file,
            workspace: workspace.into(),
            key,
            state,
        })
    }

    pub fn state(&self) -> &VaultState {
        &self.state
    }

    /// Call only under RuntimeState.vault's mutex. Commit before provider I/O.
    pub fn reserve_paid_request(
        &mut self,
        operation: Uuid,
        fingerprint: &str,
        estimate: crate::budget::CostEstimate,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, RuntimeError> {
        let mut next = self.state.clone();
        let created = next.budget.reserve(operation, fingerprint, estimate, now)?;
        if created {
            self.commit(next)?;
        }
        Ok(created)
    }

    pub fn mark_paid_dispatched(&mut self, operation: Uuid) -> Result<(), RuntimeError> {
        let mut next = self.state.clone();
        let reservation = next
            .budget
            .reservations
            .get(&operation)
            .ok_or(crate::budget::BudgetError::DispatchConflict)?;
        reservation
            .estimate
            .ceiling_microdollars(chrono::Utc::now())?;
        next.budget.mark_dispatched(operation)?;
        self.commit(next)
    }

    pub fn cancel_paid_before_dispatch(&mut self, operation: Uuid) -> Result<(), RuntimeError> {
        let mut next = self.state.clone();
        next.budget.cancel_before_dispatch(operation)?;
        self.commit(next)
    }

    pub fn settle_paid_request(
        &mut self,
        operation: Uuid,
        actual_microdollars: u64,
    ) -> Result<(), RuntimeError> {
        let mut next = self.state.clone();
        next.budget.settle(operation, actual_microdollars)?;
        self.commit(next)
    }

    /// A failed commit cannot change the effective in-memory connection.
    pub fn commit(&mut self, state: VaultState) -> Result<(), RuntimeError> {
        self.write(&state)?;
        self.state = state;
        Ok(())
    }

    fn write(&self, state: &VaultState) -> Result<(), RuntimeError> {
        let mut bytes =
            Zeroizing::new(serde_json::to_vec(state).map_err(|_| RuntimeError::StateWrite)?);
        if bytes.len() as u64 + HEADER.len() as u64 + 28 > MAX_VAULT_BYTES {
            return Err(RuntimeError::StateWrite);
        }
        let mut nonce = [0u8; 12];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| RuntimeError::StateWrite)?;
        self.key
            .seal_in_place_append_tag(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::from(self.workspace.as_bytes()),
                &mut *bytes,
            )
            .map_err(|_| RuntimeError::StateWrite)?;
        let temporary = self.file.with_extension(format!("{}.tmp", Uuid::new_v4()));
        let mut file = create_private(&temporary)?;
        file.write_all(HEADER)
            .and_then(|_| file.write_all(&nonce))
            .and_then(|_| file.write_all(&bytes))
            .and_then(|_| file.sync_all())
            .map_err(|_| RuntimeError::StateWrite)?;
        fs::rename(&temporary, &self.file).map_err(|_| RuntimeError::StateWrite)?;
        #[cfg(unix)]
        File::open(self.file.parent().ok_or(RuntimeError::StateWrite)?)
            .and_then(|f| f.sync_all())
            .map_err(|_| RuntimeError::StateWrite)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn writer_lifetime_releases_lock_even_with_a_fork_style_descriptor_duplicate() {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state");
        let key = dir.path().join("key");
        Vault::initialize(&state, &key, "sdlc2").unwrap();
        let vault = Vault::open(&state, &key, "sdlc2").unwrap();
        let inherited = vault._lock.try_clone().unwrap();
        assert!(matches!(
            Vault::open(&state, &key, "sdlc2"),
            Err(RuntimeError::Conflict)
        ));
        drop(vault);
        let reopened = Vault::open(&state, &key, "sdlc2").unwrap();
        assert!(matches!(
            Vault::open(&state, &key, "sdlc2"),
            Err(RuntimeError::Conflict)
        ));
        drop(inherited);
        drop(reopened);
        assert!(Vault::open(&state, &key, "sdlc2").is_ok());
    }
    #[tokio::test]
    async fn concurrent_reservations_and_crash_readback_preserve_the_shared_budget() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join("state");
        let key = dir.path().join("key");
        Vault::initialize(&state_dir, &key, "sdlc2").unwrap();
        let vault = std::sync::Arc::new(tokio::sync::Mutex::new(
            Vault::open(&state_dir, &key, "sdlc2").unwrap(),
        ));
        let now = chrono::Utc::now();
        let estimate = crate::budget::CostEstimate {
            pricing_revision: "fixture-price".into(),
            model: "fixture/model".into(),
            credential_generation: Uuid::new_v4(),
            input_tokens: 1000,
            output_tokens: 1,
            input_nanodollars_per_token: 19_000_000,
            output_nanodollars_per_token: 1,
            pricing_verified_at: now,
            pricing_expires_at: now + chrono::Duration::minutes(15),
        };
        let mut tasks = Vec::new();
        for _ in 0..2 {
            let vault = vault.clone();
            let estimate = estimate.clone();
            tasks.push(tokio::spawn(async move {
                let operation = Uuid::new_v4();
                let mut locked = vault.lock().await;
                locked.reserve_paid_request(operation, &"a".repeat(64), estimate, now)?;
                locked.mark_paid_dispatched(operation)?;
                Ok::<_, RuntimeError>(operation)
            }));
        }
        let mut dispatched = Vec::new();
        let mut rejected = 0;
        for task in tasks {
            match task.await.unwrap() {
                Ok(id) => dispatched.push(id),
                Err(RuntimeError::Budget(crate::budget::BudgetError::Exhausted)) => rejected += 1,
                Err(error) => panic!("unexpected error {error}"),
            }
        }
        assert_eq!(dispatched.len(), 1);
        assert_eq!(rejected, 1);
        drop(vault);
        let mut restored = Vault::open(&state_dir, &key, "sdlc2").unwrap();
        assert_eq!(
            restored.state().budget.committed_microdollars().unwrap(),
            19_000_001
        );
        assert_eq!(
            restored.cancel_paid_before_dispatch(dispatched[0]),
            Err(RuntimeError::Budget(
                crate::budget::BudgetError::DispatchConflict
            ))
        );
        assert_eq!(
            restored.mark_paid_dispatched(dispatched[0]),
            Err(RuntimeError::Budget(
                crate::budget::BudgetError::DispatchConflict
            ))
        );
        restored
            .settle_paid_request(dispatched[0], 10_000_000)
            .unwrap();
        assert_eq!(
            restored.state().budget.committed_microdollars().unwrap(),
            10_000_000
        );
    }
    #[test]
    fn ciphertext_survives_restart_and_rejects_tampering_wrong_key_and_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join("state");
        let key = dir.path().join("key");
        assert!(matches!(
            Vault::open(&state_dir, &key, "sdlc2"),
            Err(RuntimeError::NotInitialized)
        ));
        Vault::initialize(&state_dir, &key, "sdlc2").unwrap();
        assert_eq!(
            Vault::initialize(&state_dir, &key, "sdlc2"),
            Err(RuntimeError::Conflict)
        );
        let mut vault = Vault::open(&state_dir, &key, "sdlc2").unwrap();
        let mut state = vault.state().clone();
        state.connections.insert(
            "openrouter".into(),
            Connection {
                provider: ProviderId::Openrouter,
                generation: Uuid::new_v4(),
                credential: "secret-not-on-disk".into(),
            },
        );
        vault.commit(state).unwrap();
        let bytes = fs::read(state_dir.join("vault.aead")).unwrap();
        assert!(!bytes.windows(18).any(|w| w == b"secret-not-on-disk"));
        assert!(matches!(
            Vault::open(&state_dir, &key, "sdlc2"),
            Err(RuntimeError::Conflict)
        ));
        drop(vault);
        assert_eq!(
            Vault::open(&state_dir, &key, "sdlc2")
                .unwrap()
                .state()
                .connections["openrouter"]
                .credential,
            "secret-not-on-disk"
        );
        assert!(matches!(
            Vault::open(&state_dir, &key, "sdlc1"),
            Err(RuntimeError::StateIntegrity)
        ));
        let wrong_key = dir.path().join("wrong-key");
        let mut f = create_private(&wrong_key).unwrap();
        f.write_all(&[99; 32]).unwrap();
        assert!(matches!(
            Vault::open(&state_dir, &wrong_key, "sdlc2"),
            Err(RuntimeError::StateIntegrity)
        ));
        let mut changed = bytes;
        let last = changed.len() - 1;
        changed[last] ^= 1;
        fs::write(state_dir.join("vault.aead"), changed).unwrap();
        assert!(matches!(
            Vault::open(&state_dir, &key, "sdlc2"),
            Err(RuntimeError::StateIntegrity)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symbolic_links_and_exposed_key() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join("state");
        let key = dir.path().join("key");
        Vault::initialize(&state_dir, &key, "sdlc2").unwrap();
        let link = dir.path().join("link");
        symlink(&state_dir, &link).unwrap();
        assert!(matches!(
            Vault::open(&link, &key, "sdlc2"),
            Err(RuntimeError::Configuration)
        ));
        fs::set_permissions(&key, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(
            Vault::open(&state_dir, &key, "sdlc2"),
            Err(RuntimeError::Configuration)
        ));
    }
}
