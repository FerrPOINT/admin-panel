//! Workspace-owned drafts and pending publication with outbox, CAS and audit.
use admin_panel_domain::ai::{
    AiError, ProviderId, ProviderSettings, RuntimeProfile, VerificationEvidence, publish_profile,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Domain(#[from] AiError),
    #[error("database_operation_failed")]
    Database(#[from] sqlx::Error),
    #[error("invalid_stored_profile")]
    Serialization,
    #[error("operation_payload_conflict")]
    OperationConflict,
    #[error("publication_pending_readback_required")]
    PublicationPending,
}

#[derive(Clone)]
pub struct AiStore {
    pool: PgPool,
    workspace: String,
}

#[derive(Serialize)]
pub struct ProviderDraft {
    pub settings: ProviderSettings,
    pub draft_revision: i64,
    pub updated_at: DateTime<Utc>,
    pub model_contexts: Vec<ModelContext>,
}

#[derive(Deserialize, Serialize)]
pub struct ModelContext {
    pub model: String,
    pub context_window_tokens: u32,
}

pub struct Publication<'a> {
    pub expected_revision: u64,
    pub expected_draft_revision: i64,
    pub operation_id: Uuid,
    pub actor_subject: &'a str,
    pub settings: &'a ProviderSettings,
    pub credential_generation: Uuid,
    pub evidence: &'a VerificationEvidence,
    pub adapter_version: &'a str,
    pub accounting_policy: &'a str,
}

#[derive(Serialize)]
pub struct PendingPublication {
    pub operation_id: Uuid,
    pub expected_draft_revision: i64,
    pub profile: RuntimeProfile,
    pub credential_generation: Uuid,
    pub evidence: VerificationEvidence,
    pub adapter_version: String,
    pub accounting_policy: String,
    pub state: String,
    pub rejection_code: Option<String>,
}

pub enum PublicationRejection {
    Operator,
    DraftChanged,
    EvidenceExpired,
    RuntimeRejected,
    RuntimeConflict,
}

impl PublicationRejection {
    fn code(&self) -> &'static str {
        match self {
            Self::Operator => "operator_rejected",
            Self::DraftChanged => "draft_changed",
            Self::EvidenceExpired => "verification_expired",
            Self::RuntimeRejected => "runtime_rejected",
            Self::RuntimeConflict => "runtime_conflict",
        }
    }
}

/// Constructed only from authenticated runtime registration/status, not a UI DTO.
pub struct RegistrationAck<'a> {
    pub operation_id: Uuid,
    pub profile: &'a RuntimeProfile,
    pub credential_generation: Uuid,
    pub adapter_version: &'a str,
    pub accounting_policy: &'a str,
}

pub fn provider_key(provider: ProviderId) -> &'static str {
    match provider {
        ProviderId::Chatgpt => "chatgpt",
        ProviderId::Openrouter => "openrouter",
    }
}

impl AiStore {
    pub fn new(pool: PgPool, workspace: &str) -> Result<Self, AiError> {
        if workspace != "sdlc2" {
            return Err(AiError::InvalidWorkspace);
        }
        Ok(Self {
            pool,
            workspace: workspace.into(),
        })
    }

    pub async fn initialize_registry(&self) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        for (provider, model) in [
            ("chatgpt", "gpt-6-luna"),
            ("openrouter", "deepseek/deepseek-v4.1-flash"),
        ] {
            sqlx::query("INSERT INTO ai_provider_settings(workspace,provider,model) VALUES($1,$2,$3) ON CONFLICT DO NOTHING")
                .bind(&self.workspace).bind(provider).bind(model).execute(&mut *tx).await?;
        }
        sqlx::query("INSERT INTO ai_active_profile(workspace) VALUES($1) ON CONFLICT DO NOTHING")
            .bind(&self.workspace)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO ai_model_contexts(workspace,provider,model,context_window_tokens) SELECT workspace,provider,model,context_window_tokens FROM ai_provider_settings WHERE workspace=$1 ON CONFLICT DO NOTHING")
            .bind(&self.workspace).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn drafts(&self) -> Result<Vec<ProviderDraft>, StoreError> {
        let rows=sqlx::query("SELECT p.provider,p.model,p.context_window_tokens,p.draft_revision,p.updated_at,COALESCE((SELECT jsonb_agg(jsonb_build_object('model',m.model,'context_window_tokens',m.context_window_tokens) ORDER BY m.model) FROM ai_model_contexts m WHERE m.workspace=p.workspace AND m.provider=p.provider),'[]'::jsonb) AS model_contexts FROM ai_provider_settings p WHERE p.workspace=$1 ORDER BY p.provider")
            .bind(&self.workspace).fetch_all(&self.pool).await?;
        rows.iter()
            .map(|row| {
                let provider = match row.get::<String, _>("provider").as_str() {
                    "chatgpt" => ProviderId::Chatgpt,
                    "openrouter" => ProviderId::Openrouter,
                    _ => return Err(StoreError::Serialization),
                };
                let context: u32 = row
                    .get::<i64, _>("context_window_tokens")
                    .try_into()
                    .map_err(|_| StoreError::Serialization)?;
                Ok(ProviderDraft {
                    settings: ProviderSettings {
                        provider,
                        model: row.get("model"),
                        context_window_tokens: context,
                    },
                    draft_revision: row.get("draft_revision"),
                    updated_at: row.get("updated_at"),
                    model_contexts: serde_json::from_value(row.get("model_contexts"))
                        .map_err(|_| StoreError::Serialization)?,
                })
            })
            .collect()
    }

    pub async fn save_draft(
        &self,
        settings: &ProviderSettings,
        expected: i64,
        actor: &str,
    ) -> Result<ProviderDraft, StoreError> {
        if settings.model.is_empty()
            || settings.model.len() > 256
            || settings
                .model
                .chars()
                .any(|c| c.is_control() || c.is_whitespace())
        {
            return Err(AiError::InvalidModel.into());
        }
        if settings.context_window_tokens < 64000 {
            return Err(AiError::InvalidContextBudget.into());
        }
        if expected <= 0 || expected == i64::MAX {
            return Err(AiError::RevisionConflict.into());
        }
        let mut tx = self.pool.begin().await?;
        let row=sqlx::query("UPDATE ai_provider_settings SET model=$3,context_window_tokens=$4,draft_revision=draft_revision+1,updated_at=now() WHERE workspace=$1 AND provider=$2 AND draft_revision=$5 RETURNING draft_revision,updated_at")
            .bind(&self.workspace).bind(provider_key(settings.provider)).bind(&settings.model).bind(i64::from(settings.context_window_tokens)).bind(expected).fetch_optional(&mut *tx).await?.ok_or(AiError::RevisionConflict)?;
        let revision: i64 = row.get("draft_revision");
        sqlx::query("INSERT INTO ai_model_contexts(workspace,provider,model,context_window_tokens) VALUES($1,$2,$3,$4) ON CONFLICT(workspace,provider,model) DO UPDATE SET context_window_tokens=EXCLUDED.context_window_tokens,updated_at=now()")
            .bind(&self.workspace).bind(provider_key(settings.provider)).bind(&settings.model)
            .bind(i64::from(settings.context_window_tokens)).execute(&mut *tx).await?;
        let contexts:serde_json::Value=sqlx::query_scalar("SELECT jsonb_agg(jsonb_build_object('model',model,'context_window_tokens',context_window_tokens) ORDER BY model) FROM ai_model_contexts WHERE workspace=$1 AND provider=$2")
            .bind(&self.workspace).bind(provider_key(settings.provider)).fetch_one(&mut *tx).await?;
        audit(&mut tx,actor,"ai.settings.saved",serde_json::json!({"workspace":self.workspace,"provider":provider_key(settings.provider),"draft_revision":revision})).await?;
        let result = ProviderDraft {
            settings: settings.clone(),
            draft_revision: revision,
            updated_at: row.get("updated_at"),
            model_contexts: serde_json::from_value(contexts)
                .map_err(|_| StoreError::Serialization)?,
        };
        tx.commit().await?;
        Ok(result)
    }

    pub async fn selected(&self) -> Result<Option<RuntimeProfile>, StoreError> {
        let value:Option<serde_json::Value>=sqlx::query_scalar("SELECT r.profile FROM ai_active_profile a JOIN ai_profile_revisions r ON r.workspace=a.workspace AND r.revision=a.revision WHERE a.workspace=$1")
            .bind(&self.workspace).fetch_optional(&self.pool).await?;
        value
            .map(|value| serde_json::from_value(value).map_err(|_| StoreError::Serialization))
            .transpose()
    }

    /// Evidence is supplied by the trusted runtime client, never a browser DTO.
    pub async fn prepare_publication(
        &self,
        publication: Publication<'_>,
    ) -> Result<RuntimeProfile, StoreError> {
        if publication.operation_id.is_nil()
            || publication.actor_subject.is_empty()
            || publication.actor_subject.len() > 256
            || publication.actor_subject.chars().any(char::is_control)
            || [publication.adapter_version, publication.accounting_policy]
                .iter()
                .any(|value| {
                    value.is_empty() || value.len() > 256 || value.chars().any(char::is_control)
                })
        {
            return Err(StoreError::OperationConflict);
        }
        let mut tx = self.pool.begin().await?;
        let current: Option<i64> = sqlx::query_scalar(
            "SELECT revision FROM ai_active_profile WHERE workspace=$1 FOR UPDATE",
        )
        .bind(&self.workspace)
        .fetch_one(&mut *tx)
        .await?;
        if let Some(row)=sqlx::query("SELECT r.profile,r.published_by,r.credential_generation,o.expected_revision,o.expected_draft_revision,o.evidence,o.adapter_version,o.accounting_policy FROM ai_profile_revisions r JOIN ai_publication_outbox o USING(operation_id) WHERE r.workspace=$1 AND r.operation_id=$2").bind(&self.workspace).bind(publication.operation_id).fetch_optional(&mut *tx).await? {
            let stored:RuntimeProfile=serde_json::from_value(row.get("profile")).map_err(|_|StoreError::Serialization)?;
            if stored.provider!=publication.settings.provider || stored.model!=publication.settings.model || stored.context_window_tokens!=publication.settings.context_window_tokens || row.get::<String,_>("published_by")!=publication.actor_subject || row.get::<Uuid,_>("credential_generation") != publication.credential_generation
                || row.get::<i64,_>("expected_revision") as u64 != publication.expected_revision
                || row.get::<i64,_>("expected_draft_revision") != publication.expected_draft_revision
                || row.get::<String,_>("adapter_version") != publication.adapter_version
                || row.get::<String,_>("accounting_policy") != publication.accounting_policy
                || row.get::<serde_json::Value,_>("evidence") != serde_json::to_value(publication.evidence).map_err(|_|StoreError::Serialization)? {return Err(StoreError::OperationConflict);}
            return Ok(stored);
        }
        let pending: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM ai_publication_outbox WHERE workspace=$1 AND state='pending')")
            .bind(&self.workspace).fetch_one(&mut *tx).await?;
        if pending {
            return Err(StoreError::PublicationPending);
        }
        let draft=sqlx::query("SELECT model,context_window_tokens,draft_revision FROM ai_provider_settings WHERE workspace=$1 AND provider=$2 FOR UPDATE")
            .bind(&self.workspace).bind(provider_key(publication.settings.provider)).fetch_one(&mut *tx).await?;
        if draft.get::<i64, _>("draft_revision") != publication.expected_draft_revision
            || draft.get::<String, _>("model") != publication.settings.model
            || draft.get::<i64, _>("context_window_tokens")
                != i64::from(publication.settings.context_window_tokens)
        {
            return Err(AiError::RevisionConflict.into());
        }
        let revision =
            u64::try_from(current.unwrap_or(0)).map_err(|_| StoreError::Serialization)?;
        let mut profile = publish_profile(
            &self.workspace,
            revision,
            publication.expected_revision,
            publication.settings,
            publication.credential_generation,
            publication.evidence,
            Utc::now(),
        )?;
        // Rejected candidates retain their immutable IDs; never reuse a revision.
        let highest: i64 = sqlx::query_scalar(
            "SELECT COALESCE(max(revision),0) FROM ai_profile_revisions WHERE workspace=$1",
        )
        .bind(&self.workspace)
        .fetch_one(&mut *tx)
        .await?;
        profile.revision = u64::try_from(highest.checked_add(1).ok_or(AiError::RevisionConflict)?)
            .map_err(|_| StoreError::Serialization)?;
        let next: i64 = profile
            .revision
            .try_into()
            .map_err(|_| AiError::RevisionConflict)?;
        let json = serde_json::to_value(&profile).map_err(|_| StoreError::Serialization)?;
        sqlx::query("INSERT INTO ai_profile_revisions(workspace,revision,profile,credential_generation,operation_id,published_by) VALUES($1,$2,$3,$4,$5,$6)")
            .bind(&self.workspace).bind(next).bind(json).bind(publication.credential_generation).bind(publication.operation_id).bind(publication.actor_subject).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO ai_publication_outbox(operation_id,workspace,revision,expected_revision,expected_draft_revision,evidence,adapter_version,accounting_policy) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
            .bind(publication.operation_id).bind(&self.workspace).bind(next).bind(current.unwrap_or(0))
            .bind(publication.expected_draft_revision).bind(serde_json::to_value(publication.evidence).map_err(|_|StoreError::Serialization)?)
            .bind(publication.adapter_version).bind(publication.accounting_policy).execute(&mut *tx).await?;
        audit(&mut tx,publication.actor_subject,"ai.profile.pending",serde_json::json!({"workspace":self.workspace,"revision":profile.revision,"operation_id":publication.operation_id})).await?;
        tx.commit().await?;
        Ok(profile)
    }

    pub async fn publication_status(
        &self,
        operation: Uuid,
    ) -> Result<Option<PendingPublication>, StoreError> {
        let row = sqlx::query("SELECT r.profile,r.credential_generation,o.expected_draft_revision,o.evidence,o.adapter_version,o.accounting_policy,o.state,CASE WHEN o.state='rejected' THEN COALESCE((SELECT a.metadata->>'reason' FROM audit_events a WHERE a.action='ai.profile.rejected' AND a.metadata->>'workspace'=o.workspace AND a.metadata->>'operation_id'=o.operation_id::text ORDER BY a.occurred_at DESC LIMIT 1),'operator_rejected') ELSE NULL END AS rejection_code FROM ai_publication_outbox o JOIN ai_profile_revisions r USING(workspace,revision) WHERE o.workspace=$1 AND o.operation_id=$2")
            .bind(&self.workspace).bind(operation).fetch_optional(&self.pool).await?;
        row.map(|row| {
            Ok(PendingPublication {
                operation_id: operation,
                expected_draft_revision: row.get("expected_draft_revision"),
                profile: serde_json::from_value(row.get("profile"))
                    .map_err(|_| StoreError::Serialization)?,
                credential_generation: row.get("credential_generation"),
                evidence: serde_json::from_value(row.get("evidence"))
                    .map_err(|_| StoreError::Serialization)?,
                adapter_version: row.get("adapter_version"),
                accounting_policy: row.get("accounting_policy"),
                state: row.get("state"),
                rejection_code: row.get("rejection_code"),
            })
        })
        .transpose()
    }

    pub async fn pending_publication(&self) -> Result<Option<PendingPublication>, StoreError> {
        let operation: Option<Uuid> = sqlx::query_scalar("SELECT operation_id FROM ai_publication_outbox WHERE workspace=$1 AND state='pending' ORDER BY revision LIMIT 1")
            .bind(&self.workspace).fetch_optional(&self.pool).await?;
        match operation {
            Some(operation) => Ok(self
                .publication_status(operation)
                .await?
                .filter(|pending| pending.state == "pending")),
            None => Ok(None),
        }
    }

    pub async fn complete_publication(
        &self,
        ack: RegistrationAck<'_>,
    ) -> Result<RuntimeProfile, StoreError> {
        let mut tx = self.pool.begin().await?;
        let current: Option<i64> = sqlx::query_scalar(
            "SELECT revision FROM ai_active_profile WHERE workspace=$1 FOR UPDATE",
        )
        .bind(&self.workspace)
        .fetch_one(&mut *tx)
        .await?;
        let row = sqlx::query("SELECT r.profile,r.credential_generation,r.published_by,o.expected_revision,o.expected_draft_revision,o.revision,o.state,o.evidence,o.adapter_version,o.accounting_policy FROM ai_publication_outbox o JOIN ai_profile_revisions r USING(workspace,revision) WHERE o.workspace=$1 AND o.operation_id=$2 FOR UPDATE OF o")
            .bind(&self.workspace).bind(ack.operation_id).fetch_optional(&mut *tx).await?.ok_or(StoreError::OperationConflict)?;
        let profile: RuntimeProfile =
            serde_json::from_value(row.get("profile")).map_err(|_| StoreError::Serialization)?;
        if &profile != ack.profile
            || row.get::<Uuid, _>("credential_generation") != ack.credential_generation
            || row.get::<String, _>("adapter_version") != ack.adapter_version
            || row.get::<String, _>("accounting_policy") != ack.accounting_policy
        {
            return Err(StoreError::OperationConflict);
        }
        match row.get::<String, _>("state").as_str() {
            "published" => return Ok(profile),
            "pending" => {}
            _ => return Err(StoreError::OperationConflict),
        }
        let expected: i64 = row.get("expected_revision");
        if current.unwrap_or(0) != expected {
            return Err(AiError::RevisionConflict.into());
        }
        let draft=sqlx::query("SELECT model,context_window_tokens,draft_revision FROM ai_provider_settings WHERE workspace=$1 AND provider=$2 FOR UPDATE")
            .bind(&self.workspace).bind(provider_key(profile.provider)).fetch_one(&mut *tx).await?;
        if draft.get::<i64, _>("draft_revision") != row.get::<i64, _>("expected_draft_revision")
            || draft.get::<String, _>("model") != profile.model
            || draft.get::<i64, _>("context_window_tokens")
                != i64::from(profile.context_window_tokens)
        {
            return Err(AiError::RevisionConflict.into());
        }
        let evidence: VerificationEvidence =
            serde_json::from_value(row.get("evidence")).map_err(|_| StoreError::Serialization)?;
        if evidence.expires_at <= Utc::now() {
            return Err(AiError::VerificationExpired.into());
        }
        let changed = sqlx::query("UPDATE ai_active_profile SET revision=$2 WHERE workspace=$1 AND COALESCE(revision,0)=$3")
            .bind(&self.workspace).bind(row.get::<i64,_>("revision")).bind(expected).execute(&mut *tx).await?;
        if changed.rows_affected() != 1 {
            return Err(AiError::RevisionConflict.into());
        }
        sqlx::query("UPDATE ai_publication_outbox SET state='published',completed_at=now() WHERE operation_id=$1 AND state='pending'")
            .bind(ack.operation_id).execute(&mut *tx).await?;
        audit(&mut tx,&row.get::<String,_>("published_by"),"ai.profile.published",serde_json::json!({"workspace":self.workspace,"revision":profile.revision,"operation_id":ack.operation_id,"verification_id":profile.verification_id})).await?;
        tx.commit().await?;
        Ok(profile)
    }

    /// Only a definitive runtime rejection/operator reconciliation may release pending.
    /// A timeout or transport failure must keep the operation pending for readback.
    pub async fn reject_publication(&self, operation: Uuid, actor: &str) -> Result<(), StoreError> {
        self.reject_publication_with_reason(operation, actor, PublicationRejection::Operator)
            .await
    }

    pub async fn reject_publication_with_reason(
        &self,
        operation: Uuid,
        actor: &str,
        reason: PublicationRejection,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT revision FROM ai_active_profile WHERE workspace=$1 FOR UPDATE")
            .bind(&self.workspace)
            .fetch_one(&mut *tx)
            .await?;
        let row = sqlx::query("SELECT state,revision FROM ai_publication_outbox WHERE workspace=$1 AND operation_id=$2 FOR UPDATE")
            .bind(&self.workspace).bind(operation).fetch_optional(&mut *tx).await?.ok_or(StoreError::OperationConflict)?;
        match row.get::<String, _>("state").as_str() {
            "rejected" => return Ok(()),
            "pending" => {}
            _ => return Err(StoreError::OperationConflict),
        }
        sqlx::query("UPDATE ai_publication_outbox SET state='rejected',completed_at=now() WHERE operation_id=$1")
            .bind(operation).execute(&mut *tx).await?;
        audit(
            &mut tx,
            actor,
            "ai.profile.rejected",
            serde_json::json!({"workspace":self.workspace,"operation_id":operation,"revision":row.get::<i64,_>("revision"),"reason":reason.code()}),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
}

async fn audit(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor: &str,
    action: &str,
    metadata: serde_json::Value,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO audit_events(id,occurred_at,request_id,actor_subject,action,entity_type,metadata) VALUES($1,now(),$2,$3,$4,'ai_profile',$5)")
        .bind(Uuid::new_v4()).bind(Uuid::new_v4()).bind(actor).bind(action).bind(metadata).execute(&mut **tx).await?;
    Ok(())
}
