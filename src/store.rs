use crate::{
    ClaimEvidence, ClaimId, ClaimKind, DailyReviewId, EvidenceId, EvidenceRelation, MemoryRef,
    PersonId, ProfileEntry, ProfileEntryId, ProfileStability, RetrievalItem, RetrievalPack,
    SourceId, SourceKind, TenantId, Timestamp,
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::path::Path;

mod embeddings;
mod schema;

use embeddings::*;

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

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VectorNormalization {
    None,
    L2,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
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
    pub recorded_at: Timestamp,
    pub claim: Option<ClaimInput>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct TranscriptLocator {
    pub device_id: String,
    pub provider: String,
    pub stream_id: String,
    pub segment_id: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Debug, Deserialize)]
pub struct EvidenceLocatorInput {
    pub tenant_id: TenantId,
    pub person_id: PersonId,
    pub evidence_id: EvidenceId,
}

#[derive(Debug, Deserialize)]
pub struct RememberRequest {
    #[serde(flatten)]
    pub memory: RememberInput,
    #[serde(default)]
    pub locator: Option<TranscriptLocator>,
}

#[derive(Debug, Deserialize)]
pub struct ClaimInput {
    pub subject: String,
    pub predicate: String,
    pub value: String,
    pub kind: ClaimKind,
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
    #[serde(default)]
    pub as_of: Option<TemporalQuery>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TemporalQuery {
    pub valid_at: Timestamp,
    pub recorded_at: Timestamp,
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
    pub valid_at: Timestamp,
    pub recorded_at: Timestamp,
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileInput {
    pub tenant_id: TenantId,
    pub person_id: PersonId,
    pub stability: ProfileStability,
    pub claim_id: ClaimId,
    pub recorded_at: Timestamp,
}

#[derive(Debug, Deserialize)]
pub struct ProfilesInput {
    pub tenant_id: TenantId,
    pub person_id: PersonId,
    #[serde(default = "default_limit")]
    pub limit: u32,
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
        self.remember_with_locator(input, None)
    }

    pub fn remember_with_locator(
        &mut self,
        input: RememberInput,
        locator: Option<TranscriptLocator>,
    ) -> Result<Remembered> {
        require_scope(&input.tenant_id, &input.person_id)?;
        require_text("text", &input.text)?;
        if let Some(locator) = &locator {
            validate_transcript_locator(locator)?;
        }
        if let Some(key) = &input.ingestion_key {
            require_text("ingestion_key", key)?;
        }
        let transaction = self.connection.transaction()?;
        let source_id = SourceId(new_id(&transaction)?);
        let evidence_id = EvidenceId(new_id(&transaction)?);
        let kind = serde_json::to_string(&input.kind)?;
        let inserted = transaction.execute(
            "INSERT OR IGNORE INTO sources(id, tenant_id, person_id, ingestion_key, revision, kind, content, captured_at, recorded_at) VALUES(?1, ?2, ?3, ?4, 1, ?5, ?6, ?7, ?8)",
            params![source_id.0, input.tenant_id.0, input.person_id.0, input.ingestion_key, kind, input.text, input.captured_at, input.recorded_at],
        )?;
        if inserted == 0 {
            let replay = transaction.query_row(
                "SELECT s.id, e.id, c.id, s.kind, s.content, s.captured_at, s.recorded_at, s.deleted_at, e.deleted_at, c.subject, c.predicate, c.value, c.valid_from, c.kind FROM sources s JOIN evidence e ON e.id = s.origin_evidence_id AND e.tenant_id = s.tenant_id AND e.person_id = s.person_id LEFT JOIN claims c ON c.id = s.origin_claim_id AND c.tenant_id = s.tenant_id AND c.person_id = s.person_id WHERE s.tenant_id = ?1 AND s.person_id = ?2 AND s.ingestion_key = ?3",
                params![input.tenant_id.0, input.person_id.0, input.ingestion_key],
                |row| {
                    Ok((
                        Remembered {
                            source_id: SourceId(row.get(0)?),
                            evidence_id: EvidenceId(row.get(1)?),
                            claim_id: row.get::<_, Option<String>>(2)?.map(ClaimId),
                        },
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, Option<i64>>(7)?,
                        row.get::<_, Option<i64>>(8)?,
                        row.get::<_, Option<String>>(9)?,
                        row.get::<_, Option<String>>(10)?,
                        row.get::<_, Option<String>>(11)?,
                        row.get::<_, Option<i64>>(12)?,
                        row.get::<_, Option<String>>(13)?,
                    ))
                },
            ).optional()?.ok_or(Error::NotFound)?;
            if replay.5.is_some() || replay.6.is_some() {
                return Err(Error::NotFound);
            }
            let stored_locator = transaction
                .query_row(
                    "SELECT device_id, provider, stream_id, segment_id, start_ms, end_ms FROM evidence_locators WHERE tenant_id = ?1 AND person_id = ?2 AND evidence_id = ?3",
                    params![input.tenant_id.0, input.person_id.0, replay.0.evidence_id.0],
                    |row| {
                        Ok(TranscriptLocator {
                            device_id: row.get(0)?,
                            provider: row.get(1)?,
                            stream_id: row.get(2)?,
                            segment_id: row.get(3)?,
                            start_ms: row.get(4)?,
                            end_ms: row.get(5)?,
                        })
                    },
                )
                .optional()?;
            let stored_claim = match (&replay.7, &replay.8, &replay.9, replay.10, &replay.11) {
                (Some(subject), Some(predicate), Some(value), Some(valid_from), Some(kind)) => {
                    Some((subject, predicate, value, valid_from, kind.as_str()))
                }
                (None, None, None, None, None) => None,
                _ => return Err(Error::NotFound),
            };
            let input_claim = input.claim.as_ref().map(|claim| {
                (
                    &claim.subject,
                    &claim.predicate,
                    &claim.value,
                    claim.valid_from,
                    claim_kind_name(&claim.kind),
                )
            });
            if replay.1 != kind
                || replay.2 != input.text
                || replay.3 != input.captured_at
                || replay.4 != input.recorded_at
                || stored_claim != input_claim
                || stored_locator.as_ref() != locator.as_ref()
            {
                return Err(Error::Invalid(
                    "ingestion_key conflicts with different memory payload".to_owned(),
                ));
            }
            transaction.commit()?;
            return Ok(replay.0);
        }
        transaction.execute(
            "INSERT INTO source_fts(source_id, tenant_id, person_id, content) VALUES(?1, ?2, ?3, ?4)",
            params![source_id.0, input.tenant_id.0, input.person_id.0, input.text],
        )?;
        transaction.execute(
            "INSERT INTO evidence(id, tenant_id, person_id, source_id, source_revision, quote, recorded_at) VALUES(?1, ?2, ?3, ?4, 1, ?5, ?6)",
            params![evidence_id.0, input.tenant_id.0, input.person_id.0, source_id.0, input.text, input.recorded_at],
        )?;
        if let Some(locator) = locator {
            transaction.execute(
                "INSERT INTO evidence_locators(tenant_id, person_id, evidence_id, device_id, provider, stream_id, segment_id, start_ms, end_ms) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![input.tenant_id.0, input.person_id.0, evidence_id.0, locator.device_id, locator.provider, locator.stream_id, locator.segment_id, locator.start_ms, locator.end_ms],
            )?;
        }
        let claim_id = input
            .claim
            .map(|claim| {
                insert_claim(
                    &transaction,
                    &input.tenant_id,
                    &input.person_id,
                    &evidence_id,
                    claim,
                    input.recorded_at,
                )
            })
            .transpose()?;
        transaction.execute(
            "UPDATE sources SET origin_evidence_id = ?1, origin_claim_id = ?2 WHERE id = ?3 AND tenant_id = ?4 AND person_id = ?5",
            params![evidence_id.0, claim_id.as_ref().map(|id| &id.0), source_id.0, input.tenant_id.0, input.person_id.0],
        )?;
        transaction.commit()?;
        Ok(Remembered {
            source_id,
            evidence_id,
            claim_id,
        })
    }

    pub fn evidence_locator(
        &self,
        input: EvidenceLocatorInput,
    ) -> Result<Option<TranscriptLocator>> {
        require_scope(&input.tenant_id, &input.person_id)?;
        let locator = self
            .connection
            .query_row(
                "SELECT l.device_id, l.provider, l.stream_id, l.segment_id, l.start_ms, l.end_ms FROM evidence_locators l JOIN evidence e ON e.id = l.evidence_id AND e.tenant_id = l.tenant_id AND e.person_id = l.person_id WHERE l.tenant_id = ?1 AND l.person_id = ?2 AND l.evidence_id = ?3 AND e.deleted_at IS NULL",
                params![input.tenant_id.0, input.person_id.0, input.evidence_id.0],
                |row| {
                    Ok(TranscriptLocator {
                        device_id: row.get(0)?,
                        provider: row.get(1)?,
                        stream_id: row.get(2)?,
                        segment_id: row.get(3)?,
                        start_ms: row.get(4)?,
                        end_ms: row.get(5)?,
                    })
                },
            )
            .optional()?;
        Ok(locator)
    }

    pub fn search(&self, input: SearchInput) -> Result<RetrievalPack> {
        require_scope(&input.tenant_id, &input.person_id)?;
        require_text("query", &input.query)?;
        let limit = bounded_limit(input.limit);
        let candidate_limit = limit * 4;
        let query = format!("\"{}\"", input.query.replace('"', "\"\""));
        let lexical = self.lexical_targets(
            &input.tenant_id,
            &input.person_id,
            &query,
            candidate_limit,
            input.as_of.as_ref(),
        )?;
        let dense = input
            .query_embedding
            .as_ref()
            .filter(|_| input.as_of.is_none())
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
                input.as_of.as_ref(),
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

    fn lexical_targets(
        &self,
        tenant_id: &TenantId,
        person_id: &PersonId,
        query: &str,
        candidate_limit: u32,
        as_of: Option<&TemporalQuery>,
    ) -> Result<Vec<RetrievalTarget>> {
        let (sql, values): (&str, Vec<&dyn rusqlite::ToSql>) = match as_of {
            None => (
                "SELECT s.id, c.id
                 FROM source_fts
                 JOIN sources s ON s.id = source_fts.source_id AND s.tenant_id = source_fts.tenant_id AND s.person_id = source_fts.person_id
                 JOIN evidence e ON e.source_id = s.id AND e.tenant_id = s.tenant_id AND e.person_id = s.person_id AND e.deleted_at IS NULL
                 LEFT JOIN claim_evidence ce ON ce.evidence_id = e.id AND ce.tenant_id = e.tenant_id AND ce.person_id = e.person_id AND ce.relation = '\"supports\"'
                 LEFT JOIN claims c ON c.id = ce.claim_id AND c.tenant_id = ce.tenant_id AND c.person_id = ce.person_id AND c.status = 'accepted' AND c.valid_until IS NULL AND c.recorded_until IS NULL
                 WHERE source_fts MATCH ?1 AND source_fts.tenant_id = ?2 AND source_fts.person_id = ?3 AND s.deleted_at IS NULL
                 AND (c.id IS NOT NULL OR NOT EXISTS (
                     SELECT 1 FROM evidence live_e
                     JOIN claim_evidence live_ce ON live_ce.evidence_id = live_e.id AND live_ce.tenant_id = live_e.tenant_id AND live_ce.person_id = live_e.person_id AND live_ce.relation = '\"supports\"'
                     WHERE live_e.source_id = s.id AND live_e.tenant_id = s.tenant_id AND live_e.person_id = s.person_id AND live_e.deleted_at IS NULL
                 ))
                 ORDER BY bm25(source_fts), s.id, c.id LIMIT ?4",
                vec![&query, &tenant_id.0, &person_id.0, &candidate_limit],
            ),
            Some(as_of) => (
                "SELECT s.id, c.id
                 FROM source_fts
                 JOIN sources s ON s.id = source_fts.source_id AND s.tenant_id = source_fts.tenant_id AND s.person_id = source_fts.person_id
                 JOIN evidence e ON e.source_id = s.id AND e.tenant_id = s.tenant_id AND e.person_id = s.person_id AND e.deleted_at IS NULL AND e.recorded_at <= ?5
                 LEFT JOIN claim_evidence ce ON ce.evidence_id = e.id AND ce.tenant_id = e.tenant_id AND ce.person_id = e.person_id AND ce.relation = '\"supports\"'
                 LEFT JOIN claims c ON c.id = ce.claim_id AND c.tenant_id = ce.tenant_id AND c.person_id = ce.person_id AND c.status IN ('accepted', 'superseded') AND c.valid_from <= ?4 AND (c.valid_until IS NULL OR c.valid_until > ?4) AND c.recorded_from <= ?5 AND (c.recorded_until IS NULL OR c.recorded_until > ?5)
                 WHERE source_fts MATCH ?1 AND source_fts.tenant_id = ?2 AND source_fts.person_id = ?3 AND s.deleted_at IS NULL AND s.captured_at <= ?4 AND s.recorded_at <= ?5
                 AND (c.id IS NOT NULL OR NOT EXISTS (
                     SELECT 1 FROM evidence live_e
                     JOIN claim_evidence live_ce ON live_ce.evidence_id = live_e.id AND live_ce.tenant_id = live_e.tenant_id AND live_ce.person_id = live_e.person_id AND live_ce.relation = '\"supports\"'
                     JOIN claims live_c ON live_c.id = live_ce.claim_id AND live_c.tenant_id = live_ce.tenant_id AND live_c.person_id = live_ce.person_id
                     WHERE live_e.source_id = s.id AND live_e.tenant_id = s.tenant_id AND live_e.person_id = s.person_id AND live_e.deleted_at IS NULL AND live_e.recorded_at <= ?5 AND live_c.status IN ('accepted', 'superseded') AND live_c.valid_from <= ?4 AND (live_c.valid_until IS NULL OR live_c.valid_until > ?4) AND live_c.recorded_from <= ?5 AND (live_c.recorded_until IS NULL OR live_c.recorded_until > ?5)
                 ))
                 ORDER BY bm25(source_fts), s.id, c.id LIMIT ?6",
                vec![&query, &tenant_id.0, &person_id.0, &as_of.valid_at, &as_of.recorded_at, &candidate_limit],
            ),
        };
        let mut statement = self.connection.prepare(sql)?;
        let rows = statement.query_map(rusqlite::params_from_iter(values), |row| {
            let source_id = row.get::<_, String>(0)?;
            Ok(match row.get::<_, Option<String>>(1)? {
                Some(claim_id) => RetrievalTarget::Claim(ClaimId(claim_id)),
                None => RetrievalTarget::Source(SourceId(source_id)),
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn get(&self, input: GetInput) -> Result<RetrievalItem> {
        require_scope(&input.tenant_id, &input.person_id)?;
        let target = match input.target {
            EmbeddingTarget::Source(id) => RetrievalTarget::Source(id),
            EmbeddingTarget::Evidence(id) => RetrievalTarget::Evidence(id),
            EmbeddingTarget::Claim(id) => RetrievalTarget::Claim(id),
        };
        self.retrieval_item(&input.tenant_id, &input.person_id, target, 10_000, None)
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
                "SELECT id FROM claims WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3 AND status = 'accepted' AND valid_until IS NULL AND recorded_until IS NULL"
            }
            "evidence" => {
                "SELECT c.id FROM evidence e JOIN sources s ON s.id = e.source_id AND s.tenant_id = e.tenant_id AND s.person_id = e.person_id LEFT JOIN claim_evidence ce ON ce.evidence_id = e.id AND ce.tenant_id = e.tenant_id AND ce.person_id = e.person_id AND ce.relation = '\"supports\"' LEFT JOIN claims c ON c.id = ce.claim_id AND c.tenant_id = ce.tenant_id AND c.person_id = ce.person_id AND c.status = 'accepted' AND c.valid_until IS NULL AND c.recorded_until IS NULL WHERE e.id = ?1 AND e.tenant_id = ?2 AND e.person_id = ?3 AND e.deleted_at IS NULL AND s.deleted_at IS NULL ORDER BY c.id"
            }
            "source" => {
                "SELECT DISTINCT c.id FROM sources s JOIN evidence e ON e.source_id = s.id AND e.tenant_id = s.tenant_id AND e.person_id = s.person_id LEFT JOIN claim_evidence ce ON ce.evidence_id = e.id AND ce.tenant_id = e.tenant_id AND ce.person_id = e.person_id AND ce.relation = '\"supports\"' LEFT JOIN claims c ON c.id = ce.claim_id AND c.tenant_id = ce.tenant_id AND c.person_id = ce.person_id AND c.status = 'accepted' AND c.valid_until IS NULL AND c.recorded_until IS NULL WHERE s.id = ?1 AND s.tenant_id = ?2 AND s.person_id = ?3 AND s.deleted_at IS NULL AND e.deleted_at IS NULL ORDER BY c.id"
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
        if self.target_has_claim(tenant_id, person_id, target_kind, target_id)? {
            return Ok(Vec::new());
        }
        Ok(match target_kind {
            "source" => vec![RetrievalTarget::Source(SourceId(target_id.to_owned()))],
            "evidence" => vec![RetrievalTarget::Evidence(EvidenceId(target_id.to_owned()))],
            "claim" => Vec::new(),
            _ => unreachable!(),
        })
    }

    fn target_has_claim(
        &self,
        tenant_id: &TenantId,
        person_id: &PersonId,
        target_kind: &str,
        target_id: &str,
    ) -> Result<bool> {
        let sql = match target_kind {
            "source" => {
                "SELECT EXISTS(SELECT 1 FROM claim_evidence ce JOIN evidence e ON e.id = ce.evidence_id AND e.tenant_id = ce.tenant_id AND e.person_id = ce.person_id WHERE ce.relation = '\"supports\"' AND e.source_id = ?1 AND e.tenant_id = ?2 AND e.person_id = ?3)"
            }
            "evidence" => {
                "SELECT EXISTS(SELECT 1 FROM claim_evidence WHERE relation = '\"supports\"' AND evidence_id = ?1 AND tenant_id = ?2 AND person_id = ?3)"
            }
            "claim" => return Ok(true),
            _ => {
                return Err(Error::Invalid(
                    "stored embedding target is invalid".to_owned(),
                ));
            }
        };
        Ok(self
            .connection
            .query_row(sql, params![target_id, tenant_id.0, person_id.0], |row| {
                row.get(0)
            })?)
    }

    fn retrieval_item(
        &self,
        tenant_id: &TenantId,
        person_id: &PersonId,
        target: RetrievalTarget,
        relevance_basis_points: u16,
        as_of: Option<&TemporalQuery>,
    ) -> Result<RetrievalItem> {
        let (sql, values): (&str, Vec<&dyn rusqlite::ToSql>) = match &target {
            RetrievalTarget::Claim(id) => match as_of {
                None => (
                    "SELECT c.subject || ' ' || c.predicate || ' ' || c.value, ce.evidence_id
                 FROM claims c
                 JOIN claim_evidence ce ON ce.claim_id = c.id AND ce.tenant_id = c.tenant_id AND ce.person_id = c.person_id AND ce.relation = '\"supports\"'
                 JOIN evidence e ON e.id = ce.evidence_id AND e.tenant_id = ce.tenant_id AND e.person_id = ce.person_id
                 JOIN sources s ON s.id = e.source_id AND s.tenant_id = e.tenant_id AND s.person_id = e.person_id
                 WHERE c.id = ?1 AND c.tenant_id = ?2 AND c.person_id = ?3 AND c.status = 'accepted' AND c.valid_until IS NULL AND c.recorded_until IS NULL AND e.deleted_at IS NULL AND s.deleted_at IS NULL
                 ORDER BY ce.evidence_id LIMIT 1",
                    vec![&id.0, &tenant_id.0, &person_id.0],
                ),
                Some(as_of) => (
                    "SELECT c.subject || ' ' || c.predicate || ' ' || c.value, ce.evidence_id
                 FROM claims c
                 JOIN claim_evidence ce ON ce.claim_id = c.id AND ce.tenant_id = c.tenant_id AND ce.person_id = c.person_id AND ce.relation = '\"supports\"'
                 JOIN evidence e ON e.id = ce.evidence_id AND e.tenant_id = ce.tenant_id AND e.person_id = ce.person_id
                 JOIN sources s ON s.id = e.source_id AND s.tenant_id = e.tenant_id AND s.person_id = e.person_id
                 WHERE c.id = ?1 AND c.tenant_id = ?2 AND c.person_id = ?3 AND c.status IN ('accepted', 'superseded') AND c.valid_from <= ?4 AND (c.valid_until IS NULL OR c.valid_until > ?4) AND c.recorded_from <= ?5 AND (c.recorded_until IS NULL OR c.recorded_until > ?5) AND e.deleted_at IS NULL AND e.recorded_at <= ?5 AND s.deleted_at IS NULL AND s.captured_at <= ?4 AND s.recorded_at <= ?5
                 ORDER BY ce.evidence_id LIMIT 1",
                    vec![&id.0, &tenant_id.0, &person_id.0, &as_of.valid_at, &as_of.recorded_at],
                ),
            },
            RetrievalTarget::Source(id) => (
                "SELECT s.content, e.id FROM sources s JOIN evidence e ON e.source_id = s.id AND e.tenant_id = s.tenant_id AND e.person_id = s.person_id WHERE s.id = ?1 AND s.tenant_id = ?2 AND s.person_id = ?3 AND s.deleted_at IS NULL AND e.deleted_at IS NULL ORDER BY e.id LIMIT 1",
                vec![&id.0, &tenant_id.0, &person_id.0],
            ),
            RetrievalTarget::Evidence(id) => (
                "SELECT e.quote, e.id FROM evidence e JOIN sources s ON s.id = e.source_id AND s.tenant_id = e.tenant_id AND s.person_id = e.person_id WHERE e.id = ?1 AND e.tenant_id = ?2 AND e.person_id = ?3 AND e.deleted_at IS NULL AND s.deleted_at IS NULL",
                vec![&id.0, &tenant_id.0, &person_id.0],
            ),
        };
        let memory = match &target {
            RetrievalTarget::Claim(id) => MemoryRef::Claim(id.clone()),
            RetrievalTarget::Source(id) => MemoryRef::Source(id.clone()),
            RetrievalTarget::Evidence(id) => MemoryRef::Evidence(id.clone()),
        };
        let (excerpt, evidence_id) = self
            .connection
            .query_row(sql, rusqlite::params_from_iter(values), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .optional()?
            .ok_or(Error::NotFound)?;
        Ok(RetrievalItem {
            memory,
            excerpt: bounded_excerpt(excerpt),
            relevance_basis_points,
            evidence_ids: vec![EvidenceId(evidence_id)],
        })
    }

    pub fn correct(&mut self, input: CorrectInput) -> Result<Corrected> {
        require_scope(&input.tenant_id, &input.person_id)?;
        require_text("correction text", &input.text)?;
        require_text("value", &input.value)?;
        let transaction = self.connection.transaction()?;
        let old = transaction
            .query_row(
                "SELECT subject, predicate, kind, valid_from, recorded_from FROM claims WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3 AND status = 'accepted'",
                params![input.claim_id.0, input.tenant_id.0, input.person_id.0],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?, row.get::<_, i64>(4)?)),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        if input.valid_at <= old.3 || input.recorded_at <= old.4 {
            return Err(Error::Invalid(
                "correction timestamps must advance the original valid and recorded intervals"
                    .to_owned(),
            ));
        }
        let source_id = SourceId(new_id(&transaction)?);
        let evidence_id = EvidenceId(new_id(&transaction)?);
        transaction.execute(
            "INSERT INTO sources(id, tenant_id, person_id, revision, kind, content, captured_at, recorded_at) VALUES(?1, ?2, ?3, 1, '\"user_correction\"', ?4, ?5, ?6)",
            params![source_id.0, input.tenant_id.0, input.person_id.0, input.text, input.valid_at, input.recorded_at],
        )?;
        transaction.execute(
            "INSERT INTO source_fts(source_id, tenant_id, person_id, content) VALUES(?1, ?2, ?3, ?4)",
            params![source_id.0, input.tenant_id.0, input.person_id.0, input.text],
        )?;
        transaction.execute(
            "INSERT INTO evidence(id, tenant_id, person_id, source_id, source_revision, quote, recorded_at) VALUES(?1, ?2, ?3, ?4, 1, ?5, ?6)",
            params![evidence_id.0, input.tenant_id.0, input.person_id.0, source_id.0, input.text, input.recorded_at],
        )?;
        transaction.execute(
            "UPDATE claims SET status = 'superseded', valid_until = ?1, recorded_until = ?2 WHERE id = ?3 AND tenant_id = ?4 AND person_id = ?5",
            params![input.valid_at, input.recorded_at, input.claim_id.0, input.tenant_id.0, input.person_id.0],
        )?;
        transaction.execute(
            "DELETE FROM profile_entries WHERE tenant_id = ?1 AND person_id = ?2 AND claim_id = ?3",
            params![input.tenant_id.0, input.person_id.0, input.claim_id.0],
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
                kind: claim_kind(&old.2)?,
                valid_from: input.valid_at,
            },
            input.recorded_at,
        )?;
        transaction.execute(
            "UPDATE sources SET origin_evidence_id = ?1, origin_claim_id = ?2 WHERE id = ?3 AND tenant_id = ?4 AND person_id = ?5",
            params![evidence_id.0, claim_id.0, source_id.0, input.tenant_id.0, input.person_id.0],
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
            "UPDATE claims SET status = 'retracted', recorded_until = ?1
             WHERE tenant_id = ?2 AND person_id = ?3 AND status = 'accepted'
             AND id IN (SELECT ce.claim_id FROM claim_evidence ce JOIN evidence e ON e.id = ce.evidence_id AND e.tenant_id = ce.tenant_id AND e.person_id = ce.person_id WHERE e.source_id = ?4)
             AND NOT EXISTS (SELECT 1 FROM claim_evidence live_ce JOIN evidence live_e ON live_e.id = live_ce.evidence_id AND live_e.tenant_id = live_ce.tenant_id AND live_e.person_id = live_ce.person_id WHERE live_ce.claim_id = claims.id AND live_ce.relation = '\"supports\"' AND live_e.deleted_at IS NULL)",
            params![input.deleted_at, input.tenant_id.0, input.person_id.0, input.source_id.0],
        )? as u64;
        transaction.execute(
            "DELETE FROM embeddings WHERE tenant_id = ?1 AND person_id = ?2 AND ((target_kind = 'source' AND target_id = ?3) OR (target_kind = 'evidence' AND target_id IN (SELECT id FROM evidence WHERE source_id = ?3 AND tenant_id = ?1 AND person_id = ?2)) OR (target_kind = 'claim' AND target_id IN (SELECT c.id FROM claims c JOIN claim_evidence ce ON ce.claim_id = c.id AND ce.tenant_id = c.tenant_id AND ce.person_id = c.person_id JOIN evidence e ON e.id = ce.evidence_id AND e.tenant_id = ce.tenant_id AND e.person_id = ce.person_id WHERE c.tenant_id = ?1 AND c.person_id = ?2 AND c.status = 'retracted' AND e.source_id = ?3)))",
            params![input.tenant_id.0, input.person_id.0, input.source_id.0],
        )?;
        transaction.execute(
            "DELETE FROM profile_entries WHERE tenant_id = ?1 AND person_id = ?2 AND claim_id IN (SELECT id FROM claims WHERE tenant_id = ?1 AND person_id = ?2 AND status = 'retracted')",
            params![input.tenant_id.0, input.person_id.0],
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

    pub fn link_claim_evidence(&mut self, input: ClaimEvidence) -> Result<()> {
        input
            .validate()
            .map_err(|error| Error::Invalid(error.to_string()))?;
        require_scope(&input.tenant_id, &input.person_id)?;
        let transaction = self.connection.transaction()?;
        let claim_exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM claims WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3 AND status = 'accepted')",
            params![input.claim_id.0, input.tenant_id.0, input.person_id.0],
            |row| row.get(0),
        )?;
        let evidence_exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM evidence WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3 AND deleted_at IS NULL)",
            params![input.evidence_id.0, input.tenant_id.0, input.person_id.0],
            |row| row.get(0),
        )?;
        if !claim_exists || !evidence_exists {
            return Err(Error::NotFound);
        }
        transaction.execute(
            "INSERT INTO claim_evidence(tenant_id, person_id, claim_id, evidence_id, relation, confidence_basis_points) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![input.tenant_id.0, input.person_id.0, input.claim_id.0, input.evidence_id.0, serde_json::to_string(&input.relation)?, input.confidence_basis_points],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn store_profile(&mut self, input: ProfileInput) -> Result<ProfileEntry> {
        require_scope(&input.tenant_id, &input.person_id)?;
        let transaction = self.connection.transaction()?;
        let claim = transaction
            .query_row(
            "SELECT predicate, value, recorded_from FROM claims WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3 AND kind = 'profile_fact' AND status = 'accepted' AND valid_until IS NULL AND recorded_until IS NULL",
            params![input.claim_id.0, input.tenant_id.0, input.person_id.0],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Timestamp>(2)?)),
        )
            .optional()?;
        let Some((key, value, claim_recorded_at)) = claim else {
            return Err(Error::NotFound);
        };
        if input.recorded_at < claim_recorded_at {
            return Err(Error::Invalid(
                "profile entry cannot predate its backing claim".to_owned(),
            ));
        }
        let existing = transaction
            .query_row(
                "SELECT id, value, stability, claim_id, recorded_at FROM profile_entries WHERE tenant_id = ?1 AND person_id = ?2 AND key = ?3",
                params![input.tenant_id.0, input.person_id.0, key],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, Timestamp>(4)?)),
            )
            .optional()?;
        let stability = serde_json::to_string(&input.stability)?;
        if let Some((id, stored_value, stored_stability, stored_claim_id, stored_at)) = &existing {
            if stored_value == &value
                && stored_stability == &stability
                && stored_claim_id == &input.claim_id.0
                && *stored_at == input.recorded_at
            {
                transaction.commit()?;
                return Ok(ProfileEntry {
                    id: ProfileEntryId(id.clone()),
                    tenant_id: input.tenant_id,
                    person_id: input.person_id,
                    key,
                    value,
                    stability: input.stability,
                    claim_id: input.claim_id,
                    recorded_at: input.recorded_at,
                });
            }
            if input.recorded_at <= *stored_at {
                return Err(Error::Invalid(
                    "profile replacement must advance recorded_at".to_owned(),
                ));
            }
        }
        let profile = ProfileEntry {
            id: existing
                .as_ref()
                .map(|existing| ProfileEntryId(existing.0.clone()))
                .unwrap_or(ProfileEntryId(new_id(&transaction)?)),
            tenant_id: input.tenant_id,
            person_id: input.person_id,
            key,
            value,
            stability: input.stability,
            claim_id: input.claim_id,
            recorded_at: input.recorded_at,
        };
        transaction.execute(
            "INSERT INTO profile_entries(id, tenant_id, person_id, key, value, stability, claim_id, recorded_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) ON CONFLICT(tenant_id, person_id, key) DO UPDATE SET value = excluded.value, stability = excluded.stability, claim_id = excluded.claim_id, recorded_at = excluded.recorded_at",
            params![profile.id.0, profile.tenant_id.0, profile.person_id.0, profile.key, profile.value, stability, profile.claim_id.0, profile.recorded_at],
        )?;
        transaction.commit()?;
        Ok(profile)
    }

    pub fn profiles(&self, input: ProfilesInput) -> Result<Vec<ProfileEntry>> {
        require_scope(&input.tenant_id, &input.person_id)?;
        let mut statement = self.connection.prepare(
            "SELECT p.id, p.key, p.value, p.stability, p.claim_id, p.recorded_at FROM profile_entries p JOIN claims c ON c.id = p.claim_id AND c.tenant_id = p.tenant_id AND c.person_id = p.person_id WHERE p.tenant_id = ?1 AND p.person_id = ?2 AND c.kind = 'profile_fact' AND c.status = 'accepted' AND c.valid_until IS NULL AND c.recorded_until IS NULL ORDER BY p.recorded_at DESC, p.id LIMIT ?3",
        )?;
        let rows = statement.query_map(
            params![
                input.tenant_id.0,
                input.person_id.0,
                bounded_limit(input.limit)
            ],
            |row| {
                let stability: String = row.get(3)?;
                let stability = serde_json::from_str(&stability).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        3,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?;
                Ok(ProfileEntry {
                    id: ProfileEntryId(row.get(0)?),
                    tenant_id: input.tenant_id.clone(),
                    person_id: input.person_id.clone(),
                    key: row.get(1)?,
                    value: row.get(2)?,
                    stability,
                    claim_id: ClaimId(row.get(4)?),
                    recorded_at: row.get(5)?,
                })
            },
        )?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
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

    fn migrate(&mut self) -> Result<()> {
        schema::migrate(&mut self.connection)
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
        "INSERT INTO claims(id, tenant_id, person_id, subject, predicate, value, kind, valid_from, recorded_from, status) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'accepted')",
        params![id.0, tenant_id.0, person_id.0, claim.subject, claim.predicate, claim.value, claim_kind_name(&claim.kind), claim.valid_from, recorded_at],
    )?;
    let relation = serde_json::to_string(&EvidenceRelation::Supports)?;
    transaction.execute(
        "INSERT INTO claim_evidence(tenant_id, person_id, claim_id, evidence_id, relation, confidence_basis_points) VALUES(?1, ?2, ?3, ?4, ?5, 10000)",
        params![tenant_id.0, person_id.0, id.0, evidence_id.0, relation],
    )?;
    Ok(id)
}

fn claim_kind_name(kind: &ClaimKind) -> &'static str {
    match kind {
        ClaimKind::Fact => "fact",
        ClaimKind::ProfileFact => "profile_fact",
        ClaimKind::Preference => "preference",
        ClaimKind::Task => "task",
        ClaimKind::Skill => "skill",
        ClaimKind::Recommendation => "recommendation",
    }
}

fn claim_kind(value: &str) -> Result<ClaimKind> {
    match value {
        "fact" => Ok(ClaimKind::Fact),
        "profile_fact" => Ok(ClaimKind::ProfileFact),
        "preference" => Ok(ClaimKind::Preference),
        "task" => Ok(ClaimKind::Task),
        "skill" => Ok(ClaimKind::Skill),
        "recommendation" => Ok(ClaimKind::Recommendation),
        _ => Err(Error::Invalid("stored claim kind is invalid".to_owned())),
    }
}

fn validate_transcript_locator(locator: &TranscriptLocator) -> Result<()> {
    for (field, value) in [
        ("locator.device_id", locator.device_id.as_str()),
        ("locator.provider", locator.provider.as_str()),
        ("locator.stream_id", locator.stream_id.as_str()),
        ("locator.segment_id", locator.segment_id.as_str()),
    ] {
        require_text(field, value)?;
    }
    if locator.start_ms > locator.end_ms {
        return Err(Error::Invalid(
            "locator end_ms must not precede start_ms".to_owned(),
        ));
    }
    if locator.end_ms > i64::MAX as u64 {
        return Err(Error::Invalid(
            "locator end_ms exceeds the storage range".to_owned(),
        ));
    }
    Ok(())
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

const MAX_EXCERPT_BYTES: usize = 4096;

fn bounded_excerpt(mut excerpt: String) -> String {
    if excerpt.len() <= MAX_EXCERPT_BYTES {
        return excerpt;
    }
    let mut end = MAX_EXCERPT_BYTES;
    while !excerpt.is_char_boundary(end) {
        end -= 1;
    }
    excerpt.truncate(end);
    excerpt
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
mod tests;
