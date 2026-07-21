use crate::{
    ClaimId, DailyReviewId, EvidenceId, EvidenceRelation, MemoryRef, PersonId, RetrievalItem,
    RetrievalPack, SourceId, SourceKind, TenantId, Timestamp,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum RetrievalTarget {
    Source(SourceId),
    Evidence(EvidenceId),
    Claim(ClaimId),
}

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

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
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
    pub target_revision: i64,
    pub input_hash: String,
    pub created_at: Timestamp,
}

#[derive(Debug, Deserialize)]
pub struct ProjectionAuditInput {
    pub tenant_id: TenantId,
    pub person_id: PersonId,
    pub model: String,
    pub version: String,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionState {
    Missing,
    Stale,
}

#[derive(Debug, Serialize)]
pub struct ProjectionInput {
    pub target: EmbeddingTarget,
    pub text: String,
    pub target_revision: i64,
    pub input_hash: String,
}

#[derive(Debug, Serialize)]
pub struct ProjectionIssue {
    pub state: ProjectionState,
    pub input: ProjectionInput,
    pub stored_target_revision: Option<i64>,
    pub stored_input_hash: Option<String>,
    pub stored_created_at: Option<Timestamp>,
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
    #[serde(default)]
    pub ingestion_key: Option<String>,
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
    #[serde(default)]
    pub query_embedding: Option<DenseQuery>,
}

#[derive(Debug, Deserialize)]
pub struct GetInput {
    pub tenant_id: TenantId,
    pub person_id: PersonId,
    pub target: EmbeddingTarget,
}

#[derive(Debug, Deserialize)]
pub struct DenseQuery {
    pub vector: Vec<f32>,
    pub model: String,
    pub version: String,
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
        if let Some(key) = &input.ingestion_key {
            require_text("ingestion_key", key)?;
        }
        let transaction = self.connection.transaction()?;
        let source_id = SourceId(new_id(&transaction)?);
        let evidence_id = EvidenceId(new_id(&transaction)?);
        let kind = serde_json::to_string(&input.kind)?;
        let inserted = transaction.execute(
            "INSERT OR IGNORE INTO sources(id, tenant_id, person_id, ingestion_key, revision, kind, content, captured_at, recorded_at) VALUES(?1, ?2, ?3, ?4, 1, ?5, ?6, ?7, ?7)",
            params![source_id.0, input.tenant_id.0, input.person_id.0, input.ingestion_key, kind, input.text, input.captured_at],
        )?;
        if inserted == 0 {
            let remembered = transaction.query_row(
                "SELECT s.id, e.id, c.id FROM sources s JOIN evidence e ON e.source_id = s.id AND e.tenant_id = s.tenant_id AND e.person_id = s.person_id LEFT JOIN claim_evidence ce ON ce.evidence_id = e.id AND ce.tenant_id = e.tenant_id AND ce.person_id = e.person_id LEFT JOIN claims c ON c.id = ce.claim_id AND c.tenant_id = ce.tenant_id AND c.person_id = ce.person_id WHERE s.tenant_id = ?1 AND s.person_id = ?2 AND s.ingestion_key = ?3 ORDER BY e.id, c.id LIMIT 1",
                params![input.tenant_id.0, input.person_id.0, input.ingestion_key],
                |row| Ok(Remembered {
                    source_id: SourceId(row.get(0)?),
                    evidence_id: EvidenceId(row.get(1)?),
                    claim_id: row.get::<_, Option<String>>(2)?.map(ClaimId),
                }),
            )?;
            transaction.commit()?;
            return Ok(remembered);
        }
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
        let candidate_limit = limit * 4;
        let query = format!("\"{}\"", input.query.replace('"', "\"\""));
        let mut statement = self.connection.prepare(
            "SELECT s.id, c.id
             FROM source_fts
             JOIN sources s ON s.id = source_fts.source_id AND s.tenant_id = source_fts.tenant_id AND s.person_id = source_fts.person_id
             JOIN evidence e ON e.source_id = s.id AND e.tenant_id = s.tenant_id AND e.person_id = s.person_id AND e.deleted_at IS NULL
             LEFT JOIN claim_evidence ce ON ce.evidence_id = e.id AND ce.tenant_id = e.tenant_id AND ce.person_id = e.person_id
             LEFT JOIN claims c ON c.id = ce.claim_id AND c.tenant_id = ce.tenant_id AND c.person_id = ce.person_id AND c.status = 'accepted'
             WHERE source_fts MATCH ?1 AND source_fts.tenant_id = ?2 AND source_fts.person_id = ?3 AND s.deleted_at IS NULL
             AND (c.id IS NOT NULL OR NOT EXISTS (
                 SELECT 1 FROM evidence live_e
                 JOIN claim_evidence live_ce ON live_ce.evidence_id = live_e.id AND live_ce.tenant_id = live_e.tenant_id AND live_ce.person_id = live_e.person_id
                 JOIN claims live_c ON live_c.id = live_ce.claim_id AND live_c.tenant_id = live_ce.tenant_id AND live_c.person_id = live_ce.person_id
                 WHERE live_e.source_id = s.id AND live_e.tenant_id = s.tenant_id AND live_e.person_id = s.person_id AND live_e.deleted_at IS NULL AND live_c.status = 'accepted'
             ))
             ORDER BY bm25(source_fts), s.id, c.id LIMIT ?4",
        )?;
        let rows = statement.query_map(
            params![query, input.tenant_id.0, input.person_id.0, candidate_limit],
            |row| {
                let source_id = row.get::<_, String>(0)?;
                Ok(match row.get::<_, Option<String>>(1)? {
                    Some(claim_id) => RetrievalTarget::Claim(ClaimId(claim_id)),
                    None => RetrievalTarget::Source(SourceId(source_id)),
                })
            },
        )?;
        let lexical = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        let dense = input
            .query_embedding
            .as_ref()
            .map(|query| self.dense_claims(&input.tenant_id, &input.person_id, query))
            .transpose()?
            .unwrap_or_default();
        let ranked = reciprocal_rank_fusion(&lexical, &dense, limit as usize);
        let mut items = Vec::with_capacity(ranked.len());
        for (target, relevance_basis_points) in ranked {
            items.push(self.retrieval_item(
                &input.tenant_id,
                &input.person_id,
                target,
                relevance_basis_points,
            )?);
        }
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

    pub fn get(&self, input: GetInput) -> Result<RetrievalItem> {
        require_scope(&input.tenant_id, &input.person_id)?;
        let target = match input.target {
            EmbeddingTarget::Source(id) => RetrievalTarget::Source(id),
            EmbeddingTarget::Evidence(id) => RetrievalTarget::Evidence(id),
            EmbeddingTarget::Claim(id) => RetrievalTarget::Claim(id),
        };
        self.retrieval_item(&input.tenant_id, &input.person_id, target, 10_000)
    }

    fn dense_claims(
        &self,
        tenant_id: &TenantId,
        person_id: &PersonId,
        query: &DenseQuery,
    ) -> Result<Vec<RetrievalTarget>> {
        validate_dense_query(query)?;
        let mut statement = self.connection.prepare(
            "SELECT embeddings.target_kind, embeddings.target_id, embeddings.dimension, embeddings.normalization, embeddings.distance, embeddings.vector, embeddings.target_revision, embeddings.input_hash
             FROM embeddings
             WHERE embeddings.tenant_id = ?1 AND embeddings.person_id = ?2 AND embeddings.model = ?3 AND embeddings.version = ?4",
        )?;
        let rows = statement.query_map(
            params![tenant_id.0, person_id.0, query.model, query.version],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, usize>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, String>(7)?,
                ))
            },
        )?;
        let mut scores = HashMap::<RetrievalTarget, f32>::new();
        let mut lane = None;
        for row in rows {
            let (
                target_kind,
                target_id,
                dimension,
                normalization,
                distance,
                vector,
                target_revision,
                input_hash,
            ) = row?;
            let target = embedding_target(&target_kind, &target_id)?;
            let current = match self.projection_input(tenant_id, person_id, target) {
                Ok(current) => current,
                Err(Error::NotFound) => continue,
                Err(error) => return Err(error),
            };
            if current.target_revision != target_revision || current.input_hash != input_hash {
                continue;
            }
            if dimension != query.vector.len() {
                return Err(Error::Invalid(format!(
                    "query embedding dimension {} does not match stored dimension {dimension}",
                    query.vector.len()
                )));
            }
            let normalization: VectorNormalization = serde_json::from_str(&normalization)?;
            let distance: VectorDistance = serde_json::from_str(&distance)?;
            let configuration = (normalization, distance, dimension);
            if lane.as_ref().is_some_and(|lane| lane != &configuration) {
                return Err(Error::Invalid(
                    "stored embeddings mix incompatible vector configurations".to_owned(),
                ));
            }
            lane = Some(configuration.clone());
            let vector: Vec<f32> = serde_json::from_str(&vector)?;
            if vector.len() != dimension || vector.iter().any(|value| !value.is_finite()) {
                return Err(Error::Invalid("stored embedding is invalid".to_owned()));
            }
            let score = vector_score(&query.vector, &vector, &configuration.1)?;
            for target in self.retrieval_targets_for_embedding(
                tenant_id,
                person_id,
                &target_kind,
                &target_id,
            )? {
                scores
                    .entry(target)
                    .and_modify(|existing| *existing = existing.max(score))
                    .or_insert(score);
            }
        }
        let mut ranked = scores.into_iter().collect::<Vec<_>>();
        ranked.sort_by(|left, right| {
            right
                .1
                .total_cmp(&left.1)
                .then_with(|| left.0.cmp(&right.0))
        });
        Ok(ranked.into_iter().map(|(target, _)| target).collect())
    }

    fn retrieval_targets_for_embedding(
        &self,
        tenant_id: &TenantId,
        person_id: &PersonId,
        target_kind: &str,
        target_id: &str,
    ) -> Result<Vec<RetrievalTarget>> {
        let sql = match target_kind {
            "claim" => {
                "SELECT id FROM claims WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3 AND status = 'accepted'"
            }
            "evidence" => {
                "SELECT c.id FROM evidence e JOIN sources s ON s.id = e.source_id AND s.tenant_id = e.tenant_id AND s.person_id = e.person_id LEFT JOIN claim_evidence ce ON ce.evidence_id = e.id AND ce.tenant_id = e.tenant_id AND ce.person_id = e.person_id LEFT JOIN claims c ON c.id = ce.claim_id AND c.tenant_id = ce.tenant_id AND c.person_id = ce.person_id AND c.status = 'accepted' WHERE e.id = ?1 AND e.tenant_id = ?2 AND e.person_id = ?3 AND e.deleted_at IS NULL AND s.deleted_at IS NULL ORDER BY c.id"
            }
            "source" => {
                "SELECT DISTINCT c.id FROM sources s JOIN evidence e ON e.source_id = s.id AND e.tenant_id = s.tenant_id AND e.person_id = s.person_id LEFT JOIN claim_evidence ce ON ce.evidence_id = e.id AND ce.tenant_id = e.tenant_id AND ce.person_id = e.person_id LEFT JOIN claims c ON c.id = ce.claim_id AND c.tenant_id = ce.tenant_id AND c.person_id = ce.person_id AND c.status = 'accepted' WHERE s.id = ?1 AND s.tenant_id = ?2 AND s.person_id = ?3 AND s.deleted_at IS NULL AND e.deleted_at IS NULL ORDER BY c.id"
            }
            _ => {
                return Err(Error::Invalid(
                    "stored embedding target is invalid".to_owned(),
                ));
            }
        };
        let mut statement = self.connection.prepare(sql)?;
        let rows = statement.query_map(params![target_id, tenant_id.0, person_id.0], |row| {
            row.get::<_, Option<String>>(0)
        })?;
        let rows = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        if rows.is_empty() {
            return Ok(Vec::new());
        }
        let claims = rows
            .into_iter()
            .flatten()
            .map(|id| RetrievalTarget::Claim(ClaimId(id)))
            .collect::<Vec<_>>();
        if !claims.is_empty() {
            return Ok(claims);
        }
        Ok(match target_kind {
            "source" => vec![RetrievalTarget::Source(SourceId(target_id.to_owned()))],
            "evidence" => vec![RetrievalTarget::Evidence(EvidenceId(target_id.to_owned()))],
            "claim" => Vec::new(),
            _ => unreachable!(),
        })
    }

    fn retrieval_item(
        &self,
        tenant_id: &TenantId,
        person_id: &PersonId,
        target: RetrievalTarget,
        relevance_basis_points: u16,
    ) -> Result<RetrievalItem> {
        let (sql, id) = match &target {
            RetrievalTarget::Claim(id) => (
                "SELECT c.subject || ' ' || c.predicate || ' ' || c.value, ce.evidence_id
                 FROM claims c
                 JOIN claim_evidence ce ON ce.claim_id = c.id AND ce.tenant_id = c.tenant_id AND ce.person_id = c.person_id
                 JOIN evidence e ON e.id = ce.evidence_id AND e.tenant_id = ce.tenant_id AND e.person_id = ce.person_id
                 JOIN sources s ON s.id = e.source_id AND s.tenant_id = e.tenant_id AND s.person_id = e.person_id
                 WHERE c.id = ?1 AND c.tenant_id = ?2 AND c.person_id = ?3 AND c.status = 'accepted' AND e.deleted_at IS NULL AND s.deleted_at IS NULL
                 ORDER BY ce.evidence_id LIMIT 1",
                &id.0,
            ),
            RetrievalTarget::Source(id) => (
                "SELECT s.content, e.id FROM sources s JOIN evidence e ON e.source_id = s.id AND e.tenant_id = s.tenant_id AND e.person_id = s.person_id WHERE s.id = ?1 AND s.tenant_id = ?2 AND s.person_id = ?3 AND s.deleted_at IS NULL AND e.deleted_at IS NULL ORDER BY e.id LIMIT 1",
                &id.0,
            ),
            RetrievalTarget::Evidence(id) => (
                "SELECT e.quote, e.id FROM evidence e JOIN sources s ON s.id = e.source_id AND s.tenant_id = e.tenant_id AND s.person_id = e.person_id WHERE e.id = ?1 AND e.tenant_id = ?2 AND e.person_id = ?3 AND e.deleted_at IS NULL AND s.deleted_at IS NULL",
                &id.0,
            ),
        };
        let memory = match &target {
            RetrievalTarget::Claim(id) => MemoryRef::Claim(id.clone()),
            RetrievalTarget::Source(id) => MemoryRef::Source(id.clone()),
            RetrievalTarget::Evidence(id) => MemoryRef::Evidence(id.clone()),
        };
        self.connection
            .query_row(sql, params![id, tenant_id.0, person_id.0], |row| {
                Ok(RetrievalItem {
                    memory,
                    excerpt: row.get(0)?,
                    relevance_basis_points,
                    evidence_ids: vec![EvidenceId(row.get(1)?)],
                })
            })
            .optional()?
            .ok_or(Error::NotFound)
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
        let projection = self.projection_input(&tenant_id, &person_id, target.clone())?;
        let embedding = embedder
            .embed(&projection.text)
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
        let projection =
            self.projection_input(&input.tenant_id, &input.person_id, input.target.clone())?;
        if input.embedding.input_hash != projection.input_hash {
            return Err(Error::Invalid(format!(
                "embedding input_hash does not match current target input; expected {}",
                projection.input_hash
            )));
        }
        let (target_kind, target_id) = embedding_target_parts(&input.target);
        let dimension = input.embedding.vector.len();
        let created_at = self
            .connection
            .query_row("SELECT unixepoch()", [], |row| row.get(0))?;
        self.connection.execute(
            "INSERT INTO embeddings(tenant_id, person_id, target_kind, target_id, model, version, dimension, input_hash, target_revision, created_at, normalization, distance, vector)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(tenant_id, person_id, target_kind, target_id, model, version) DO UPDATE SET dimension = excluded.dimension, input_hash = excluded.input_hash, target_revision = excluded.target_revision, created_at = excluded.created_at, normalization = excluded.normalization, distance = excluded.distance, vector = excluded.vector",
            params![input.tenant_id.0, input.person_id.0, target_kind, target_id, input.embedding.model, input.embedding.version, dimension, projection.input_hash, projection.target_revision, created_at, serde_json::to_string(&input.embedding.normalization)?, serde_json::to_string(&input.embedding.distance)?, serde_json::to_string(&input.embedding.vector)?],
        )?;
        Ok(StoredEmbedding {
            target: input.target,
            dimension,
            target_revision: projection.target_revision,
            input_hash: projection.input_hash,
            created_at,
        })
    }

    pub fn projection_input(
        &self,
        tenant_id: &TenantId,
        person_id: &PersonId,
        target: EmbeddingTarget,
    ) -> Result<ProjectionInput> {
        require_scope(tenant_id, person_id)?;
        let (table, id, expression, revision, live) = match &target {
            EmbeddingTarget::Source(id) => (
                "sources",
                &id.0,
                "content",
                "revision",
                "deleted_at IS NULL",
            ),
            EmbeddingTarget::Evidence(id) => (
                "evidence",
                &id.0,
                "quote",
                "source_revision",
                "deleted_at IS NULL",
            ),
            EmbeddingTarget::Claim(id) => (
                "claims",
                &id.0,
                "subject || ' ' || predicate || ' ' || value",
                "recorded_from",
                "status = 'accepted'",
            ),
        };
        let (text, target_revision) = self
            .connection
            .query_row(
                &format!("SELECT {expression}, {revision} FROM {table} WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3 AND {live}"),
                params![id, tenant_id.0, person_id.0],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        Ok(ProjectionInput {
            target,
            input_hash: input_hash(&text),
            text,
            target_revision,
        })
    }

    pub fn projection_issues(&self, input: ProjectionAuditInput) -> Result<Vec<ProjectionIssue>> {
        require_scope(&input.tenant_id, &input.person_id)?;
        require_text("embedding model", &input.model)?;
        require_text("embedding version", &input.version)?;
        let mut statement = self.connection.prepare(
            "SELECT target_kind, target_id FROM (
                SELECT 'source' AS target_kind, id AS target_id FROM sources WHERE tenant_id = ?1 AND person_id = ?2 AND deleted_at IS NULL
                UNION ALL SELECT 'evidence', e.id FROM evidence e JOIN sources s ON s.id = e.source_id AND s.tenant_id = e.tenant_id AND s.person_id = e.person_id WHERE e.tenant_id = ?1 AND e.person_id = ?2 AND e.deleted_at IS NULL AND s.deleted_at IS NULL
                UNION ALL SELECT 'claim', c.id FROM claims c WHERE c.tenant_id = ?1 AND c.person_id = ?2 AND c.status = 'accepted'
             ) ORDER BY target_kind, target_id",
        )?;
        let targets = statement
            .query_map(params![input.tenant_id.0, input.person_id.0], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut issues = Vec::new();
        let limit = bounded_limit(input.limit) as usize;
        for (kind, id) in targets {
            let projection = self.projection_input(
                &input.tenant_id,
                &input.person_id,
                embedding_target(&kind, &id)?,
            )?;
            let stored = self
                .connection
                .query_row(
                    "SELECT target_revision, input_hash, created_at FROM embeddings WHERE tenant_id = ?1 AND person_id = ?2 AND target_kind = ?3 AND target_id = ?4 AND model = ?5 AND version = ?6",
                    params![input.tenant_id.0, input.person_id.0, kind, embedding_target_parts(&projection.target).1, input.model, input.version],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?)),
                )
                .optional()?;
            let state = match &stored {
                None => ProjectionState::Missing,
                Some((revision, hash, _))
                    if *revision != projection.target_revision
                        || hash != &projection.input_hash =>
                {
                    ProjectionState::Stale
                }
                Some(_) => continue,
            };
            issues.push(ProjectionIssue {
                state,
                input: projection,
                stored_target_revision: stored.as_ref().map(|value| value.0),
                stored_input_hash: stored.as_ref().map(|value| value.1.clone()),
                stored_created_at: stored.map(|value| value.2),
            });
            if issues.len() == limit {
                break;
            }
        }
        Ok(issues)
    }

    fn migrate(&mut self) -> Result<()> {
        self.connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             BEGIN IMMEDIATE;
             CREATE TABLE IF NOT EXISTS sources(id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, ingestion_key TEXT, revision INTEGER NOT NULL, kind TEXT NOT NULL, content TEXT NOT NULL, captured_at INTEGER NOT NULL, recorded_at INTEGER NOT NULL, deleted_at INTEGER);
             CREATE INDEX IF NOT EXISTS sources_scope ON sources(tenant_id, person_id, id);
             CREATE VIRTUAL TABLE IF NOT EXISTS source_fts USING fts5(source_id UNINDEXED, tenant_id UNINDEXED, person_id UNINDEXED, content, tokenize='unicode61');
             CREATE TABLE IF NOT EXISTS evidence(id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, source_id TEXT NOT NULL REFERENCES sources(id), source_revision INTEGER NOT NULL, quote TEXT NOT NULL, recorded_at INTEGER NOT NULL, deleted_at INTEGER);
             CREATE INDEX IF NOT EXISTS evidence_scope ON evidence(tenant_id, person_id, source_id);
             CREATE TABLE IF NOT EXISTS claims(id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, subject TEXT NOT NULL, predicate TEXT NOT NULL, value TEXT NOT NULL, valid_from INTEGER NOT NULL, valid_until INTEGER, recorded_from INTEGER NOT NULL, recorded_until INTEGER, status TEXT NOT NULL);
             CREATE INDEX IF NOT EXISTS claims_scope ON claims(tenant_id, person_id, status);
             CREATE TABLE IF NOT EXISTS claim_evidence(tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, claim_id TEXT NOT NULL REFERENCES claims(id), evidence_id TEXT NOT NULL REFERENCES evidence(id), relation TEXT NOT NULL, confidence_basis_points INTEGER NOT NULL, PRIMARY KEY(tenant_id, person_id, claim_id, evidence_id));
             CREATE TABLE IF NOT EXISTS daily_reviews(id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, day TEXT NOT NULL, summary TEXT NOT NULL, evidence_ids TEXT NOT NULL, recorded_at INTEGER NOT NULL);
             CREATE INDEX IF NOT EXISTS reviews_scope ON daily_reviews(tenant_id, person_id, day);
             CREATE TABLE IF NOT EXISTS embeddings(tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, target_kind TEXT NOT NULL, target_id TEXT NOT NULL, model TEXT NOT NULL, version TEXT NOT NULL, dimension INTEGER NOT NULL, input_hash TEXT NOT NULL, target_revision INTEGER NOT NULL, created_at INTEGER NOT NULL, normalization TEXT NOT NULL, distance TEXT NOT NULL, vector TEXT NOT NULL, PRIMARY KEY(tenant_id, person_id, target_kind, target_id, model, version));
             CREATE INDEX IF NOT EXISTS embeddings_scope ON embeddings(tenant_id, person_id, target_kind, target_id);
             PRAGMA user_version = 1;
             COMMIT;",
        )?;
        for (column, definition) in [
            ("target_revision", "INTEGER NOT NULL DEFAULT 0"),
            ("created_at", "INTEGER NOT NULL DEFAULT 0"),
        ] {
            let exists: bool = self.connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('embeddings') WHERE name = ?1)",
                [column],
                |row| row.get(0),
            )?;
            if !exists {
                self.connection.execute(
                    &format!("ALTER TABLE embeddings ADD COLUMN {column} {definition}"),
                    [],
                )?;
            }
        }
        let has_ingestion_key: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('sources') WHERE name = 'ingestion_key')",
            [],
            |row| row.get(0),
        )?;
        if !has_ingestion_key {
            self.connection
                .execute("ALTER TABLE sources ADD COLUMN ingestion_key TEXT", [])?;
        }
        self.connection.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS sources_ingestion_key ON sources(tenant_id, person_id, ingestion_key) WHERE ingestion_key IS NOT NULL",
            [],
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

fn input_hash(text: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(text.as_bytes()))
}

fn validate_dense_query(query: &DenseQuery) -> Result<()> {
    require_text("query embedding model", &query.model)?;
    require_text("query embedding version", &query.version)?;
    if query.vector.is_empty() || query.vector.iter().any(|value| !value.is_finite()) {
        return Err(Error::Invalid(
            "query embedding vector must contain finite values".to_owned(),
        ));
    }
    Ok(())
}

fn vector_score(query: &[f32], stored: &[f32], distance: &VectorDistance) -> Result<f32> {
    let dot = query
        .iter()
        .zip(stored)
        .map(|(left, right)| left * right)
        .sum::<f32>();
    let score = match distance {
        VectorDistance::Dot => dot,
        VectorDistance::Euclidean => -query
            .iter()
            .zip(stored)
            .map(|(left, right)| (left - right).powi(2))
            .sum::<f32>()
            .sqrt(),
        VectorDistance::Cosine => {
            let query_norm = query.iter().map(|value| value * value).sum::<f32>().sqrt();
            let stored_norm = stored.iter().map(|value| value * value).sum::<f32>().sqrt();
            if query_norm == 0.0 || stored_norm == 0.0 {
                return Err(Error::Invalid(
                    "cosine embeddings must have non-zero magnitude".to_owned(),
                ));
            }
            dot / (query_norm * stored_norm)
        }
    };
    if !score.is_finite() {
        return Err(Error::Invalid(
            "embedding similarity is not finite".to_owned(),
        ));
    }
    Ok(score)
}

fn reciprocal_rank_fusion(
    lexical: &[RetrievalTarget],
    dense: &[RetrievalTarget],
    limit: usize,
) -> Vec<(RetrievalTarget, u16)> {
    let mut scores = HashMap::<RetrievalTarget, u32>::new();
    for ranking in [lexical, dense] {
        let mut seen = HashSet::new();
        for (offset, id) in ranking.iter().enumerate() {
            if seen.insert(id) {
                *scores.entry(id.clone()).or_default() += 1_000_000 / (61 + offset as u32);
            }
        }
    }
    let maximum = scores.values().copied().max().unwrap_or(1);
    let mut ranked = scores
        .into_iter()
        .map(|(id, score)| (id, ((score as u64 * 10_000) / maximum as u64) as u16))
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    ranked.truncate(limit);
    ranked
}

fn embedding_target_parts(target: &EmbeddingTarget) -> (&'static str, &str) {
    match target {
        EmbeddingTarget::Source(id) => ("source", &id.0),
        EmbeddingTarget::Evidence(id) => ("evidence", &id.0),
        EmbeddingTarget::Claim(id) => ("claim", &id.0),
    }
}

fn embedding_target(kind: &str, id: &str) -> Result<EmbeddingTarget> {
    Ok(match kind {
        "source" => EmbeddingTarget::Source(SourceId(id.to_owned())),
        "evidence" => EmbeddingTarget::Evidence(EvidenceId(id.to_owned())),
        "claim" => EmbeddingTarget::Claim(ClaimId(id.to_owned())),
        _ => {
            return Err(Error::Invalid(
                "stored embedding target is invalid".to_owned(),
            ));
        }
    })
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
            ingestion_key: None,
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

    fn remember_raw(tenant: &str, person: &str, text: &str) -> RememberInput {
        RememberInput {
            tenant_id: TenantId(tenant.into()),
            person_id: PersonId(person.into()),
            ingestion_key: None,
            kind: SourceKind::Conversation,
            text: text.into(),
            captured_at: 10,
            claim: None,
        }
    }

    fn hash_for(db: &MemoryDb, target: EmbeddingTarget) -> String {
        db.projection_input(&TenantId("a".into()), &PersonId("sam".into()), target)
            .unwrap()
            .input_hash
    }

    #[test]
    fn ingestion_keys_are_idempotent_within_scope() {
        let mut db = MemoryDb {
            connection: Connection::open_in_memory().unwrap(),
        };
        db.migrate().unwrap();
        let mut first = remember_raw("a", "sam", "Remember once");
        first.ingestion_key = Some("turn-1".into());
        let stored = db.remember(first).unwrap();
        let mut replay = remember_raw("a", "sam", "Changed replay content");
        replay.ingestion_key = Some("turn-1".into());
        let replayed = db.remember(replay).unwrap();
        assert_eq!(stored.source_id, replayed.source_id);
        assert_eq!(stored.evidence_id, replayed.evidence_id);
        assert_eq!(
            db.connection
                .query_row("SELECT count(*) FROM sources", [], |row| row
                    .get::<_, u64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn raw_sources_are_scoped_cited_and_deleted_from_retrieval() {
        let mut db = MemoryDb {
            connection: Connection::open_in_memory().unwrap(),
        };
        db.migrate().unwrap();
        let raw = db
            .remember(remember_raw("a", "sam", "The launch code is marigold"))
            .unwrap();
        db.remember(remember_raw("b", "sam", "Marigold belongs elsewhere"))
            .unwrap();

        let found = db
            .search(SearchInput {
                tenant_id: TenantId("a".into()),
                person_id: PersonId("sam".into()),
                query: "marigold".into(),
                limit: 5,
                query_embedding: None,
            })
            .unwrap();
        assert_eq!(found.items.len(), 1);
        assert_eq!(
            found.items[0].memory,
            MemoryRef::Source(raw.source_id.clone())
        );
        assert_eq!(found.items[0].evidence_ids, vec![raw.evidence_id]);

        db.delete_source(DeleteInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            source_id: raw.source_id.clone(),
            deleted_at: 20,
        })
        .unwrap();
        assert!(matches!(
            db.get(GetInput {
                tenant_id: TenantId("a".into()),
                person_id: PersonId("sam".into()),
                target: EmbeddingTarget::Source(raw.source_id),
            }),
            Err(Error::NotFound)
        ));
        assert!(
            db.search(SearchInput {
                tenant_id: TenantId("a".into()),
                person_id: PersonId("sam".into()),
                query: "marigold".into(),
                limit: 5,
                query_embedding: None,
            })
            .unwrap()
            .items
            .is_empty()
        );
    }

    #[test]
    fn accepted_claim_replaces_its_source_in_retrieval() {
        let mut db = MemoryDb {
            connection: Connection::open_in_memory().unwrap(),
        };
        db.migrate().unwrap();
        let remembered = db.remember(remember("a", "sam", "Acme")).unwrap();
        let target = EmbeddingTarget::Source(remembered.source_id);
        let input_hash = hash_for(&db, target.clone());
        db.upsert_embedding(EmbeddingInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            target,
            embedding: Embedding {
                vector: vec![1.0, 0.0],
                model: "test/model".into(),
                version: "1".into(),
                input_hash,
                normalization: VectorNormalization::L2,
                distance: VectorDistance::Cosine,
            },
        })
        .unwrap();
        let found = db
            .search(SearchInput {
                tenant_id: TenantId("a".into()),
                person_id: PersonId("sam".into()),
                query: "Acme".into(),
                limit: 5,
                query_embedding: Some(DenseQuery {
                    vector: vec![1.0, 0.0],
                    model: "test/model".into(),
                    version: "1".into(),
                }),
            })
            .unwrap();
        assert_eq!(found.items.len(), 1);
        assert_eq!(
            found.items[0].memory,
            MemoryRef::Claim(remembered.claim_id.unwrap())
        );
    }

    #[test]
    fn dense_evidence_without_a_claim_is_retrievable() {
        let mut db = MemoryDb {
            connection: Connection::open_in_memory().unwrap(),
        };
        db.migrate().unwrap();
        let raw = db
            .remember(remember_raw("a", "sam", "Quiet desk near a window"))
            .unwrap();
        let target = EmbeddingTarget::Evidence(raw.evidence_id.clone());
        let input_hash = hash_for(&db, target.clone());
        db.upsert_embedding(EmbeddingInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            target,
            embedding: Embedding {
                vector: vec![1.0, 0.0],
                model: "test/model".into(),
                version: "1".into(),
                input_hash,
                normalization: VectorNormalization::L2,
                distance: VectorDistance::Cosine,
            },
        })
        .unwrap();
        let found = db
            .search(SearchInput {
                tenant_id: TenantId("a".into()),
                person_id: PersonId("sam".into()),
                query: "unmatched lexical phrase".into(),
                limit: 5,
                query_embedding: Some(DenseQuery {
                    vector: vec![1.0, 0.0],
                    model: "test/model".into(),
                    version: "1".into(),
                }),
            })
            .unwrap();
        assert_eq!(found.items.len(), 1);
        assert_eq!(found.items[0].memory, MemoryRef::Evidence(raw.evidence_id));

        db.delete_source(DeleteInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            source_id: raw.source_id,
            deleted_at: 20,
        })
        .unwrap();
        assert!(
            db.search(SearchInput {
                tenant_id: TenantId("a".into()),
                person_id: PersonId("sam".into()),
                query: "unmatched lexical phrase".into(),
                limit: 5,
                query_embedding: Some(DenseQuery {
                    vector: vec![1.0, 0.0],
                    model: "test/model".into(),
                    version: "1".into(),
                }),
            })
            .unwrap()
            .items
            .is_empty()
        );
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
                query_embedding: None,
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
                limit: 5,
                query_embedding: None,
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
        let target = EmbeddingTarget::Evidence(remembered.evidence_id.clone());
        let embedding = Embedding {
            vector: vec![0.1, 0.2],
            model: "provider/model".into(),
            version: "1".into(),
            input_hash: hash_for(&db, target.clone()),
            normalization: VectorNormalization::L2,
            distance: VectorDistance::Cosine,
        };
        let stored = db
            .upsert_embedding(EmbeddingInput {
                tenant_id: TenantId("a".into()),
                person_id: PersonId("sam".into()),
                target,
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

    #[test]
    fn search_fuses_lexical_and_real_dense_ranks_deterministically() {
        let mut db = MemoryDb {
            connection: Connection::open_in_memory().unwrap(),
        };
        db.migrate().unwrap();
        let lexical = db.remember(remember("a", "sam", "Jazz Club")).unwrap();
        let dense = db.remember(remember("a", "sam", "Music Venue")).unwrap();
        for (claim_id, vector) in [
            (lexical.claim_id.clone().unwrap(), vec![0.9, 0.1]),
            (dense.claim_id.unwrap(), vec![1.0, 0.0]),
        ] {
            let target = EmbeddingTarget::Claim(claim_id);
            let input_hash = hash_for(&db, target.clone());
            db.upsert_embedding(EmbeddingInput {
                tenant_id: TenantId("a".into()),
                person_id: PersonId("sam".into()),
                target,
                embedding: Embedding {
                    vector,
                    model: "test/model".into(),
                    version: "1".into(),
                    input_hash,
                    normalization: VectorNormalization::L2,
                    distance: VectorDistance::Cosine,
                },
            })
            .unwrap();
        }

        let found = db
            .search(SearchInput {
                tenant_id: TenantId("a".into()),
                person_id: PersonId("sam".into()),
                query: "Jazz Club".into(),
                limit: 2,
                query_embedding: Some(DenseQuery {
                    vector: vec![1.0, 0.0],
                    model: "test/model".into(),
                    version: "1".into(),
                }),
            })
            .unwrap();

        assert_eq!(
            found.items[0].memory,
            MemoryRef::Claim(lexical.claim_id.unwrap())
        );
        assert_eq!(found.items.len(), 2);
        assert!(!found.items[0].evidence_ids.is_empty());
    }

    #[test]
    fn stale_projections_are_excluded_and_reported_with_current_inputs() {
        let mut db = MemoryDb {
            connection: Connection::open_in_memory().unwrap(),
        };
        db.migrate().unwrap();
        let raw = db
            .remember(remember_raw("a", "sam", "A quiet desk"))
            .unwrap();
        let claimed = db.remember(remember("a", "sam", "Acme")).unwrap();
        let other = db.remember(remember("b", "sam", "Other")).unwrap();
        let targets = [
            EmbeddingTarget::Source(raw.source_id.clone()),
            EmbeddingTarget::Evidence(raw.evidence_id.clone()),
            EmbeddingTarget::Claim(claimed.claim_id.clone().unwrap()),
        ];
        for target in &targets {
            db.upsert_embedding(EmbeddingInput {
                tenant_id: TenantId("a".into()),
                person_id: PersonId("sam".into()),
                target: target.clone(),
                embedding: Embedding {
                    vector: vec![1.0, 0.0],
                    model: "test/model".into(),
                    version: "1".into(),
                    input_hash: hash_for(&db, target.clone()),
                    normalization: VectorNormalization::L2,
                    distance: VectorDistance::Cosine,
                },
            })
            .unwrap();
        }
        db.connection
            .execute(
                "UPDATE sources SET content = 'A changed desk', revision = revision + 1 WHERE id = ?1",
                [&raw.source_id.0],
            )
            .unwrap();
        db.connection
            .execute(
                "UPDATE evidence SET quote = 'Changed evidence' WHERE id = ?1",
                [&raw.evidence_id.0],
            )
            .unwrap();
        db.connection
            .execute(
                "UPDATE claims SET value = 'Changed employer' WHERE id = ?1",
                [&claimed.claim_id.as_ref().unwrap().0],
            )
            .unwrap();

        let found = db
            .search(SearchInput {
                tenant_id: TenantId("a".into()),
                person_id: PersonId("sam".into()),
                query: "no lexical match".into(),
                limit: 10,
                query_embedding: Some(DenseQuery {
                    vector: vec![1.0, 0.0],
                    model: "test/model".into(),
                    version: "1".into(),
                }),
            })
            .unwrap();
        assert!(found.items.is_empty());

        let issues = db
            .projection_issues(ProjectionAuditInput {
                tenant_id: TenantId("a".into()),
                person_id: PersonId("sam".into()),
                model: "test/model".into(),
                version: "1".into(),
                limit: 100,
            })
            .unwrap();
        let stale = issues
            .iter()
            .filter(|issue| issue.state == ProjectionState::Stale)
            .map(|issue| &issue.input.target)
            .collect::<HashSet<_>>();
        assert_eq!(stale, targets.iter().collect::<HashSet<_>>());
        assert!(issues.iter().all(|issue| {
            issue.input.target != EmbeddingTarget::Source(other.source_id.clone())
                && issue.input.target != EmbeddingTarget::Evidence(other.evidence_id.clone())
                && issue.input.target != EmbeddingTarget::Claim(other.claim_id.clone().unwrap())
        }));
        assert_eq!(
            db.projection_issues(ProjectionAuditInput {
                tenant_id: TenantId("a".into()),
                person_id: PersonId("sam".into()),
                model: "test/model".into(),
                version: "1".into(),
                limit: 2,
            })
            .unwrap()
            .len(),
            2
        );
    }

    #[test]
    fn embedding_rejects_input_hash_for_different_text() {
        let mut db = MemoryDb {
            connection: Connection::open_in_memory().unwrap(),
        };
        db.migrate().unwrap();
        let remembered = db
            .remember(remember_raw("a", "sam", "Current text"))
            .unwrap();
        let result = db.upsert_embedding(EmbeddingInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            target: EmbeddingTarget::Source(remembered.source_id),
            embedding: Embedding {
                vector: vec![1.0],
                model: "test/model".into(),
                version: "1".into(),
                input_hash: input_hash("different text"),
                normalization: VectorNormalization::L2,
                distance: VectorDistance::Cosine,
            },
        });
        assert!(matches!(result, Err(Error::Invalid(_))));
    }

    #[test]
    fn migration_marks_existing_projections_for_lifecycle_revalidation() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE embeddings(tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, target_kind TEXT NOT NULL, target_id TEXT NOT NULL, model TEXT NOT NULL, version TEXT NOT NULL, dimension INTEGER NOT NULL, input_hash TEXT NOT NULL, normalization TEXT NOT NULL, distance TEXT NOT NULL, vector TEXT NOT NULL, PRIMARY KEY(tenant_id, person_id, target_kind, target_id, model, version));
                 INSERT INTO embeddings VALUES('a', 'sam', 'source', 'old', 'model', '1', 1, 'sha256:old', '\"l2\"', '\"cosine\"', '[1.0]');",
            )
            .unwrap();
        let mut db = MemoryDb { connection };
        db.migrate().unwrap();
        let lifecycle = db
            .connection
            .query_row(
                "SELECT target_revision, created_at FROM embeddings WHERE target_id = 'old'",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .unwrap();
        assert_eq!(lifecycle, (0, 0));
    }
}
