use super::export::{append_records, begin_commit, validate_record_scope};
use super::lifecycle::{
    claim_kind_name, claim_status_name, processing_state_name, tier_name,
    validate_transcript_locator,
};
use super::repair::{enqueue_projection_repair, record_operation};
use super::*;
use sha2::{Digest, Sha256};

const SOURCE_PASS: u8 = 0;
const EVIDENCE_PASS: u8 = 1;
const CLAIM_PASS: u8 = 2;
const ORIGIN_PASS: u8 = 3;
const CLAIM_EVIDENCE_PASS: u8 = 4;
const PROFILE_PASS: u8 = 5;
const REVIEW_PASS: u8 = 6;
const CORRECTION_PASS: u8 = 7;
const DELETION_PASS: u8 = 8;
const PASSES: [u8; 9] = [
    SOURCE_PASS,
    EVIDENCE_PASS,
    CLAIM_PASS,
    ORIGIN_PASS,
    CLAIM_EVIDENCE_PASS,
    PROFILE_PASS,
    REVIEW_PASS,
    CORRECTION_PASS,
    DELETION_PASS,
];

impl MemoryDb {
    pub fn apply(&mut self, input: ApplyInput) -> Result<Applied> {
        require_scope(&input.tenant_id, &input.person_id)?;
        if input.export_format != EXPORT_FORMAT_VERSION {
            return Err(Error::Invalid(format!(
                "unsupported export_format {}; expected {EXPORT_FORMAT_VERSION}",
                input.export_format
            )));
        }
        if input
            .database_schema_version
            .is_some_and(|version| version > DATABASE_SCHEMA_VERSION)
        {
            return Err(Error::Invalid(format!(
                "applied records use a schema newer than supported version {DATABASE_SCHEMA_VERSION}"
            )));
        }
        let mut previous_sequence = 0;
        for commit in &input.commits {
            if commit.sequence <= previous_sequence {
                return Err(Error::Invalid(
                    "applied commits must use strictly ascending sequences".to_owned(),
                ));
            }
            previous_sequence = commit.sequence;
            if commit.first_event_index != 0 {
                return Err(Error::Invalid(format!(
                    "commit {} is partial; apply requires event index 0",
                    commit.sequence
                )));
            }
            if commit.records.len() as i64 != commit.event_count {
                return Err(Error::Invalid(format!(
                    "commit {} declares {} events but carries {}",
                    commit.sequence,
                    commit.event_count,
                    commit.records.len()
                )));
            }
            for record in &commit.records {
                validate_record_scope(record, &input.tenant_id, &input.person_id)?;
            }
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let applied_at: Timestamp =
            transaction.query_row("SELECT unixepoch()", [], |row| row.get(0))?;
        let mut applied = Applied {
            commits_applied: 0,
            commits_skipped: 0,
            records_applied: 0,
            records_skipped: 0,
        };
        for commit in &input.commits {
            let mut accepted = vec![false; commit.records.len()];
            for pass in PASSES {
                for (index, record) in commit.records.iter().enumerate() {
                    if pass == ORIGIN_PASS {
                        if let ExportRecord::Source(record) = record {
                            if accepted[index] {
                                apply_source_origin(&transaction, record)?;
                            }
                        }
                        continue;
                    }
                    if record_pass(record) != pass {
                        continue;
                    }
                    let (record_kind, record_id) = record_identity(record);
                    let payload_hash = record_hash(record)?;
                    let seen: bool = transaction.query_row(
                        "SELECT EXISTS(SELECT 1 FROM memory_applied_records WHERE tenant_id = ?1 AND person_id = ?2 AND record_kind = ?3 AND record_id = ?4 AND payload_hash = ?5)",
                        params![input.tenant_id.0, input.person_id.0, record_kind, record_id, payload_hash],
                        |row| row.get(0),
                    )?;
                    if seen {
                        applied.records_skipped += 1;
                        continue;
                    }
                    apply_record(&transaction, record, applied_at)?;
                    transaction.execute(
                        "INSERT INTO memory_applied_records(tenant_id, person_id, record_kind, record_id, payload_hash, applied_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                        params![input.tenant_id.0, input.person_id.0, record_kind, record_id, payload_hash, applied_at],
                    )?;
                    accepted[index] = true;
                    applied.records_applied += 1;
                }
            }
            if !accepted.iter().any(|accepted| *accepted) {
                applied.commits_skipped += 1;
                continue;
            }
            let sequence = begin_commit(
                &transaction,
                &input.tenant_id,
                &input.person_id,
                commit.recorded_at,
            )?;
            append_records(
                &transaction,
                sequence,
                commit
                    .records
                    .iter()
                    .zip(&accepted)
                    .filter(|(_, accepted)| **accepted)
                    .map(|(record, _)| record.clone()),
            )?;
            applied.commits_applied += 1;
        }
        record_operation(
            &transaction,
            &input.tenant_id,
            &input.person_id,
            "apply",
            "success",
            None,
            applied_at,
        )?;
        transaction.commit()?;
        Ok(applied)
    }
}

fn apply_record(
    transaction: &Transaction<'_>,
    record: &ExportRecord,
    applied_at: Timestamp,
) -> Result<()> {
    match record {
        ExportRecord::Source(record) => apply_source(transaction, record, applied_at),
        ExportRecord::Evidence(record) => apply_evidence(transaction, record, applied_at),
        ExportRecord::Claim(record) => apply_claim(transaction, record, applied_at),
        ExportRecord::ClaimEvidence(record) => apply_claim_evidence(transaction, record),
        ExportRecord::Correction(record) => apply_correction(transaction, record),
        ExportRecord::Profile(record) => apply_profile(transaction, record),
        ExportRecord::DailyReview(record) => apply_review(transaction, record),
        ExportRecord::Deletion(record) => apply_deletion(transaction, record, applied_at),
    }
}

fn apply_source(
    transaction: &Transaction<'_>,
    record: &SourceRecord,
    applied_at: Timestamp,
) -> Result<()> {
    let source = &record.source;
    let kind = serde_json::to_string(&source.kind)?;
    let stored = transaction
        .query_row(
            "SELECT ingestion_key, revision, kind, content, captured_at, recorded_at, deleted_at, feature_flag FROM sources WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3",
            params![source.id.0, source.tenant_id.0, source.person_id.0],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Timestamp>(4)?,
                    row.get::<_, Timestamp>(5)?,
                    row.get::<_, Option<Timestamp>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                ))
            },
        )
        .optional()?;
    let Some(stored) = stored else {
        transaction.execute(
            "INSERT INTO sources(id, tenant_id, person_id, ingestion_key, revision, kind, content, captured_at, recorded_at, deleted_at, feature_flag) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![source.id.0, source.tenant_id.0, source.person_id.0, record.ingestion_key, source.revision, kind, source.content, source.captured_at, source.recorded_at, source.deleted_at, source.feature_flag],
        )?;
        if source.deleted_at.is_none() {
            transaction.execute(
                "INSERT INTO source_fts(source_id, tenant_id, person_id, content) VALUES(?1, ?2, ?3, ?4)",
                params![source.id.0, source.tenant_id.0, source.person_id.0, source.content],
            )?;
        }
        return Ok(());
    };
    if stored.0 != record.ingestion_key
        || stored.2 != kind
        || stored.3 != source.content
        || stored.4 != source.captured_at
        || stored.5 != source.recorded_at
        || stored.7 != source.feature_flag
    {
        return Err(Error::Invalid(format!(
            "applied source {} conflicts with the stored source payload",
            source.id.0
        )));
    }
    if source.revision < stored.1 {
        return Ok(());
    }
    if source.revision == stored.1 && source.deleted_at != stored.6 {
        return Err(Error::Invalid(format!(
            "applied source {} conflicts with the stored deletion state",
            source.id.0
        )));
    }
    if stored.6.is_some() && source.deleted_at.is_none() {
        return Err(Error::Invalid(format!(
            "applied source {} cannot revive a tombstoned source",
            source.id.0
        )));
    }
    transaction.execute(
        "UPDATE sources SET revision = ?1, deleted_at = ?2, feature_flag = ?3 WHERE id = ?4 AND tenant_id = ?5 AND person_id = ?6",
        params![source.revision, source.deleted_at, source.feature_flag, source.id.0, source.tenant_id.0, source.person_id.0],
    )?;
    if stored.6.is_none() && source.deleted_at.is_some() {
        remove_source_index(transaction, source, applied_at)?;
    }
    Ok(())
}

fn apply_source_origin(transaction: &Transaction<'_>, record: &SourceRecord) -> Result<()> {
    let source = &record.source;
    if record.origin_evidence_id.is_none() && record.origin_claim_id.is_none() {
        return Ok(());
    }
    if let Some(evidence_id) = &record.origin_evidence_id {
        require_evidence(
            transaction,
            &source.tenant_id,
            &source.person_id,
            evidence_id,
        )?;
    }
    if let Some(claim_id) = &record.origin_claim_id {
        require_claim(transaction, &source.tenant_id, &source.person_id, claim_id)?;
    }
    transaction.execute(
        "UPDATE sources SET origin_evidence_id = ?1, origin_claim_id = ?2 WHERE id = ?3 AND tenant_id = ?4 AND person_id = ?5",
        params![
            record.origin_evidence_id.as_ref().map(|id| &id.0),
            record.origin_claim_id.as_ref().map(|id| &id.0),
            source.id.0,
            source.tenant_id.0,
            source.person_id.0
        ],
    )?;
    Ok(())
}

fn apply_evidence(
    transaction: &Transaction<'_>,
    record: &EvidenceRecord,
    applied_at: Timestamp,
) -> Result<()> {
    let evidence = &record.evidence;
    if evidence.byte_range.is_some() {
        return Err(Error::Invalid(format!(
            "applied evidence {} carries an unsupported byte_range",
            evidence.id.0
        )));
    }
    if let Some(locator) = &record.locator {
        validate_transcript_locator(locator)?;
    }
    let source_exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sources WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3)",
        params![
            evidence.source_id.0,
            evidence.tenant_id.0,
            evidence.person_id.0
        ],
        |row| row.get(0),
    )?;
    if !source_exists {
        return Err(Error::Invalid(format!(
            "applied evidence {} references unknown source {}",
            evidence.id.0, evidence.source_id.0
        )));
    }
    let stored = transaction
        .query_row(
            "SELECT source_id, source_revision, quote, recorded_at, deleted_at FROM evidence WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3",
            params![evidence.id.0, evidence.tenant_id.0, evidence.person_id.0],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Timestamp>(3)?,
                    row.get::<_, Option<Timestamp>>(4)?,
                ))
            },
        )
        .optional()?;
    let Some(stored) = stored else {
        transaction.execute(
            "INSERT INTO evidence(id, tenant_id, person_id, source_id, source_revision, quote, recorded_at, deleted_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![evidence.id.0, evidence.tenant_id.0, evidence.person_id.0, evidence.source_id.0, evidence.source_revision, evidence.quote, evidence.recorded_at, record.deleted_at],
        )?;
        if let Some(locator) = &record.locator {
            transaction.execute(
                "INSERT INTO evidence_locators(tenant_id, person_id, evidence_id, device_id, provider, stream_id, segment_id, start_ms, end_ms) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![evidence.tenant_id.0, evidence.person_id.0, evidence.id.0, locator.device_id, locator.provider, locator.stream_id, locator.segment_id, locator.start_ms, locator.end_ms],
            )?;
        }
        return Ok(());
    };
    if stored.0 != evidence.source_id.0
        || stored.1 != evidence.source_revision
        || stored.2 != evidence.quote
        || stored.3 != evidence.recorded_at
    {
        return Err(Error::Invalid(format!(
            "applied evidence {} conflicts with the stored evidence payload",
            evidence.id.0
        )));
    }
    let stored_locator = transaction
        .query_row(
            "SELECT device_id, provider, stream_id, segment_id, start_ms, end_ms FROM evidence_locators WHERE tenant_id = ?1 AND person_id = ?2 AND evidence_id = ?3",
            params![evidence.tenant_id.0, evidence.person_id.0, evidence.id.0],
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
    if stored_locator.as_ref() != record.locator.as_ref() {
        return Err(Error::Invalid(format!(
            "applied evidence {} conflicts with the stored transcript locator",
            evidence.id.0
        )));
    }
    if stored.4.is_some() && record.deleted_at.is_none() {
        return Err(Error::Invalid(format!(
            "applied evidence {} cannot revive tombstoned evidence",
            evidence.id.0
        )));
    }
    if let Some(deleted_at) = record.deleted_at.filter(|_| stored.4.is_none()) {
        transaction.execute(
            "UPDATE evidence SET deleted_at = ?1 WHERE id = ?2 AND tenant_id = ?3 AND person_id = ?4",
            params![deleted_at, evidence.id.0, evidence.tenant_id.0, evidence.person_id.0],
        )?;
        enqueue_projection_repair(
            transaction,
            &evidence.tenant_id,
            &evidence.person_id,
            EmbeddingTarget::Evidence(evidence.id.clone()),
            "apply_sync",
            applied_at,
        )?;
    }
    Ok(())
}

fn apply_claim(transaction: &Transaction<'_>, record: &Claim, applied_at: Timestamp) -> Result<()> {
    assert_legal_state(&record.tier, &record.status, &record.processing_state)
        .map_err(|error| Error::Invalid(error.to_string()))?;
    let kind = claim_kind_name(&record.kind);
    let status = claim_status_name(&record.status);
    let tier = tier_name(&record.tier);
    let processing_state = processing_state_name(&record.processing_state);
    let stored = transaction
        .query_row(
            "SELECT subject, predicate, value, kind, valid_from, recorded_from, valid_until, recorded_until, status, tier, processing_state FROM claims WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3",
            params![record.id.0, record.tenant_id.0, record.person_id.0],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Timestamp>(4)?,
                    row.get::<_, Timestamp>(5)?,
                    row.get::<_, Option<Timestamp>>(6)?,
                    row.get::<_, Option<Timestamp>>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                ))
            },
        )
        .optional()?;
    let Some(stored) = stored else {
        transaction.execute(
            "INSERT INTO claims(id, tenant_id, person_id, subject, predicate, value, kind, valid_from, valid_until, recorded_from, recorded_until, status, tier, processing_state) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![record.id.0, record.tenant_id.0, record.person_id.0, record.subject, record.predicate, record.value, kind, record.valid_time.from, record.valid_time.until, record.recorded_time.from, record.recorded_time.until, status, tier, processing_state],
        ).map_err(claim_interval_error)?;
        return Ok(());
    };
    if stored.0 != record.subject
        || stored.1 != record.predicate
        || stored.2 != record.value
        || stored.3 != kind
        || stored.4 != record.valid_time.from
        || stored.5 != record.recorded_time.from
    {
        return Err(Error::Invalid(format!(
            "applied claim {} conflicts with the stored claim payload",
            record.id.0
        )));
    }
    if stored.6 == record.valid_time.until
        && stored.7 == record.recorded_time.until
        && stored.8 == status
        && stored.9 == tier
        && stored.10 == processing_state
    {
        return Ok(());
    }
    transaction.execute(
        "UPDATE claims SET valid_until = ?1, recorded_until = ?2, status = ?3, tier = ?4, processing_state = ?5 WHERE id = ?6 AND tenant_id = ?7 AND person_id = ?8",
        params![record.valid_time.until, record.recorded_time.until, status, tier, processing_state, record.id.0, record.tenant_id.0, record.person_id.0],
    ).map_err(claim_interval_error)?;
    enqueue_projection_repair(
        transaction,
        &record.tenant_id,
        &record.person_id,
        EmbeddingTarget::Claim(record.id.clone()),
        "apply_sync",
        applied_at,
    )?;
    Ok(())
}

fn apply_claim_evidence(transaction: &Transaction<'_>, record: &ClaimEvidence) -> Result<()> {
    record
        .validate()
        .map_err(|error| Error::Invalid(error.to_string()))?;
    require_claim(
        transaction,
        &record.tenant_id,
        &record.person_id,
        &record.claim_id,
    )?;
    require_evidence(
        transaction,
        &record.tenant_id,
        &record.person_id,
        &record.evidence_id,
    )?;
    let relation = serde_json::to_string(&record.relation)?;
    let stored = transaction
        .query_row(
            "SELECT relation, confidence_basis_points FROM claim_evidence WHERE tenant_id = ?1 AND person_id = ?2 AND claim_id = ?3 AND evidence_id = ?4",
            params![record.tenant_id.0, record.person_id.0, record.claim_id.0, record.evidence_id.0],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, u16>(1)?)),
        )
        .optional()?;
    if let Some((stored_relation, stored_confidence)) = stored {
        if stored_relation != relation || stored_confidence != record.confidence_basis_points {
            return Err(Error::Invalid(format!(
                "applied claim evidence {}/{} conflicts with the stored link",
                record.claim_id.0, record.evidence_id.0
            )));
        }
        return Ok(());
    }
    transaction.execute(
        "INSERT INTO claim_evidence(tenant_id, person_id, claim_id, evidence_id, relation, confidence_basis_points) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
        params![record.tenant_id.0, record.person_id.0, record.claim_id.0, record.evidence_id.0, relation, record.confidence_basis_points],
    )?;
    Ok(())
}

fn apply_correction(transaction: &Transaction<'_>, record: &CorrectionRecord) -> Result<()> {
    require_claim(
        transaction,
        &record.tenant_id,
        &record.person_id,
        &record.superseded_claim_id,
    )?;
    require_claim(
        transaction,
        &record.tenant_id,
        &record.person_id,
        &record.claim_id,
    )?;
    require_evidence(
        transaction,
        &record.tenant_id,
        &record.person_id,
        &record.evidence_id,
    )?;
    let stored = transaction
        .query_row(
            "SELECT source_id, evidence_id, valid_at, recorded_at FROM corrections WHERE tenant_id = ?1 AND person_id = ?2 AND superseded_claim_id = ?3 AND claim_id = ?4",
            params![record.tenant_id.0, record.person_id.0, record.superseded_claim_id.0, record.claim_id.0],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Timestamp>(2)?,
                    row.get::<_, Timestamp>(3)?,
                ))
            },
        )
        .optional()?;
    if let Some(stored) = stored {
        if stored.0 != record.source_id.0
            || stored.1 != record.evidence_id.0
            || stored.2 != record.valid_at
            || stored.3 != record.recorded_at
        {
            return Err(Error::Invalid(format!(
                "applied correction {}/{} conflicts with the stored correction",
                record.superseded_claim_id.0, record.claim_id.0
            )));
        }
        return Ok(());
    }
    transaction.execute(
        "INSERT INTO corrections(tenant_id, person_id, superseded_claim_id, claim_id, source_id, evidence_id, valid_at, recorded_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![record.tenant_id.0, record.person_id.0, record.superseded_claim_id.0, record.claim_id.0, record.source_id.0, record.evidence_id.0, record.valid_at, record.recorded_at],
    ).map_err(|error| match error {
        rusqlite::Error::SqliteFailure(_, Some(message)) if message == "correction scope mismatch" => {
            Error::Invalid(format!(
                "applied correction {}/{} does not match its source lineage",
                record.superseded_claim_id.0, record.claim_id.0
            ))
        }
        error => Error::Sql(error),
    })?;
    Ok(())
}

fn apply_profile(transaction: &Transaction<'_>, record: &ProfileEntry) -> Result<()> {
    require_claim(
        transaction,
        &record.tenant_id,
        &record.person_id,
        &record.claim_id,
    )?;
    let stability = serde_json::to_string(&record.stability)?;
    transaction.execute(
        "DELETE FROM profile_entries WHERE tenant_id = ?1 AND person_id = ?2 AND key = ?3 AND id <> ?4",
        params![record.tenant_id.0, record.person_id.0, record.key, record.id.0],
    )?;
    transaction.execute(
        "INSERT INTO profile_entries(id, tenant_id, person_id, key, value, stability, claim_id, recorded_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) ON CONFLICT(id) DO UPDATE SET key = excluded.key, value = excluded.value, stability = excluded.stability, claim_id = excluded.claim_id, recorded_at = excluded.recorded_at",
        params![record.id.0, record.tenant_id.0, record.person_id.0, record.key, record.value, stability, record.claim_id.0, record.recorded_at],
    ).map_err(|error| match error {
        rusqlite::Error::SqliteFailure(_, Some(message)) if message == "profile entry claim scope mismatch" => {
            Error::Invalid(format!(
                "applied profile entry {} does not match its backing claim",
                record.id.0
            ))
        }
        error => Error::Sql(error),
    })?;
    Ok(())
}

fn apply_review(transaction: &Transaction<'_>, record: &DailyReview) -> Result<()> {
    for evidence_id in &record.evidence_ids {
        require_evidence(
            transaction,
            &record.tenant_id,
            &record.person_id,
            evidence_id,
        )?;
    }
    let evidence_ids = serde_json::to_string(&record.evidence_ids)?;
    let stored = transaction
        .query_row(
            "SELECT day, summary, evidence_ids, recorded_at FROM daily_reviews WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3",
            params![record.id.0, record.tenant_id.0, record.person_id.0],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Timestamp>(3)?,
                ))
            },
        )
        .optional()?;
    if let Some(stored) = stored {
        if stored.0 != record.day
            || stored.1 != record.summary
            || stored.2 != evidence_ids
            || stored.3 != record.recorded_at
        {
            return Err(Error::Invalid(format!(
                "applied daily review {} conflicts with the stored review",
                record.id.0
            )));
        }
        return Ok(());
    }
    transaction.execute(
        "INSERT INTO daily_reviews(id, tenant_id, person_id, day, summary, evidence_ids, recorded_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![record.id.0, record.tenant_id.0, record.person_id.0, record.day, record.summary, evidence_ids, record.recorded_at],
    )?;
    Ok(())
}

fn apply_deletion(
    transaction: &Transaction<'_>,
    record: &DeletionRecord,
    applied_at: Timestamp,
) -> Result<()> {
    match &record.target {
        MemoryRef::Source(source_id) => {
            let stored = transaction
                .query_row(
                    "SELECT deleted_at, content FROM sources WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3",
                    params![source_id.0, record.tenant_id.0, record.person_id.0],
                    |row| Ok((row.get::<_, Option<Timestamp>>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?
                .ok_or_else(|| unknown_target(&record.target))?;
            if stored.0.is_none() {
                transaction.execute(
                    "UPDATE sources SET deleted_at = ?1 WHERE id = ?2 AND tenant_id = ?3 AND person_id = ?4",
                    params![record.deleted_at, source_id.0, record.tenant_id.0, record.person_id.0],
                )?;
            }
            transaction.execute(
                "DELETE FROM source_fts WHERE source_id = ?1 AND tenant_id = ?2 AND person_id = ?3",
                params![source_id.0, record.tenant_id.0, record.person_id.0],
            )?;
            enqueue_projection_repair(
                transaction,
                &record.tenant_id,
                &record.person_id,
                EmbeddingTarget::Source(source_id.clone()),
                "apply_sync",
                applied_at,
            )?;
        }
        MemoryRef::Evidence(evidence_id) => {
            let changed = transaction.execute(
                "UPDATE evidence SET deleted_at = ?1 WHERE id = ?2 AND tenant_id = ?3 AND person_id = ?4 AND deleted_at IS NULL",
                params![record.deleted_at, evidence_id.0, record.tenant_id.0, record.person_id.0],
            )?;
            if changed == 0 {
                require_evidence(
                    transaction,
                    &record.tenant_id,
                    &record.person_id,
                    evidence_id,
                )?;
            }
            enqueue_projection_repair(
                transaction,
                &record.tenant_id,
                &record.person_id,
                EmbeddingTarget::Evidence(evidence_id.clone()),
                "apply_sync",
                applied_at,
            )?;
        }
        MemoryRef::Claim(claim_id) => {
            require_claim(transaction, &record.tenant_id, &record.person_id, claim_id)?;
            transaction.execute(
                "UPDATE claims SET status = 'retracted', recorded_until = ?1 WHERE id = ?2 AND tenant_id = ?3 AND person_id = ?4 AND status = 'accepted'",
                params![record.deleted_at, claim_id.0, record.tenant_id.0, record.person_id.0],
            ).map_err(claim_interval_error)?;
            enqueue_projection_repair(
                transaction,
                &record.tenant_id,
                &record.person_id,
                EmbeddingTarget::Claim(claim_id.clone()),
                "apply_sync",
                applied_at,
            )?;
        }
        MemoryRef::ProfileEntry(profile_id) => {
            transaction.execute(
                "DELETE FROM profile_entries WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3",
                params![profile_id.0, record.tenant_id.0, record.person_id.0],
            )?;
        }
        MemoryRef::DailyReview(review_id) => {
            transaction.execute(
                "DELETE FROM daily_reviews WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3",
                params![review_id.0, record.tenant_id.0, record.person_id.0],
            )?;
        }
    }
    Ok(())
}

fn remove_source_index(
    transaction: &Transaction<'_>,
    source: &Source,
    applied_at: Timestamp,
) -> Result<()> {
    transaction.execute(
        "DELETE FROM source_fts WHERE source_id = ?1 AND tenant_id = ?2 AND person_id = ?3",
        params![source.id.0, source.tenant_id.0, source.person_id.0],
    )?;
    enqueue_projection_repair(
        transaction,
        &source.tenant_id,
        &source.person_id,
        EmbeddingTarget::Source(source.id.clone()),
        "apply_sync",
        applied_at,
    )
}

fn require_claim(
    transaction: &Transaction<'_>,
    tenant_id: &TenantId,
    person_id: &PersonId,
    claim_id: &ClaimId,
) -> Result<()> {
    let exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM claims WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3)",
        params![claim_id.0, tenant_id.0, person_id.0],
        |row| row.get(0),
    )?;
    if exists {
        return Ok(());
    }
    Err(Error::Invalid(format!(
        "applied record references unknown claim {}",
        claim_id.0
    )))
}

fn require_evidence(
    transaction: &Transaction<'_>,
    tenant_id: &TenantId,
    person_id: &PersonId,
    evidence_id: &EvidenceId,
) -> Result<()> {
    let exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM evidence WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3)",
        params![evidence_id.0, tenant_id.0, person_id.0],
        |row| row.get(0),
    )?;
    if exists {
        return Ok(());
    }
    Err(Error::Invalid(format!(
        "applied record references unknown evidence {}",
        evidence_id.0
    )))
}

fn unknown_target(target: &MemoryRef) -> Error {
    Error::Invalid(format!(
        "applied deletion references unknown target {}",
        memory_ref_key(target)
    ))
}

fn claim_interval_error(error: rusqlite::Error) -> Error {
    match error {
        rusqlite::Error::SqliteFailure(_, Some(message))
            if message == schema::CLAIM_TIME_INTERVAL_ERROR =>
        {
            Error::Invalid("applied claim carries an invalid time interval".to_owned())
        }
        error => Error::Sql(error),
    }
}

fn record_pass(record: &ExportRecord) -> u8 {
    match record {
        ExportRecord::Source(_) => SOURCE_PASS,
        ExportRecord::Evidence(_) => EVIDENCE_PASS,
        ExportRecord::Claim(_) => CLAIM_PASS,
        ExportRecord::ClaimEvidence(_) => CLAIM_EVIDENCE_PASS,
        ExportRecord::Profile(_) => PROFILE_PASS,
        ExportRecord::DailyReview(_) => REVIEW_PASS,
        ExportRecord::Correction(_) => CORRECTION_PASS,
        ExportRecord::Deletion(_) => DELETION_PASS,
    }
}

fn record_identity(record: &ExportRecord) -> (&'static str, String) {
    match record {
        ExportRecord::Source(value) => ("source", value.source.id.0.clone()),
        ExportRecord::Evidence(value) => ("evidence", value.evidence.id.0.clone()),
        ExportRecord::Claim(value) => ("claim", value.id.0.clone()),
        ExportRecord::ClaimEvidence(value) => (
            "claim_evidence",
            format!("{}/{}", value.claim_id.0, value.evidence_id.0),
        ),
        ExportRecord::Correction(value) => (
            "correction",
            format!("{}/{}", value.superseded_claim_id.0, value.claim_id.0),
        ),
        ExportRecord::Profile(value) => ("profile", value.id.0.clone()),
        ExportRecord::DailyReview(value) => ("daily_review", value.id.0.clone()),
        ExportRecord::Deletion(value) => ("deletion", memory_ref_key(&value.target)),
    }
}

fn memory_ref_key(target: &MemoryRef) -> String {
    match target {
        MemoryRef::Source(id) => format!("source:{}", id.0),
        MemoryRef::Evidence(id) => format!("evidence:{}", id.0),
        MemoryRef::Claim(id) => format!("claim:{}", id.0),
        MemoryRef::ProfileEntry(id) => format!("profile_entry:{}", id.0),
        MemoryRef::DailyReview(id) => format!("daily_review:{}", id.0),
    }
}

fn record_hash(record: &ExportRecord) -> Result<String> {
    Ok(format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(record)?)
    ))
}
