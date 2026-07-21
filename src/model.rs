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

impl TimeRange {
    pub fn new(from: Timestamp, until: Option<Timestamp>) -> Result<Self, ValidationError> {
        if until.is_some_and(|until| until <= from) {
            return Err(ValidationError::InvalidTimeRange { from, until });
        }
        Ok(Self { from, until })
    }

    pub fn contains(&self, timestamp: Timestamp) -> bool {
        timestamp >= self.from && self.until.is_none_or(|until| timestamp < until)
    }
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

impl Source {
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_ids([
            ("source id", &self.id.0),
            ("tenant id", &self.tenant_id.0),
            ("person id", &self.person_id.0),
        ])?;
        if self.revision == 0 {
            return Err(ValidationError::ZeroRevision);
        }
        validate_text("source content", &self.content)?;
        if self
            .deleted_at
            .is_some_and(|deleted| deleted < self.recorded_at)
        {
            return Err(ValidationError::DeletionBeforeRecording);
        }
        Ok(())
    }

    pub fn tombstone(&self, deleted_at: Timestamp) -> Result<Self, ValidationError> {
        if deleted_at < self.recorded_at {
            return Err(ValidationError::DeletionBeforeRecording);
        }
        Ok(Self {
            revision: self
                .revision
                .checked_add(1)
                .ok_or(ValidationError::RevisionOverflow)?,
            recorded_at: deleted_at,
            deleted_at: Some(deleted_at),
            ..self.clone()
        })
    }
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

impl Evidence {
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_ids([
            ("evidence id", &self.id.0),
            ("tenant id", &self.tenant_id.0),
            ("person id", &self.person_id.0),
            ("source id", &self.source_id.0),
        ])?;
        if self.source_revision == 0 {
            return Err(ValidationError::ZeroRevision);
        }
        validate_text("evidence quote", &self.quote)?;
        if let Some(range) = &self.byte_range {
            range.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ByteRange {
    pub start: u64,
    pub end: u64,
}

impl ByteRange {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.start >= self.end {
            return Err(ValidationError::InvalidByteRange {
                start: self.start,
                end: self.end,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Claim {
    pub id: ClaimId,
    pub tenant_id: TenantId,
    pub person_id: PersonId,
    pub subject: String,
    pub predicate: String,
    pub value: String,
    pub valid_time: TimeRange,
    pub recorded_time: TimeRange,
    pub status: ClaimStatus,
}

impl Claim {
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_ids([
            ("claim id", &self.id.0),
            ("tenant id", &self.tenant_id.0),
            ("person id", &self.person_id.0),
        ])?;
        validate_text("claim subject", &self.subject)?;
        validate_text("claim predicate", &self.predicate)?;
        validate_text("claim value", &self.value)?;
        TimeRange::new(self.valid_time.from, self.valid_time.until)?;
        TimeRange::new(self.recorded_time.from, self.recorded_time.until)?;
        Ok(())
    }

    pub fn accept(&mut self) -> Result<(), ValidationError> {
        if self.status != ClaimStatus::Proposed {
            return Err(ValidationError::InvalidClaimTransition {
                from: self.status.clone(),
                to: ClaimStatus::Accepted,
            });
        }
        self.status = ClaimStatus::Accepted;
        Ok(())
    }

    pub fn supersede(&mut self, at: Timestamp) -> Result<(), ValidationError> {
        if self.status != ClaimStatus::Accepted {
            return Err(ValidationError::InvalidClaimTransition {
                from: self.status.clone(),
                to: ClaimStatus::Superseded,
            });
        }
        self.recorded_time = TimeRange::new(self.recorded_time.from, Some(at))?;
        self.status = ClaimStatus::Superseded;
        Ok(())
    }

    pub fn reject(&mut self) -> Result<(), ValidationError> {
        if self.status != ClaimStatus::Proposed {
            return Err(ValidationError::InvalidClaimTransition {
                from: self.status.clone(),
                to: ClaimStatus::Rejected,
            });
        }
        self.status = ClaimStatus::Rejected;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimStatus {
    Proposed,
    Accepted,
    Superseded,
    Rejected,
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
        validate_ids([
            ("tenant id", &self.tenant_id.0),
            ("person id", &self.person_id.0),
            ("claim id", &self.claim_id.0),
            ("evidence id", &self.evidence_id.0),
        ])?;
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

impl ProfileEntry {
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_ids([
            ("profile entry id", &self.id.0),
            ("tenant id", &self.tenant_id.0),
            ("person id", &self.person_id.0),
            ("claim id", &self.claim_id.0),
        ])?;
        validate_text("profile key", &self.key)?;
        validate_text("profile value", &self.value)
    }
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

impl DailyReview {
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_ids([
            ("daily review id", &self.id.0),
            ("tenant id", &self.tenant_id.0),
            ("person id", &self.person_id.0),
        ])?;
        validate_text("daily review day", &self.day)?;
        validate_text("daily review summary", &self.summary)?;
        if self.evidence_ids.is_empty() {
            return Err(ValidationError::MissingEvidence);
        }
        for id in &self.evidence_ids {
            validate_text("evidence id", &id.0)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RetrievalPack {
    pub query: String,
    pub items: Vec<RetrievalItem>,
    pub gaps: Vec<String>,
}

impl RetrievalPack {
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_text("retrieval query", &self.query)?;
        for item in &self.items {
            item.validate()?;
        }
        if self.gaps.iter().any(|gap| gap.trim().is_empty()) {
            return Err(ValidationError::EmptyText("retrieval gap"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RetrievalItem {
    pub memory: MemoryRef,
    pub excerpt: String,
    pub relevance_basis_points: u16,
    pub evidence_ids: Vec<EvidenceId>,
}

impl RetrievalItem {
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.memory.validate()?;
        validate_text("retrieval excerpt", &self.excerpt)?;
        if self.relevance_basis_points > 10_000 {
            return Err(ValidationError::InvalidRelevance(
                self.relevance_basis_points,
            ));
        }
        if self.evidence_ids.is_empty() {
            return Err(ValidationError::MissingEvidence);
        }
        for id in &self.evidence_ids {
            validate_text("evidence id", &id.0)?;
        }
        Ok(())
    }
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

impl MemoryRef {
    fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Source(id) => validate_text("source id", &id.0),
            Self::Evidence(id) => validate_text("evidence id", &id.0),
            Self::Claim(id) => validate_text("claim id", &id.0),
            Self::ProfileEntry(id) => validate_text("profile entry id", &id.0),
            Self::DailyReview(id) => validate_text("daily review id", &id.0),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ValidationError {
    #[error("{0} must not be empty")]
    EmptyText(&'static str),
    #[error("revision must be greater than zero")]
    ZeroRevision,
    #[error("source revision overflowed")]
    RevisionOverflow,
    #[error("time range ending at {until:?} must end after {from}")]
    InvalidTimeRange {
        from: Timestamp,
        until: Option<Timestamp>,
    },
    #[error("byte range {start}..{end} must not be empty or reversed")]
    InvalidByteRange { start: u64, end: u64 },
    #[error("source cannot be deleted before it was recorded")]
    DeletionBeforeRecording,
    #[error("confidence {0} must be at most 10000 basis points")]
    InvalidConfidence(u16),
    #[error("relevance {0} must be at most 10000 basis points")]
    InvalidRelevance(u16),
    #[error("record requires at least one evidence citation")]
    MissingEvidence,
    #[error("claim cannot transition from {from:?} to {to:?}")]
    InvalidClaimTransition { from: ClaimStatus, to: ClaimStatus },
}

fn validate_text(field: &'static str, value: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        return Err(ValidationError::EmptyText(field));
    }
    Ok(())
}

fn validate_ids<const N: usize>(ids: [(&'static str, &str); N]) -> Result<(), ValidationError> {
    for (field, value) in ids {
        validate_text(field, value)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim(status: ClaimStatus) -> Claim {
        Claim {
            id: ClaimId("claim-1".into()),
            tenant_id: TenantId("tenant-1".into()),
            person_id: PersonId("person-1".into()),
            subject: "person-1".into(),
            predicate: "employer".into(),
            value: "Example Corp".into(),
            valid_time: TimeRange::new(100, None).expect("valid time"),
            recorded_time: TimeRange::new(110, None).expect("recorded time"),
            status,
        }
    }

    #[test]
    fn time_ranges_are_half_open() {
        let range = TimeRange::new(10, Some(20)).expect("valid range");
        assert!(range.contains(10));
        assert!(range.contains(19));
        assert!(!range.contains(20));
        assert!(TimeRange::new(10, Some(10)).is_err());
    }

    #[test]
    fn accepted_claim_can_be_superseded_without_losing_history() {
        let mut old = claim(ClaimStatus::Proposed);
        old.accept().expect("proposed claim can be accepted");
        old.supersede(200)
            .expect("accepted claim can be superseded");

        assert_eq!(old.status, ClaimStatus::Superseded);
        assert_eq!(old.recorded_time.until, Some(200));
        assert!(old.recorded_time.contains(199));
        assert!(!old.recorded_time.contains(200));
    }

    #[test]
    fn invalid_claim_transitions_are_rejected() {
        let mut accepted = claim(ClaimStatus::Accepted);
        assert!(matches!(
            accepted.reject(),
            Err(ValidationError::InvalidClaimTransition { .. })
        ));

        let mut proposed = claim(ClaimStatus::Proposed);
        assert!(matches!(
            proposed.supersede(200),
            Err(ValidationError::InvalidClaimTransition { .. })
        ));
    }

    #[test]
    fn evidence_backed_outputs_require_citations() {
        let review = DailyReview {
            id: DailyReviewId("review-1".into()),
            tenant_id: TenantId("tenant-1".into()),
            person_id: PersonId("person-1".into()),
            day: "2026-07-21".into(),
            summary: "Finished the memory core.".into(),
            evidence_ids: Vec::new(),
            recorded_at: 100,
        };
        assert_eq!(review.validate(), Err(ValidationError::MissingEvidence));

        let item = RetrievalItem {
            memory: MemoryRef::Claim(ClaimId("claim-1".into())),
            excerpt: "Works at Example Corp".into(),
            relevance_basis_points: 9_000,
            evidence_ids: Vec::new(),
        };
        assert_eq!(item.validate(), Err(ValidationError::MissingEvidence));
    }

    #[test]
    fn source_tombstone_cannot_predate_recording() {
        let source = Source {
            id: SourceId("source-1".into()),
            tenant_id: TenantId("tenant-1".into()),
            person_id: PersonId("person-1".into()),
            revision: 1,
            kind: SourceKind::Conversation,
            content: "I joined Example Corp.".into(),
            captured_at: 90,
            recorded_at: 100,
            deleted_at: None,
        };

        assert_eq!(
            source.tombstone(99),
            Err(ValidationError::DeletionBeforeRecording)
        );
        let tombstone = source.tombstone(101).expect("valid deletion timestamp");
        assert_eq!(source.revision, 1);
        assert_eq!(source.deleted_at, None);
        assert_eq!(tombstone.revision, 2);
        assert_eq!(tombstone.deleted_at, Some(101));
    }

    #[test]
    fn serde_keeps_memory_reference_kind_explicit() {
        let memory = MemoryRef::Claim(ClaimId("claim-1".into()));
        let json = serde_json::to_string(&memory).expect("serialize memory reference");
        assert_eq!(json, r#"{"kind":"claim","id":"claim-1"}"#);
    }
}
