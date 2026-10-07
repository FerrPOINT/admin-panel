//! Durable runtime registration. This module cannot create verification evidence.
use crate::{error::RuntimeError, vault::Vault};
use admin_panel_domain::ai::publish_profile;
pub use admin_panel_domain::ai::{
    RegisteredRevision, RevisionRegistration, VerifiedAdapterEvidence,
};
use chrono::{DateTime, Utc};

/// Version markers identify deployed code; they do not prove model access or framing.
pub fn deployed_contract(
    provider: admin_panel_domain::ai::ProviderId,
) -> (&'static str, &'static str) {
    match provider {
        admin_panel_domain::ai::ProviderId::Chatgpt => (
            "codex-app-server-0.159.0-alpha.12.1-v5-readonly-tool-policy",
            "native-byte-bound-full-output-reserve-v1",
        ),
        admin_panel_domain::ai::ProviderId::Openrouter => (
            "openrouter-sse-v3",
            "openrouter-wire-byte-bound-priced-private-history-v3",
        ),
    }
}

impl Vault {
    /// Deployed adapter descriptors are trusted server inputs, never request fields.
    /// The HTTP handler must authenticate Admin before calling this method.
    pub fn register_revision(
        &mut self,
        request: RevisionRegistration,
        deployed_adapter: &str,
        deployed_accounting: &str,
        now: DateTime<Utc>,
    ) -> Result<RegisteredRevision, RuntimeError> {
        if request.schema_version != 1
            || request.draft_revision <= 0
            || request.operation_id.is_nil()
            || request.profile.revision == 0
            || request.profile.workspace != "sdlc2"
            || request.profile.schema_version != 1
        {
            return Err(RuntimeError::InvalidRequest);
        }
        if request.adapter_version != deployed_adapter
            || request.accounting_policy != deployed_accounting
        {
            return Err(RuntimeError::CapabilityNotVerified);
        }
        // Compare operation contents before considering expiry; replay reads the durable
        // receipt, and does not re-register, run inference, or extend verification TTL.
        if let Some(existing) = self.state().registered_revisions.get(&request.operation_id) {
            return if existing.registration == request {
                Ok(existing.clone())
            } else {
                Err(RuntimeError::Conflict)
            };
        }
        if self
            .state()
            .registered_revisions
            .values()
            .any(|registered| registered.registration.profile.revision == request.profile.revision)
        {
            return Err(RuntimeError::Conflict);
        }
        let verified = self
            .state()
            .verified_adapters
            .get(&request.profile.verification_id)
            .ok_or(RuntimeError::CapabilityNotVerified)?;
        if verified.adapter_version != deployed_adapter
            || verified.accounting_policy != deployed_accounting
            || verified.draft_revision != Some(request.draft_revision)
        {
            return Err(RuntimeError::CapabilityNotVerified);
        }
        let evidence = &verified.evidence;
        let provider = match request.profile.provider {
            admin_panel_domain::ai::ProviderId::Chatgpt => "chatgpt",
            admin_panel_domain::ai::ProviderId::Openrouter => "openrouter",
        };
        let connection = self
            .state()
            .connections
            .get(provider)
            .ok_or(RuntimeError::Disconnected)?;
        if connection.generation != request.credential_generation
            || connection.provider != request.profile.provider
        {
            return Err(RuntimeError::Conflict);
        }
        let expected = publish_profile(
            "sdlc2",
            request.profile.revision - 1,
            request.profile.revision - 1,
            &evidence.settings,
            request.credential_generation,
            evidence,
            now,
        )
        .map_err(|_| RuntimeError::CapabilityNotVerified)?;
        if expected != request.profile {
            return Err(RuntimeError::CapabilityNotVerified);
        }
        let registered = RegisteredRevision {
            registration: request,
            registered_at: now,
        };
        let mut next = self.state().clone();
        next.registered_revisions
            .insert(registered.registration.operation_id, registered.clone());
        self.commit(next)?;
        Ok(registered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::Connection;
    use admin_panel_domain::ai::{
        ModelCapabilities, ProviderId, ProviderSettings, VerificationEvidence,
    };
    use chrono::Duration;
    use uuid::Uuid;

    #[test]
    fn registration_needs_runtime_owned_proof_and_survives_crash_without_reexecution() {
        let directory = tempfile::tempdir().unwrap();
        let state = directory.path().join("state");
        let key = directory.path().join("key");
        Vault::initialize(&state, &key, "sdlc2").unwrap();
        let mut vault = Vault::open(&state, &key, "sdlc2").unwrap();
        let now = Utc::now();
        let generation = Uuid::new_v4();
        let evidence = VerificationEvidence {
            id: Uuid::new_v4(),
            settings: ProviderSettings {
                provider: ProviderId::Openrouter,
                model: "deepseek/deepseek-v4.1-flash".into(),
                context_window_tokens: 256000,
            },
            credential_generation: generation,
            capabilities: ModelCapabilities {
                model: "deepseek/deepseek-v4.1-flash".into(),
                context_limit_tokens: 1048576,
                max_output_tokens: 65536,
                tools: true,
                structured_output: true,
                streaming: true,
                cancellation: true,
            },
            verified_at: now,
            expires_at: now + Duration::minutes(15),
        };
        let profile = publish_profile(
            "sdlc2",
            2,
            2,
            &evidence.settings,
            generation,
            &evidence,
            now,
        )
        .unwrap();
        let request = RevisionRegistration {
            draft_revision: 1,
            schema_version: 1,
            operation_id: Uuid::new_v4(),
            profile,
            credential_generation: generation,
            adapter_version: "fixture-v1".into(),
            accounting_policy: "fixture-accounting-v1".into(),
        };
        assert!(matches!(
            vault.register_revision(request.clone(), "fixture-v1", "fixture-accounting-v1", now),
            Err(RuntimeError::CapabilityNotVerified)
        ));
        let mut next = vault.state().clone();
        next.connections.insert(
            "openrouter".into(),
            Connection {
                generation,
                provider: ProviderId::Openrouter,
                credential: "test-only-secret".into(),
            },
        );
        // Legacy bare evidence is not trusted as a deployed adapter proof.
        next.verifications.insert(evidence.id, evidence.clone());
        vault.commit(next).unwrap();
        assert!(matches!(
            vault.register_revision(request.clone(), "fixture-v1", "fixture-accounting-v1", now),
            Err(RuntimeError::CapabilityNotVerified)
        ));
        let mut next = vault.state().clone();
        next.verified_adapters.insert(
            evidence.id,
            VerifiedAdapterEvidence {
                draft_revision: Some(1),
                evidence: evidence.clone(),
                adapter_version: "fixture-v1".into(),
                accounting_policy: "fixture-accounting-v1".into(),
            },
        );
        vault.commit(next).unwrap();
        let mut stale_draft = request.clone();
        stale_draft.draft_revision += 1;
        assert!(matches!(
            vault.register_revision(stale_draft, "fixture-v1", "fixture-accounting-v1", now),
            Err(RuntimeError::CapabilityNotVerified)
        ));
        assert!(matches!(
            vault.register_revision(
                request.clone(),
                "changed-adapter",
                "fixture-accounting-v1",
                now
            ),
            Err(RuntimeError::CapabilityNotVerified)
        ));
        assert!(matches!(
            vault.register_revision(
                request.clone(),
                "fixture-v1",
                "fixture-accounting-v1",
                now + Duration::minutes(16)
            ),
            Err(RuntimeError::CapabilityNotVerified)
        ));
        vault
            .register_revision(request.clone(), "fixture-v1", "fixture-accounting-v1", now)
            .unwrap();
        drop(vault);
        let mut restored = Vault::open(&state, &key, "sdlc2").unwrap();
        assert_eq!(
            restored
                .register_revision(
                    request.clone(),
                    "fixture-v1",
                    "fixture-accounting-v1",
                    now + Duration::minutes(16)
                )
                .unwrap()
                .registration,
            request
        );
        let mut changed = request.clone();
        changed.profile.context_window_tokens = 128000;
        assert!(matches!(
            restored.register_revision(changed, "fixture-v1", "fixture-accounting-v1", now),
            Err(RuntimeError::Conflict)
        ));
        let mut duplicate = request;
        duplicate.operation_id = Uuid::new_v4();
        assert!(matches!(
            restored.register_revision(duplicate, "fixture-v1", "fixture-accounting-v1", now),
            Err(RuntimeError::Conflict)
        ));
    }
}
