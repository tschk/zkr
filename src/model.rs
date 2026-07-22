use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! string_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
                let value = value.into();
                validate_text(stringify!($name), &value)?;
                Ok(Self(value))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

string_id!(TenantId);
string_id!(PersonId);
string_id!(SourceId);
string_id!(EvidenceId);
string_id!(ClaimId);
string_id!(ProfileEntryId);
string_id!(DailyReviewId);

pub type Timestamp = i64;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TimeRange {
    pub from: Timestamp,
    pub until: Option<Timestamp>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub id: SourceId,
    pub tenant_id: TenantId,
    pub person_id: PersonId,
    pub revision: u64,
    pub kind: SourceKind,
    pub content: String,
    pub captured_at: Timestamp,
    pub recorded_at: Timestamp,
    pub deleted_at: Option<Timestamp>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Conversation,
    Screen,
    Audio,
    Document,
    Integration,
    UserCorrection,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub id: EvidenceId,
    pub tenant_id: TenantId,
    pub person_id: PersonId,
    pub source_id: SourceId,
    pub source_revision: u64,
    pub quote: String,
    pub byte_range: Option<ByteRange>,
    pub recorded_at: Timestamp,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ByteRange {
    pub start: u64,
    pub end: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Claim {
    pub id: ClaimId,
    pub tenant_id: TenantId,
    pub person_id: PersonId,
    pub subject: String,
    pub predicate: String,
    pub value: String,
    pub kind: ClaimKind,
    pub valid_time: TimeRange,
    pub recorded_time: TimeRange,
    pub status: ClaimStatus,
    #[serde(default)]
    pub tier: MemoryTier,
    #[serde(default)]
    pub processing_state: MemoryProcessingState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimKind {
    Fact,
    ProfileFact,
    Preference,
    Task,
    Skill,
    Recommendation,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimStatus {
    Accepted,
    Superseded,
    Retracted,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryTier {
    ShortTerm,
    #[default]
    LongTerm,
    Archive,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryProcessingState {
    Pending,
    #[default]
    Processed,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ClaimEvidence {
    pub tenant_id: TenantId,
    pub person_id: PersonId,
    pub claim_id: ClaimId,
    pub evidence_id: EvidenceId,
    pub relation: EvidenceRelation,
    pub confidence_basis_points: u16,
}

impl ClaimEvidence {
    pub fn validate(&self) -> Result<(), ValidationError> {
        for (field, value) in [
            ("tenant id", &self.tenant_id.0),
            ("person id", &self.person_id.0),
            ("claim id", &self.claim_id.0),
            ("evidence id", &self.evidence_id.0),
        ] {
            validate_text(field, value)?;
        }
        if self.confidence_basis_points > 10_000 {
            return Err(ValidationError::InvalidConfidence(
                self.confidence_basis_points,
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceRelation {
    Supports,
    Contradicts,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProfileEntry {
    pub id: ProfileEntryId,
    pub tenant_id: TenantId,
    pub person_id: PersonId,
    pub key: String,
    pub value: String,
    pub stability: ProfileStability,
    pub claim_id: ClaimId,
    pub recorded_at: Timestamp,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileStability {
    Stable,
    Current,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DailyReview {
    pub id: DailyReviewId,
    pub tenant_id: TenantId,
    pub person_id: PersonId,
    pub day: String,
    pub summary: String,
    pub evidence_ids: Vec<EvidenceId>,
    pub recorded_at: Timestamp,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RetrievalPack {
    pub query: String,
    pub items: Vec<RetrievalItem>,
    pub gaps: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RetrievalItem {
    pub memory: MemoryRef,
    pub excerpt: String,
    pub relevance_basis_points: u16,
    pub evidence_ids: Vec<EvidenceId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum MemoryRef {
    Source(SourceId),
    Evidence(EvidenceId),
    Claim(ClaimId),
    ProfileEntry(ProfileEntryId),
    DailyReview(DailyReviewId),
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ValidationError {
    #[error("{0} must not be empty")]
    EmptyText(&'static str),
    #[error("confidence {0} must be at most 10000 basis points")]
    InvalidConfidence(u16),
    #[error("illegal memory state combination: tier={0}, status={1}, processing_state={2}")]
    IllegalMemoryState(String, String, String),
}

pub fn is_legal_state_combination(
    tier: &MemoryTier,
    status: &ClaimStatus,
    processing_state: &MemoryProcessingState,
) -> bool {
    if *tier == MemoryTier::Archive && *status == ClaimStatus::Superseded {
        return false;
    }
    if (*tier == MemoryTier::LongTerm || *tier == MemoryTier::Archive)
        && *processing_state != MemoryProcessingState::Processed
    {
        return false;
    }
    true
}

pub fn assert_legal_state(
    tier: &MemoryTier,
    status: &ClaimStatus,
    processing_state: &MemoryProcessingState,
) -> Result<(), ValidationError> {
    if is_legal_state_combination(tier, status, processing_state) {
        return Ok(());
    }
    Err(ValidationError::IllegalMemoryState(
        serde_json::to_string(tier)
            .unwrap_or_default()
            .trim_matches('"')
            .to_owned(),
        serde_json::to_string(status)
            .unwrap_or_default()
            .trim_matches('"')
            .to_owned(),
        serde_json::to_string(processing_state)
            .unwrap_or_default()
            .trim_matches('"')
            .to_owned(),
    ))
}

fn validate_text(field: &'static str, value: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        return Err(ValidationError::EmptyText(field));
    }
    Ok(())
}
