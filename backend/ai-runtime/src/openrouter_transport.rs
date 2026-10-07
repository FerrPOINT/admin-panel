//! One durable dispatch attempt; connection abort never substitutes for terminal proof.
use crate::{
    error::RuntimeError,
    execution_grant::AuthorizedExecution,
    inference_journal::DispatchPreparation,
    openrouter::{OpenRouter, bounded_json, provider_status},
    openrouter_output::{DecodedTurn, OutputDecoder},
    openrouter_request::ChatRequest,
    vault::Vault,
};
use admin_panel_domain::ai::ProviderId;
use chrono::{DateTime, Utc};
use reqwest::{Response, StatusCode};
use serde_json::Value;
use uuid::Uuid;
use zeroize::Zeroizing;

// No Clone/Debug/Deserialize: this owns credentials and consumes one persisted intent.
pub struct OpenRouterDispatch {
    credential: Zeroizing<String>,
    request: ChatRequest,
    decoder: OutputDecoder,
    authorization: AuthorizedExecution,
    #[cfg(test)]
    fixture_now: Option<DateTime<Utc>>,
}

impl OpenRouterDispatch {
    /// Server-owned accounting/pricing must be confirmed before this method is used.
    pub fn begin(
        vault: &mut Vault,
        request_id: Uuid,
        grant: &AuthorizedExecution,
        preparation: DispatchPreparation,
        now: DateTime<Utc>,
    ) -> Result<Self, RuntimeError> {
        let run = vault.inference_readback(request_id, grant, now)?;
        if run.registration.registration.profile.provider != ProviderId::Openrouter {
            return Err(RuntimeError::InvalidRequest);
        }
        let proof = vault
            .state()
            .verified_adapters
            .get(&run.registration.registration.profile.verification_id)
            .ok_or(RuntimeError::CapabilityNotVerified)?;
        let request = ChatRequest::from_paid_stored(
            &run,
            grant.execution(),
            &proof.evidence.capabilities,
            preparation.framing_tokens,
            preparation.paid_cost.as_ref(),
            now,
        )?;
        let decoder = OutputDecoder::new(request.model(), &run.request)?;
        let credential = Zeroizing::new(
            vault
                .state()
                .connections
                .get("openrouter")
                .ok_or(RuntimeError::Disconnected)?
                .credential
                .clone(),
        );
        vault.dispatch_inference_once(request_id, grant, preparation, now)?;
        Ok(Self {
            credential,
            request,
            decoder,
            authorization: grant.clone(),
            #[cfg(test)]
            fixture_now: None,
        })
    }
}

pub struct TransportFailure {
    error: RuntimeError,
    generation_id: Option<String>,
}

impl TransportFailure {
    pub fn error(&self) -> RuntimeError {
        self.error
    }
    pub fn generation_id(&self) -> Option<&str> {
        self.generation_id.as_deref()
    }
}

impl std::fmt::Debug for TransportFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransportFailure")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

// Dropping this response aborts local transport; the journal keeps the paid reserve.
pub struct OpenRouterResponse {
    response: Option<Response>,
    decoder: Option<OutputDecoder>,
    generation_id: Option<String>,
    completed: Option<DecodedTurn>,
    failed: bool,
    authorization: AuthorizedExecution,
    #[cfg(test)]
    fixture_now: Option<DateTime<Utc>>,
}

impl OpenRouterResponse {
    fn now(&self) -> DateTime<Utc> {
        #[cfg(test)]
        if let Some(now) = self.fixture_now {
            return now;
        }
        Utc::now()
    }
    pub fn generation_id(&self) -> Option<&str> {
        self.generation_id.as_deref()
    }

    /// Heartbeat renews only the same verified execution, revision and fencing generation.
    pub fn renew_authorization(
        &mut self,
        renewed: &AuthorizedExecution,
    ) -> Result<(), RuntimeError> {
        if self.failed || self.completed.is_some() {
            return Err(RuntimeError::Conflict);
        }
        let now = self.now();
        if self.authorization.check_current(now).is_err() {
            return self.fail(RuntimeError::Forbidden);
        }
        renewed.check_current(now)?;
        if renewed.execution() != self.authorization.execution()
            || renewed.machine_subject() != self.authorization.machine_subject()
            || renewed.profile_revision() != self.authorization.profile_revision()
            || renewed.fencing_token() != self.authorization.fencing_token()
            || renewed.valid_until() < self.authorization.valid_until()
        {
            return Err(RuntimeError::Forbidden);
        }
        self.authorization = renewed.clone();
        Ok(())
    }

    fn fail<T>(&mut self, error: RuntimeError) -> Result<T, RuntimeError> {
        self.failed = true;
        self.response.take();
        Err(error)
    }

    /// None means EOF plus validated finish/usage/DONE, not just successful headers.
    pub async fn next_deltas(&mut self) -> Result<Option<Vec<String>>, RuntimeError> {
        if self.failed {
            return Err(RuntimeError::Protocol);
        }
        if self.completed.is_some() {
            return Ok(None);
        }
        let remaining = match authorization_remaining(&self.authorization, self.now()) {
            Ok(remaining) => remaining,
            Err(error) => return self.fail(error),
        };
        let bytes = match tokio::time::timeout(
            remaining,
            self.response
                .as_mut()
                .ok_or(RuntimeError::Protocol)?
                .chunk(),
        )
        .await
        {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(_)) => return self.fail(RuntimeError::Unavailable),
            Err(_) => return self.fail(RuntimeError::Forbidden),
        };
        if self.authorization.check_current(self.now()).is_err() {
            return self.fail(RuntimeError::Forbidden);
        }
        match bytes {
            Some(bytes) => {
                let decoder = self.decoder.as_mut().ok_or(RuntimeError::Protocol)?;
                let deltas = decoder.feed(&bytes);
                if let Some(id) = decoder.generation_id() {
                    if !valid_generation(id)
                        || self
                            .generation_id
                            .as_deref()
                            .is_some_and(|prior| prior != id)
                    {
                        return self.fail(RuntimeError::Protocol);
                    }
                    self.generation_id = Some(id.into());
                }
                match deltas {
                    Ok(deltas) => Ok(Some(deltas)),
                    Err(error) => self.fail(error),
                }
            }
            None => {
                self.response.take();
                match self.decoder.take().ok_or(RuntimeError::Protocol)?.finish() {
                    Ok(turn) => {
                        self.completed = Some(turn);
                        Ok(None)
                    }
                    Err(error) => self.fail(error),
                }
            }
        }
    }

    pub fn finish(self) -> Result<DecodedTurn, RuntimeError> {
        if self.failed {
            return Err(RuntimeError::Protocol);
        }
        self.completed.ok_or(RuntimeError::Protocol)
    }
}

impl OpenRouter {
    /// Consumes the durable permit even on timeout; callers must not repeat POST.
    pub async fn start(
        &self,
        dispatch: OpenRouterDispatch,
    ) -> Result<OpenRouterResponse, TransportFailure> {
        let fail = |error| TransportFailure {
            error,
            generation_id: None,
        };
        let now = Utc::now();
        #[cfg(test)]
        let now = dispatch.fixture_now.unwrap_or(now);
        let remaining = authorization_remaining(&dispatch.authorization, now).map_err(fail)?;
        let pending = self
            .client
            .post(
                self.endpoint
                    .join("chat/completions")
                    .map_err(|_| fail(RuntimeError::Configuration))?,
            )
            .bearer_auth(dispatch.credential.as_str())
            .header("Accept", "text/event-stream")
            .json(dispatch.request.body())
            .send();
        let response = tokio::time::timeout(remaining, pending)
            .await
            .map_err(|_| fail(RuntimeError::Forbidden))?
            .map_err(|_| fail(RuntimeError::Unavailable))?;
        let generation_id = match response.headers().get("x-generation-id") {
            Some(value) => Some(
                value
                    .to_str()
                    .ok()
                    .filter(|id| valid_generation(id))
                    .ok_or_else(|| fail(RuntimeError::Protocol))?
                    .to_owned(),
            ),
            None => None,
        };
        if let Err(error) = provider_status(response.status()) {
            return Err(TransportFailure {
                error,
                generation_id,
            });
        }
        if response.status() != StatusCode::OK
            || response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.split(';').next())
                .is_none_or(|v| !v.trim().eq_ignore_ascii_case("text/event-stream"))
        {
            return Err(TransportFailure {
                error: RuntimeError::Protocol,
                generation_id,
            });
        }
        Ok(OpenRouterResponse {
            response: Some(response),
            decoder: Some(dispatch.decoder),
            generation_id,
            completed: None,
            failed: false,
            authorization: dispatch.authorization,
            #[cfg(test)]
            fixture_now: dispatch.fixture_now,
        })
    }

    /// Readback only. Missing metadata does not authorize a new paid request.
    pub async fn generation(
        &self,
        credential: &str,
        id: &str,
        model: &str,
    ) -> Result<GenerationReceipt, RuntimeError> {
        if !valid_generation(id) || model.is_empty() || model.len() > 256 {
            return Err(RuntimeError::InvalidRequest);
        }
        let response = self
            .client
            .get(
                self.endpoint
                    .join("generation")
                    .map_err(|_| RuntimeError::Configuration)?,
            )
            .query(&[("id", id)])
            .bearer_auth(credential)
            .send()
            .await
            .map_err(|_| RuntimeError::Unavailable)?;
        if response.status() == StatusCode::NOT_FOUND {
            return Err(RuntimeError::Unavailable);
        }
        parse_generation(&bounded_json(response).await?, id, model)
    }
}

fn authorization_remaining(
    grant: &AuthorizedExecution,
    now: DateTime<Utc>,
) -> Result<std::time::Duration, RuntimeError> {
    grant.check_current(now)?;
    (grant.valid_until() - now)
        .to_std()
        .map_err(|_| RuntimeError::Forbidden)
}

pub struct GenerationReceipt {
    id: String,
    model: String,
    actual_microdollars: u64,
    cancelled: bool,
    finish_reason: Option<String>,
}

impl GenerationReceipt {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn model(&self) -> &str {
        &self.model
    }
    pub fn actual_microdollars(&self) -> u64 {
        self.actual_microdollars
    }
    pub fn cancelled(&self) -> bool {
        self.cancelled
    }
    pub fn finish_reason(&self) -> Option<&str> {
        self.finish_reason.as_deref()
    }
}

pub(crate) fn valid_generation(id: &str) -> bool {
    id.len() > 4
        && id.len() <= 128
        && id.starts_with("gen-")
        && id.bytes().all(|v| v.is_ascii_alphanumeric() || v == b'-')
}

fn parse_generation(
    value: &Value,
    id: &str,
    model: &str,
) -> Result<GenerationReceipt, RuntimeError> {
    let data = &value["data"];
    if !data.is_object()
        || data["id"].as_str() != Some(id)
        || data["model"].as_str() != Some(model)
        || data["streamed"].as_bool() != Some(true)
        || data["is_byok"].as_bool() != Some(false)
    {
        return Err(RuntimeError::Protocol);
    }
    let cancelled = data["cancelled"].as_bool().ok_or(RuntimeError::Protocol)?;
    let finish_reason = data["finish_reason"].as_str().map(str::to_owned);
    if !cancelled
        && !finish_reason.as_deref().is_some_and(|reason| {
            matches!(
                reason,
                "stop" | "tool_calls" | "length" | "content_filter" | "error"
            )
        })
    {
        return Err(RuntimeError::Protocol);
    }
    Ok(GenerationReceipt {
        id: id.into(),
        model: model.into(),
        cancelled,
        finish_reason,
        actual_microdollars: scaled_decimal(&data["total_cost"], 6)?,
    })
}

impl Vault {
    /// Persist the provider correlation before consuming transcript or losing transport.
    pub fn record_openrouter_generation(
        &mut self,
        request_id: Uuid,
        generation: &str,
    ) -> Result<(), RuntimeError> {
        if !valid_generation(generation) {
            return Err(RuntimeError::Protocol);
        }
        let mut next = self.state().clone();
        let run = next
            .inference_runs
            .get_mut(&request_id)
            .ok_or(RuntimeError::Conflict)?;
        if run.registration.registration.profile.provider != ProviderId::Openrouter
            || matches!(run.state, crate::inference_journal::RunState::Prepared)
        {
            return Err(RuntimeError::Conflict);
        }
        if let Some(prior) = &run.openrouter_generation_id {
            return if prior == generation {
                Ok(())
            } else {
                Err(RuntimeError::Conflict)
            };
        }
        run.openrouter_generation_id = Some(generation.into());
        self.commit(next)
    }

    /// A local abort/error has no terminal or billing proof and must retain the reserve.
    pub fn mark_inference_unknown(
        &mut self,
        request_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<(), RuntimeError> {
        use crate::inference_journal::{JournalEventKind, RunState, append};
        let mut next = self.state().clone();
        let run = next
            .inference_runs
            .get_mut(&request_id)
            .ok_or(RuntimeError::Conflict)?;
        if run.state == RunState::Unknown {
            return Ok(());
        }
        if !matches!(
            run.state,
            RunState::Dispatching | RunState::Running | RunState::CancellationRequested
        ) {
            return Err(RuntimeError::Conflict);
        }
        if run.registration.registration.profile.provider == ProviderId::Openrouter {
            next.budget.mark_uncertain(request_id)?;
        }
        append(run, JournalEventKind::UnknownOutcome, now)?;
        run.state = RunState::Unknown;
        self.commit(next)
    }

    /// A stopped provider with lost output cannot manufacture a successful agent result.
    pub fn reconcile_openrouter_generation(
        &mut self,
        request_id: Uuid,
        receipt: &GenerationReceipt,
        now: DateTime<Utc>,
    ) -> Result<(), RuntimeError> {
        use crate::inference_journal::{RunState, TerminalOutcome};
        let run = self
            .state()
            .inference_runs
            .get(&request_id)
            .ok_or(RuntimeError::Conflict)?;
        if run.registration.registration.profile.provider != ProviderId::Openrouter
            || run.registration.registration.profile.model != receipt.model
            || run.openrouter_generation_id.as_deref() != Some(receipt.id.as_str())
            || !matches!(
                run.state,
                RunState::Unknown
                    | RunState::CancellationRequested
                    | RunState::Failed
                    | RunState::Cancelled
            )
        {
            return Err(RuntimeError::Conflict);
        }
        self.finish_inference(
            request_id,
            if receipt.cancelled {
                TerminalOutcome::Cancelled
            } else {
                TerminalOutcome::Failed
            },
            Some(receipt.actual_microdollars),
            now,
        )
    }
}

// Decimal provider USD must never round down through binary floating point.
pub(crate) fn scaled_decimal(value: &Value, scale: u32) -> Result<u64, RuntimeError> {
    let text = match value {
        Value::String(v) => v.clone(),
        Value::Number(v) => v.to_string(),
        _ => return Err(RuntimeError::Protocol),
    };
    if text.is_empty() || text.len() > 64 {
        return Err(RuntimeError::Protocol);
    }
    let parts: Vec<_> = text.split(['e', 'E']).collect();
    if parts.len() > 2 {
        return Err(RuntimeError::Protocol);
    }
    let exponent = if parts.len() == 2 {
        parts[1]
            .parse::<i32>()
            .map_err(|_| RuntimeError::Protocol)?
    } else {
        0
    };
    if !(-38..=38).contains(&exponent) {
        return Err(RuntimeError::Protocol);
    }
    let decimal: Vec<_> = parts[0].split('.').collect();
    if decimal.len() > 2
        || decimal
            .iter()
            .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(RuntimeError::Protocol);
    }
    let fraction = decimal.get(1).map_or(0, |v| v.len());
    let coefficient = decimal
        .concat()
        .parse::<u128>()
        .map_err(|_| RuntimeError::Protocol)?;
    if coefficient == 0 {
        return Ok(0);
    }
    let shift = i64::from(exponent) + i64::from(scale) - fraction as i64;
    let power = 10u128
        .checked_pow(u32::try_from(shift.unsigned_abs()).map_err(|_| RuntimeError::Protocol)?)
        .ok_or(RuntimeError::Protocol)?;
    let amount = if shift >= 0 {
        coefficient
            .checked_mul(power)
            .ok_or(RuntimeError::Protocol)?
    } else {
        coefficient.div_ceil(power)
    };
    u64::try_from(amount).map_err(|_| RuntimeError::Protocol)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        budget::{CostEstimate, ReservationStatus},
        execution_grant::fixtures,
        inference_journal::{RunState, tests::prepared_fixture_state},
    };
    use axum::{
        Json, Router,
        body::Body,
        http::HeaderMap,
        routing::{get, post},
    };
    use serde_json::json;
    use std::{
        convert::Infallible,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    const MODEL: &str = "deepseek/deepseek-v4.1-flash";
    const GENERATION: &str = "gen-QA-123";

    struct Fixture {
        _directory: tempfile::TempDir,
        vault: Vault,
        claims: crate::execution_grant::ExecutionClaims,
        id: Uuid,
    }

    fn fixture() -> Fixture {
        let (state, claims, id) = prepared_fixture_state();
        let directory = tempfile::tempdir().unwrap();
        let state_path = directory.path().join("state");
        let key = directory.path().join("key");
        Vault::initialize(&state_path, &key, "sdlc2").unwrap();
        let mut vault = Vault::open(&state_path, &key, "sdlc2").unwrap();
        vault.commit(state).unwrap();
        Fixture {
            _directory: directory,
            vault,
            claims,
            id,
        }
    }

    fn preparation(f: &Fixture) -> DispatchPreparation {
        DispatchPreparation {
            framing_tokens: Some(1000),
            output_limit_supported: true,
            paid_cost: Some(CostEstimate {
                pricing_revision: "fixture-only-pricing".into(),
                model: MODEL.into(),
                credential_generation: f.vault.state().connections["openrouter"].generation,
                input_tokens: 10000,
                output_tokens: 1000,
                input_nanodollars_per_token: 300,
                output_nanodollars_per_token: 1200,
                pricing_verified_at: f.claims.issued_at,
                pricing_expires_at: f.claims.issued_at + chrono::Duration::minutes(15),
            }),
        }
    }

    fn begin(f: &mut Fixture) -> OpenRouterDispatch {
        let now = f.claims.issued_at;
        let grant = fixtures::authorize(&f.claims, now);
        let preparation = preparation(f);
        let mut dispatch =
            OpenRouterDispatch::begin(&mut f.vault, f.id, &grant, preparation, now).unwrap();
        // Fixture wall time must not expire while disk/HTTP setup is scheduled.
        // The production binary has no clock override; it uses UTC and the 30s lease.
        dispatch.fixture_now = Some(now);
        dispatch
    }

    #[test]
    fn rejected_pricing_never_creates_dispatch_intent_or_reservation() {
        for scenario in 0..6 {
            let mut f = fixture();
            let now = f.claims.issued_at;
            let grant = fixtures::authorize(&f.claims, now);
            let mut input = preparation(&f);
            match scenario {
                0 => input.paid_cost = None,
                1 => input.paid_cost.as_mut().unwrap().model = "other-model".into(),
                2 => input.paid_cost.as_mut().unwrap().credential_generation = Uuid::new_v4(),
                3 => input.paid_cost.as_mut().unwrap().pricing_expires_at = now,
                4 => input.paid_cost.as_mut().unwrap().input_tokens = 1,
                5 => input.paid_cost.as_mut().unwrap().output_tokens = 1,
                _ => unreachable!(),
            }
            let before = serde_json::to_vec(&f.vault.state().inference_runs[&f.id]).unwrap();
            assert!(OpenRouterDispatch::begin(&mut f.vault, f.id, &grant, input, now).is_err());
            assert_eq!(
                serde_json::to_vec(&f.vault.state().inference_runs[&f.id]).unwrap(),
                before
            );
            assert!(f.vault.state().budget.reservations.is_empty());
        }
    }

    #[test]
    fn price_fields_are_included_in_mandatory_context_before_dispatch() {
        let mut f = fixture();
        let now = f.claims.issued_at;
        let grant = fixtures::authorize(&f.claims, now);
        let mut state = f.vault.state().clone();
        let run = &state.inference_runs[&f.id];
        let capabilities = state.verified_adapters
            [&run.registration.registration.profile.verification_id]
            .evidence
            .capabilities
            .clone();
        let wire =
            ChatRequest::from_stored(run, grant.execution(), &capabilities, Some(1000)).unwrap();
        let padding = u64::from(run.registration.registration.profile.context_window_tokens)
            - u64::from(run.request.output_reserve_tokens)
            - wire.input_upper_bound_tokens();
        let run = state.inference_runs.get_mut(&f.id).unwrap();
        let admin_panel_domain::inference::Message::User { content } = &mut run.request.messages[0]
        else {
            panic!("fixture must contain a user message");
        };
        content.push_str(&"x".repeat(padding as usize));
        let wire =
            ChatRequest::from_stored(run, grant.execution(), &capabilities, Some(1000)).unwrap();
        assert_eq!(
            wire.input_upper_bound_tokens() + u64::from(run.request.output_reserve_tokens),
            256000
        );
        f.vault.commit(state).unwrap();
        let mut input = preparation(&f);
        input.paid_cost.as_mut().unwrap().input_tokens = 256000;
        assert!(matches!(
            OpenRouterDispatch::begin(&mut f.vault, f.id, &grant, input, now),
            Err(RuntimeError::ContextExceeded)
        ));
        assert!(f.vault.state().budget.reservations.is_empty());
        assert_eq!(
            f.vault.state().inference_runs[&f.id].state,
            RunState::Prepared
        );
    }

    struct Server(tokio::task::JoinHandle<()>);
    impl Drop for Server {
        fn drop(&mut self) {
            self.0.abort();
        }
    }

    async fn server(router: Router) -> (OpenRouter, Server) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let adapter = OpenRouter::fixture(&format!(
            "http://127.0.0.1:{}/api/v1/",
            listener.local_addr().unwrap().port()
        ));
        let handle = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (adapter, Server(handle))
    }

    fn event(choice: Value, usage: Option<Value>) -> String {
        let mut value = json!({"id":GENERATION,"model":MODEL,"choices":[choice]});
        if let Some(usage) = usage {
            value["usage"] = usage;
        }
        format!("data: {value}\n\n")
    }

    fn complete_stream() -> String {
        event(
            json!({"index":0,"delta":{"role":"assistant","content":"готово"},"finish_reason":null}),
            None,
        ) + &event(json!({"index":0,"delta":{},"finish_reason":"stop"}), None)
            + &event(
                json!({"index":0,"delta":{"content":""},"finish_reason":"stop"}),
                Some(json!({"prompt_tokens":5,"completion_tokens":2})),
            )
            + "data: [DONE]\n\n"
    }

    #[test]
    fn permit_requires_durable_budget_and_cannot_be_reissued_after_drop() {
        let mut f = fixture();
        let now = f.claims.issued_at;
        let grant = fixtures::authorize(&f.claims, now);
        let mut absent_cost = preparation(&f);
        absent_cost.paid_cost = None;
        assert!(matches!(
            OpenRouterDispatch::begin(&mut f.vault, f.id, &grant, absent_cost, now),
            Err(RuntimeError::CapabilityNotVerified)
        ));
        assert_eq!(
            f.vault.state().inference_runs[&f.id].state,
            RunState::Prepared
        );
        assert!(f.vault.state().budget.reservations.is_empty());
        drop(begin(&mut f));
        assert_eq!(
            f.vault.state().inference_runs[&f.id].state,
            RunState::Dispatching
        );
        assert_eq!(
            f.vault.state().budget.reservations[&f.id].status,
            ReservationStatus::Dispatched
        );
        let preparation = preparation(&f);
        assert!(matches!(
            OpenRouterDispatch::begin(&mut f.vault, f.id, &grant, preparation, now),
            Err(RuntimeError::Conflict)
        ));
    }

    #[tokio::test]
    async fn actual_http_uses_frozen_body_and_validates_terminal_before_completion() {
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let router = Router::new().route(
            "/api/v1/chat/completions",
            post(move |headers: HeaderMap, Json(body): Json<Value>| {
                let observed = observed.clone();
                async move {
                    observed.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(headers["authorization"], "Bearer fixture-only-provider-key");
                    assert_eq!(headers["accept"], "text/event-stream");
                    assert_eq!(body["model"], MODEL);
                    assert_eq!(body["max_tokens"], 1000);
                    assert_eq!(body["provider"]["allow_fallbacks"], false);
                    assert_eq!(body["provider"]["require_parameters"], true);
                    assert_eq!(
                        body["provider"]["max_price"],
                        json!({"prompt":"0.3","completion":"1.2","request":"0"})
                    );
                    assert_eq!(body["messages"][0]["content"], "Private required evidence");
                    let pieces: Vec<_> = complete_stream()
                        .as_bytes()
                        .chunks(7)
                        .map(|v| Ok::<_, Infallible>(v.to_vec()))
                        .collect();
                    axum::response::Response::builder()
                        .header("content-type", "text/event-stream; charset=utf-8")
                        .header("x-generation-id", GENERATION)
                        .body(Body::from_stream(futures_util::stream::iter(pieces)))
                        .unwrap()
                }
            }),
        );
        let (adapter, _server) = server(router).await;
        let mut f = fixture();
        let mut response = adapter.start(begin(&mut f)).await.unwrap();
        assert_eq!(response.generation_id(), Some(GENERATION));
        let mut text = String::new();
        while let Some(deltas) = response.next_deltas().await.unwrap() {
            text += &deltas.concat();
        }
        let turn = response.finish().unwrap();
        assert_eq!(text, "готово");
        assert_eq!(turn.text(), text);
        assert_eq!(turn.generation_id(), GENERATION);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        // Only orchestrator receipt reconciliation can settle or advance durable state.
        assert_eq!(
            f.vault.state().inference_runs[&f.id].state,
            RunState::Dispatching
        );
        assert_eq!(
            f.vault.state().budget.reservations[&f.id].status,
            ReservationStatus::Dispatched
        );
    }

    #[tokio::test]
    async fn redirects_quota_and_transient_failures_never_retry_or_expose_raw_error() {
        for (status, expected) in [
            (302, RuntimeError::Unavailable),
            (402, RuntimeError::Quota),
            (429, RuntimeError::Quota),
            (503, RuntimeError::Unavailable),
            (401, RuntimeError::ProviderAuth),
        ] {
            let calls = Arc::new(AtomicUsize::new(0));
            let observed = calls.clone();
            let router = Router::new().fallback(move || {
                let observed = observed.clone();
                async move {
                    observed.fetch_add(1, Ordering::SeqCst);
                    axum::response::Response::builder()
                        .status(status)
                        .header("location", "/would-be-fallback")
                        .header("x-generation-id", GENERATION)
                        .body(Body::from("private fixture provider error"))
                        .unwrap()
                }
            });
            let (adapter, _server) = server(router).await;
            let mut f = fixture();
            let failure = match adapter.start(begin(&mut f)).await {
                Err(failure) => failure,
                Ok(_) => panic!("error admitted"),
            };
            assert_eq!(failure.error(), expected);
            assert_eq!(failure.generation_id(), Some(GENERATION));
            assert!(!format!("{failure:?}").contains("private fixture"));
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(
                f.vault.state().budget.reservations[&f.id].status,
                ReservationStatus::Dispatched
            );
        }
    }

    #[tokio::test]
    async fn eof_midstream_error_and_generation_mismatch_are_not_terminal_success() {
        let cases = [
            event(
                json!({"index":0,"delta":{"content":"partial"},"finish_reason":null}),
                None,
            ),
            "data: {\"error\":{\"code\":429,\"message\":\"private fixture\"}}\n\n".into(),
            complete_stream().replace(GENERATION, "gen-foreign"),
        ];
        for (index, body) in cases.into_iter().enumerate() {
            let router = Router::new().route(
                "/api/v1/chat/completions",
                post(move || {
                    let body = body.clone();
                    async move {
                        (
                            [
                                ("content-type", "text/event-stream"),
                                ("x-generation-id", GENERATION),
                            ],
                            body,
                        )
                    }
                }),
            );
            let (adapter, _server) = server(router).await;
            let mut f = fixture();
            let mut response = adapter.start(begin(&mut f)).await.unwrap();
            let error = loop {
                match response.next_deltas().await {
                    Ok(Some(_)) => continue,
                    Ok(None) => panic!("invalid terminal admitted"),
                    Err(error) => break error,
                }
            };
            assert_eq!(
                error,
                if index == 1 {
                    RuntimeError::Quota
                } else {
                    RuntimeError::Protocol
                }
            );
            assert_eq!(response.next_deltas().await, Err(RuntimeError::Protocol));
            assert!(response.finish().is_err());
            assert!(f.vault.state().budget.committed_microdollars().unwrap() > 0);
        }
    }

    #[tokio::test]
    async fn local_drop_does_not_fabricate_cancel_or_release_paid_reservation() {
        let router = Router::new().route(
            "/api/v1/chat/completions",
            post(|| async {
                axum::response::Response::builder()
                    .header("content-type", "text/event-stream")
                    .header("x-generation-id", GENERATION)
                    .body(Body::from_stream(futures_util::stream::pending::<
                        Result<String, Infallible>,
                    >()))
                    .unwrap()
            }),
        );
        let (adapter, _server) = server(router).await;
        let mut f = fixture();
        let response = tokio::time::timeout(Duration::from_secs(3), adapter.start(begin(&mut f)))
            .await
            .unwrap()
            .unwrap();
        drop(response);
        assert_eq!(
            f.vault.state().inference_runs[&f.id].state,
            RunState::Dispatching
        );
        assert_eq!(
            f.vault.state().budget.reservations[&f.id].status,
            ReservationStatus::Dispatched
        );
    }

    #[tokio::test]
    async fn lease_expiry_aborts_stalled_stream_and_cannot_be_resurrected() {
        let router = Router::new().route(
            "/api/v1/chat/completions",
            post(|| async {
                axum::response::Response::builder()
                    .header("content-type", "text/event-stream")
                    .header("x-generation-id", GENERATION)
                    .body(Body::from_stream(futures_util::stream::pending::<
                        Result<String, Infallible>,
                    >()))
                    .unwrap()
            }),
        );
        let (adapter, _server) = server(router).await;
        let mut f = fixture();
        let mut response = adapter.start(begin(&mut f)).await.unwrap();
        // Headers are accepted first; only the stalled body gets a one-second lease.
        f.claims.lease_expires_at = f.claims.issued_at + chrono::Duration::seconds(1);
        f.claims.expires_at = f.claims.lease_expires_at;
        response.authorization = fixtures::authorize(&f.claims, f.claims.issued_at);
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(3), response.next_deltas())
                .await
                .unwrap(),
            Err(RuntimeError::Forbidden)
        );
        let now = f.claims.issued_at + chrono::Duration::seconds(1);
        f.claims.issued_at = now;
        f.claims.expires_at = now + chrono::Duration::seconds(30);
        f.claims.lease_expires_at = f.claims.expires_at;
        let renewed = fixtures::authorize(&f.claims, now);
        assert!(matches!(
            response.renew_authorization(&renewed),
            Err(RuntimeError::Conflict)
        ));
        assert_eq!(
            f.vault.state().budget.reservations[&f.id].status,
            ReservationStatus::Dispatched
        );
    }

    #[tokio::test]
    async fn heartbeat_cannot_change_owner_machine_execution_revision_or_fencing() {
        let router = Router::new().route(
            "/api/v1/chat/completions",
            post(|| async {
                axum::response::Response::builder()
                    .header("content-type", "text/event-stream")
                    .body(Body::from_stream(futures_util::stream::pending::<
                        Result<String, Infallible>,
                    >()))
                    .unwrap()
            }),
        );
        let (adapter, _server) = server(router).await;
        let mut f = fixture();
        let mut response = adapter.start(begin(&mut f)).await.unwrap();
        for change in 0..5 {
            let mut claims = f.claims.clone();
            match change {
                0 => claims.execution.owner_subject = Uuid::new_v4().to_string(),
                1 => claims.machine_subject = "sdlc2:hermes:foreign".into(),
                2 => claims.execution.execution_id = Uuid::new_v4(),
                3 => claims.profile_revision += 1,
                _ => claims.fencing_token += 1,
            }
            let grant = fixtures::authorize(&claims, f.claims.issued_at);
            assert!(matches!(
                response.renew_authorization(&grant),
                Err(RuntimeError::Forbidden)
            ));
        }
        let now = f.claims.issued_at + chrono::Duration::seconds(1);
        response.fixture_now = Some(now);
        let mut claims = f.claims.clone();
        claims.issued_at = now;
        claims.expires_at = now + chrono::Duration::seconds(30);
        claims.lease_expires_at = claims.expires_at;
        response
            .renew_authorization(&fixtures::authorize(&claims, now))
            .unwrap();
        assert_eq!(
            response.authorization.valid_until(),
            claims.lease_expires_at
        );
    }

    #[tokio::test]
    async fn explicit_expiry_rejects_before_post_and_before_read_without_releasing_reserve() {
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let router = Router::new().route(
            "/api/v1/chat/completions",
            post(move || {
                observed.fetch_add(1, Ordering::SeqCst);
                async { ([("content-type", "text/event-stream")], complete_stream()) }
            }),
        );
        let (adapter, _server) = server(router).await;
        let mut before_post = fixture();
        let mut dispatch = begin(&mut before_post);
        dispatch.fixture_now = Some(dispatch.authorization.valid_until());
        assert_eq!(
            adapter.start(dispatch).await.err().unwrap().error(),
            RuntimeError::Forbidden
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        let mut before_read = fixture();
        let mut response = adapter.start(begin(&mut before_read)).await.unwrap();
        response.fixture_now = Some(response.authorization.valid_until());
        assert_eq!(response.next_deltas().await, Err(RuntimeError::Forbidden));
        let now = before_read.claims.expires_at;
        let mut claims = before_read.claims.clone();
        claims.issued_at = now;
        claims.expires_at = now + chrono::Duration::seconds(30);
        claims.lease_expires_at = claims.expires_at;
        assert_eq!(
            response.renew_authorization(&fixtures::authorize(&claims, now)),
            Err(RuntimeError::Conflict)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        for fixture in [&before_post, &before_read] {
            assert_eq!(
                fixture.vault.state().budget.reservations[&fixture.id].status,
                ReservationStatus::Dispatched
            );
        }
    }

    fn metadata() -> Value {
        json!({"data":{"id":GENERATION,"model":MODEL,"streamed":true,"is_byok":false,
            "cancelled":false,"finish_reason":"stop","total_cost":0.0015000001}})
    }

    #[test]
    fn decimal_cost_rounds_up_and_rejects_invalid_or_overflowing_values() {
        for (value, expected) in [
            (json!("0.0015"), 1500),
            (json!(0.0015000001), 1501),
            (json!("1.2e-6"), 2),
            (json!("0.0000000001"), 1),
            (json!(0), 0),
            (json!("18446744073709.551615"), u64::MAX),
        ] {
            assert_eq!(scaled_decimal(&value, 6).unwrap(), expected);
        }
        for value in [
            json!("-1"),
            json!("NaN"),
            json!("Infinity"),
            json!(" 1"),
            json!("1."),
            json!("1e100"),
            json!("1e-100"),
            json!("1e1e1"),
            json!("18446744073709.551616"),
            json!(null),
            json!({"amount":1}),
        ] {
            assert_eq!(scaled_decimal(&value, 6), Err(RuntimeError::Protocol));
        }
    }

    #[test]
    fn billing_readback_requires_exact_generation_model_and_terminal_metadata() {
        let original = metadata();
        let receipt = parse_generation(&original, GENERATION, MODEL).unwrap();
        assert_eq!(receipt.id(), GENERATION);
        assert_eq!(receipt.model(), MODEL);
        assert_eq!(receipt.actual_microdollars(), 1501);
        assert!(!receipt.cancelled());
        for (key, value) in [
            ("id", json!("gen-other")),
            ("model", json!("other-model")),
            ("streamed", json!(false)),
            ("is_byok", json!(true)),
            ("cancelled", json!(null)),
            ("finish_reason", json!(null)),
            ("finish_reason", json!("unknown")),
            ("total_cost", json!(null)),
        ] {
            let mut data = original.clone();
            data["data"][key] = value;
            assert!(matches!(
                parse_generation(&data, GENERATION, MODEL),
                Err(RuntimeError::Protocol)
            ));
        }
        let mut cancelled = original;
        cancelled["data"]["cancelled"] = json!(true);
        cancelled["data"]["finish_reason"] = Value::Null;
        assert!(
            parse_generation(&cancelled, GENERATION, MODEL)
                .unwrap()
                .cancelled()
        );
    }

    #[test]
    fn correlation_and_unknown_cost_survive_restart_without_successful_output_or_resend() {
        let mut f = fixture();
        assert!(matches!(
            f.vault.record_openrouter_generation(f.id, GENERATION),
            Err(RuntimeError::Conflict)
        ));
        assert!(matches!(
            f.vault.mark_inference_unknown(f.id, f.claims.issued_at),
            Err(RuntimeError::Conflict)
        ));
        drop(begin(&mut f));
        f.vault
            .record_openrouter_generation(f.id, GENERATION)
            .unwrap();
        f.vault
            .record_openrouter_generation(f.id, GENERATION)
            .unwrap();
        assert!(matches!(
            f.vault.record_openrouter_generation(f.id, "gen-foreign"),
            Err(RuntimeError::Conflict)
        ));
        f.vault
            .mark_inference_unknown(f.id, f.claims.issued_at)
            .unwrap();
        f.vault
            .mark_inference_unknown(f.id, f.claims.issued_at)
            .unwrap();
        assert_eq!(
            f.vault.state().budget.reservations[&f.id].status,
            ReservationStatus::Uncertain
        );
        assert!(f.vault.state().budget.committed_microdollars().unwrap() > 0);
        let state_path = f._directory.path().join("state");
        let key = f._directory.path().join("key");
        drop(f.vault);
        let mut restored = Vault::open(&state_path, &key, "sdlc2").unwrap();
        assert_eq!(
            restored.state().inference_runs[&f.id]
                .openrouter_generation_id
                .as_deref(),
            Some(GENERATION)
        );
        assert_eq!(
            restored.state().inference_runs[&f.id].state,
            RunState::Unknown
        );
        let mut cancelled = metadata();
        cancelled["data"]["cancelled"] = json!(true);
        cancelled["data"]["finish_reason"] = Value::Null;
        let foreign = parse_generation(&cancelled, "gen-foreign", MODEL);
        assert!(foreign.is_err());
        let receipt = parse_generation(&cancelled, GENERATION, MODEL).unwrap();
        restored
            .reconcile_openrouter_generation(f.id, &receipt, f.claims.issued_at)
            .unwrap();
        restored
            .reconcile_openrouter_generation(f.id, &receipt, f.claims.issued_at)
            .unwrap();
        assert_eq!(
            restored.state().inference_runs[&f.id].state,
            RunState::Cancelled
        );
        assert_eq!(
            restored.state().budget.reservations[&f.id].status,
            ReservationStatus::Settled
        );
        assert_eq!(
            restored.state().budget.reservations[&f.id].actual_microdollars,
            Some(1501)
        );
    }

    #[test]
    fn metadata_completion_with_lost_output_is_failure_and_mismatched_receipt_cannot_settle() {
        let mut f = fixture();
        drop(begin(&mut f));
        f.vault
            .record_openrouter_generation(f.id, GENERATION)
            .unwrap();
        f.vault
            .mark_inference_unknown(f.id, f.claims.issued_at)
            .unwrap();
        let mut foreign = metadata();
        foreign["data"]["id"] = json!("gen-foreign");
        let foreign = parse_generation(&foreign, "gen-foreign", MODEL).unwrap();
        assert!(matches!(
            f.vault
                .reconcile_openrouter_generation(f.id, &foreign, f.claims.issued_at),
            Err(RuntimeError::Conflict)
        ));
        assert_eq!(
            f.vault.state().budget.reservations[&f.id].status,
            ReservationStatus::Uncertain
        );
        let receipt = parse_generation(&metadata(), GENERATION, MODEL).unwrap();
        f.vault
            .reconcile_openrouter_generation(f.id, &receipt, f.claims.issued_at)
            .unwrap();
        assert_eq!(
            f.vault.state().inference_runs[&f.id].state,
            RunState::Failed
        );
        assert_eq!(
            f.vault.state().budget.reservations[&f.id].actual_microdollars,
            Some(1501)
        );
    }

    #[tokio::test]
    async fn generation_http_readback_has_exact_query_and_missing_metadata_stays_unknown() {
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let router = Router::new().route(
            "/api/v1/generation",
            get(
                move |headers: HeaderMap,
                      axum::extract::Query(query): axum::extract::Query<
                    std::collections::BTreeMap<String, String>,
                >| {
                    let observed = observed.clone();
                    async move {
                        assert_eq!(headers["authorization"], "Bearer fixture-only-provider-key");
                        assert_eq!(query.len(), 1);
                        assert_eq!(query["id"], GENERATION);
                        let index = observed.fetch_add(1, Ordering::SeqCst);
                        (
                            if index == 0 {
                                StatusCode::NOT_FOUND
                            } else {
                                StatusCode::OK
                            },
                            Json(metadata()),
                        )
                    }
                },
            ),
        );
        let (adapter, _server) = server(router).await;
        assert!(matches!(
            adapter
                .generation("fixture-only-provider-key", "../foreign?secret", MODEL)
                .await,
            Err(RuntimeError::InvalidRequest)
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(matches!(
            adapter
                .generation("fixture-only-provider-key", GENERATION, MODEL)
                .await,
            Err(RuntimeError::Unavailable)
        ));
        let receipt = adapter
            .generation("fixture-only-provider-key", GENERATION, MODEL)
            .await
            .unwrap();
        assert_eq!(receipt.actual_microdollars(), 1501);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}
