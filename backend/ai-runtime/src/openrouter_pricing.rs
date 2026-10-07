//! TLS catalog pricing, bound to a connection generation; never access evidence.
use crate::{
    budget::CostEstimate,
    error::RuntimeError,
    openrouter::{OpenRouter, bounded_json},
};
use chrono::{DateTime, Duration, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Only the adapter can construct this snapshot. Prices are not inference DTO fields.
pub struct PricingSnapshot {
    model: String,
    generation: Uuid,
    revision: String,
    received_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    input_nanodollars_per_token: u64,
    output_nanodollars_per_token: u64,
}

impl OpenRouter {
    pub async fn pricing_snapshot(
        &self,
        credential: &str,
        generation: Uuid,
        model: &str,
    ) -> Result<PricingSnapshot, RuntimeError> {
        if generation.is_nil() || model.is_empty() || model.len() > 256 || credential.is_empty() {
            return Err(RuntimeError::InvalidRequest);
        }
        let response = self
            .client
            .get(
                self.endpoint
                    .join("models")
                    .map_err(|_| RuntimeError::Configuration)?,
            )
            .bearer_auth(credential)
            .send()
            .await
            .map_err(|_| RuntimeError::Unavailable)?;
        let data = bounded_json(response).await?;
        PricingSnapshot::from_catalog(&data, generation, model, Utc::now())
    }
}

impl PricingSnapshot {
    fn from_catalog(
        data: &Value,
        generation: Uuid,
        model: &str,
        now: DateTime<Utc>,
    ) -> Result<Self, RuntimeError> {
        let rows = data["data"].as_array().ok_or(RuntimeError::Protocol)?;
        let mut matches = rows.iter().filter(|row| row["id"].as_str() == Some(model));
        let row = matches.next().ok_or(RuntimeError::CapabilityNotVerified)?;
        if matches.next().is_some() || generation.is_nil() {
            return Err(RuntimeError::Protocol);
        }
        let prices = row["pricing"]
            .as_object()
            .ok_or(RuntimeError::CapabilityNotVerified)?;
        let rate = |name: &str| -> Result<u64, RuntimeError> {
            let value = prices
                .get(name)
                .ok_or(RuntimeError::CapabilityNotVerified)?;
            // Catalog prices are decimal USD/token strings, unlike generation totals.
            if !value.is_string() {
                return Err(RuntimeError::Protocol);
            }
            crate::openrouter_transport::scaled_decimal(value, 9)
        };
        let input = rate("prompt")?;
        let output = rate("completion")?;
        if input == 0 || output == 0 {
            return Err(RuntimeError::CapabilityNotVerified);
        }
        for (dimension, value) in prices {
            if matches!(dimension.as_str(), "prompt" | "completion") {
                continue;
            }
            if !value.is_string() {
                return Err(RuntimeError::Protocol);
            }
            let amount = crate::openrouter_transport::scaled_decimal(value, 9)?;
            match dimension.as_str() {
                "input_cache_read" | "input_cache_write" if amount <= input => {}
                _ if amount == 0 => {}
                // A non-token fee or cache surcharge needs an explicit accounting policy.
                _ => return Err(RuntimeError::CapabilityNotVerified),
            }
        }
        let encoded = serde_json::to_vec(&row["pricing"]).map_err(|_| RuntimeError::Protocol)?;
        let revision = format!(
            "openrouter-catalog-v1:{}",
            hex::encode(Sha256::digest(encoded))
        );
        let expires_at = now
            .checked_add_signed(Duration::minutes(15))
            .ok_or(RuntimeError::Protocol)?;
        Ok(Self {
            model: model.into(),
            generation,
            revision,
            received_at: now,
            expires_at,
            input_nanodollars_per_token: input,
            output_nanodollars_per_token: output,
        })
    }

    pub fn estimate(
        &self,
        input_tokens: u32,
        output_tokens: u32,
        now: DateTime<Utc>,
    ) -> Result<CostEstimate, RuntimeError> {
        let cost = CostEstimate {
            pricing_revision: self.revision.clone(),
            model: self.model.clone(),
            credential_generation: self.generation,
            input_tokens,
            output_tokens,
            input_nanodollars_per_token: self.input_nanodollars_per_token,
            output_nanodollars_per_token: self.output_nanodollars_per_token,
            pricing_verified_at: self.received_at,
            pricing_expires_at: self.expires_at,
        };
        cost.ceiling_microdollars(now)?;
        Ok(cost)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const MODEL: &str = "deepseek/deepseek-v4.1-flash";
    fn catalog() -> Value {
        json!({"data":[{"id":MODEL,"pricing":{"prompt":"0.0000003",
            "completion":"0.0000012","input_cache_read":"0.000000006"}}]})
    }

    #[test]
    fn snapshot_binds_price_generation_and_ttl_without_float_or_access_claims() {
        let now = Utc::now();
        let generation = Uuid::new_v4();
        let snapshot = PricingSnapshot::from_catalog(&catalog(), generation, MODEL, now).unwrap();
        let cost = snapshot.estimate(10000, 1000, now).unwrap();
        assert_eq!(cost.input_nanodollars_per_token, 300);
        assert_eq!(cost.output_nanodollars_per_token, 1200);
        assert_eq!(cost.ceiling_microdollars(now).unwrap(), 4200);
        assert_eq!(cost.credential_generation, generation);
        assert_eq!(cost.model, MODEL);
        assert!(
            snapshot
                .estimate(10000, 1000, now + Duration::minutes(15))
                .is_err()
        );
        assert!(snapshot.estimate(0, 1000, now).is_err());
        assert!(snapshot.estimate(1000, 0, now).is_err());
        assert!(
            snapshot
                .estimate(1000, 1000, now - Duration::seconds(1))
                .is_err()
        );
    }

    #[test]
    fn rejects_unknown_fees_cache_surcharges_missing_rates_and_ambiguous_models() {
        let now = Utc::now();
        for (dimension, value) in [
            ("request", "0.01"),
            ("image", "0.001"),
            ("future_sku", "0.000000001"),
            ("input_cache_write", "0.0000004"),
            ("prompt", "-0.1"),
            ("completion", "0"),
            ("prompt", "NaN"),
        ] {
            let mut data = catalog();
            data["data"][0]["pricing"][dimension] = json!(value);
            assert!(PricingSnapshot::from_catalog(&data, Uuid::new_v4(), MODEL, now).is_err());
        }
        let mut missing = catalog();
        missing["data"][0]["pricing"]
            .as_object_mut()
            .unwrap()
            .remove("prompt");
        assert!(PricingSnapshot::from_catalog(&missing, Uuid::new_v4(), MODEL, now).is_err());
        let mut ambiguous = catalog();
        let row = ambiguous["data"][0].clone();
        ambiguous["data"].as_array_mut().unwrap().push(row);
        assert!(PricingSnapshot::from_catalog(&ambiguous, Uuid::new_v4(), MODEL, now).is_err());
        assert!(
            PricingSnapshot::from_catalog(&catalog(), Uuid::new_v4(), "other-model", now).is_err()
        );
    }

    #[test]
    fn rounding_is_conservative_and_rate_changes_change_revision() {
        let now = Utc::now();
        let generation = Uuid::new_v4();
        let mut data = catalog();
        data["data"][0]["pricing"]["prompt"] = json!("3.000000001e-7");
        let changed = PricingSnapshot::from_catalog(&data, generation, MODEL, now).unwrap();
        assert_eq!(
            changed
                .estimate(1000, 1000, now)
                .unwrap()
                .input_nanodollars_per_token,
            301
        );
        let original = PricingSnapshot::from_catalog(&catalog(), generation, MODEL, now).unwrap();
        assert_ne!(changed.revision, original.revision);
    }

    #[tokio::test]
    async fn actual_client_uses_own_key_and_exact_models_route() {
        use axum::{Json, Router, http::HeaderMap, routing::get};
        let router = Router::new().route(
            "/api/v1/models",
            get(|headers: HeaderMap| async move {
                assert_eq!(
                    headers["authorization"],
                    "Bearer fixture-own-openrouter-key"
                );
                Json(catalog())
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let adapter = OpenRouter::fixture(&format!(
            "http://127.0.0.1:{}/api/v1/",
            listener.local_addr().unwrap().port()
        ));
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let generation = Uuid::new_v4();
        let result = adapter
            .pricing_snapshot("fixture-own-openrouter-key", generation, MODEL)
            .await;
        server.abort();
        let cost = result.unwrap().estimate(1000, 1000, Utc::now()).unwrap();
        assert_eq!(cost.credential_generation, generation);
        assert_eq!(cost.input_nanodollars_per_token, 300);
    }

    #[tokio::test]
    async fn catalog_errors_and_redirect_never_retry_or_disclose_provider_body() {
        use axum::{Router, http::StatusCode, routing::get};
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        for (status, expected) in [
            (StatusCode::UNAUTHORIZED, RuntimeError::ProviderAuth),
            (StatusCode::FORBIDDEN, RuntimeError::ProviderAuth),
            (StatusCode::PAYMENT_REQUIRED, RuntimeError::Quota),
            (StatusCode::TOO_MANY_REQUESTS, RuntimeError::Quota),
            (StatusCode::TEMPORARY_REDIRECT, RuntimeError::Unavailable),
            (StatusCode::SERVICE_UNAVAILABLE, RuntimeError::Unavailable),
        ] {
            let calls = Arc::new(AtomicUsize::new(0));
            let redirects = Arc::new(AtomicUsize::new(0));
            let observed = calls.clone();
            let redirected = redirects.clone();
            let router = Router::new()
                .route(
                    "/api/v1/models",
                    get(move || {
                        observed.fetch_add(1, Ordering::SeqCst);
                        async move {
                            (
                                status,
                                [("location", "/foreign")],
                                "private-provider-error-fixture",
                            )
                        }
                    }),
                )
                .route(
                    "/foreign",
                    get(move || {
                        redirected.fetch_add(1, Ordering::SeqCst);
                        async { "must never reach foreign route" }
                    }),
                );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let adapter = OpenRouter::fixture(&format!(
                "http://127.0.0.1:{}/api/v1/",
                listener.local_addr().unwrap().port()
            ));
            let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
            let result = adapter
                .pricing_snapshot("fixture-own-key", Uuid::new_v4(), MODEL)
                .await;
            server.abort();
            assert_eq!(result.err(), Some(expected));
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(redirects.load(Ordering::SeqCst), 0);
        }
    }
}
