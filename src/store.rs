use crate::{
    ClaimId, DailyReviewId, EvidenceId, EvidenceRelation, MemoryRef, PersonId, RetrievalItem,
    RetrievalPack, SourceId, SourceKind, TenantId, Timestamp,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub trait Embedder {
    type Error: std::error::Error + Send + Sync + 'static;

    fn embed(&self, input: &str) -> std::result::Result<Embedding, Self::Error>;
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Embedding {
    pub vector: Vec<f32>,
    pub model: String,
    pub version: String,
    pub input_hash: String,
    pub normalization: VectorNormalization,
    pub distance: VectorDistance,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VectorNormalization {
    None,
    L2,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VectorDistance {
    Cosine,
    Dot,
    Euclidean,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum EmbeddingTarget {
    Source(SourceId),
    Evidence(EvidenceId),
    Claim(ClaimId),
}

#[derive(Debug, Deserialize)]
pub struct EmbeddingInput {
    pub tenant_id: TenantId,
    pub person_id: PersonId,
    pub target: EmbeddingTarget,
    pub embedding: Embedding,
}

#[derive(Debug, Serialize)]
pub struct StoredEmbedding {
    pub target: EmbeddingTarget,
    pub dimension: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Invalid(String),
    #[error("record not found")]
    NotFound,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Deserialize)]
pub struct RememberInput {
    pub tenant_id: TenantId,
    pub person_id: PersonId,
    pub kind: SourceKind,
    pub text: String,
    pub captured_at: Timestamp,
    pub claim: Option<ClaimInput>,
}

#[derive(Debug, Deserialize)]
pub struct ClaimInput {
    pub subject: String,
    pub predicate: String,
    pub value: String,
    pub valid_from: Timestamp,
}

#[derive(Debug, Serialize)]
pub struct Remembered {
    pub source_id: SourceId,
    pub evidence_id: EvidenceId,
    pub claim_id: Option<ClaimId>,
}

#[derive(Debug, Deserialize)]
pub struct SearchInput {
    pub tenant_id: TenantId,
    pub person_id: PersonId,
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

#[derive(Debug, Deserialize)]
pub struct CorrectInput {
    pub tenant_id: TenantId,
    pub person_id: PersonId,
    pub claim_id: ClaimId,
    pub text: String,
    pub value: String,
    pub occurred_at: Timestamp,
}

#[derive(Debug, Serialize)]
pub struct Corrected {
    pub source_id: SourceId,
    pub evidence_id: EvidenceId,
    pub claim_id: ClaimId,
    pub superseded_claim_id: ClaimId,
}

#[derive(Debug, Deserialize)]
pub struct DeleteInput {
    pub tenant_id: TenantId,
    pub person_id: PersonId,
    pub source_id: SourceId,
    pub deleted_at: Timestamp,
}

#[derive(Debug, Serialize)]
pub struct Deleted {
    pub source_id: SourceId,
    pub evidence_count: u64,
    pub claim_count: u64,
}

#[derive(Debug, Deserialize)]
pub struct ReviewInput {
    pub tenant_id: TenantId,
    pub person_id: PersonId,
    pub day: String,
    pub summary: String,
    pub evidence_ids: Vec<EvidenceId>,
    pub recorded_at: Timestamp,
}

#[derive(Debug, Deserialize)]
pub struct ReviewsInput {
    pub tenant_id: TenantId,
    pub person_id: PersonId,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

#[derive(Debug, Serialize)]
pub struct StoredReview {
    pub id: DailyReviewId,
}

#[derive(Debug, Serialize)]
pub struct ReviewRecord {
    pub id: DailyReviewId,
    pub day: String,
    pub summary: String,
    pub evidence_ids: Vec<EvidenceId>,
    pub recorded_at: Timestamp,
}

pub struct MemoryDb {
    connection: Connection,
}

impl MemoryDb {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let connection = Connection::open(path)?;
        let mut database = Self { connection };
        database.migrate()?;
        Ok(database)
    }

    pub fn remember(&mut self, input: RememberInput) -> Result<Remembered> {
        require_scope(&input.tenant_id, &input.person_id)?;
        require_text("text", &input.text)?;
        let transaction = self.connection.transaction()?;
        let source_id = SourceId(new_id(&transaction)?);
        let evidence_id = EvidenceId(new_id(&transaction)?);
        let kind = serde_json::to_string(&input.kind)?;
        transaction.execute(
            "INSERT INTO sources(id, tenant_id, person_id, revision, kind, content, captured_at, recorded_at) VALUES(?1, ?2, ?3, 1, ?4, ?5, ?6, ?6)",
            params![source_id.0, input.tenant_id.0, input.person_id.0, kind, input.text, input.captured_at],
        )?;
        transaction.execute(
            "INSERT INTO source_fts(source_id, tenant_id, person_id, content) VALUES(?1, ?2, ?3, ?4)",
            params![source_id.0, input.tenant_id.0, input.person_id.0, input.text],
        )?;
        transaction.execute(
            "INSERT INTO evidence(id, tenant_id, person_id, source_id, source_revision, quote, recorded_at) VALUES(?1, ?2, ?3, ?4, 1, ?5, ?6)",
            params![evidence_id.0, input.tenant_id.0, input.person_id.0, source_id.0, input.text, input.captured_at],
        )?;
        let claim_id = input
            .claim
            .map(|claim| {
                insert_claim(
                    &transaction,
                    &input.tenant_id,
                    &input.person_id,
                    &evidence_id,
                    claim,
                    input.captured_at,
                )
            })
            .transpose()?;
        transaction.commit()?;
        Ok(Remembered {
            source_id,
            evidence_id,
            claim_id,
        })
    }

    pub fn search(&self, input: SearchInput) -> Result<RetrievalPack> {
        require_scope(&input.tenant_id, &input.person_id)?;
        require_text("query", &input.query)?;
        let limit = bounded_limit(input.limit);
        let query = format!("\"{}\"", input.query.replace('"', "\"\""));
        let mut statement = self.connection.prepare(
            "SELECT c.id, c.subject || ' ' || c.predicate || ' ' || c.value, ce.evidence_id, CAST(max(0, 10000 - (bm25(source_fts) * 1000)) AS INTEGER)
             FROM source_fts
             JOIN sources s ON s.id = source_fts.source_id AND s.tenant_id = source_fts.tenant_id AND s.person_id = source_fts.person_id
             JOIN evidence e ON e.source_id = s.id AND e.tenant_id = s.tenant_id AND e.person_id = s.person_id AND e.deleted_at IS NULL
             JOIN claim_evidence ce ON ce.evidence_id = e.id AND ce.tenant_id = e.tenant_id AND ce.person_id = e.person_id
             JOIN claims c ON c.id = ce.claim_id AND c.tenant_id = ce.tenant_id AND c.person_id = ce.person_id
             WHERE source_fts MATCH ?1 AND source_fts.tenant_id = ?2 AND source_fts.person_id = ?3 AND s.deleted_at IS NULL AND c.status = 'accepted'
             ORDER BY bm25(source_fts) LIMIT ?4",
        )?;
        let rows = statement.query_map(
            params![query, input.tenant_id.0, input.person_id.0, limit],
            |row| {
                Ok(RetrievalItem {
                    memory: MemoryRef::Claim(ClaimId(row.get(0)?)),
                    excerpt: row.get(1)?,
                    relevance_basis_points: row.get::<_, i64>(3)?.clamp(0, 10_000) as u16,
                    evidence_ids: vec![EvidenceId(row.get(2)?)],
                })
            },
        )?;
        let items = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        let gaps = if items.is_empty() {
            vec!["no cited memory matched".to_owned()]
        } else {
            Vec::new()
        };
        Ok(RetrievalPack {
            query: input.query,
            items,
            gaps,
        })
    }

    pub fn correct(&mut self, input: CorrectInput) -> Result<Corrected> {
        require_scope(&input.tenant_id, &input.person_id)?;
        require_text("correction text", &input.text)?;
        require_text("value", &input.value)?;
        let transaction = self.connection.transaction()?;
        let old = transaction
            .query_row(
                "SELECT subject, predicate, valid_from, recorded_from FROM claims WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3 AND status = 'accepted'",
                params![input.claim_id.0, input.tenant_id.0, input.person_id.0],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?)),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        if input.occurred_at <= old.3 {
            return Err(Error::Invalid(
                "occurred_at must be after the original claim was recorded".to_owned(),
            ));
        }
        let source_id = SourceId(new_id(&transaction)?);
        let evidence_id = EvidenceId(new_id(&transaction)?);
        transaction.execute(
            "INSERT INTO sources(id, tenant_id, person_id, revision, kind, content, captured_at, recorded_at) VALUES(?1, ?2, ?3, 1, '\"user_correction\"', ?4, ?5, ?5)",
            params![source_id.0, input.tenant_id.0, input.person_id.0, input.text, input.occurred_at],
        )?;
        transaction.execute(
            "INSERT INTO source_fts(source_id, tenant_id, person_id, content) VALUES(?1, ?2, ?3, ?4)",
            params![source_id.0, input.tenant_id.0, input.person_id.0, input.text],
        )?;
        transaction.execute(
            "INSERT INTO evidence(id, tenant_id, person_id, source_id, source_revision, quote, recorded_at) VALUES(?1, ?2, ?3, ?4, 1, ?5, ?6)",
            params![evidence_id.0, input.tenant_id.0, input.person_id.0, source_id.0, input.text, input.occurred_at],
        )?;
        transaction.execute(
            "UPDATE claims SET status = 'superseded', recorded_until = ?1 WHERE id = ?2 AND tenant_id = ?3 AND person_id = ?4",
            params![input.occurred_at, input.claim_id.0, input.tenant_id.0, input.person_id.0],
        )?;
        let claim_id = insert_claim(
            &transaction,
            &input.tenant_id,
            &input.person_id,
            &evidence_id,
            ClaimInput {
                subject: old.0,
                predicate: old.1,
                value: input.value,
                valid_from: input.occurred_at.max(old.2),
            },
            input.occurred_at,
        )?;
        transaction.commit()?;
        Ok(Corrected {
            source_id,
            evidence_id,
            claim_id,
            superseded_claim_id: input.claim_id,
        })
    }

    pub fn delete_source(&mut self, input: DeleteInput) -> Result<Deleted> {
        require_scope(&input.tenant_id, &input.person_id)?;
        let transaction = self.connection.transaction()?;
        let recorded_at = transaction
            .query_row(
                "SELECT recorded_at FROM sources WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3 AND deleted_at IS NULL",
                params![input.source_id.0, input.tenant_id.0, input.person_id.0],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        if input.deleted_at < recorded_at {
            return Err(Error::Invalid(
                "deleted_at cannot predate source recording".to_owned(),
            ));
        }
        let changed = transaction.execute(
            "UPDATE sources SET deleted_at = ?1, revision = revision + 1 WHERE id = ?2 AND tenant_id = ?3 AND person_id = ?4 AND deleted_at IS NULL",
            params![input.deleted_at, input.source_id.0, input.tenant_id.0, input.person_id.0],
        )?;
        if changed == 0 {
            return Err(Error::NotFound);
        }
        transaction.execute(
            "DELETE FROM source_fts WHERE source_id = ?1 AND tenant_id = ?2 AND person_id = ?3",
            params![input.source_id.0, input.tenant_id.0, input.person_id.0],
        )?;
        let evidence_count = transaction.execute(
            "UPDATE evidence SET deleted_at = ?1 WHERE source_id = ?2 AND tenant_id = ?3 AND person_id = ?4 AND deleted_at IS NULL",
            params![input.deleted_at, input.source_id.0, input.tenant_id.0, input.person_id.0],
        )? as u64;
        let claim_count = transaction.execute(
            "UPDATE claims SET status = 'rejected', recorded_until = ?1
             WHERE tenant_id = ?2 AND person_id = ?3 AND status = 'accepted'
             AND id IN (SELECT ce.claim_id FROM claim_evidence ce JOIN evidence e ON e.id = ce.evidence_id AND e.tenant_id = ce.tenant_id AND e.person_id = ce.person_id WHERE e.source_id = ?4)
             AND NOT EXISTS (SELECT 1 FROM claim_evidence live_ce JOIN evidence live_e ON live_e.id = live_ce.evidence_id AND live_e.tenant_id = live_ce.tenant_id AND live_e.person_id = live_ce.person_id WHERE live_ce.claim_id = claims.id AND live_e.deleted_at IS NULL)",
            params![input.deleted_at, input.tenant_id.0, input.person_id.0, input.source_id.0],
        )? as u64;
        transaction.execute(
            "DELETE FROM embeddings WHERE tenant_id = ?1 AND person_id = ?2 AND ((target_kind = 'source' AND target_id = ?3) OR (target_kind = 'evidence' AND target_id IN (SELECT id FROM evidence WHERE source_id = ?3 AND tenant_id = ?1 AND person_id = ?2)) OR (target_kind = 'claim' AND target_id IN (SELECT id FROM claims WHERE tenant_id = ?1 AND person_id = ?2 AND status = 'rejected')))",
            params![input.tenant_id.0, input.person_id.0, input.source_id.0],
        )?;
        transaction.execute(
            "DELETE FROM daily_reviews WHERE tenant_id = ?1 AND person_id = ?2 AND EXISTS (SELECT 1 FROM json_each(evidence_ids) citation JOIN evidence e ON e.id = citation.value WHERE e.source_id = ?3 AND e.tenant_id = ?1 AND e.person_id = ?2)",
            params![input.tenant_id.0, input.person_id.0, input.source_id.0],
        )?;
        transaction.commit()?;
        Ok(Deleted {
            source_id: input.source_id,
            evidence_count,
            claim_count,
        })
    }

    pub fn store_review(&mut self, input: ReviewInput) -> Result<StoredReview> {
        require_scope(&input.tenant_id, &input.person_id)?;
        require_text("day", &input.day)?;
        require_text("summary", &input.summary)?;
        if input.evidence_ids.is_empty() {
            return Err(Error::Invalid("review needs evidence_ids".to_owned()));
        }
        let transaction = self.connection.transaction()?;
        for evidence_id in &input.evidence_ids {
            let found: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM evidence WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3 AND deleted_at IS NULL)",
                params![evidence_id.0, input.tenant_id.0, input.person_id.0],
                |row| row.get(0),
            )?;
            if !found {
                return Err(Error::Invalid(format!(
                    "evidence {} is unavailable",
                    evidence_id.0
                )));
            }
        }
        let id = DailyReviewId(new_id(&transaction)?);
        transaction.execute(
            "INSERT INTO daily_reviews(id, tenant_id, person_id, day, summary, evidence_ids, recorded_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id.0, input.tenant_id.0, input.person_id.0, input.day, input.summary, serde_json::to_string(&input.evidence_ids)?, input.recorded_at],
        )?;
        transaction.commit()?;
        Ok(StoredReview { id })
    }

    pub fn reviews(&self, input: ReviewsInput) -> Result<Vec<ReviewRecord>> {
        require_scope(&input.tenant_id, &input.person_id)?;
        let mut statement = self.connection.prepare(
            "SELECT id, day, summary, evidence_ids, recorded_at FROM daily_reviews WHERE tenant_id = ?1 AND person_id = ?2 ORDER BY day DESC, recorded_at DESC LIMIT ?3",
        )?;
        let rows = statement.query_map(
            params![
                input.tenant_id.0,
                input.person_id.0,
                bounded_limit(input.limit)
            ],
            |row| {
                let json: String = row.get(3)?;
                let evidence_ids = serde_json::from_str(&json).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        3,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?;
                Ok(ReviewRecord {
                    id: DailyReviewId(row.get(0)?),
                    day: row.get(1)?,
                    summary: row.get(2)?,
                    evidence_ids,
                    recorded_at: row.get(4)?,
                })
            },
        )?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn rebuild_embedding<E: Embedder>(
        &mut self,
        tenant_id: TenantId,
        person_id: PersonId,
        target: EmbeddingTarget,
        embedder: &E,
    ) -> Result<StoredEmbedding> {
        require_scope(&tenant_id, &person_id)?;
        let text = self.target_text(&tenant_id, &person_id, &target)?;
        let embedding = embedder
            .embed(&text)
            .map_err(|error| Error::Invalid(format!("embedder failed: {error}")))?;
        self.upsert_embedding(EmbeddingInput {
            tenant_id,
            person_id,
            target,
            embedding,
        })
    }

    pub fn upsert_embedding(&mut self, input: EmbeddingInput) -> Result<StoredEmbedding> {
        require_scope(&input.tenant_id, &input.person_id)?;
        validate_embedding(&input.embedding)?;
        self.target_text(&input.tenant_id, &input.person_id, &input.target)?;
        let (target_kind, target_id) = embedding_target_parts(&input.target);
        let dimension = input.embedding.vector.len();
        self.connection.execute(
            "INSERT INTO embeddings(tenant_id, person_id, target_kind, target_id, model, version, dimension, input_hash, normalization, distance, vector)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(tenant_id, person_id, target_kind, target_id, model, version) DO UPDATE SET dimension = excluded.dimension, input_hash = excluded.input_hash, normalization = excluded.normalization, distance = excluded.distance, vector = excluded.vector",
            params![input.tenant_id.0, input.person_id.0, target_kind, target_id, input.embedding.model, input.embedding.version, dimension, input.embedding.input_hash, serde_json::to_string(&input.embedding.normalization)?, serde_json::to_string(&input.embedding.distance)?, serde_json::to_string(&input.embedding.vector)?],
        )?;
        Ok(StoredEmbedding {
            target: input.target,
            dimension,
        })
    }

    fn target_text(
        &self,
        tenant_id: &TenantId,
        person_id: &PersonId,
        target: &EmbeddingTarget,
    ) -> Result<String> {
        let (table, id, expression, live) = match target {
            EmbeddingTarget::Source(id) => ("sources", &id.0, "content", "deleted_at IS NULL"),
            EmbeddingTarget::Evidence(id) => ("evidence", &id.0, "quote", "deleted_at IS NULL"),
            EmbeddingTarget::Claim(id) => (
                "claims",
                &id.0,
                "subject || ' ' || predicate || ' ' || value",
                "status = 'accepted'",
            ),
        };
        self.connection
            .query_row(
                &format!("SELECT {expression} FROM {table} WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3 AND {live}"),
                params![id, tenant_id.0, person_id.0],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(Error::NotFound)
    }

    fn migrate(&mut self) -> Result<()> {
        self.connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             BEGIN IMMEDIATE;
             CREATE TABLE IF NOT EXISTS sources(id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, revision INTEGER NOT NULL, kind TEXT NOT NULL, content TEXT NOT NULL, captured_at INTEGER NOT NULL, recorded_at INTEGER NOT NULL, deleted_at INTEGER);
             CREATE INDEX IF NOT EXISTS sources_scope ON sources(tenant_id, person_id, id);
             CREATE VIRTUAL TABLE IF NOT EXISTS source_fts USING fts5(source_id UNINDEXED, tenant_id UNINDEXED, person_id UNINDEXED, content, tokenize='unicode61');
             CREATE TABLE IF NOT EXISTS evidence(id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, source_id TEXT NOT NULL REFERENCES sources(id), source_revision INTEGER NOT NULL, quote TEXT NOT NULL, recorded_at INTEGER NOT NULL, deleted_at INTEGER);
             CREATE INDEX IF NOT EXISTS evidence_scope ON evidence(tenant_id, person_id, source_id);
             CREATE TABLE IF NOT EXISTS claims(id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, subject TEXT NOT NULL, predicate TEXT NOT NULL, value TEXT NOT NULL, valid_from INTEGER NOT NULL, valid_until INTEGER, recorded_from INTEGER NOT NULL, recorded_until INTEGER, status TEXT NOT NULL);
             CREATE INDEX IF NOT EXISTS claims_scope ON claims(tenant_id, person_id, status);
             CREATE TABLE IF NOT EXISTS claim_evidence(tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, claim_id TEXT NOT NULL REFERENCES claims(id), evidence_id TEXT NOT NULL REFERENCES evidence(id), relation TEXT NOT NULL, confidence_basis_points INTEGER NOT NULL, PRIMARY KEY(tenant_id, person_id, claim_id, evidence_id));
             CREATE TABLE IF NOT EXISTS daily_reviews(id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, day TEXT NOT NULL, summary TEXT NOT NULL, evidence_ids TEXT NOT NULL, recorded_at INTEGER NOT NULL);
             CREATE INDEX IF NOT EXISTS reviews_scope ON daily_reviews(tenant_id, person_id, day);
             CREATE TABLE IF NOT EXISTS embeddings(tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, target_kind TEXT NOT NULL, target_id TEXT NOT NULL, model TEXT NOT NULL, version TEXT NOT NULL, dimension INTEGER NOT NULL, input_hash TEXT NOT NULL, normalization TEXT NOT NULL, distance TEXT NOT NULL, vector TEXT NOT NULL, PRIMARY KEY(tenant_id, person_id, target_kind, target_id, model, version));
             CREATE INDEX IF NOT EXISTS embeddings_scope ON embeddings(tenant_id, person_id, target_kind, target_id);
             PRAGMA user_version = 1;
             COMMIT;",
        )?;
        Ok(())
    }
}

fn insert_claim(
    transaction: &Transaction<'_>,
    tenant_id: &TenantId,
    person_id: &PersonId,
    evidence_id: &EvidenceId,
    claim: ClaimInput,
    recorded_at: Timestamp,
) -> Result<ClaimId> {
    require_text("claim subject", &claim.subject)?;
    require_text("claim predicate", &claim.predicate)?;
    require_text("claim value", &claim.value)?;
    let id = ClaimId(new_id(transaction)?);
    transaction.execute(
        "INSERT INTO claims(id, tenant_id, person_id, subject, predicate, value, valid_from, recorded_from, status) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'accepted')",
        params![id.0, tenant_id.0, person_id.0, claim.subject, claim.predicate, claim.value, claim.valid_from, recorded_at],
    )?;
    let relation = serde_json::to_string(&EvidenceRelation::Supports)?;
    transaction.execute(
        "INSERT INTO claim_evidence(tenant_id, person_id, claim_id, evidence_id, relation, confidence_basis_points) VALUES(?1, ?2, ?3, ?4, ?5, 10000)",
        params![tenant_id.0, person_id.0, id.0, evidence_id.0, relation],
    )?;
    Ok(id)
}

fn new_id(transaction: &Transaction<'_>) -> Result<String> {
    Ok(transaction.query_row("SELECT lower(hex(randomblob(16)))", [], |row| row.get(0))?)
}

fn require_scope(tenant_id: &TenantId, person_id: &PersonId) -> Result<()> {
    require_text("tenant_id", &tenant_id.0)?;
    require_text("person_id", &person_id.0)
}

fn require_text(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::Invalid(format!("{field} must not be empty")));
    }
    Ok(())
}

fn validate_embedding(embedding: &Embedding) -> Result<()> {
    require_text("embedding model", &embedding.model)?;
    require_text("embedding version", &embedding.version)?;
    require_text("embedding input_hash", &embedding.input_hash)?;
    if embedding.vector.is_empty() || embedding.vector.iter().any(|value| !value.is_finite()) {
        return Err(Error::Invalid(
            "embedding vector must contain finite values".to_owned(),
        ));
    }
    Ok(())
}

fn embedding_target_parts(target: &EmbeddingTarget) -> (&'static str, &str) {
    match target {
        EmbeddingTarget::Source(id) => ("source", &id.0),
        EmbeddingTarget::Evidence(id) => ("evidence", &id.0),
        EmbeddingTarget::Claim(id) => ("claim", &id.0),
    }
}

const fn default_limit() -> u32 {
    10
}
const fn bounded_limit(limit: u32) -> u32 {
    if limit == 0 {
        10
    } else if limit > 100 {
        100
    } else {
        limit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remember(tenant: &str, person: &str, value: &str) -> RememberInput {
        RememberInput {
            tenant_id: TenantId(tenant.into()),
            person_id: PersonId(person.into()),
            kind: SourceKind::Conversation,
            text: format!("Sam works at {value}"),
            captured_at: 10,
            claim: Some(ClaimInput {
                subject: "Sam".into(),
                predicate: "employer".into(),
                value: value.into(),
                valid_from: 10,
            }),
        }
    }

    #[test]
    fn lifecycle_is_scoped_cited_and_propagates_deletion() {
        let mut db = MemoryDb {
            connection: Connection::open_in_memory().unwrap(),
        };
        db.migrate().unwrap();
        let first = db.remember(remember("a", "sam", "Acme")).unwrap();
        db.remember(remember("b", "sam", "Other")).unwrap();
        let found = db
            .search(SearchInput {
                tenant_id: TenantId("a".into()),
                person_id: PersonId("sam".into()),
                query: "Acme".into(),
                limit: 5,
            })
            .unwrap();
        assert_eq!(found.items.len(), 1);
        assert_eq!(found.items[0].evidence_ids, vec![first.evidence_id.clone()]);
        let corrected = db
            .correct(CorrectInput {
                tenant_id: TenantId("a".into()),
                person_id: PersonId("sam".into()),
                claim_id: first.claim_id.unwrap(),
                text: "I moved to Beta".into(),
                value: "Beta".into(),
                occurred_at: 20,
            })
            .unwrap();
        let deleted = db
            .delete_source(DeleteInput {
                tenant_id: TenantId("a".into()),
                person_id: PersonId("sam".into()),
                source_id: corrected.source_id,
                deleted_at: 30,
            })
            .unwrap();
        assert_eq!((deleted.evidence_count, deleted.claim_count), (1, 1));
        assert!(
            db.search(SearchInput {
                tenant_id: TenantId("a".into()),
                person_id: PersonId("sam".into()),
                query: "Beta".into(),
                limit: 5
            })
            .unwrap()
            .items
            .is_empty()
        );
    }

    #[test]
    fn reviews_require_live_same_scope_evidence() {
        let mut db = MemoryDb {
            connection: Connection::open_in_memory().unwrap(),
        };
        db.migrate().unwrap();
        let remembered = db.remember(remember("a", "sam", "Acme")).unwrap();
        db.store_review(ReviewInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            day: "2026-07-21".into(),
            summary: "Worked at Acme".into(),
            evidence_ids: vec![remembered.evidence_id.clone()],
            recorded_at: 20,
        })
        .unwrap();
        assert_eq!(
            db.reviews(ReviewsInput {
                tenant_id: TenantId("a".into()),
                person_id: PersonId("sam".into()),
                limit: 1
            })
            .unwrap()
            .len(),
            1
        );
        db.delete_source(DeleteInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            source_id: remembered.source_id,
            deleted_at: 30,
        })
        .unwrap();
        assert!(
            db.reviews(ReviewsInput {
                tenant_id: TenantId("a".into()),
                person_id: PersonId("sam".into()),
                limit: 1
            })
            .unwrap()
            .is_empty()
        );
    }

    #[test]
    fn embedding_projection_is_validated_and_scoped() {
        let mut db = MemoryDb {
            connection: Connection::open_in_memory().unwrap(),
        };
        db.migrate().unwrap();
        let remembered = db.remember(remember("a", "sam", "Acme")).unwrap();
        let embedding = Embedding {
            vector: vec![0.1, 0.2],
            model: "provider/model".into(),
            version: "1".into(),
            input_hash: "sha256:abc".into(),
            normalization: VectorNormalization::L2,
            distance: VectorDistance::Cosine,
        };
        let stored = db
            .upsert_embedding(EmbeddingInput {
                tenant_id: TenantId("a".into()),
                person_id: PersonId("sam".into()),
                target: EmbeddingTarget::Evidence(remembered.evidence_id.clone()),
                embedding: embedding.clone(),
            })
            .unwrap();
        assert_eq!(stored.dimension, 2);
        assert!(matches!(
            db.upsert_embedding(EmbeddingInput {
                tenant_id: TenantId("b".into()),
                person_id: PersonId("sam".into()),
                target: EmbeddingTarget::Evidence(remembered.evidence_id),
                embedding,
            }),
            Err(Error::NotFound)
        ));
    }
}
