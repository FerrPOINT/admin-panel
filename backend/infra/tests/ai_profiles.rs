use admin_panel_domain::ai::{
    AiError, ModelCapabilities, ProviderId, ProviderSettings, VerificationEvidence,
};
use admin_panel_infra::ai::{
    AiStore, Publication, PublicationRejection, RegistrationAck, StoreError,
};
use chrono::{Duration, Utc};
use sqlx::{PgPool, postgres::PgPoolOptions};
use uuid::Uuid;

#[tokio::test]
#[ignore = "Requires a dedicated sdlc2_ai_registry_test PostgreSQL database"]
async fn publication_cas_audit_immutability_replay_and_restart() {
    let url = std::env::var("AI_REGISTRY_TEST_DATABASE_URL")
        .expect("dedicated PostgreSQL test URL required");
    let admin = PgPool::connect(&url).await.unwrap();
    let name: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&admin)
        .await
        .unwrap();
    assert_eq!(name, "sdlc2_ai_registry_test", "refuse working databases");
    let schema = format!("ai_test_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    let search_schema = schema.clone();
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .after_connect(move |connection, _| {
            let schema = search_schema.clone();
            Box::pin(async move {
                sqlx::query(&format!("SET search_path TO {schema}"))
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!(
        "../../migration/migrations/0001_admin_panel_v1.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../../migration/migrations/0007_ai_profiles.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../../migration/migrations/0009_ai_publication_outbox.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO ai_provider_settings(workspace,provider,model,context_window_tokens) VALUES('sdlc1','openrouter','foreign-model',64000),('sdlc2','chatgpt','gpt-6-luna',256000)")
        .execute(&pool).await.unwrap();
    sqlx::raw_sql(include_str!(
        "../../migration/migrations/0010_ai_model_contexts.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    let store = AiStore::new(pool.clone(), "sdlc2").unwrap();
    assert!(AiStore::new(pool.clone(), "sdlc1").is_err());
    store.initialize_registry().await.unwrap();
    store.initialize_registry().await.unwrap();
    let drafts = store.drafts().await.unwrap();
    assert_eq!(drafts.len(), 2);
    assert!(
        drafts
            .iter()
            .all(|d| d.settings.context_window_tokens == 256000)
    );
    assert!(store.selected().await.unwrap().is_none());
    assert!(store.pending_publication().await.unwrap().is_none());
    assert!(drafts.iter().all(|d| d.model_contexts.len() == 1));
    assert!(
        drafts
            .iter()
            .all(|d| d.model_contexts.iter().all(|m| m.model != "foreign-model"))
    );
    let mut settings = drafts
        .into_iter()
        .find(|d| d.settings.provider == ProviderId::Openrouter)
        .unwrap()
        .settings;
    settings.context_window_tokens = 128000;
    let saved = store.save_draft(&settings, 1, "qa-owner").await.unwrap();
    assert_eq!(saved.draft_revision, 2);
    assert!(matches!(
        store.save_draft(&settings, 1, "qa-owner").await,
        Err(StoreError::Domain(AiError::RevisionConflict))
    ));
    let now = Utc::now();
    let generation = Uuid::new_v4();
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
    let op = Uuid::new_v4();
    let publication = || Publication {
        expected_revision: 0,
        expected_draft_revision: 2,
        operation_id: op,
        actor_subject: "qa-owner",
        settings: &settings,
        credential_generation: generation,
        evidence: &evidence,
        adapter_version: "openrouter-test-v1",
        accounting_policy: "test-byte-bound-v1",
    };
    let profile = store.prepare_publication(publication()).await.unwrap();
    assert_eq!(profile.revision, 1);
    assert_eq!(
        store.prepare_publication(publication()).await.unwrap(),
        profile
    );
    assert!(
        store.selected().await.unwrap().is_none(),
        "pending is never active"
    );
    let restarted = AiStore::new(pool.clone(), "sdlc2").unwrap();
    let pending = restarted.publication_status(op).await.unwrap().unwrap();
    assert_eq!(pending.state, "pending");
    assert_eq!(pending.profile, profile);
    assert!(pending.rejection_code.is_none());
    assert_eq!(
        restarted
            .pending_publication()
            .await
            .unwrap()
            .unwrap()
            .operation_id,
        op
    );
    assert!(matches!(
        store
            .prepare_publication(Publication {
                operation_id: Uuid::new_v4(),
                ..publication()
            })
            .await,
        Err(StoreError::PublicationPending)
    ));
    let ack = || RegistrationAck {
        operation_id: op,
        profile: &profile,
        credential_generation: generation,
        adapter_version: "openrouter-test-v1",
        accounting_policy: "test-byte-bound-v1",
    };
    assert!(matches!(
        store
            .complete_publication(RegistrationAck {
                credential_generation: Uuid::new_v4(),
                ..ack()
            })
            .await,
        Err(StoreError::OperationConflict)
    ));
    assert!(store.selected().await.unwrap().is_none());
    assert_eq!(
        restarted.complete_publication(ack()).await.unwrap(),
        profile
    );
    assert_eq!(
        store.complete_publication(ack()).await.unwrap(),
        profile,
        "ACK replay does not repeat publication"
    );
    assert_eq!(
        store.publication_status(op).await.unwrap().unwrap().state,
        "published"
    );
    assert!(restarted.pending_publication().await.unwrap().is_none());
    assert_eq!(
        AiStore::new(pool.clone(), "sdlc2")
            .unwrap()
            .selected()
            .await
            .unwrap(),
        Some(profile.clone())
    );
    assert!(
        sqlx::query("UPDATE ai_profile_revisions SET published_by='changed'")
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM ai_profile_revisions")
            .execute(&pool)
            .await
            .is_err()
    );
    let operations: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE action='ai.profile.published'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(operations, 1);
    assert!(
        sqlx::query("UPDATE ai_publication_outbox SET adapter_version='forged'")
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM ai_publication_outbox")
            .execute(&pool)
            .await
            .is_err()
    );
    let different_actor = Publication {
        actor_subject: "other-owner",
        ..publication()
    };
    assert!(matches!(
        store.prepare_publication(different_actor).await,
        Err(StoreError::OperationConflict)
    ));
    assert!(matches!(
        store
            .prepare_publication(Publication {
                credential_generation: Uuid::new_v4(),
                ..publication()
            })
            .await,
        Err(StoreError::OperationConflict)
    ));
    let first = Publication {
        operation_id: Uuid::new_v4(),
        expected_revision: 1,
        ..publication()
    };
    let first_op = first.operation_id;
    let second = Publication {
        operation_id: Uuid::new_v4(),
        expected_revision: 1,
        ..publication()
    };
    let second_op = second.operation_id;
    let (a, b) = tokio::join!(
        store.prepare_publication(first),
        store.prepare_publication(second)
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert!(
        matches!(&a, Err(StoreError::PublicationPending))
            || matches!(&b, Err(StoreError::PublicationPending))
    );
    assert_eq!(store.selected().await.unwrap().unwrap(), profile);
    let (candidate, candidate_op) = match a {
        Ok(profile) => (profile, first_op),
        _ => (b.unwrap(), second_op),
    };
    assert!(matches!(
        store
            .complete_publication(RegistrationAck {
                operation_id: candidate_op,
                profile: &candidate,
                credential_generation: generation,
                adapter_version: "wrong-version",
                accounting_policy: "test-byte-bound-v1"
            })
            .await,
        Err(StoreError::OperationConflict)
    ));
    store
        .complete_publication(RegistrationAck {
            operation_id: candidate_op,
            profile: &candidate,
            credential_generation: generation,
            adapter_version: "openrouter-test-v1",
            accounting_policy: "test-byte-bound-v1",
        })
        .await
        .unwrap();
    let selected = store.selected().await.unwrap().unwrap();
    assert_eq!(selected.revision, 2);
    // A settings update cannot rewrite either saved execution profile.
    let changed = ProviderSettings {
        context_window_tokens: 256000,
        ..settings.clone()
    };
    store.save_draft(&changed, 2, "qa-owner").await.unwrap();
    assert_eq!(store.selected().await.unwrap().unwrap(), selected);
    let expired = VerificationEvidence {
        settings: changed.clone(),
        expires_at: now - Duration::seconds(1),
        ..evidence.clone()
    };
    assert!(matches!(
        store
            .prepare_publication(Publication {
                expected_revision: 2,
                expected_draft_revision: 3,
                operation_id: Uuid::new_v4(),
                actor_subject: "qa-owner",
                settings: &changed,
                credential_generation: generation,
                evidence: &expired,
                adapter_version: "openrouter-test-v1",
                accounting_policy: "test-byte-bound-v1",
            })
            .await,
        Err(StoreError::Domain(AiError::VerificationExpired))
    ));
    assert_eq!(store.selected().await.unwrap().unwrap(), selected);
    // A definitive rejected candidate is preserved; its ID is never reused.
    let next_op = Uuid::new_v4();
    let rejected = store
        .prepare_publication(Publication {
            expected_revision: 2,
            operation_id: next_op,
            ..publication()
        })
        .await;
    assert!(
        matches!(rejected, Err(StoreError::Domain(AiError::RevisionConflict))),
        "updated draft invalidates original settings"
    );
    let fresh = VerificationEvidence {
        settings: changed.clone(),
        ..evidence.clone()
    };
    let candidate = store
        .prepare_publication(Publication {
            expected_revision: 2,
            expected_draft_revision: 3,
            operation_id: next_op,
            actor_subject: "qa-owner",
            settings: &changed,
            credential_generation: generation,
            evidence: &fresh,
            adapter_version: "openrouter-test-v1",
            accounting_policy: "test-byte-bound-v1",
        })
        .await
        .unwrap();
    assert_eq!(candidate.revision, 3);
    store.reject_publication(next_op, "qa-owner").await.unwrap();
    store.reject_publication(next_op, "qa-owner").await.unwrap();
    // A replay cannot overwrite the durable reason or add another audit event.
    store
        .reject_publication_with_reason(
            next_op,
            "ai-runtime-reconciler",
            PublicationRejection::RuntimeConflict,
        )
        .await
        .unwrap();
    assert_eq!(
        restarted
            .publication_status(next_op)
            .await
            .unwrap()
            .unwrap()
            .rejection_code
            .as_deref(),
        Some("operator_rejected")
    );
    let rejected_audits: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE action='ai.profile.rejected' AND metadata->>'operation_id'=$1")
        .bind(next_op.to_string()).fetch_one(&pool).await.unwrap();
    assert_eq!(rejected_audits, 1);
    assert!(matches!(
        store
            .complete_publication(RegistrationAck {
                operation_id: next_op,
                profile: &candidate,
                credential_generation: generation,
                adapter_version: "openrouter-test-v1",
                accounting_policy: "test-byte-bound-v1"
            })
            .await,
        Err(StoreError::OperationConflict)
    ));
    let stale_op = Uuid::new_v4();
    let after_rejection = store
        .prepare_publication(Publication {
            expected_revision: 2,
            expected_draft_revision: 3,
            operation_id: stale_op,
            actor_subject: "qa-owner",
            settings: &changed,
            credential_generation: generation,
            evidence: &fresh,
            adapter_version: "openrouter-test-v1",
            accounting_policy: "test-byte-bound-v1",
        })
        .await
        .unwrap();
    assert_eq!(after_rejection.revision, 4);
    assert_eq!(store.selected().await.unwrap().unwrap(), selected);
    // A draft changed after preparation must not become active when ACK arrives.
    let another = ProviderSettings {
        model: "qa/other-model".into(),
        context_window_tokens: 192000,
        ..changed.clone()
    };
    store.save_draft(&another, 3, "qa-owner").await.unwrap();
    assert!(matches!(
        store
            .complete_publication(RegistrationAck {
                operation_id: stale_op,
                profile: &after_rejection,
                credential_generation: generation,
                adapter_version: "openrouter-test-v1",
                accounting_policy: "test-byte-bound-v1",
            })
            .await,
        Err(StoreError::Domain(AiError::RevisionConflict))
    ));
    assert_eq!(
        store
            .publication_status(stale_op)
            .await
            .unwrap()
            .unwrap()
            .state,
        "pending"
    );
    assert_eq!(store.selected().await.unwrap().unwrap(), selected);
    restarted
        .reject_publication_with_reason(
            stale_op,
            "ai-runtime-reconciler",
            PublicationRejection::DraftChanged,
        )
        .await
        .unwrap();
    assert!(store.pending_publication().await.unwrap().is_none());
    let rejected = store.publication_status(stale_op).await.unwrap().unwrap();
    assert_eq!(rejected.state, "rejected");
    assert_eq!(rejected.rejection_code.as_deref(), Some("draft_changed"));
    assert_eq!(store.selected().await.unwrap().unwrap(), selected);
    let back = store.save_draft(&changed, 4, "qa-owner").await.unwrap();
    assert_eq!(
        back.model_contexts
            .iter()
            .find(|m| m.model == another.model)
            .unwrap()
            .context_window_tokens,
        192000
    );
    assert_eq!(
        back.model_contexts
            .iter()
            .find(|m| m.model == changed.model)
            .unwrap()
            .context_window_tokens,
        256000
    );
    let conflict = ProviderSettings {
        context_window_tokens: 64000,
        ..another.clone()
    };
    assert!(matches!(
        store.save_draft(&conflict, 4, "qa-owner").await,
        Err(StoreError::Domain(AiError::RevisionConflict))
    ));
    let restarted = AiStore::new(pool.clone(), "sdlc2").unwrap();
    restarted.initialize_registry().await.unwrap();
    let remembered = restarted
        .drafts()
        .await
        .unwrap()
        .into_iter()
        .find(|d| d.settings.provider == ProviderId::Openrouter)
        .unwrap();
    assert_eq!(
        remembered
            .model_contexts
            .iter()
            .find(|m| m.model == another.model)
            .unwrap()
            .context_window_tokens,
        192000
    );
    assert_eq!(remembered.model_contexts.len(), 2);
    let chatgpt = ProviderSettings {
        provider: ProviderId::Chatgpt,
        model: another.model.clone(),
        context_window_tokens: 128000,
    };
    store.save_draft(&chatgpt, 1, "qa-owner").await.unwrap();
    assert_eq!(
        store
            .drafts()
            .await
            .unwrap()
            .into_iter()
            .find(|d| d.settings.provider == ProviderId::Openrouter)
            .unwrap()
            .model_contexts
            .iter()
            .find(|m| m.model == another.model)
            .unwrap()
            .context_window_tokens,
        192000
    );
    pool.close().await;
    // Only the generated schema in the expressly named test database is removed.
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
}
