//! Branding revisions persistence.

use admin_panel_domain::{
    BrandingDocument, BrandingRevision, DomainError, RevisionState,
};
use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Clone)]
pub struct BrandingStore {
    pool: PgPool,
}

impl BrandingStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn next_revision_number(&self) -> Result<i64, DomainError> {
        let max: Option<i64> =
            sqlx::query_scalar("SELECT max(revision) FROM branding_revisions")
                .fetch_one(&self.pool)
                .await
                .map_err(db)?;
        Ok(max.unwrap_or(0) + 1)
    }

    pub async fn insert_draft(&self, revision: &BrandingRevision) -> Result<(), DomainError> {
        let result = sqlx::query(
            "INSERT INTO branding_revisions \
             (id, revision, state, document, document_hash, etag, created_by_subject, \
             created_at, based_on_revision) \
             VALUES ($1, $2, 'draft', $3, $4, $5, $6, $7, $8)",
        )
        .bind(revision.id)
        .bind(revision.revision)
        .bind(serde_json::to_value(&revision.document).unwrap())
        .bind(&revision.document_hash)
        .bind(&revision.etag)
        .bind(&revision.created_by_subject)
        .bind(revision.created_at)
        .bind(revision.based_on_revision)
        .execute(&self.pool)
        .await
        .map_err(db)?;
        if result.rows_affected() != 1 {
            return Err(DomainError::Conflict("draft not created".into()));
        }
        Ok(())
    }

    pub async fn find_by_revision(
        &self,
        revision: i64,
    ) -> Result<Option<BrandingRevision>, sqlx::Error> {
        sqlx::query_as::<_, RevisionRow>(&(String::from(select_sql()) + " WHERE revision = $1"))
            .bind(revision)
            .fetch_optional(&self.pool)
            .await
            .map(|row| row.map(Into::into))
    }

    pub async fn find_published(&self) -> Result<Option<BrandingRevision>, sqlx::Error> {
        sqlx::query_as::<_, RevisionRow>(&(String::from(select_sql()) + " WHERE state = 'published'"))
            .fetch_optional(&self.pool)
            .await
            .map(|row| row.map(Into::into))
    }

    pub async fn list(&self) -> Result<Vec<BrandingRevision>, sqlx::Error> {
        let rows = sqlx::query_as::<_, RevisionRow>(&(String::from(select_sql()) + " ORDER BY revision DESC"))
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn update_draft_document(
        &self,
        revision: i64,
        document: &BrandingDocument,
        document_hash: &str,
    ) -> Result<(), DomainError> {
        let result = sqlx::query(
            "UPDATE branding_revisions SET document = $3, document_hash = $4 \
             WHERE revision = $1 AND state = 'draft'",
        )
        .bind(revision)
        .bind(revision)
        .bind(serde_json::to_value(document).unwrap())
        .bind(document_hash)
        .execute(&self.pool)
        .await
        .map_err(db)?;
        if result.rows_affected() != 1 {
            return Err(DomainError::PreconditionFailed(
                "revision is not an editable draft".into(),
            ));
        }
        Ok(())
    }

    /// Atomically publishes a draft: previous published -> superseded.
    pub async fn publish(
        &self,
        revision: i64,
        publisher: &str,
        etag: &str,
    ) -> Result<BrandingRevision, DomainError> {
        let mut tx = self.pool.begin().await.map_err(db)?;
        sqlx::query(
            "UPDATE branding_revisions SET state = 'superseded' WHERE state = 'published'",
        )
        .execute(&mut *tx)
        .await
        .map_err(db)?;
        let row = sqlx::query_as::<_, RevisionRow>(
            &(String::from(select_sql()) + " WHERE revision = $1 AND state = 'draft' FOR UPDATE"),
        )
        .bind(revision)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?
        .ok_or(DomainError::Conflict("revision is not a draft".into()))?;
        sqlx::query(
            "UPDATE branding_revisions \
             SET state = 'published', published_by_subject = $2, published_at = $3, etag = $4 \
             WHERE revision = $1",
        )
        .bind(revision)
        .bind(publisher)
        .bind(Utc::now())
        .bind(etag)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
        tx.commit().await.map_err(db)?;
        let mut published: BrandingRevision = row.into();
        published.state = RevisionState::Published;
        published.published_by_subject = Some(publisher.to_string());
        published.etag = etag.to_string();
        Ok(published)
    }
}

fn select_sql() -> &'static str {
    "SELECT id, revision, state::text, document, document_hash, etag, \
     created_by_subject, created_at, published_by_subject, published_at, based_on_revision \
     FROM branding_revisions"
}

fn db(err: sqlx::Error) -> DomainError {
    DomainError::Conflict(err.to_string())
}

#[derive(sqlx::FromRow)]
struct RevisionRow {
    id: Uuid,
    revision: i64,
    #[sqlx(rename = "state")]
    state_text: String,
    document: serde_json::Value,
    document_hash: String,
    etag: String,
    created_by_subject: String,
    created_at: chrono::DateTime<Utc>,
    published_by_subject: Option<String>,
    published_at: Option<chrono::DateTime<Utc>>,
    based_on_revision: Option<i64>,
}

impl From<RevisionRow> for BrandingRevision {
    fn from(row: RevisionRow) -> Self {
        Self {
            id: row.id,
            revision: row.revision,
            state: RevisionState::parse(&row.state_text).unwrap_or(RevisionState::Draft),
            document: serde_json::from_value(row.document).unwrap_or_default(),
            document_hash: row.document_hash,
            etag: row.etag,
            created_by_subject: row.created_by_subject,
            created_at: row.created_at,
            published_by_subject: row.published_by_subject,
            published_at: row.published_at,
            based_on_revision: row.based_on_revision,
        }
    }
}


