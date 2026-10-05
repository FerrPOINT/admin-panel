//! Encrypted request history and a single durable dispatch intent, without I/O.
use crate::{
    budget::CostEstimate, error::RuntimeError, execution_grant::AuthorizedExecution,
    publication::deployed_contract, vault::Vault,
};
use admin_panel_domain::{
    ai::{ProviderId, RegisteredRevision},
    inference::{ExecutionScope, InferenceRequest},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub(crate) const MAX_RUNS: usize = 4096;
const MAX_EVENTS: usize = 16384;
const MAX_EVENT_BYTES: usize = 65536;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Prepared,
    Dispatching,
    Running,
    CancellationRequested,
    Completed,
    Cancelled,
    Failed,
    Unknown,
    AwaitingTools,
    ToolsSubmitted,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{
        budget::{BudgetError, ReservationStatus},
        execution_grant::{ExecutionClaims, fixtures},
        vault::Connection,
    };
    use admin_panel_domain::{
        ai::{
            ModelCapabilities, ProviderSettings, RevisionRegistration, RuntimeProfile,
            VerificationEvidence, VerifiedAdapterEvidence,
        },
        inference::Message,
    };
    use chrono::Duration;

    struct Fixture {
        directory: tempfile::TempDir,
        vault: Vault,
        now: DateTime<Utc>,
        claims: ExecutionClaims,
        request: InferenceRequest,
        generation: Uuid,
    }

    fn fixture() -> Fixture {
        let directory = tempfile::tempdir().unwrap();
        let state = directory.path().join("state");
        let key = directory.path().join("key");
        Vault::initialize(&state, &key, "sdlc2").unwrap();
        let mut vault = Vault::open(&state, &key, "sdlc2").unwrap();
        let now = Utc::now();
        let generation = Uuid::new_v4();
        let claims = fixtures::claims(now);
        let settings = ProviderSettings {
            provider: ProviderId::Openrouter,
            model: "deepseek/deepseek-v4.1-flash".into(),
            context_window_tokens: 256000,
        };
        let evidence = VerificationEvidence {
            id: Uuid::new_v4(),
            settings: settings.clone(),
            credential_generation: generation,
            capabilities: ModelCapabilities {
                model: settings.model.clone(),
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
        let (adapter, accounting) = deployed_contract(ProviderId::Openrouter);
        let mut next = vault.state().clone();
        next.connections.insert(
            "openrouter".into(),
            Connection {
                generation,
                provider: ProviderId::Openrouter,
                credential: "fixture-only-provider-key".into(),
            },
        );
        next.verified_adapters.insert(
            evidence.id,
            VerifiedAdapterEvidence {
                draft_revision: Some(1),
                evidence: evidence.clone(),
                adapter_version: adapter.into(),
                accounting_policy: accounting.into(),
            },
        );
        vault.commit(next).unwrap();
        vault
            .register_revision(
                RevisionRegistration {
                    schema_version: 1,
                    operation_id: Uuid::new_v4(),
                    draft_revision: 1,
                    profile: RuntimeProfile {
                        schema_version: 1,
                        workspace: "sdlc2".into(),
                        revision: 3,
                        provider: ProviderId::Openrouter,
                        model: settings.model,
                        context_window_tokens: settings.context_window_tokens,
                        verification_id: evidence.id,
                    },
                    credential_generation: generation,
                    adapter_version: adapter.into(),
                    accounting_policy: accounting.into(),
                },
                adapter,
                accounting,
                now,
            )
            .unwrap();
        let request = InferenceRequest {
            schema_version: 1,
            request_id: Uuid::new_v4(),
            profile_revision: 3,
            execution: claims.execution.clone(),
            messages: vec![Message::User {
                content: "Private required evidence".into(),
            }],
            tools: vec![],
            output_schema: None,
            output_reserve_tokens: 1000,
        };
        Fixture {
            directory,
            vault,
            now,
            claims,
            request,
            generation,
        }
    }

    fn preparation(fixture: &Fixture) -> DispatchPreparation {
        DispatchPreparation {
            framing_tokens: Some(1000),
            output_limit_supported: true,
            paid_cost: Some(CostEstimate {
                pricing_revision: "fixture-price-1".into(),
                model: "deepseek/deepseek-v4.1-flash".into(),
                credential_generation: fixture.generation,
                input_tokens: 10000,
                output_tokens: 1000,
                input_nanodollars_per_token: 100,
                output_nanodollars_per_token: 500,
                pricing_verified_at: fixture.now,
                pricing_expires_at: fixture.now + Duration::minutes(15),
            }),
        }
    }

    pub(crate) fn prepared_fixture_state() -> (crate::vault::VaultState, ExecutionClaims, Uuid) {
        let mut f = fixture();
        let grant = fixtures::authorize(&f.claims, f.now);
        let id = f.request.request_id;
        f.vault.prepare_inference(f.request, &grant, f.now).unwrap();
        (f.vault.state().clone(), f.claims, id)
    }

    #[test]
    fn crash_retains_dispatch_intent_cost_and_encrypted_history_without_resend() {
        let mut f = fixture();
        let grant = fixtures::authorize(&f.claims, f.now);
        let id = f.request.request_id;
        assert_eq!(
            f.vault.prepare_inference(f.request.clone(), &grant, f.now),
            Ok(true)
        );
        let prepared = preparation(&f);
        f.vault
            .dispatch_inference_once(id, &grant, prepared, f.now)
            .unwrap();
        assert_eq!(f.vault.state().budget.committed_microdollars(), Ok(1500));
        let disk = std::fs::read(f.directory.path().join("state/vault.aead")).unwrap();
        assert!(
            !disk
                .windows(b"Private required evidence".len())
                .any(|part| part == b"Private required evidence")
        );
        drop(f.vault);
        let mut restored = Vault::open(
            &f.directory.path().join("state"),
            &f.directory.path().join("key"),
            "sdlc2",
        )
        .unwrap();
        assert_eq!(restored.reconcile_inference_restart(f.now), Ok(1));
        assert_eq!(restored.reconcile_inference_restart(f.now), Ok(0));
        assert_eq!(
            restored.prepare_inference(f.request.clone(), &grant, f.now),
            Ok(false)
        );
        let run = restored.inference_readback(id, &grant, f.now).unwrap();
        assert_eq!(run.state, RunState::Unknown);
        assert_eq!(run.events.len(), 3);
        let cost = CostEstimate {
            pricing_revision: "fixture-price-1".into(),
            model: "deepseek/deepseek-v4.1-flash".into(),
            credential_generation: f.generation,
            input_tokens: 10000,
            output_tokens: 1000,
            input_nanodollars_per_token: 100,
            output_nanodollars_per_token: 500,
            pricing_verified_at: f.now,
            pricing_expires_at: f.now + Duration::minutes(15),
        };
        assert_eq!(
            restored.dispatch_inference_once(
                id,
                &grant,
                DispatchPreparation {
                    framing_tokens: Some(1000),
                    output_limit_supported: true,
                    paid_cost: Some(cost),
                },
                f.now
            ),
            Err(RuntimeError::Conflict)
        );
        assert_eq!(
            restored.state().budget.reservations[&id].status,
            ReservationStatus::Uncertain
        );
        assert_eq!(restored.state().budget.committed_microdollars(), Ok(1500));
        let mut changed = f.request;
        changed.output_reserve_tokens += 1;
        assert_eq!(
            restored.prepare_inference(changed, &grant, f.now),
            Err(RuntimeError::Conflict)
        );
    }

    #[test]
    fn transcript_status_and_cancel_require_the_exact_machine_and_execution() {
        let mut f = fixture();
        let own = fixtures::authorize(&f.claims, f.now);
        let id = f.request.request_id;
        f.vault
            .prepare_inference(f.request.clone(), &own, f.now)
            .unwrap();
        for field in 0..8 {
            let mut claims = f.claims.clone();
            match field {
                0 => claims.machine_subject = "sdlc2:hermes:tester".into(),
                1 => claims.execution.owner_subject = "other-central-subject".into(),
                2 => claims.execution.project_id = Uuid::new_v4(),
                3 => claims.execution.root_task_id = Uuid::new_v4(),
                4 => claims.execution.task_id = Uuid::new_v4(),
                5 => claims.execution.agent_id = Uuid::new_v4(),
                6 => claims.execution.execution_id = Uuid::new_v4(),
                _ => claims.fencing_token += 1,
            }
            let foreign = fixtures::authorize(&claims, f.now);
            assert!(matches!(
                f.vault.inference_readback(id, &foreign, f.now),
                Err(RuntimeError::Forbidden)
            ));
            assert_eq!(
                f.vault.cancel_inference(id, &foreign, f.now),
                Err(RuntimeError::Forbidden)
            );
        }
        assert_eq!(
            f.vault.inference_readback(id, &own, f.now).unwrap().state,
            RunState::Prepared
        );
    }

    #[test]
    fn children_rework_and_auxiliary_requests_keep_the_root_profile_revision() {
        let mut f = fixture();
        let own = fixtures::authorize(&f.claims, f.now);
        f.vault
            .prepare_inference(f.request.clone(), &own, f.now)
            .unwrap();
        let mut child = f.claims.clone();
        child.execution.task_id = Uuid::new_v4();
        child.execution.execution_id = Uuid::new_v4();
        child.execution.agent_id = Uuid::new_v4();
        child.machine_subject = "sdlc2:hermes:analyst".into();
        let mut request = f.request.clone();
        request.request_id = Uuid::new_v4();
        request.execution = child.execution.clone();
        let authorized = fixtures::authorize(&child, f.now);
        assert_eq!(
            f.vault
                .prepare_inference(request.clone(), &authorized, f.now),
            Ok(true)
        );
        child.profile_revision = 4;
        request.request_id = Uuid::new_v4();
        request.profile_revision = 4;
        assert_eq!(
            f.vault
                .prepare_inference(request, &fixtures::authorize(&child, f.now), f.now),
            Err(RuntimeError::Forbidden)
        );
        assert_eq!(
            f.vault.state().root_profile_bindings[&f.claims.execution.root_task_id]
                .profile_revision,
            3
        );
    }

    #[test]
    fn cancellation_before_intent_is_terminal_but_after_intent_requires_receipt() {
        let mut f = fixture();
        let grant = fixtures::authorize(&f.claims, f.now);
        let id = f.request.request_id;
        f.vault
            .prepare_inference(f.request.clone(), &grant, f.now)
            .unwrap();
        assert_eq!(
            f.vault
                .finish_inference(id, TerminalOutcome::Completed, Some(0), f.now),
            Err(RuntimeError::Conflict)
        );
        assert_eq!(
            f.vault.cancel_inference(id, &grant, f.now),
            Ok(RunState::Cancelled)
        );
        assert_eq!(f.vault.state().budget.committed_microdollars(), Ok(0));
        let prepared = preparation(&f);
        assert_eq!(
            f.vault.dispatch_inference_once(id, &grant, prepared, f.now),
            Err(RuntimeError::Conflict)
        );
        f.request.request_id = Uuid::new_v4();
        let id = f.request.request_id;
        f.vault
            .prepare_inference(f.request.clone(), &grant, f.now)
            .unwrap();
        let prepared = preparation(&f);
        f.vault
            .dispatch_inference_once(id, &grant, prepared, f.now)
            .unwrap();
        assert_eq!(
            f.vault.cancel_inference(id, &grant, f.now),
            Ok(RunState::CancellationRequested)
        );
        assert_eq!(f.vault.state().budget.committed_microdollars(), Ok(1500));
        f.vault
            .finish_inference(id, TerminalOutcome::Cancelled, None, f.now)
            .unwrap();
        assert_eq!(f.vault.state().budget.committed_microdollars(), Ok(1500));
        f.vault.reconcile_inference_cost(id, 1200, f.now).unwrap();
        f.vault.reconcile_inference_cost(id, 1200, f.now).unwrap();
        assert_eq!(
            f.vault.reconcile_inference_cost(id, 1100, f.now),
            Err(RuntimeError::Conflict)
        );
        assert_eq!(f.vault.state().budget.committed_microdollars(), Ok(1200));
        f.vault
            .finish_inference(id, TerminalOutcome::Cancelled, Some(1200), f.now)
            .unwrap();
    }

    #[test]
    fn unknown_assignment_holds_slot_until_proven_stopped_and_rejects_stale_fencing() {
        let mut f = fixture();
        let grant = fixtures::authorize(&f.claims, f.now);
        let id = f.request.request_id;
        f.vault
            .prepare_inference(f.request.clone(), &grant, f.now)
            .unwrap();
        let prepared = preparation(&f);
        f.vault
            .dispatch_inference_once(id, &grant, prepared, f.now)
            .unwrap();
        f.vault.reconcile_inference_restart(f.now).unwrap();
        let mut replacement = f.claims.clone();
        replacement.execution.execution_id = Uuid::new_v4();
        replacement.fencing_token = 2;
        let replacement_grant = fixtures::authorize(&replacement, f.now);
        let mut request = f.request.clone();
        request.request_id = Uuid::new_v4();
        request.execution = replacement.execution;
        assert_eq!(
            f.vault
                .prepare_inference(request.clone(), &replacement_grant, f.now),
            Err(RuntimeError::Forbidden)
        );
        f.vault
            .finish_inference(id, TerminalOutcome::Cancelled, Some(100), f.now)
            .unwrap();
        assert_eq!(
            f.vault
                .prepare_inference(request, &replacement_grant, f.now),
            Ok(true)
        );
        f.request.request_id = Uuid::new_v4();
        assert_eq!(
            f.vault.prepare_inference(f.request, &grant, f.now),
            Err(RuntimeError::Forbidden)
        );
    }

    #[test]
    fn failed_accounting_pricing_or_budget_cannot_leave_a_partial_dispatch() {
        let mut f = fixture();
        let grant = fixtures::authorize(&f.claims, f.now);
        let id = f.request.request_id;
        f.vault
            .prepare_inference(f.request.clone(), &grant, f.now)
            .unwrap();
        let mut prepared = preparation(&f);
        prepared.framing_tokens = None;
        assert_eq!(
            f.vault.dispatch_inference_once(id, &grant, prepared, f.now),
            Err(RuntimeError::CapabilityNotVerified)
        );
        let mut prepared = preparation(&f);
        prepared.paid_cost.as_mut().unwrap().pricing_expires_at = f.now;
        assert_eq!(
            f.vault.dispatch_inference_once(id, &grant, prepared, f.now),
            Err(RuntimeError::Budget(BudgetError::InvalidEstimate))
        );
        let mut prepared = preparation(&f);
        prepared
            .paid_cost
            .as_mut()
            .unwrap()
            .input_nanodollars_per_token = 4_000_000;
        assert_eq!(
            f.vault.dispatch_inference_once(id, &grant, prepared, f.now),
            Err(RuntimeError::Budget(BudgetError::Exhausted))
        );
        let run = f.vault.inference_readback(id, &grant, f.now).unwrap();
        assert_eq!(run.state, RunState::Prepared);
        assert_eq!(run.events.len(), 1);
        assert!(f.vault.state().budget.reservations.is_empty());
    }

    #[test]
    fn streaming_is_ordered_idempotent_bounded_and_eof_is_never_completion() {
        let mut f = fixture();
        let grant = fixtures::authorize(&f.claims, f.now);
        let id = f.request.request_id;
        f.vault
            .prepare_inference(f.request.clone(), &grant, f.now)
            .unwrap();
        let prepared = preparation(&f);
        f.vault
            .dispatch_inference_once(id, &grant, prepared, f.now)
            .unwrap();
        assert_eq!(
            f.vault.record_inference_delta(id, 4, "later".into(), f.now),
            Err(RuntimeError::Conflict)
        );
        f.vault
            .record_inference_delta(id, 3, "own private transcript".into(), f.now)
            .unwrap();
        f.vault
            .record_inference_delta(id, 3, "own private transcript".into(), f.now)
            .unwrap();
        assert_eq!(
            f.vault
                .record_inference_delta(id, 3, "changed".into(), f.now),
            Err(RuntimeError::Conflict)
        );
        assert_eq!(
            f.vault
                .record_inference_delta(id, 4, "x".repeat(MAX_EVENT_BYTES), f.now),
            Err(RuntimeError::Protocol)
        );
        assert_eq!(
            f.vault.inference_readback(id, &grant, f.now).unwrap().state,
            RunState::Running
        );
        assert_eq!(f.vault.reconcile_inference_restart(f.now), Ok(1));
        assert_eq!(
            f.vault.inference_readback(id, &grant, f.now).unwrap().state,
            RunState::Unknown
        );
        assert_eq!(
            f.vault.record_inference_delta(id, 5, "late".into(), f.now),
            Err(RuntimeError::Conflict)
        );
    }

    #[test]
    fn status_is_secret_free_and_stream_pages_require_exact_cursor_and_owner() {
        let mut f = fixture();
        let grant = fixtures::authorize(&f.claims, f.now);
        let id = f.request.request_id;
        f.vault
            .prepare_inference(f.request.clone(), &grant, f.now)
            .unwrap();
        let prepared = preparation(&f);
        f.vault
            .dispatch_inference_once(id, &grant, prepared, f.now)
            .unwrap();
        f.vault
            .record_inference_delta(id, 3, "own private text".into(), f.now)
            .unwrap();
        let status =
            serde_json::to_value(f.vault.inference_status(id, &grant, f.now).unwrap()).unwrap();
        assert_eq!(status["last_event_sequence"], 3);
        for field in [
            "registration",
            "request",
            "events",
            "credential_generation",
            "credential",
            "machine_subject",
        ] {
            assert!(status.get(field).is_none(), "{field}");
        }
        let first = f.vault.inference_events(id, &grant, 1, 2, f.now).unwrap();
        assert_eq!(first.events.len(), 2);
        assert!(first.has_more);
        assert_eq!(first.next_sequence, 3);
        let second = f
            .vault
            .inference_events(id, &grant, first.next_sequence, 2, f.now)
            .unwrap();
        assert_eq!(second.events.len(), 1);
        assert!(!second.has_more);
        assert_eq!(second.next_sequence, 4);
        assert!(
            f.vault
                .inference_events(id, &grant, 4, 2, f.now)
                .unwrap()
                .events
                .is_empty()
        );
        for (cursor, limit) in [(0, 2), (5, 2), (1, 0), (1, 201)] {
            assert!(matches!(
                f.vault.inference_events(id, &grant, cursor, limit, f.now),
                Err(RuntimeError::InvalidRequest)
            ));
        }
        let mut foreign = f.claims;
        foreign.execution.owner_subject = "other-subject".into();
        let foreign = fixtures::authorize(&foreign, f.now);
        assert!(matches!(
            f.vault.inference_events(id, &foreign, 1, 2, f.now),
            Err(RuntimeError::Forbidden)
        ));
    }

    #[test]
    fn empty_journal_preserves_legacy_vault_shape_until_explicit_execution_admission() {
        let f = fixture();
        let encoded = serde_json::to_value(f.vault.state()).unwrap();
        assert!(encoded.get("inference_runs").is_none());
        assert!(encoded.get("root_profile_bindings").is_none());
        let decoded: crate::vault::VaultState = serde_json::from_value(encoded).unwrap();
        assert!(decoded.inference_runs.is_empty());
        assert!(decoded.root_profile_bindings.is_empty());
    }

    #[test]
    fn tool_continuation_preserves_full_history_and_creates_one_atomic_child_after_restart() {
        use admin_panel_domain::inference::{
            ToolCall, ToolContinuation, ToolDefinition, ToolResult,
        };
        let mut f = fixture();
        f.request.tools.push(ToolDefinition {
            name: "read_evidence".into(),
            description: "Own evidence".into(),
            parameters: serde_json::json!({"type":"object"}),
        });
        let grant = fixtures::authorize(&f.claims, f.now);
        let parent_id = f.request.request_id;
        f.vault
            .prepare_inference(f.request.clone(), &grant, f.now)
            .unwrap();
        let prepared = preparation(&f);
        f.vault
            .dispatch_inference_once(parent_id, &grant, prepared, f.now)
            .unwrap();
        f.vault
            .record_inference_delta(parent_id, 3, "required assistant evidence".into(), f.now)
            .unwrap();
        let calls = vec![ToolCall {
            id: "call_own_1".into(),
            name: "read_evidence".into(),
            arguments: serde_json::json!({"task":"own"}),
        }];
        let private_blocks = serde_json::json!([
            {"type":"reasoning.encrypted","data":"fixture-opaque","signature":"fixture-signature","index":0},
            {"type":"reasoning.text","text":"private fixture only","index":1}
        ]);
        let mut reasoning = crate::openrouter_reasoning::OpenRouterReasoning::default();
        reasoning.append_delta(&serde_json::json!({"reasoning":"private fixture only","reasoning_details":private_blocks})).unwrap();
        f.vault
            .finish_inference_tools_with_reasoning(
                parent_id,
                calls.clone(),
                Some(reasoning.clone()),
                Some(100),
                f.now,
            )
            .unwrap();
        f.vault
            .finish_inference_tools_with_reasoning(
                parent_id,
                calls.clone(),
                Some(reasoning),
                Some(100),
                f.now,
            )
            .unwrap();
        assert_eq!(
            f.vault
                .finish_inference_tools(parent_id, calls, Some(100), f.now),
            Err(RuntimeError::Conflict)
        );
        let exposed = serde_json::to_string(
            &f.vault
                .inference_events(parent_id, &grant, 1, 200, f.now)
                .unwrap(),
        )
        .unwrap();
        assert!(!exposed.contains("fixture-opaque") && !exposed.contains("private fixture only"));
        assert_eq!(
            f.vault
                .inference_status(parent_id, &grant, f.now)
                .unwrap()
                .state,
            RunState::AwaitingTools
        );
        assert_eq!(f.vault.reconcile_inference_restart(f.now), Ok(0));
        drop(f.vault);
        let mut restored = Vault::open(
            &f.directory.path().join("state"),
            &f.directory.path().join("key"),
            "sdlc2",
        )
        .unwrap();
        let next_id = Uuid::new_v4();
        let continuation = ToolContinuation {
            schema_version: 1,
            request_id: next_id,
            parent_request_id: parent_id,
            profile_revision: 3,
            execution: f.claims.execution.clone(),
            results: vec![ToolResult {
                tool_call_id: "call_own_1".into(),
                content: "mandatory tool result".into(),
            }],
        };
        assert_eq!(
            restored.continue_inference_tools(continuation.clone(), &grant, f.now),
            Ok(true)
        );
        assert_eq!(
            restored.continue_inference_tools(continuation.clone(), &grant, f.now),
            Ok(false)
        );
        let child = restored.inference_readback(next_id, &grant, f.now).unwrap();
        assert_eq!(child.parent_request_id, Some(parent_id));
        assert_eq!(child.state, RunState::Prepared);
        assert_eq!(child.request.profile_revision, f.request.profile_revision);
        assert_eq!(
            child.request.output_reserve_tokens,
            f.request.output_reserve_tokens
        );
        assert_eq!(
            serde_json::to_value(&child.request.tools).unwrap(),
            serde_json::to_value(&f.request.tools).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&child.request.messages[..f.request.messages.len()]).unwrap(),
            serde_json::to_value(&f.request.messages).unwrap()
        );
        assert_eq!(child.request.messages.len(), f.request.messages.len() + 2);
        assert!(child.request.validate_conversation().is_ok());
        let capabilities = &restored.state().verified_adapters
            [&child.registration.registration.profile.verification_id]
            .evidence
            .capabilities;
        let wire = crate::openrouter_request::ChatRequest::from_stored(
            &child,
            grant.execution(),
            capabilities,
            Some(1000),
        )
        .unwrap();
        assert_eq!(
            wire.body()["messages"][f.request.messages.len()]["reasoning_details"],
            private_blocks
        );
        assert_eq!(
            wire.body()["messages"][f.request.messages.len()]["reasoning"],
            "private fixture only"
        );
        assert_eq!(
            wire.input_upper_bound_tokens(),
            serde_json::to_vec(wire.body()).unwrap().len() as u64 + 1000
        );
        assert!(
            !serde_json::to_string(&child.request)
                .unwrap()
                .contains("fixture-opaque")
        );
        let mut oversized = child.clone();
        let mut too_big = crate::openrouter_reasoning::OpenRouterReasoning::default();
        too_big
            .append_delta(&serde_json::json!({"reasoning":"r".repeat(256000)}))
            .unwrap();
        oversized
            .openrouter_history
            .insert(f.request.messages.len(), too_big);
        assert!(matches!(
            crate::openrouter_request::ChatRequest::from_stored(
                &oversized,
                grant.execution(),
                capabilities,
                Some(1000)
            ),
            Err(admin_panel_domain::inference::InferenceError::Context(_))
        ));
        assert_eq!(restored.state().budget.reservations.len(), 1);
        let mut changed = continuation.clone();
        changed.results[0].content = "changed result".into();
        assert_eq!(
            restored.continue_inference_tools(changed, &grant, f.now),
            Err(RuntimeError::Conflict)
        );
        let mut new_attempt = continuation;
        new_attempt.request_id = Uuid::new_v4();
        assert_eq!(
            restored.continue_inference_tools(new_attempt, &grant, f.now),
            Err(RuntimeError::Conflict)
        );
        assert_eq!(restored.state().inference_runs.len(), 2);
    }

    #[test]
    fn tools_require_declared_names_exact_result_ids_ownership_and_a_proven_parent_terminal() {
        use admin_panel_domain::inference::{
            ToolCall, ToolContinuation, ToolDefinition, ToolResult,
        };
        let mut f = fixture();
        f.request.tools.push(ToolDefinition {
            name: "read_evidence".into(),
            description: "Own evidence".into(),
            parameters: serde_json::json!({"type":"object"}),
        });
        let grant = fixtures::authorize(&f.claims, f.now);
        let parent_id = f.request.request_id;
        f.vault
            .prepare_inference(f.request.clone(), &grant, f.now)
            .unwrap();
        let prepared = preparation(&f);
        f.vault
            .dispatch_inference_once(parent_id, &grant, prepared, f.now)
            .unwrap();
        let continuation = ToolContinuation {
            schema_version: 1,
            request_id: Uuid::new_v4(),
            parent_request_id: parent_id,
            profile_revision: 3,
            execution: f.claims.execution.clone(),
            results: vec![ToolResult {
                tool_call_id: "call_own_1".into(),
                content: "result".into(),
            }],
        };
        assert_eq!(
            f.vault
                .continue_inference_tools(continuation.clone(), &grant, f.now),
            Err(RuntimeError::Conflict)
        );
        let call = ToolCall {
            id: "call_own_1".into(),
            name: "read_evidence".into(),
            arguments: serde_json::json!({}),
        };
        let mut builtin = call.clone();
        builtin.name = "exec_command".into();
        assert_eq!(
            f.vault
                .finish_inference_tools(parent_id, vec![builtin], Some(100), f.now),
            Err(RuntimeError::Protocol)
        );
        assert_eq!(
            f.vault.finish_inference_tools(
                parent_id,
                vec![call.clone(), call.clone()],
                Some(100),
                f.now
            ),
            Err(RuntimeError::Protocol)
        );
        assert_eq!(f.vault.state().budget.committed_microdollars(), Ok(1500));
        f.vault
            .finish_inference_tools(parent_id, vec![call], Some(100), f.now)
            .unwrap();
        for kind in 0..3 {
            let mut invalid = continuation.clone();
            match kind {
                0 => invalid.results.clear(),
                1 => invalid.results[0].tool_call_id = "foreign_call".into(),
                _ => invalid.results.push(invalid.results[0].clone()),
            }
            assert_eq!(
                f.vault.continue_inference_tools(invalid, &grant, f.now),
                Err(RuntimeError::InvalidRequest)
            );
        }
        let mut foreign = f.claims.clone();
        foreign.execution.agent_id = Uuid::new_v4();
        let foreign = fixtures::authorize(&foreign, f.now);
        assert_eq!(
            f.vault
                .continue_inference_tools(continuation.clone(), &foreign, f.now),
            Err(RuntimeError::Forbidden)
        );
        assert_eq!(
            f.vault.cancel_inference(parent_id, &grant, f.now),
            Ok(RunState::Cancelled)
        );
        assert_eq!(
            f.vault
                .continue_inference_tools(continuation, &grant, f.now),
            Err(RuntimeError::Conflict)
        );
        assert_eq!(f.vault.state().budget.committed_microdollars(), Ok(100));
        assert_eq!(f.vault.state().inference_runs.len(), 1);
    }

    #[test]
    fn terminal_tool_and_late_cost_receipts_durably_freeze_paid_dispatch_on_overrun() {
        use admin_panel_domain::inference::{ToolCall, ToolDefinition};
        for mode in 0..3 {
            let mut f = fixture();
            f.request.tools.push(ToolDefinition {
                name: "read_evidence".into(),
                description: "Read own evidence".into(),
                parameters: serde_json::json!({"type":"object"}),
            });
            let grant = fixtures::authorize(&f.claims, f.now);
            let id = f.request.request_id;
            f.vault
                .prepare_inference(f.request.clone(), &grant, f.now)
                .unwrap();
            f.vault
                .dispatch_inference_once(id, &grant, preparation(&f), f.now)
                .unwrap();
            let overrun = 1600;
            match mode {
                0 => f
                    .vault
                    .finish_inference(id, TerminalOutcome::Completed, Some(overrun), f.now)
                    .unwrap(),
                1 => f
                    .vault
                    .finish_inference_tools(
                        id,
                        vec![ToolCall {
                            id: "call_1".into(),
                            name: "read_evidence".into(),
                            arguments: serde_json::json!({}),
                        }],
                        Some(overrun),
                        f.now,
                    )
                    .unwrap(),
                _ => {
                    f.vault
                        .finish_inference(id, TerminalOutcome::Cancelled, None, f.now)
                        .unwrap();
                    f.vault
                        .reconcile_inference_cost(id, overrun, f.now)
                        .unwrap();
                    f.vault
                        .reconcile_inference_cost(id, overrun, f.now)
                        .unwrap();
                }
            }
            assert_eq!(f.vault.state().budget.committed_microdollars(), Ok(overrun));
            let price = preparation(&f).paid_cost.unwrap();
            drop(f.vault);
            let mut restored = Vault::open(
                &f.directory.path().join("state"),
                &f.directory.path().join("key"),
                "sdlc2",
            )
            .unwrap();
            assert!(restored.state().budget.paid_dispatch_blocked());
            let run = restored.inference_readback(id, &grant, f.now).unwrap();
            assert_eq!(
                run.events
                    .iter()
                    .filter(|event| matches!(
                        event.kind,
                        JournalEventKind::CostCeilingExceeded { .. }
                    ))
                    .count(),
                1
            );
            assert_eq!(
                restored.state().budget.reservations[&id].status,
                ReservationStatus::CostOverrun
            );
            assert_eq!(
                restored.reserve_paid_request(Uuid::new_v4(), &"a".repeat(64), price, f.now),
                Err(RuntimeError::Budget(BudgetError::UpperBoundExceeded))
            );
        }
    }
}

impl RunState {
    fn terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Cancelled | Self::Failed | Self::ToolsSubmitted
        )
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum JournalEventKind {
    Prepared,
    DispatchIntent,
    TextDelta {
        text: String,
    },
    CancellationRequested,
    CancelledBeforeDispatch,
    UnknownOutcome,
    ProviderTerminal {
        outcome: TerminalOutcome,
    },
    CostReconciled {
        actual_microdollars: u64,
    },
    CostCeilingExceeded {
        actual_microdollars: u64,
        ceiling_microdollars: u64,
    },
    ToolCalls {
        calls: Vec<admin_panel_domain::inference::ToolCall>,
        content: Option<String>,
    },
    ToolResultsSubmitted {
        continuation_request_id: Uuid,
    },
    CancelledWaitingTools,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalOutcome {
    Completed,
    Cancelled,
    Failed,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalEvent {
    pub sequence: u64,
    pub occurred_at: DateTime<Utc>,
    pub kind: JournalEventKind,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredInference {
    pub request: InferenceRequest,
    pub request_fingerprint: String,
    pub machine_subject: String,
    pub fencing_token: u64,
    pub state: RunState,
    pub registration: RegisteredRevision,
    pub events: Vec<JournalEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_request_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation_request_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub(crate) openrouter_history:
        std::collections::BTreeMap<usize, crate::openrouter_reasoning::OpenRouterReasoning>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) openrouter_response_reasoning:
        Option<crate::openrouter_reasoning::OpenRouterReasoning>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) openrouter_generation_id: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootProfileBinding {
    pub workspace: String,
    pub owner_subject: String,
    pub project_id: Uuid,
    pub profile_revision: u64,
}

#[derive(Clone, Serialize)]
pub struct InferenceStatus {
    pub schema_version: u32,
    pub request_id: Uuid,
    pub execution_id: Uuid,
    pub profile_revision: u64,
    pub state: RunState,
    pub last_event_sequence: u64,
}

#[derive(Clone, Serialize)]
pub struct InferenceEventsPage {
    pub schema_version: u32,
    pub request_id: Uuid,
    pub events: Vec<JournalEvent>,
    pub next_sequence: u64,
    pub has_more: bool,
}

impl RootProfileBinding {
    fn matches(&self, scope: &ExecutionScope, revision: u64) -> bool {
        self.workspace == scope.workspace
            && self.owner_subject == scope.owner_subject
            && self.project_id == scope.project_id
            && self.profile_revision == revision
    }
}

/// Values come from the trusted adapter, not the caller or model metadata.
pub struct DispatchPreparation {
    pub framing_tokens: Option<u64>,
    pub output_limit_supported: bool,
    pub paid_cost: Option<CostEstimate>,
}

fn owns(run: &StoredInference, grant: &AuthorizedExecution) -> bool {
    run.machine_subject == grant.machine_subject()
        && run.request.execution == *grant.execution()
        && run.request.profile_revision == grant.profile_revision()
        && run.fencing_token == grant.fencing_token()
}

pub(crate) fn append(
    run: &mut StoredInference,
    kind: JournalEventKind,
    now: DateTime<Utc>,
) -> Result<(), RuntimeError> {
    if run.events.len() >= MAX_EVENTS
        || serde_json::to_vec(&kind)
            .map_err(|_| RuntimeError::Protocol)?
            .len()
            > MAX_EVENT_BYTES
    {
        return Err(RuntimeError::Protocol);
    }
    run.events.push(JournalEvent {
        sequence: run.events.len() as u64 + 1,
        occurred_at: now,
        kind,
    });
    Ok(())
}

pub(crate) fn append_cost_overrun(
    run: &mut StoredInference,
    budget: &crate::budget::BudgetLedger,
    request_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), RuntimeError> {
    if let Some(reservation) = budget.reservations.get(&request_id) {
        if reservation.status == crate::budget::ReservationStatus::CostOverrun {
            append(
                run,
                JournalEventKind::CostCeilingExceeded {
                    actual_microdollars: reservation
                        .actual_microdollars
                        .ok_or(RuntimeError::Protocol)?,
                    ceiling_microdollars: reservation.ceiling_microdollars,
                },
                now,
            )?;
        }
    }
    Ok(())
}

impl Vault {
    /// True admits a new request; false is readback, never a dispatch permit.
    /// The grant issuer must confirm Admin's active revision, not a pending receipt.
    pub fn prepare_inference(
        &mut self,
        request: InferenceRequest,
        grant: &AuthorizedExecution,
        now: DateTime<Utc>,
    ) -> Result<bool, RuntimeError> {
        grant.check_current(now)?;
        if request.request_id.is_nil()
            || request.execution != *grant.execution()
            || request.profile_revision != grant.profile_revision()
            || request.schema_version != 1
            || request.validate_conversation().is_err()
        {
            return Err(RuntimeError::Forbidden);
        }
        let fingerprint = hex::encode(Sha256::digest(
            serde_json::to_vec(&request).map_err(|_| RuntimeError::InvalidRequest)?,
        ));
        if let Some(existing) = self.state().inference_runs.get(&request.request_id) {
            if !owns(existing, grant) {
                return Err(RuntimeError::Forbidden);
            }
            return if existing.request_fingerprint == fingerprint {
                Ok(false)
            } else {
                Err(RuntimeError::Conflict)
            };
        }
        crate::schema_validation::InferenceSchemas::compile(&request)?;
        if self.state().inference_runs.len() >= MAX_RUNS {
            return Err(RuntimeError::Unavailable);
        }
        if self
            .state()
            .root_profile_bindings
            .get(&request.execution.root_task_id)
            .is_some_and(|root| !root.matches(&request.execution, request.profile_revision))
        {
            return Err(RuntimeError::Forbidden);
        }
        // Assignment replacement cannot take over an unconfirmed old run.
        for run in self
            .state()
            .inference_runs
            .values()
            .filter(|run| run.request.execution.task_id == request.execution.task_id)
        {
            let same_assignment = run.request.execution == request.execution
                && run.machine_subject == grant.machine_subject()
                && run.fencing_token == grant.fencing_token();
            if run.fencing_token > grant.fencing_token()
                || (!same_assignment && !run.state.terminal())
                || (!same_assignment && run.fencing_token == grant.fencing_token())
            {
                return Err(RuntimeError::Forbidden);
            }
        }
        let registered = self
            .state()
            .registered_revisions
            .values()
            .find(|record| record.registration.profile.revision == request.profile_revision)
            .ok_or(RuntimeError::CapabilityNotVerified)?
            .clone();
        request
            .validate_binding(grant.execution(), &registered.registration.profile)
            .map_err(|_| RuntimeError::Forbidden)?;
        self.check_inference_connection(&registered)?;
        let mut run = StoredInference {
            request,
            request_fingerprint: fingerprint,
            machine_subject: grant.machine_subject().into(),
            fencing_token: grant.fencing_token(),
            state: RunState::Prepared,
            registration: registered,
            events: vec![],
            parent_request_id: None,
            continuation_request_id: None,
            openrouter_history: Default::default(),
            openrouter_response_reasoning: None,
            openrouter_generation_id: None,
        };
        append(&mut run, JournalEventKind::Prepared, now)?;
        let mut next = self.state().clone();
        let scope = &run.request.execution;
        next.root_profile_bindings
            .entry(scope.root_task_id)
            .or_insert_with(|| RootProfileBinding {
                workspace: scope.workspace.clone(),
                owner_subject: scope.owner_subject.clone(),
                project_id: scope.project_id,
                profile_revision: run.request.profile_revision,
            });
        next.inference_runs.insert(run.request.request_id, run);
        self.commit(next)?;
        Ok(true)
    }

    pub(crate) fn check_inference_connection(
        &self,
        registered: &RegisteredRevision,
    ) -> Result<(), RuntimeError> {
        let registration = &registered.registration;
        let provider = match registration.profile.provider {
            ProviderId::Chatgpt => "chatgpt",
            ProviderId::Openrouter => "openrouter",
        };
        let (adapter, accounting) = deployed_contract(registration.profile.provider);
        if registration.adapter_version != adapter || registration.accounting_policy != accounting {
            return Err(RuntimeError::CapabilityNotVerified);
        }
        let connection = self
            .state()
            .connections
            .get(provider)
            .ok_or(RuntimeError::Disconnected)?;
        if connection.provider != registration.profile.provider
            || connection.generation != registration.credential_generation
        {
            return Err(RuntimeError::Conflict);
        }
        Ok(())
    }

    /// An Ok result permits exactly one I/O attempt by the trusted adapter.
    /// Reservation and intent persist together before that attempt; retry fails.
    pub fn dispatch_inference_once(
        &mut self,
        request_id: Uuid,
        grant: &AuthorizedExecution,
        preparation: DispatchPreparation,
        now: DateTime<Utc>,
    ) -> Result<(), RuntimeError> {
        grant.check_current(now)?;
        let run = self.inference_readback(request_id, grant, now)?;
        if run.state != RunState::Prepared {
            return Err(RuntimeError::Conflict);
        }
        self.check_inference_connection(&run.registration)?;
        let proof = self
            .state()
            .verified_adapters
            .get(&run.registration.registration.profile.verification_id)
            .ok_or(RuntimeError::CapabilityNotVerified)?;
        if proof.adapter_version != run.registration.registration.adapter_version
            || proof.accounting_policy != run.registration.registration.accounting_policy
            || proof.draft_revision != Some(run.registration.registration.draft_revision)
            || proof.evidence.credential_generation
                != run.registration.registration.credential_generation
            || proof.evidence.settings.provider != run.registration.registration.profile.provider
            || proof.evidence.settings.model != run.registration.registration.profile.model
            || proof.evidence.settings.context_window_tokens
                != run.registration.registration.profile.context_window_tokens
            || proof
                .evidence
                .settings
                .validate(&proof.evidence.capabilities)
                .is_err()
        {
            return Err(RuntimeError::CapabilityNotVerified);
        }
        let input = if run.registration.registration.profile.provider == ProviderId::Openrouter {
            if !preparation.output_limit_supported {
                return Err(RuntimeError::CapabilityNotVerified);
            }
            crate::openrouter_request::ChatRequest::from_paid_stored(
                &run,
                grant.execution(),
                &proof.evidence.capabilities,
                preparation.framing_tokens,
                preparation.paid_cost.as_ref(),
                now,
            )
            .map(|wire| wire.input_upper_bound_tokens())?
        } else {
            run.request
                .check_byte_upper_bound(
                    run.registration.registration.profile.context_window_tokens,
                    &proof.evidence.capabilities,
                    preparation.framing_tokens,
                    preparation.output_limit_supported,
                )
                .map_err(|error| match error {
                    admin_panel_domain::inference::InferenceError::Context(_) => {
                        RuntimeError::ContextExceeded
                    }
                    _ => RuntimeError::CapabilityNotVerified,
                })?
        };
        let mut next = self.state().clone();
        match (
            run.registration.registration.profile.provider,
            preparation.paid_cost,
        ) {
            (ProviderId::Openrouter, Some(cost)) => {
                if cost.model != run.registration.registration.profile.model
                    || cost.credential_generation
                        != run.registration.registration.credential_generation
                    || u64::from(cost.input_tokens) < input
                    || cost.output_tokens < run.request.output_reserve_tokens
                {
                    return Err(RuntimeError::CapabilityNotVerified);
                }
                if !next
                    .budget
                    .reserve(request_id, &run.request_fingerprint, cost, now)?
                {
                    return Err(RuntimeError::Conflict);
                }
                next.budget.mark_dispatched(request_id)?;
            }
            (ProviderId::Chatgpt, None) => {}
            _ => return Err(RuntimeError::CapabilityNotVerified),
        }
        let run = next.inference_runs.get_mut(&request_id).unwrap();
        append(run, JournalEventKind::DispatchIntent, now)?;
        run.state = RunState::Dispatching;
        self.commit(next)
    }

    pub(crate) fn inference_readback(
        &self,
        request_id: Uuid,
        grant: &AuthorizedExecution,
        now: DateTime<Utc>,
    ) -> Result<StoredInference, RuntimeError> {
        grant.check_current(now)?;
        let run = self
            .state()
            .inference_runs
            .get(&request_id)
            .ok_or(RuntimeError::Forbidden)?;
        if !owns(run, grant) {
            return Err(RuntimeError::Forbidden);
        }
        Ok(run.clone())
    }

    pub fn inference_status(
        &self,
        request_id: Uuid,
        grant: &AuthorizedExecution,
        now: DateTime<Utc>,
    ) -> Result<InferenceStatus, RuntimeError> {
        let run = self.inference_readback(request_id, grant, now)?;
        Ok(InferenceStatus {
            schema_version: 1,
            request_id,
            execution_id: run.request.execution.execution_id,
            profile_revision: run.request.profile_revision,
            state: run.state,
            last_event_sequence: run.events.len() as u64,
        })
    }

    /// Cursor is the next sequence requested; bounds never silently skip events.
    pub fn inference_events(
        &self,
        request_id: Uuid,
        grant: &AuthorizedExecution,
        from_sequence: u64,
        limit: usize,
        now: DateTime<Utc>,
    ) -> Result<InferenceEventsPage, RuntimeError> {
        let run = self.inference_readback(request_id, grant, now)?;
        if from_sequence == 0
            || from_sequence > run.events.len() as u64 + 1
            || limit == 0
            || limit > 200
        {
            return Err(RuntimeError::InvalidRequest);
        }
        let events: Vec<_> = run
            .events
            .iter()
            .skip(from_sequence as usize - 1)
            .take(limit)
            .cloned()
            .collect();
        let next_sequence = from_sequence + events.len() as u64;
        Ok(InferenceEventsPage {
            schema_version: 1,
            request_id,
            events,
            next_sequence,
            has_more: next_sequence <= run.events.len() as u64,
        })
    }

    pub fn cancel_inference(
        &mut self,
        request_id: Uuid,
        grant: &AuthorizedExecution,
        now: DateTime<Utc>,
    ) -> Result<RunState, RuntimeError> {
        let current = self.inference_readback(request_id, grant, now)?;
        if current.state.terminal() || current.state == RunState::CancellationRequested {
            return Ok(current.state);
        }
        let mut next = self.state().clone();
        let run = next.inference_runs.get_mut(&request_id).unwrap();
        let kind = if run.state == RunState::AwaitingTools {
            run.state = RunState::Cancelled;
            JournalEventKind::CancelledWaitingTools
        } else if run.state == RunState::Prepared {
            run.state = RunState::Cancelled;
            JournalEventKind::CancelledBeforeDispatch
        } else {
            run.state = RunState::CancellationRequested;
            JournalEventKind::CancellationRequested
        };
        append(run, kind, now)?;
        let state = run.state;
        self.commit(next)?;
        Ok(state)
    }

    /// Adapter-only append. Agents cannot supply provider completion evidence.
    pub fn record_inference_delta(
        &mut self,
        request_id: Uuid,
        expected_sequence: u64,
        text: String,
        now: DateTime<Utc>,
    ) -> Result<(), RuntimeError> {
        let mut next = self.state().clone();
        let run = next
            .inference_runs
            .get_mut(&request_id)
            .ok_or(RuntimeError::Conflict)?;
        if expected_sequence > 0 && expected_sequence <= run.events.len() as u64 {
            return if run.events[expected_sequence as usize - 1].kind
                == (JournalEventKind::TextDelta { text })
            {
                Ok(())
            } else {
                Err(RuntimeError::Conflict)
            };
        }
        if !matches!(
            run.state,
            RunState::Dispatching | RunState::Running | RunState::CancellationRequested
        ) || expected_sequence != run.events.len() as u64 + 1
            || text.is_empty()
        {
            return Err(RuntimeError::Conflict);
        }
        append(run, JournalEventKind::TextDelta { text }, now)?;
        if run.state != RunState::CancellationRequested {
            run.state = RunState::Running;
        }
        self.commit(next)
    }

    /// Only a reconciled provider terminal receipt permits terminal state.
    /// Unknown cost retains the full reservation even if provider execution stopped.
    pub fn finish_inference(
        &mut self,
        request_id: Uuid,
        outcome: TerminalOutcome,
        actual_microdollars: Option<u64>,
        now: DateTime<Utc>,
    ) -> Result<(), RuntimeError> {
        let mut next = self.state().clone();
        let run = next
            .inference_runs
            .get_mut(&request_id)
            .ok_or(RuntimeError::Conflict)?;
        if run.state.terminal() {
            let same_outcome = run
                .events
                .iter()
                .rev()
                .find(|event| matches!(event.kind, JournalEventKind::ProviderTerminal { .. }))
                .is_some_and(|event| event.kind == JournalEventKind::ProviderTerminal { outcome });
            let same_cost = match next.budget.reservations.get(&request_id) {
                Some(reservation) => reservation.actual_microdollars == actual_microdollars,
                None => actual_microdollars.is_none(),
            };
            return if same_outcome && same_cost {
                Ok(())
            } else {
                Err(RuntimeError::Conflict)
            };
        }
        if matches!(run.state, RunState::Prepared | RunState::AwaitingTools) {
            return Err(RuntimeError::Conflict);
        }
        match (
            run.registration.registration.profile.provider,
            actual_microdollars,
        ) {
            (ProviderId::Openrouter, Some(cost)) => next.budget.settle(request_id, cost)?,
            (ProviderId::Openrouter, None) => next.budget.mark_uncertain(request_id)?,
            (ProviderId::Chatgpt, None) => {}
            _ => return Err(RuntimeError::Conflict),
        }
        append_cost_overrun(run, &next.budget, request_id, now)?;
        append(run, JournalEventKind::ProviderTerminal { outcome }, now)?;
        run.state = match outcome {
            TerminalOutcome::Completed => RunState::Completed,
            TerminalOutcome::Cancelled => RunState::Cancelled,
            TerminalOutcome::Failed => RunState::Failed,
        };
        self.commit(next)
    }

    /// A later billing readback can reconcile cost without replaying provider I/O.
    pub fn reconcile_inference_cost(
        &mut self,
        request_id: Uuid,
        actual_microdollars: u64,
        now: DateTime<Utc>,
    ) -> Result<(), RuntimeError> {
        let mut next = self.state().clone();
        let run = next
            .inference_runs
            .get_mut(&request_id)
            .ok_or(RuntimeError::Conflict)?;
        if !(run.state.terminal() || run.state == RunState::AwaitingTools)
            || run.registration.registration.profile.provider != ProviderId::Openrouter
        {
            return Err(RuntimeError::Conflict);
        }
        let reservation = next
            .budget
            .reservations
            .get(&request_id)
            .ok_or(RuntimeError::Conflict)?;
        if let Some(previous) = reservation.actual_microdollars {
            return if previous == actual_microdollars {
                Ok(())
            } else {
                Err(RuntimeError::Conflict)
            };
        }
        next.budget.settle(request_id, actual_microdollars)?;
        append_cost_overrun(run, &next.budget, request_id, now)?;
        append(
            run,
            JournalEventKind::CostReconciled {
                actual_microdollars,
            },
            now,
        )?;
        self.commit(next)
    }

    /// Startup recovery never retries I/O or refunds an unknown paid result.
    pub fn reconcile_inference_restart(
        &mut self,
        now: DateTime<Utc>,
    ) -> Result<usize, RuntimeError> {
        let mut next = self.state().clone();
        let mut changed = 0;
        for (id, run) in &mut next.inference_runs {
            if matches!(
                run.state,
                RunState::Dispatching | RunState::Running | RunState::CancellationRequested
            ) {
                if run.registration.registration.profile.provider == ProviderId::Openrouter {
                    next.budget.mark_uncertain(*id)?;
                }
                append(run, JournalEventKind::UnknownOutcome, now)?;
                run.state = RunState::Unknown;
                changed += 1;
            }
        }
        if changed > 0 {
            self.commit(next)?;
        }
        Ok(changed)
    }
}
