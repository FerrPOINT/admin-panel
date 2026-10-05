//! Reconcile pending immutable registrations by readback before any delivery.
use admin_panel_api::SharedState;
use admin_panel_domain::ai::{AiError, ProviderId, RevisionRegistration};
use admin_panel_infra::{
    ai::{PublicationRejection, RegistrationAck, StoreError},
    ai_runtime::{BridgeError, ProviderStatus},
};
use std::time::Duration;
use tokio::{sync::watch, task::JoinHandle};

const ACTOR: &str = "ai-runtime-reconciler";

pub fn spawn(state: SharedState, mut shutdown: watch::Receiver<bool>) -> Option<JoinHandle<()>> {
    if state.ai.is_none() || state.ai_runtime.is_none() {
        return None;
    }
    Some(tokio::spawn(async move {
        loop {
            if *shutdown.borrow() {
                break;
            }
            tokio::select! {
                _ = shutdown.changed() => break,
                result = reconcile(&state) => {
                    if result.is_err() { tracing::warn!("AI publication reconciliation pending; readback required"); }
                }
            }
            tokio::select! {
                _ = shutdown.changed() => break,
                _ = tokio::time::sleep(Duration::from_secs(5)) => {}
            }
        }
    }))
}

async fn reconcile(state: &SharedState) -> Result<(), ()> {
    let store = state.ai.as_ref().ok_or(())?;
    let client = state.ai_runtime.as_ref().ok_or(())?;
    let Some(pending) = store.pending_publication().await.map_err(|_| ())? else {
        return Ok(());
    };
    let request = RevisionRegistration {
        schema_version: 1,
        operation_id: pending.operation_id,
        draft_revision: pending.expected_draft_revision,
        profile: pending.profile,
        credential_generation: pending.credential_generation,
        adapter_version: pending.adapter_version,
        accounting_policy: pending.accounting_policy,
    };
    // Any transport/protocol error retains the slot; an absent receipt must be confirmed.
    let status = client
        .registration_status(request.operation_id)
        .await
        .map_err(|_| ())?;
    let receipt = match status.receipt {
        Some(receipt) => receipt,
        None => {
            if pending.evidence.expires_at <= chrono::Utc::now() {
                return store
                    .reject_publication_with_reason(
                        request.operation_id,
                        ACTOR,
                        PublicationRejection::EvidenceExpired,
                    )
                    .await
                    .map_err(|_| ());
            }
            // Registration only persists this exact candidate; it performs no inference.
            // Concurrent/replayed delivery is protected by its durable operation receipt.
            match client.register_profile(&request).await {
                Ok(receipt) => receipt,
                Err(error) => {
                    let Some(reason) = definitive_rejection(error) else {
                        return Err(());
                    };
                    return store
                        .reject_publication_with_reason(request.operation_id, ACTOR, reason)
                        .await
                        .map_err(|_| ());
                }
            }
        }
    };
    if receipt.registration != request {
        return Err(());
    }
    // Historical ACK is not proof that the connection is still current.
    let registry = client.registry().await.map_err(|_| ())?;
    let current = registry
        .providers
        .iter()
        .find(|p| p.id == request.profile.provider)
        .ok_or(())?;
    if let Some(reason) = current_connection(current, request.credential_generation)? {
        return store
            .reject_publication_with_reason(request.operation_id, ACTOR, reason)
            .await
            .map_err(|_| ());
    }
    match store
        .complete_publication(RegistrationAck {
            operation_id: request.operation_id,
            profile: &request.profile,
            credential_generation: request.credential_generation,
            adapter_version: &request.adapter_version,
            accounting_policy: &request.accounting_policy,
        })
        .await
    {
        Ok(_) => Ok(()),
        Err(StoreError::Domain(AiError::RevisionConflict)) => store
            .reject_publication_with_reason(
                request.operation_id,
                ACTOR,
                PublicationRejection::DraftChanged,
            )
            .await
            .map_err(|_| ()),
        Err(StoreError::Domain(AiError::VerificationExpired)) => store
            .reject_publication_with_reason(
                request.operation_id,
                ACTOR,
                PublicationRejection::EvidenceExpired,
            )
            .await
            .map_err(|_| ()),
        Err(_) => Err(()),
    }
}

fn current_connection(
    status: &ProviderStatus,
    generation: uuid::Uuid,
) -> Result<Option<PublicationRejection>, ()> {
    if status.connected != status.generation.is_some() {
        return Err(());
    }
    if !status.connected {
        return Ok(Some(PublicationRejection::RuntimeRejected));
    }
    if status.generation != Some(generation) {
        return Ok(Some(PublicationRejection::RuntimeConflict));
    }
    if status.id == ProviderId::Chatgpt
        && (status.runtime_available != Some(true)
            || status.authorization_checkpoint_ready != Some(true))
    {
        return Err(());
    }
    Ok(None)
}

fn definitive_rejection(error: BridgeError) -> Option<PublicationRejection> {
    match error {
        BridgeError::Rejected | BridgeError::Disconnected => {
            Some(PublicationRejection::RuntimeRejected)
        }
        BridgeError::Conflict => Some(PublicationRejection::RuntimeConflict),
        // No confirmed terminal receipt: retain pending for subsequent readback.
        BridgeError::Configuration
        | BridgeError::Unavailable
        | BridgeError::Protocol
        | BridgeError::Quota => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn historical_ack_cannot_activate_with_replaced_disconnected_or_uncertain_authorization() {
        let generation = uuid::Uuid::new_v4();
        let mut status = ProviderStatus {
            id: ProviderId::Chatgpt,
            connected: true,
            generation: Some(generation),
            pending_login_operation: None,
            runtime_available: Some(true),
            authorization_checkpoint_ready: Some(true),
            capabilities_verified: false,
        };
        assert!(current_connection(&status, generation).unwrap().is_none());
        assert!(matches!(
            current_connection(&status, uuid::Uuid::new_v4()),
            Ok(Some(PublicationRejection::RuntimeConflict))
        ));
        status.authorization_checkpoint_ready = Some(false);
        assert!(current_connection(&status, generation).is_err());
        status.generation = None;
        assert!(current_connection(&status, generation).is_err());
        status.connected = false;
        assert!(matches!(
            current_connection(&status, generation),
            Ok(Some(PublicationRejection::RuntimeRejected))
        ));
        status.id = ProviderId::Openrouter;
        status.connected = true;
        status.generation = Some(generation);
        status.runtime_available = None;
        status.authorization_checkpoint_ready = None;
        assert!(current_connection(&status, generation).unwrap().is_none());
    }

    #[test]
    fn unknown_transport_protocol_and_non_terminal_errors_keep_the_pending_slot() {
        for error in [
            BridgeError::Configuration,
            BridgeError::Unavailable,
            BridgeError::Protocol,
            BridgeError::Quota,
        ] {
            assert!(definitive_rejection(error).is_none());
        }
        assert!(matches!(
            definitive_rejection(BridgeError::Rejected),
            Some(PublicationRejection::RuntimeRejected)
        ));
        assert!(matches!(
            definitive_rejection(BridgeError::Disconnected),
            Some(PublicationRejection::RuntimeRejected)
        ));
        assert!(matches!(
            definitive_rejection(BridgeError::Conflict),
            Some(PublicationRejection::RuntimeConflict)
        ));
    }
}
