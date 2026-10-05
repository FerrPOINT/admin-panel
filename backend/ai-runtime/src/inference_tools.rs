//! Tool results continue the exact encrypted conversation; the broker executes none.
use crate::{
    error::RuntimeError,
    execution_grant::AuthorizedExecution,
    inference_journal::{
        JournalEventKind, MAX_RUNS, RunState, StoredInference, TerminalOutcome, append,
        append_cost_overrun,
    },
    vault::Vault,
};
use admin_panel_domain::{
    ai::ProviderId,
    inference::{Message, ToolCall, ToolContinuation},
};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use uuid::Uuid;

impl Vault {
    /// Called only after the adapter proves a terminal tool-call turn and validates
    /// its arguments/schema. This boundary additionally rejects undeclared tools.
    pub fn finish_inference_tools(
        &mut self,
        request_id: Uuid,
        calls: Vec<ToolCall>,
        actual_microdollars: Option<u64>,
        now: DateTime<Utc>,
    ) -> Result<(), RuntimeError> {
        self.finish_inference_tools_with_reasoning(
            request_id,
            calls,
            None,
            actual_microdollars,
            now,
        )
    }

    pub(crate) fn finish_inference_tools_with_reasoning(
        &mut self,
        request_id: Uuid,
        calls: Vec<ToolCall>,
        reasoning: Option<crate::openrouter_reasoning::OpenRouterReasoning>,
        actual_microdollars: Option<u64>,
        now: DateTime<Utc>,
    ) -> Result<(), RuntimeError> {
        let mut next = self.state().clone();
        let run = next
            .inference_runs
            .get_mut(&request_id)
            .ok_or(RuntimeError::Conflict)?;
        if let Some(reasoning) = &reasoning {
            reasoning.validate()?;
            if run.registration.registration.profile.provider != ProviderId::Openrouter
                || reasoning.is_empty()
            {
                return Err(RuntimeError::Protocol);
            }
        }
        run.request
            .validate_returned_tools(&calls)
            .map_err(|_| RuntimeError::Protocol)?;
        crate::schema_validation::InferenceSchemas::compile(&run.request)?
            .validate_tool_calls(&calls)?;
        let content = run
            .events
            .iter()
            .filter_map(|event| match &event.kind {
                JournalEventKind::TextDelta { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<String>();
        let event = JournalEventKind::ToolCalls {
            calls,
            content: (!content.is_empty()).then_some(content),
        };
        if let Some(previous) = run
            .events
            .iter()
            .find(|event| matches!(event.kind, JournalEventKind::ToolCalls { .. }))
        {
            let same_cost = match next.budget.reservations.get(&request_id) {
                Some(reservation) => reservation.actual_microdollars == actual_microdollars,
                None => actual_microdollars.is_none(),
            };
            return if previous.kind == event
                && same_cost
                && run.openrouter_response_reasoning == reasoning
            {
                Ok(())
            } else {
                Err(RuntimeError::Conflict)
            };
        }
        if !matches!(
            run.state,
            RunState::Dispatching
                | RunState::Running
                | RunState::Unknown
                | RunState::CancellationRequested
        ) {
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
        append(run, event, now)?;
        run.openrouter_response_reasoning = reasoning;
        if run.state == RunState::CancellationRequested {
            append(
                run,
                JournalEventKind::ProviderTerminal {
                    outcome: TerminalOutcome::Cancelled,
                },
                now,
            )?;
            run.state = RunState::Cancelled;
        } else {
            run.state = RunState::AwaitingTools;
        }
        self.commit(next)
    }

    /// True prepares a new call; false reads back the same continuation. It does
    /// not dispatch, modify model/context, or create a second parent continuation.
    pub fn continue_inference_tools(
        &mut self,
        continuation: ToolContinuation,
        grant: &AuthorizedExecution,
        now: DateTime<Utc>,
    ) -> Result<bool, RuntimeError> {
        grant.check_current(now)?;
        if continuation.schema_version != 1
            || continuation.request_id.is_nil()
            || continuation.request_id == continuation.parent_request_id
            || continuation.execution != *grant.execution()
            || continuation.profile_revision != grant.profile_revision()
        {
            return Err(RuntimeError::Forbidden);
        }
        let parent = self.inference_readback(continuation.parent_request_id, grant, now)?;
        if !matches!(
            parent.state,
            RunState::AwaitingTools | RunState::ToolsSubmitted
        ) {
            return Err(RuntimeError::Conflict);
        }
        let (calls, content) = parent
            .events
            .iter()
            .rev()
            .find_map(|event| match &event.kind {
                JournalEventKind::ToolCalls { calls, content } => Some((calls, content)),
                _ => None,
            })
            .ok_or(RuntimeError::Protocol)?;
        let mut results = BTreeMap::new();
        for result in continuation.results {
            if results
                .insert(result.tool_call_id, result.content)
                .is_some()
            {
                return Err(RuntimeError::InvalidRequest);
            }
        }
        if calls.len() != results.len() || calls.iter().any(|call| !results.contains_key(&call.id))
        {
            return Err(RuntimeError::InvalidRequest);
        }
        let mut request = parent.request.clone();
        let mut openrouter_history = parent.openrouter_history.clone();
        if let Some(reasoning) = &parent.openrouter_response_reasoning {
            openrouter_history.insert(request.messages.len(), reasoning.clone());
        }
        request.request_id = continuation.request_id;
        request.messages.push(Message::Assistant {
            content: content.clone(),
            tool_calls: calls.clone(),
        });
        for call in calls {
            request.messages.push(Message::Tool {
                tool_call_id: call.id.clone(),
                content: results.remove(&call.id).unwrap(),
            });
        }
        request
            .validate_conversation()
            .map_err(|_| RuntimeError::InvalidRequest)?;
        let fingerprint = hex::encode(Sha256::digest(
            serde_json::to_vec(&request).map_err(|_| RuntimeError::InvalidRequest)?,
        ));
        if let Some(previous) = parent.continuation_request_id {
            let run = self.inference_readback(previous, grant, now)?;
            return if previous == continuation.request_id
                && run.request_fingerprint == fingerprint
                && run.parent_request_id == Some(continuation.parent_request_id)
            {
                Ok(false)
            } else {
                Err(RuntimeError::Conflict)
            };
        }
        if self
            .state()
            .inference_runs
            .contains_key(&continuation.request_id)
        {
            return Err(RuntimeError::Conflict);
        }
        if self.state().inference_runs.len() >= MAX_RUNS {
            return Err(RuntimeError::Unavailable);
        }
        self.check_inference_connection(&parent.registration)?;
        let mut run = StoredInference {
            request,
            request_fingerprint: fingerprint,
            machine_subject: parent.machine_subject,
            fencing_token: parent.fencing_token,
            state: RunState::Prepared,
            registration: parent.registration,
            events: vec![],
            parent_request_id: Some(continuation.parent_request_id),
            continuation_request_id: None,
            openrouter_history,
            openrouter_response_reasoning: None,
            openrouter_generation_id: None,
        };
        append(&mut run, JournalEventKind::Prepared, now)?;
        let mut next = self.state().clone();
        let parent = next
            .inference_runs
            .get_mut(&continuation.parent_request_id)
            .unwrap();
        append(
            parent,
            JournalEventKind::ToolResultsSubmitted {
                continuation_request_id: continuation.request_id,
            },
            now,
        )?;
        parent.state = RunState::ToolsSubmitted;
        parent.continuation_request_id = Some(continuation.request_id);
        next.inference_runs.insert(continuation.request_id, run);
        self.commit(next)?;
        Ok(true)
    }
}
