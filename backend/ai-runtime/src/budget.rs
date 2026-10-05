//! A single durable acceptance budget; unknown provider outcomes stay reserved.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

pub const ACCEPTANCE_LIMIT_MICRODOLLARS: u64 = 30_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BudgetError {
    #[error("ai_acceptance_budget_exhausted")]
    Exhausted,
    #[error("invalid_cost_estimate")]
    InvalidEstimate,
    #[error("operation_payload_conflict")]
    Conflict,
    #[error("dispatch_state_conflict")]
    DispatchConflict,
    #[error("provider_cost_exceeds_reservation")]
    UpperBoundExceeded,
}

/// Deployment/provider pricing evidence, never accepted from inference callers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CostEstimate {
    pub pricing_revision: String,
    pub model: String,
    pub credential_generation: Uuid,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub input_nanodollars_per_token: u64,
    pub output_nanodollars_per_token: u64,
    pub pricing_verified_at: DateTime<Utc>,
    pub pricing_expires_at: DateTime<Utc>,
}

impl CostEstimate {
    pub fn ceiling_microdollars(&self, now: DateTime<Utc>) -> Result<u64, BudgetError> {
        if self.pricing_revision.is_empty()
            || self.pricing_revision.len() > 256
            || self.model.is_empty()
            || self.model.len() > 256
            || self.input_tokens == 0
            || self.output_tokens == 0
            || self.input_nanodollars_per_token == 0
            || self.output_nanodollars_per_token == 0
            || self.pricing_verified_at > now
            || self.pricing_expires_at <= now
            || self.pricing_expires_at - self.pricing_verified_at > chrono::Duration::minutes(15)
        {
            return Err(BudgetError::InvalidEstimate);
        }
        let nanodollars = u128::from(self.input_tokens)
            * u128::from(self.input_nanodollars_per_token)
            + u128::from(self.output_tokens) * u128::from(self.output_nanodollars_per_token);
        u64::try_from(nanodollars.div_ceil(1000)).map_err(|_| BudgetError::InvalidEstimate)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReservationStatus {
    Reserved,
    Dispatched,
    Uncertain,
    Settled,
    CostOverrun,
    CancelledBeforeDispatch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reservation {
    pub operation_id: Uuid,
    pub request_fingerprint: String,
    pub estimate: CostEstimate,
    pub ceiling_microdollars: u64,
    pub actual_microdollars: Option<u64>,
    pub status: ReservationStatus,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetLedger {
    pub reservations: BTreeMap<Uuid, Reservation>,
}

impl BudgetLedger {
    pub fn summary(&self) -> Result<admin_panel_domain::ai::AcceptanceBudget, BudgetError> {
        let mut settled = 0u64;
        let mut reserved = 0u64;
        let mut uncertain = 0u64;
        let mut unsettled_requests = 0u64;
        let mut uncertain_requests = 0u64;
        for entry in self.reservations.values() {
            match entry.status {
                ReservationStatus::CancelledBeforeDispatch => {}
                ReservationStatus::Settled | ReservationStatus::CostOverrun => {
                    settled = settled
                        .checked_add(
                            entry
                                .actual_microdollars
                                .ok_or(BudgetError::InvalidEstimate)?,
                        )
                        .ok_or(BudgetError::InvalidEstimate)?;
                }
                status => {
                    reserved = reserved
                        .checked_add(entry.ceiling_microdollars)
                        .ok_or(BudgetError::InvalidEstimate)?;
                    unsettled_requests += 1;
                    if status == ReservationStatus::Uncertain {
                        uncertain = uncertain
                            .checked_add(entry.ceiling_microdollars)
                            .ok_or(BudgetError::InvalidEstimate)?;
                        uncertain_requests += 1;
                    }
                }
            }
        }
        let committed = self.committed_microdollars()?;
        Ok(admin_panel_domain::ai::AcceptanceBudget {
            schema_version: 1,
            workspace: "sdlc2".into(),
            currency: "USD".into(),
            limit_microdollars: ACCEPTANCE_LIMIT_MICRODOLLARS.to_string(),
            settled_microdollars: settled.to_string(),
            reserved_microdollars: reserved.to_string(),
            uncertain_microdollars: uncertain.to_string(),
            available_microdollars: ACCEPTANCE_LIMIT_MICRODOLLARS
                .saturating_sub(committed)
                .to_string(),
            unsettled_requests,
            uncertain_requests,
            blocked_reason: if self.paid_dispatch_blocked() {
                Some("provider_cost_exceeds_reservation".into())
            } else if committed >= ACCEPTANCE_LIMIT_MICRODOLLARS {
                Some("ai_acceptance_budget_exhausted".into())
            } else {
                None
            },
        })
    }

    /// A trusted billing receipt violated the pre-dispatch price/usage ceiling.
    /// Readback and reconciliation stay available; no new paid I/O is allowed.
    pub fn paid_dispatch_blocked(&self) -> bool {
        self.reservations
            .values()
            .any(|entry| entry.status == ReservationStatus::CostOverrun)
    }

    pub fn committed_microdollars(&self) -> Result<u64, BudgetError> {
        self.reservations.values().try_fold(0u64, |total, entry| {
            let amount = match entry.status {
                ReservationStatus::CancelledBeforeDispatch => 0,
                ReservationStatus::Settled | ReservationStatus::CostOverrun => entry
                    .actual_microdollars
                    .ok_or(BudgetError::InvalidEstimate)?,
                _ => entry.ceiling_microdollars,
            };
            total
                .checked_add(amount)
                .ok_or(BudgetError::InvalidEstimate)
        })
    }

    /// False means readback only. A replay is never permission to dispatch again.
    pub fn reserve(
        &mut self,
        operation_id: Uuid,
        fingerprint: &str,
        estimate: CostEstimate,
        now: DateTime<Utc>,
    ) -> Result<bool, BudgetError> {
        if fingerprint.len() != 64 || !fingerprint.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(BudgetError::InvalidEstimate);
        }
        if let Some(previous) = self.reservations.get(&operation_id) {
            return if previous.request_fingerprint == fingerprint && previous.estimate == estimate {
                Ok(false)
            } else {
                Err(BudgetError::Conflict)
            };
        }
        if self.paid_dispatch_blocked() {
            return Err(BudgetError::UpperBoundExceeded);
        }
        let ceiling = estimate.ceiling_microdollars(now)?;
        if self
            .committed_microdollars()?
            .checked_add(ceiling)
            .is_none_or(|total| total > ACCEPTANCE_LIMIT_MICRODOLLARS)
        {
            return Err(BudgetError::Exhausted);
        }
        self.reservations.insert(
            operation_id,
            Reservation {
                operation_id,
                request_fingerprint: fingerprint.into(),
                estimate,
                ceiling_microdollars: ceiling,
                actual_microdollars: None,
                status: ReservationStatus::Reserved,
            },
        );
        Ok(true)
    }

    pub fn mark_dispatched(&mut self, id: Uuid) -> Result<(), BudgetError> {
        if self.paid_dispatch_blocked() {
            return Err(BudgetError::UpperBoundExceeded);
        }
        let reservation = self
            .reservations
            .get_mut(&id)
            .ok_or(BudgetError::DispatchConflict)?;
        if reservation.status != ReservationStatus::Reserved {
            return Err(BudgetError::DispatchConflict);
        }
        reservation.status = ReservationStatus::Dispatched;
        Ok(())
    }

    pub fn mark_uncertain(&mut self, id: Uuid) -> Result<(), BudgetError> {
        let reservation = self
            .reservations
            .get_mut(&id)
            .ok_or(BudgetError::DispatchConflict)?;
        if !matches!(
            reservation.status,
            ReservationStatus::Dispatched | ReservationStatus::Uncertain
        ) {
            return Err(BudgetError::DispatchConflict);
        }
        reservation.status = ReservationStatus::Uncertain;
        Ok(())
    }

    pub fn cancel_before_dispatch(&mut self, id: Uuid) -> Result<(), BudgetError> {
        let reservation = self
            .reservations
            .get_mut(&id)
            .ok_or(BudgetError::DispatchConflict)?;
        if !matches!(
            reservation.status,
            ReservationStatus::Reserved | ReservationStatus::CancelledBeforeDispatch
        ) {
            return Err(BudgetError::DispatchConflict);
        }
        reservation.status = ReservationStatus::CancelledBeforeDispatch;
        Ok(())
    }

    /// Only the trusted adapter's reconciled terminal receipt may settle cost.
    pub fn settle(&mut self, id: Uuid, actual_microdollars: u64) -> Result<(), BudgetError> {
        let reservation = self
            .reservations
            .get_mut(&id)
            .ok_or(BudgetError::DispatchConflict)?;
        if matches!(
            reservation.status,
            ReservationStatus::Settled | ReservationStatus::CostOverrun
        ) {
            return if reservation.actual_microdollars == Some(actual_microdollars) {
                Ok(())
            } else {
                Err(BudgetError::Conflict)
            };
        }
        if !matches!(
            reservation.status,
            ReservationStatus::Dispatched | ReservationStatus::Uncertain
        ) {
            return Err(BudgetError::DispatchConflict);
        }
        reservation.actual_microdollars = Some(actual_microdollars);
        // Success means the trusted receipt has been accounted for, not approval
        // of its cost. Returning an error here used to discard cloned vault state.
        reservation.status = if actual_microdollars > reservation.ceiling_microdollars {
            ReservationStatus::CostOverrun
        } else {
            ReservationStatus::Settled
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn estimate(now: DateTime<Utc>) -> CostEstimate {
        CostEstimate {
            pricing_revision: "fixture-price-1".into(),
            model: "fixture/model".into(),
            credential_generation: Uuid::new_v4(),
            input_tokens: 1000,
            output_tokens: 100,
            input_nanodollars_per_token: 100,
            output_nanodollars_per_token: 500,
            pricing_verified_at: now,
            pricing_expires_at: now + chrono::Duration::minutes(15),
        }
    }
    #[test]
    fn readback_keeps_unknown_reserved_and_accounts_terminal_cost_without_mutation() {
        let now = Utc::now();
        let mut ledger = BudgetLedger::default();
        let empty = ledger.summary().unwrap();
        assert!(empty.valid_for("sdlc2"));
        assert_eq!(empty.available_microdollars, "30000000");
        let held = Uuid::new_v4();
        let settled = Uuid::new_v4();
        let cancelled = Uuid::new_v4();
        for id in [held, settled, cancelled] {
            ledger
                .reserve(id, &"a".repeat(64), estimate(now), now)
                .unwrap();
        }
        ledger.mark_dispatched(held).unwrap();
        ledger.mark_uncertain(held).unwrap();
        ledger.mark_dispatched(settled).unwrap();
        ledger.settle(settled, 99).unwrap();
        ledger.cancel_before_dispatch(cancelled).unwrap();
        let before = serde_json::to_vec(&ledger).unwrap();
        let summary = ledger.summary().unwrap();
        assert!(summary.valid_for("sdlc2"));
        assert_eq!(summary.settled_microdollars, "99");
        assert_eq!(summary.reserved_microdollars, "150");
        assert_eq!(summary.uncertain_microdollars, "150");
        assert_eq!(summary.available_microdollars, "29999751");
        assert_eq!(summary.unsettled_requests, 1);
        assert_eq!(summary.uncertain_requests, 1);
        assert_eq!(serde_json::to_vec(&ledger).unwrap(), before);
        ledger.settle(held, 301).unwrap();
        let summary = ledger.summary().unwrap();
        assert!(summary.valid_for("sdlc2"));
        assert_eq!(summary.settled_microdollars, "400");
        assert_eq!(summary.reserved_microdollars, "0");
        assert_eq!(
            summary.blocked_reason.as_deref(),
            Some("provider_cost_exceeds_reservation")
        );
    }
    #[test]
    fn ceiling_rounds_up_and_rejects_unknown_expired_or_overflowing_prices() {
        let now = Utc::now();
        let mut price = estimate(now);
        assert_eq!(price.ceiling_microdollars(now), Ok(150));
        price.input_tokens += 1;
        assert_eq!(price.ceiling_microdollars(now), Ok(151));
        assert_eq!(
            price.ceiling_microdollars(price.pricing_expires_at),
            Err(BudgetError::InvalidEstimate)
        );
        price.input_nanodollars_per_token = 0;
        assert_eq!(
            price.ceiling_microdollars(now),
            Err(BudgetError::InvalidEstimate)
        );
        price.input_nanodollars_per_token = u64::MAX;
        price.input_tokens = u32::MAX;
        assert_eq!(
            price.ceiling_microdollars(now),
            Err(BudgetError::InvalidEstimate)
        );
    }
    #[test]
    fn shared_budget_reservations_cannot_exceed_thirty_dollars() {
        let now = Utc::now();
        let mut ledger = BudgetLedger::default();
        let mut price = estimate(now);
        price.input_nanodollars_per_token = 30_000_000;
        price.output_nanodollars_per_token = 1;
        price.input_tokens = 999;
        let id = Uuid::new_v4();
        assert_eq!(
            ledger.reserve(id, &"a".repeat(64), price.clone(), now),
            Ok(true)
        );
        assert_eq!(
            ledger.reserve(Uuid::new_v4(), &"b".repeat(64), price, now),
            Err(BudgetError::Exhausted)
        );
        assert!(ledger.committed_microdollars().unwrap() <= ACCEPTANCE_LIMIT_MICRODOLLARS);
        ledger.cancel_before_dispatch(id).unwrap();
        assert_eq!(ledger.committed_microdollars(), Ok(0));
    }
    #[test]
    fn unknown_outcome_survives_restart_and_cannot_be_refunded_or_dispatched_twice() {
        let now = Utc::now();
        let id = Uuid::new_v4();
        let price = estimate(now);
        let mut ledger = BudgetLedger::default();
        assert_eq!(
            ledger.reserve(id, &"a".repeat(64), price.clone(), now),
            Ok(true)
        );
        ledger.mark_dispatched(id).unwrap();
        ledger.mark_uncertain(id).unwrap();
        let mut restored: BudgetLedger =
            serde_json::from_slice(&serde_json::to_vec(&ledger).unwrap()).unwrap();
        assert_eq!(restored.committed_microdollars(), Ok(150));
        assert_eq!(
            restored.reserve(id, &"a".repeat(64), price.clone(), now),
            Ok(false)
        );
        assert_eq!(
            restored.reserve(id, &"b".repeat(64), price, now),
            Err(BudgetError::Conflict)
        );
        assert_eq!(
            restored.mark_dispatched(id),
            Err(BudgetError::DispatchConflict)
        );
        assert_eq!(
            restored.cancel_before_dispatch(id),
            Err(BudgetError::DispatchConflict)
        );
        restored.settle(id, 125).unwrap();
        restored.settle(id, 125).unwrap();
        assert_eq!(restored.settle(id, 120), Err(BudgetError::Conflict));
        assert_eq!(restored.committed_microdollars(), Ok(125));
    }

    #[test]
    fn confirmed_overrun_survives_restart_and_blocks_reserved_and_new_dispatch() {
        let now = Utc::now();
        let id = Uuid::new_v4();
        let queued = Uuid::new_v4();
        let price = estimate(now);
        let mut ledger = BudgetLedger::default();
        ledger
            .reserve(id, &"a".repeat(64), price.clone(), now)
            .unwrap();
        ledger
            .reserve(queued, &"b".repeat(64), price.clone(), now)
            .unwrap();
        ledger.mark_dispatched(id).unwrap();
        ledger.mark_uncertain(id).unwrap();
        ledger.settle(id, 30_000_001).unwrap();
        let mut restored: BudgetLedger =
            serde_json::from_slice(&serde_json::to_vec(&ledger).unwrap()).unwrap();
        assert!(restored.paid_dispatch_blocked());
        assert_eq!(restored.committed_microdollars(), Ok(30_000_151));
        assert_eq!(
            restored.reservations[&id].status,
            ReservationStatus::CostOverrun
        );
        restored.settle(id, 30_000_001).unwrap();
        assert_eq!(restored.settle(id, 125), Err(BudgetError::Conflict));
        assert_eq!(
            restored.reserve(id, &"a".repeat(64), price.clone(), now),
            Ok(false)
        );
        assert_eq!(
            restored.reserve(Uuid::new_v4(), &"c".repeat(64), price, now),
            Err(BudgetError::UpperBoundExceeded)
        );
        assert_eq!(
            restored.mark_dispatched(queued),
            Err(BudgetError::UpperBoundExceeded)
        );
        restored.cancel_before_dispatch(queued).unwrap();
        assert_eq!(restored.committed_microdollars(), Ok(30_000_001));
        assert!(restored.paid_dispatch_blocked());
    }
}
