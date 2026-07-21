use super::*;

pub(super) fn begin_commit(
    transaction: &Transaction<'_>,
    tenant_id: &TenantId,
    person_id: &PersonId,
    recorded_at: Timestamp,
) -> Result<i64> {
    transaction.execute(
        "INSERT INTO memory_commits(tenant_id, person_id, recorded_at) VALUES(?1, ?2, ?3)",
        params![tenant_id.0, person_id.0, recorded_at],
    )?;
    Ok(transaction.last_insert_rowid())
}

pub(super) fn append_records(
    transaction: &Transaction<'_>,
    commit_sequence: i64,
    records: impl IntoIterator<Item = ExportRecord>,
) -> Result<()> {
    let (tenant_id, person_id) = transaction.query_row(
        "SELECT tenant_id, person_id FROM memory_commits WHERE sequence = ?1",
        [commit_sequence],
        |row| Ok((TenantId(row.get(0)?), PersonId(row.get(1)?))),
    )?;
    for (index, record) in records.into_iter().enumerate() {
        validate_record_scope(&record, &tenant_id, &person_id)?;
        let payload = serde_json::to_string(&record)?;
        if payload.len() > MAX_EXPORT_RECORD_BYTES {
            return Err(Error::Invalid(format!(
                "export record exceeds {MAX_EXPORT_RECORD_BYTES} bytes"
            )));
        }
        transaction.execute(
            "INSERT INTO memory_export_events(commit_sequence, event_index, payload) VALUES(?1, ?2, ?3)",
            params![commit_sequence, index as i64, payload],
        )?;
    }
    Ok(())
}

fn validate_record_scope(
    record: &ExportRecord,
    tenant_id: &TenantId,
    person_id: &PersonId,
) -> Result<()> {
    let scope = match record {
        ExportRecord::Source(value) => (&value.source.tenant_id, &value.source.person_id),
        ExportRecord::Evidence(value) => (&value.evidence.tenant_id, &value.evidence.person_id),
        ExportRecord::Claim(value) => (&value.tenant_id, &value.person_id),
        ExportRecord::ClaimEvidence(value) => (&value.tenant_id, &value.person_id),
        ExportRecord::Correction(value) => (&value.tenant_id, &value.person_id),
        ExportRecord::Profile(value) => (&value.tenant_id, &value.person_id),
        ExportRecord::DailyReview(value) => (&value.tenant_id, &value.person_id),
        ExportRecord::Deletion(value) => (&value.tenant_id, &value.person_id),
    };
    if scope != (tenant_id, person_id) {
        return Err(Error::Invalid(
            "export record scope does not match its commit".to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn source_record(
    transaction: &Transaction<'_>,
    tenant_id: &TenantId,
    person_id: &PersonId,
    source_id: &SourceId,
) -> Result<SourceRecord> {
    transaction.query_row(
        "SELECT revision, kind, content, captured_at, recorded_at, deleted_at, ingestion_key, origin_evidence_id, origin_claim_id FROM sources WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3",
        params![source_id.0, tenant_id.0, person_id.0],
        |row| {
            let kind: String = row.get(1)?;
            Ok(SourceRecord {
                source: Source {
                    id: source_id.clone(),
                    tenant_id: tenant_id.clone(),
                    person_id: person_id.clone(),
                    revision: row.get(0)?,
                    kind: serde_json::from_str(&kind).map_err(sql_json_error)?,
                    content: row.get(2)?,
                    captured_at: row.get(3)?,
                    recorded_at: row.get(4)?,
                    deleted_at: row.get(5)?,
                },
                ingestion_key: row.get(6)?,
                origin_evidence_id: row.get::<_, Option<String>>(7)?.map(EvidenceId),
                origin_claim_id: row.get::<_, Option<String>>(8)?.map(ClaimId),
            })
        },
    ).map_err(Error::from)
}

pub(super) fn evidence_record(
    transaction: &Transaction<'_>,
    tenant_id: &TenantId,
    person_id: &PersonId,
    evidence_id: &EvidenceId,
) -> Result<EvidenceRecord> {
    transaction.query_row(
        "SELECT e.source_id, e.source_revision, e.quote, e.recorded_at, e.deleted_at, l.device_id, l.provider, l.stream_id, l.segment_id, l.start_ms, l.end_ms FROM evidence e LEFT JOIN evidence_locators l ON l.evidence_id = e.id AND l.tenant_id = e.tenant_id AND l.person_id = e.person_id WHERE e.id = ?1 AND e.tenant_id = ?2 AND e.person_id = ?3",
        params![evidence_id.0, tenant_id.0, person_id.0],
        |row| {
            let device_id = row.get::<_, Option<String>>(5)?;
            let locator = match device_id {
                Some(device_id) => Some(TranscriptLocator {
                    device_id,
                    provider: row.get(6)?,
                    stream_id: row.get(7)?,
                    segment_id: row.get(8)?,
                    start_ms: row.get(9)?,
                    end_ms: row.get(10)?,
                }),
                None => None,
            };
            Ok(EvidenceRecord {
                evidence: Evidence {
                    id: evidence_id.clone(),
                    tenant_id: tenant_id.clone(),
                    person_id: person_id.clone(),
                    source_id: SourceId(row.get(0)?),
                    source_revision: row.get(1)?,
                    quote: row.get(2)?,
                    byte_range: None,
                    recorded_at: row.get(3)?,
                },
                locator,
                deleted_at: row.get(4)?,
            })
        },
    ).map_err(Error::from)
}

pub(super) fn claim_record(
    transaction: &Transaction<'_>,
    tenant_id: &TenantId,
    person_id: &PersonId,
    claim_id: &ClaimId,
) -> Result<Claim> {
    transaction.query_row(
        "SELECT subject, predicate, value, kind, valid_from, valid_until, recorded_from, recorded_until, status FROM claims WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3",
        params![claim_id.0, tenant_id.0, person_id.0],
        |row| {
            let kind: String = row.get(3)?;
            let status: String = row.get(8)?;
            Ok(Claim {
                id: claim_id.clone(),
                tenant_id: tenant_id.clone(),
                person_id: person_id.clone(),
                subject: row.get(0)?,
                predicate: row.get(1)?,
                value: row.get(2)?,
                kind: serde_json::from_str(&format!("\"{kind}\""))
                    .map_err(sql_json_error)?,
                valid_time: crate::TimeRange { from: row.get(4)?, until: row.get(5)? },
                recorded_time: crate::TimeRange { from: row.get(6)?, until: row.get(7)? },
                status: serde_json::from_str(&format!("\"{status}\""))
                    .map_err(sql_json_error)?,
            })
        },
    ).map_err(Error::from)
}

pub(super) fn claim_evidence_record(
    transaction: &Transaction<'_>,
    tenant_id: &TenantId,
    person_id: &PersonId,
    claim_id: &ClaimId,
    evidence_id: &EvidenceId,
) -> Result<ClaimEvidence> {
    transaction.query_row(
        "SELECT relation, confidence_basis_points FROM claim_evidence WHERE tenant_id = ?1 AND person_id = ?2 AND claim_id = ?3 AND evidence_id = ?4",
        params![tenant_id.0, person_id.0, claim_id.0, evidence_id.0],
        |row| {
            let relation: String = row.get(0)?;
            Ok(ClaimEvidence {
                tenant_id: tenant_id.clone(),
                person_id: person_id.clone(),
                claim_id: claim_id.clone(),
                evidence_id: evidence_id.clone(),
                relation: serde_json::from_str(&relation).map_err(sql_json_error)?,
                confidence_basis_points: row.get(1)?,
            })
        },
    ).map_err(Error::from)
}

fn sql_json_error(error: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

impl MemoryDb {
    pub fn export(&mut self, input: ExportInput) -> Result<ExportPage> {
        require_scope(&input.tenant_id, &input.person_id)?;
        if input.export_format != EXPORT_FORMAT_VERSION {
            return Err(Error::Invalid(format!(
                "unsupported export_format {}; expected {EXPORT_FORMAT_VERSION}",
                input.export_format
            )));
        }
        if input.after_commit < 0 {
            return Err(Error::Invalid(
                "after_commit must not be negative".to_owned(),
            ));
        }
        if input.after_event_index < -1 {
            return Err(Error::Invalid(
                "after_event_index must be at least -1".to_owned(),
            ));
        }
        if input.after_commit == 0 && input.after_event_index != -1 {
            return Err(Error::Invalid(
                "the initial cursor must use after_event_index -1".to_owned(),
            ));
        }
        if input
            .high_water_mark
            .is_some_and(|high_water| high_water < input.after_commit)
        {
            return Err(Error::Invalid(
                "high_water_mark must not precede after_commit".to_owned(),
            ));
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Deferred)?;
        let current_high_water: i64 = transaction.query_row(
                "SELECT COALESCE(MAX(sequence), 0) FROM memory_commits WHERE tenant_id = ?1 AND person_id = ?2",
                params![input.tenant_id.0, input.person_id.0],
                |row| row.get(0),
            )?;
        if input
            .high_water_mark
            .is_some_and(|value| value > current_high_water)
        {
            return Err(Error::Invalid(
                "high_water_mark exceeds the current scoped maximum".to_owned(),
            ));
        }
        let high_water_mark = input.high_water_mark.unwrap_or(current_high_water);
        let limit = bounded_limit(input.limit) as usize;
        let mut statement = transaction.prepare(
            "SELECT c.sequence, c.recorded_at, e.event_index, (SELECT COUNT(*) FROM memory_export_events all_events WHERE all_events.commit_sequence = c.sequence), e.payload
             FROM memory_commits c JOIN memory_export_events e ON e.commit_sequence = c.sequence
             WHERE c.tenant_id = ?1 AND c.person_id = ?2 AND c.sequence <= ?5
               AND (c.sequence > ?3 OR (c.sequence = ?3 AND e.event_index > ?4))
             ORDER BY c.sequence, e.event_index LIMIT ?6",
        )?;
        if input.after_commit > 0 {
            let event_count = transaction
                .query_row(
                    "SELECT COUNT(e.event_index) FROM memory_commits c LEFT JOIN memory_export_events e ON e.commit_sequence = c.sequence WHERE c.sequence = ?1 AND c.tenant_id = ?2 AND c.person_id = ?3 GROUP BY c.sequence",
                    params![input.after_commit, input.tenant_id.0, input.person_id.0],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?
                .ok_or_else(|| Error::Invalid("after_commit is not in scope".to_owned()))?;
            if input.after_event_index >= event_count {
                return Err(Error::Invalid(
                    "after_event_index exceeds the commit event count".to_owned(),
                ));
            }
        }
        let rows = statement.query_map(
            params![
                input.tenant_id.0,
                input.person_id.0,
                input.after_commit,
                input.after_event_index,
                high_water_mark,
                (limit + 1) as i64
            ],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Timestamp>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )?;
        let events = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        drop(statement);
        let more_by_count = events.len() > limit;
        let mut exported: Vec<ExportCommit> = Vec::new();
        let mut payload_bytes = 0;
        let mut truncated_by_bytes = false;
        for (sequence, recorded_at, event_index, event_count, payload) in
            events.into_iter().take(limit)
        {
            if payload_bytes > 0 && payload_bytes + payload.len() > MAX_EXPORT_PAGE_BYTES {
                truncated_by_bytes = true;
                break;
            }
            let record = serde_json::from_str::<ExportRecord>(&payload)?;
            validate_record_scope(&record, &input.tenant_id, &input.person_id)?;
            payload_bytes += payload.len();
            if let Some(commit) = exported.last_mut().filter(|item| item.sequence == sequence) {
                commit.records.push(record);
            } else {
                exported.push(ExportCommit {
                    sequence,
                    recorded_at,
                    event_count,
                    first_event_index: event_index,
                    records: vec![record],
                });
            }
        }
        let complete = !more_by_count && !truncated_by_bytes;
        let (next_after_commit, next_after_event_index) =
            exported
                .last()
                .map_or((input.after_commit, input.after_event_index), |commit| {
                    (
                        commit.sequence,
                        commit.first_event_index + commit.records.len() as i64 - 1,
                    )
                });
        transaction.commit()?;
        Ok(ExportPage {
            export_format: EXPORT_FORMAT_VERSION,
            database_schema_version: DATABASE_SCHEMA_VERSION,
            high_water_mark,
            next_after_commit,
            next_after_event_index,
            complete,
            commits: exported,
        })
    }
}
