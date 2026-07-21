use crate::{
    Claim, ClaimEvidence, ClaimId, ClaimKind, DailyReview, DailyReviewId, Evidence, EvidenceId,
    EvidenceRelation, MemoryRef, PersonId, ProfileEntry, ProfileEntryId, ProfileStability,
    RetrievalItem, RetrievalPack, Source, SourceId, SourceKind, TenantId, Timestamp,
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::path::Path;

mod embeddings;
mod export;
mod lifecycle;
mod retrieval;
mod schema;

use embeddings::*;
#[cfg(test)]
use retrieval::MAX_EXCERPT_BYTES;

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourceRecord {
    pub source: Source,
    pub ingestion_key: Option<String>,
    pub origin_evidence_id: Option<EvidenceId>,
    pub origin_claim_id: Option<ClaimId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EvidenceRecord {
    pub evidence: Evidence,
    pub locator: Option<TranscriptLocator>,
    pub deleted_at: Option<Timestamp>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CorrectionRecord {
    pub tenant_id: TenantId,
    pub person_id: PersonId,
    pub superseded_claim_id: ClaimId,
    pub claim_id: ClaimId,
    pub source_id: SourceId,
    pub evidence_id: EvidenceId,
    pub valid_at: Timestamp,
    pub recorded_at: Timestamp,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DeletionRecord {
    pub tenant_id: TenantId,
    pub person_id: PersonId,
    pub target: MemoryRef,
    pub deleted_at: Timestamp,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "record", rename_all = "snake_case")]
pub enum ExportRecord {
    Source(SourceRecord),
    Evidence(EvidenceRecord),
    Claim(Claim),
    ClaimEvidence(ClaimEvidence),
    Correction(CorrectionRecord),
    Deletion(DeletionRecord),
    Profile(ProfileEntry),
    DailyReview(DailyReview),
}

#[derive(Debug, Deserialize)]
pub struct ExportInput {
    pub export_format: u32,
    pub tenant_id: TenantId,
    pub person_id: PersonId,
    #[serde(default)]
    pub after_commit: i64,
    #[serde(default = "default_after_event_index")]
    pub after_event_index: i64,
    #[serde(default)]
    pub high_water_mark: Option<i64>,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExportCommit {
    pub sequence: i64,
    pub recorded_at: Timestamp,
    pub event_count: i64,
    pub first_event_index: i64,
    pub records: Vec<ExportRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExportPage {
    pub export_format: u32,
    pub database_schema_version: i64,
    pub high_water_mark: i64,
    pub next_after_commit: i64,
    pub next_after_event_index: i64,
    pub complete: bool,
    pub commits: Vec<ExportCommit>,
}

pub const EXPORT_FORMAT_VERSION: u32 = 1;
pub const DATABASE_SCHEMA_VERSION: i64 = 8;
pub const MAX_EXPORT_RECORD_BYTES: usize = 1024 * 1024;
pub const MAX_EXPORT_PAGE_BYTES: usize = 1024 * 1024;

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

    fn migrate(&mut self) -> Result<()> {
        schema::migrate(&mut self.connection)
    }
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

const fn default_limit() -> u32 {
    10
}
const fn default_after_event_index() -> i64 {
    -1
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
