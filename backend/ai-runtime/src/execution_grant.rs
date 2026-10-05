//! Fleet delegation is separate from human identity and provider credentials.
use crate::error::RuntimeError;
use admin_panel_domain::inference::ExecutionScope;
use chrono::{DateTime, Duration, Utc};
use ring::signature::{ED25519, UnparsedPublicKey};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

const SIGNATURE_DOMAIN: &[u8] = b"SDLC-AI-EXECUTION-V1\0";
const MAX_PAYLOAD_BYTES: usize = 8192;
const ISSUER: &str = "sdlc2-fleet-control";
const AUDIENCE: &str = "sdlc2-ai-runtime";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantEnvelope {
    pub schema_version: u32,
    pub key_id: String,
    pub payload: String,
    pub signature_hex: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionClaims {
    pub schema_version: u32,
    pub issuer: String,
    pub audience: String,
    pub machine_subject: String,
    pub grant_id: Uuid,
    pub execution: ExecutionScope,
    pub profile_revision: u64,
    pub fencing_token: u64,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub lease_expires_at: DateTime<Utc>,
}

/// Only the verifier constructs this value; request JSON cannot deserialize it.
#[derive(Clone)]
pub struct AuthorizedExecution {
    claims: ExecutionClaims,
}

impl AuthorizedExecution {
    pub fn execution(&self) -> &ExecutionScope {
        &self.claims.execution
    }

    pub fn machine_subject(&self) -> &str {
        &self.claims.machine_subject
    }

    pub fn profile_revision(&self) -> u64 {
        self.claims.profile_revision
    }

    pub fn fencing_token(&self) -> u64 {
        self.claims.fencing_token
    }

    pub fn valid_until(&self) -> DateTime<Utc> {
        self.claims.expires_at.min(self.claims.lease_expires_at)
    }

    pub fn check_current(&self, now: DateTime<Utc>) -> Result<(), RuntimeError> {
        if now < self.claims.issued_at
            || now >= self.claims.expires_at
            || now >= self.claims.lease_expires_at
        {
            return Err(RuntimeError::Forbidden);
        }
        Ok(())
    }
}

/// Deployment trust only. An envelope cannot supply its own public key or issuer.
pub struct GrantVerifier {
    keys: BTreeMap<String, [u8; 32]>,
}

pub(crate) fn valid_machine_subject(subject: &str) -> bool {
    subject.starts_with("sdlc2:")
        && subject.len() <= 128
        && subject.len() > "sdlc2:".len()
        && subject
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'_' | b'-' | b'.'))
}

impl GrantVerifier {
    pub fn load(path: &std::path::Path) -> Result<Self, RuntimeError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Trust {
            schema_version: u32,
            workspace: String,
            issuer: String,
            audience: String,
            keys: BTreeMap<String, String>,
        }
        crate::vault::check_private_path(path)?;
        if std::fs::metadata(path)
            .map_err(|_| RuntimeError::Configuration)?
            .len()
            > 65536
        {
            return Err(RuntimeError::Configuration);
        }
        let input: Trust =
            serde_json::from_slice(&std::fs::read(path).map_err(|_| RuntimeError::Configuration)?)
                .map_err(|_| RuntimeError::Configuration)?;
        if input.schema_version != 1
            || input.workspace != "sdlc2"
            || input.issuer != ISSUER
            || input.audience != AUDIENCE
        {
            return Err(RuntimeError::Configuration);
        }
        let keys = input
            .keys
            .into_iter()
            .map(|(id, public)| {
                let bytes = hex::decode(public).map_err(|_| RuntimeError::Configuration)?;
                let key: [u8; 32] = bytes.try_into().map_err(|_| RuntimeError::Configuration)?;
                if key == [0; 32] {
                    return Err(RuntimeError::Configuration);
                }
                Ok((id, key))
            })
            .collect::<Result<BTreeMap<_, _>, RuntimeError>>()?;
        Self::new(keys)
    }

    pub fn new(keys: BTreeMap<String, [u8; 32]>) -> Result<Self, RuntimeError> {
        if keys.is_empty()
            || keys.len() > 8
            || keys.keys().any(|id| {
                id.is_empty()
                    || id.len() > 64
                    || !id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
            })
        {
            return Err(RuntimeError::Configuration);
        }
        Ok(Self { keys })
    }

    /// `authenticated_machine` comes from the service credential, never the body.
    /// Fleet must verify central subject/project access/current assignment before
    /// signing; signature verification cannot invent that upstream authorization.
    pub fn verify(
        &self,
        envelope: &GrantEnvelope,
        authenticated_machine: &str,
        now: DateTime<Utc>,
    ) -> Result<AuthorizedExecution, RuntimeError> {
        if envelope.schema_version != 1
            || envelope.payload.len() > MAX_PAYLOAD_BYTES
            || envelope.signature_hex.len() != 128
            || !valid_machine_subject(authenticated_machine)
        {
            return Err(RuntimeError::Forbidden);
        }
        let key = self
            .keys
            .get(&envelope.key_id)
            .ok_or(RuntimeError::Forbidden)?;
        let signature =
            hex::decode(&envelope.signature_hex).map_err(|_| RuntimeError::Forbidden)?;
        let mut signed = Vec::with_capacity(SIGNATURE_DOMAIN.len() + envelope.payload.len());
        signed.extend_from_slice(SIGNATURE_DOMAIN);
        signed.extend_from_slice(envelope.payload.as_bytes());
        UnparsedPublicKey::new(&ED25519, key)
            .verify(&signed, &signature)
            .map_err(|_| RuntimeError::Forbidden)?;
        let claims: ExecutionClaims =
            serde_json::from_str(&envelope.payload).map_err(|_| RuntimeError::Forbidden)?;
        if claims.schema_version != 1
            || claims.issuer != ISSUER
            || claims.audience != AUDIENCE
            || claims.machine_subject != authenticated_machine
            || claims.grant_id.is_nil()
            || claims.profile_revision == 0
            || claims.fencing_token == 0
            || claims.execution.validate().is_err()
            || claims.expires_at <= claims.issued_at
            || claims
                .issued_at
                .checked_add_signed(Duration::minutes(5))
                .is_none_or(|limit| claims.expires_at > limit)
            || claims.expires_at > claims.lease_expires_at
            || claims
                .issued_at
                .checked_add_signed(Duration::seconds(30))
                .is_none_or(|limit| claims.lease_expires_at > limit)
        {
            return Err(RuntimeError::Forbidden);
        }
        let authorized = AuthorizedExecution { claims };
        authorized.check_current(now)?;
        Ok(authorized)
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    pub fn claims(now: DateTime<Utc>) -> ExecutionClaims {
        ExecutionClaims {
            schema_version: 1,
            issuer: ISSUER.into(),
            audience: AUDIENCE.into(),
            machine_subject: "sdlc2:hermes:developer".into(),
            grant_id: Uuid::new_v4(),
            execution: ExecutionScope {
                workspace: "sdlc2".into(),
                owner_subject: "own-central-subject".into(),
                project_id: Uuid::new_v4(),
                root_task_id: Uuid::new_v4(),
                task_id: Uuid::new_v4(),
                agent_id: Uuid::new_v4(),
                execution_id: Uuid::new_v4(),
            },
            profile_revision: 3,
            fencing_token: 1,
            issued_at: now,
            expires_at: now + Duration::seconds(30),
            lease_expires_at: now + Duration::seconds(30),
        }
    }

    pub fn key_pair() -> Ed25519KeyPair {
        Ed25519KeyPair::from_seed_unchecked(&[42; 32]).unwrap()
    }

    pub fn verifier() -> GrantVerifier {
        GrantVerifier::new(BTreeMap::from([(
            "own-test-key".into(),
            key_pair().public_key().as_ref().try_into().unwrap(),
        )]))
        .unwrap()
    }

    pub fn envelope(claims: &ExecutionClaims) -> GrantEnvelope {
        let payload = serde_json::to_string(claims).unwrap();
        let mut signed = SIGNATURE_DOMAIN.to_vec();
        signed.extend_from_slice(payload.as_bytes());
        GrantEnvelope {
            schema_version: 1,
            key_id: "own-test-key".into(),
            payload,
            signature_hex: hex::encode(key_pair().sign(&signed).as_ref()),
        }
    }

    pub fn authorize(claims: &ExecutionClaims, now: DateTime<Utc>) -> AuthorizedExecution {
        verifier()
            .verify(&envelope(claims), &claims.machine_subject, now)
            .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::{fixtures::*, *};

    #[test]
    fn deployment_trust_rejects_foreign_owner_empty_keys_and_invalid_public_key() {
        use ring::signature::KeyPair;
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("trust.json");
        let value = serde_json::json!({"schema_version":1,"workspace":"sdlc2","issuer":ISSUER,"audience":AUDIENCE,
            "keys":{"own-test-key":hex::encode(key_pair().public_key().as_ref())}});
        std::fs::write(&file, value.to_string()).unwrap();
        let loaded = GrantVerifier::load(&file).unwrap();
        let now = Utc::now();
        let claims = claims(now);
        loaded
            .verify(&envelope(&claims), &claims.machine_subject, now)
            .unwrap();
        for variant in 0..6 {
            let mut invalid = value.clone();
            match variant {
                0 => invalid["workspace"] = serde_json::json!("sdlc1"),
                1 => invalid["issuer"] = serde_json::json!("sdlc1-fleet-control"),
                2 => invalid["audience"] = serde_json::json!("admin"),
                3 => invalid["keys"] = serde_json::json!({}),
                4 => invalid["keys"]["own-test-key"] = serde_json::json!("00".repeat(32)),
                _ => invalid["keys"]["own-test-key"] = serde_json::json!("abcd"),
            }
            std::fs::write(&file, invalid.to_string()).unwrap();
            assert!(matches!(
                GrantVerifier::load(&file),
                Err(RuntimeError::Configuration)
            ));
        }
        std::fs::write(&file, vec![b' '; 65537]).unwrap();
        assert!(matches!(
            GrantVerifier::load(&file),
            Err(RuntimeError::Configuration)
        ));
    }

    #[test]
    fn grant_binds_signature_machine_audience_workspace_and_live_lease() {
        let now = Utc::now();
        let claims = claims(now);
        let signed = envelope(&claims);
        let verifier = verifier();
        let authorized = verifier
            .verify(&signed, &claims.machine_subject, now)
            .unwrap();
        assert!(authorized.execution() == &claims.execution);
        assert_eq!(authorized.profile_revision(), 3);
        assert!(matches!(
            verifier.verify(&signed, "sdlc2:hermes:tester", now),
            Err(RuntimeError::Forbidden)
        ));
        for offset in [-1, 30, 31] {
            assert!(
                verifier
                    .verify(
                        &signed,
                        &claims.machine_subject,
                        now + Duration::seconds(offset)
                    )
                    .is_err()
            );
        }
        let mut tampered = signed.clone();
        tampered.payload = tampered
            .payload
            .replace("own-central-subject", "other-subject");
        assert!(
            verifier
                .verify(&tampered, &claims.machine_subject, now)
                .is_err()
        );
        tampered = signed.clone();
        tampered.key_id = "neighbour-key".into();
        assert!(
            verifier
                .verify(&tampered, &claims.machine_subject, now)
                .is_err()
        );
        for altered in 0..5 {
            let mut invalid = claims.clone();
            match altered {
                0 => invalid.issuer = "sdlc1-fleet-control".into(),
                1 => invalid.audience = "sdlc".into(),
                2 => invalid.execution.workspace = "sdlc1".into(),
                3 => invalid.profile_revision = 0,
                _ => invalid.fencing_token = 0,
            }
            assert!(
                verifier
                    .verify(&envelope(&invalid), &claims.machine_subject, now)
                    .is_err()
            );
        }
    }

    #[test]
    fn duplicate_unknown_claim_fields_and_expiry_beyond_lease_are_rejected() {
        let now = Utc::now();
        let mut invalid = claims(now);
        invalid.expires_at += Duration::seconds(1);
        assert!(
            verifier()
                .verify(&envelope(&invalid), &invalid.machine_subject, now)
                .is_err()
        );
        invalid.expires_at = invalid.lease_expires_at;
        invalid.lease_expires_at += Duration::seconds(1);
        assert!(
            verifier()
                .verify(&envelope(&invalid), &invalid.machine_subject, now)
                .is_err()
        );
        for extra in [
            "\"audience\":\"sdlc2-ai-runtime\",",
            "\"private_key\":\"forged\",",
        ] {
            let mut signed = envelope(&claims(now));
            signed.payload.insert_str(1, extra);
            let mut bytes = SIGNATURE_DOMAIN.to_vec();
            bytes.extend_from_slice(signed.payload.as_bytes());
            signed.signature_hex = hex::encode(key_pair().sign(&bytes).as_ref());
            assert!(
                verifier()
                    .verify(&signed, "sdlc2:hermes:developer", now)
                    .is_err()
            );
        }
    }
}
