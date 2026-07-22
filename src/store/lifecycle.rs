use super::export::{
    append_commit, claim_evidence_record, claim_record, evidence_record, source_record,
};
use super::repair::{enqueue_projection_repair, record_operation};
use super::*;

impl MemoryDb {
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
        let mut records = vec![
            ExportRecord::Source(source_record(
                &transaction,
                &input.tenant_id,
                &input.person_id,
                &source_id,
            )?),
            ExportRecord::Evidence(evidence_record(
                &transaction,
                &input.tenant_id,
                &input.person_id,
                &evidence_id,
            )?),
        ];
        if let Some(claim_id) = &claim_id {
            records.push(ExportRecord::Claim(claim_record(
                &transaction,
                &input.tenant_id,
                &input.person_id,
                claim_id,
            )?));
            records.push(ExportRecord::ClaimEvidence(claim_evidence_record(
                &transaction,
                &input.tenant_id,
                &input.person_id,
                claim_id,
                &evidence_id,
            )?));
        }
        append_commit(
            &transaction,
            &input.tenant_id,
            &input.person_id,
            input.recorded_at,
            records,
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

    pub fn correct(&mut self, input: CorrectInput) -> Result<Corrected> {
        require_scope(&input.tenant_id, &input.person_id)?;
        require_text("correction text", &input.text)?;
        require_text("value", &input.value)?;
        let transaction = self.connection.transaction()?;
        let removed_profiles = transaction
            .prepare(
                "SELECT id FROM profile_entries WHERE tenant_id = ?1 AND person_id = ?2 AND claim_id = ?3 ORDER BY id",
            )?
            .query_map(
                params![input.tenant_id.0, input.person_id.0, input.claim_id.0],
                |row| row.get::<_, String>(0),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
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
        enqueue_projection_repair(
            &transaction,
            &input.tenant_id,
            &input.person_id,
            EmbeddingTarget::Claim(input.claim_id.clone()),
            "superseded_sync",
            input.recorded_at,
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
                tier: MemoryTier::LongTerm,
                processing_state: MemoryProcessingState::Processed,
            },
            input.recorded_at,
        )?;
        transaction.execute(
            "UPDATE sources SET origin_evidence_id = ?1, origin_claim_id = ?2 WHERE id = ?3 AND tenant_id = ?4 AND person_id = ?5",
            params![evidence_id.0, claim_id.0, source_id.0, input.tenant_id.0, input.person_id.0],
        )?;
        let correction = CorrectionRecord {
            tenant_id: input.tenant_id.clone(),
            person_id: input.person_id.clone(),
            superseded_claim_id: input.claim_id.clone(),
            claim_id: claim_id.clone(),
            source_id: source_id.clone(),
            evidence_id: evidence_id.clone(),
            valid_at: input.valid_at,
            recorded_at: input.recorded_at,
        };
        transaction.execute(
            "INSERT INTO corrections(tenant_id, person_id, superseded_claim_id, claim_id, source_id, evidence_id, valid_at, recorded_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![correction.tenant_id.0, correction.person_id.0, correction.superseded_claim_id.0, correction.claim_id.0, correction.source_id.0, correction.evidence_id.0, correction.valid_at, correction.recorded_at],
        )?;
        let mut records = vec![
            ExportRecord::Source(source_record(
                &transaction,
                &input.tenant_id,
                &input.person_id,
                &source_id,
            )?),
            ExportRecord::Evidence(evidence_record(
                &transaction,
                &input.tenant_id,
                &input.person_id,
                &evidence_id,
            )?),
            ExportRecord::Claim(claim_record(
                &transaction,
                &input.tenant_id,
                &input.person_id,
                &input.claim_id,
            )?),
            ExportRecord::Claim(claim_record(
                &transaction,
                &input.tenant_id,
                &input.person_id,
                &claim_id,
            )?),
            ExportRecord::ClaimEvidence(claim_evidence_record(
                &transaction,
                &input.tenant_id,
                &input.person_id,
                &claim_id,
                &evidence_id,
            )?),
            ExportRecord::Correction(correction),
        ];
        records.extend(removed_profiles.into_iter().map(|id| {
            ExportRecord::Deletion(DeletionRecord {
                tenant_id: input.tenant_id.clone(),
                person_id: input.person_id.clone(),
                target: MemoryRef::ProfileEntry(ProfileEntryId(id)),
                deleted_at: input.recorded_at,
            })
        }));
        append_commit(
            &transaction,
            &input.tenant_id,
            &input.person_id,
            input.recorded_at,
            records,
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
        let evidence_ids = transaction
            .prepare(
                "SELECT id FROM evidence WHERE source_id = ?1 AND tenant_id = ?2 AND person_id = ?3 ORDER BY id",
            )?
            .query_map(
                params![input.source_id.0, input.tenant_id.0, input.person_id.0],
                |row| row.get::<_, String>(0).map(EvidenceId),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let candidate_claim_ids = transaction
            .prepare(
                "SELECT DISTINCT c.id FROM claims c JOIN claim_evidence ce ON ce.claim_id = c.id AND ce.tenant_id = c.tenant_id AND ce.person_id = c.person_id JOIN evidence e ON e.id = ce.evidence_id AND e.tenant_id = ce.tenant_id AND e.person_id = ce.person_id WHERE e.source_id = ?1 AND c.tenant_id = ?2 AND c.person_id = ?3 AND c.status = 'accepted' ORDER BY c.id",
            )?
            .query_map(
                params![input.source_id.0, input.tenant_id.0, input.person_id.0],
                |row| row.get::<_, String>(0).map(ClaimId),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let profile_ids = transaction
            .prepare(
                "SELECT p.id FROM profile_entries p WHERE p.tenant_id = ?1 AND p.person_id = ?2 AND p.claim_id IN (SELECT DISTINCT ce.claim_id FROM claim_evidence ce JOIN evidence e ON e.id = ce.evidence_id AND e.tenant_id = ce.tenant_id AND e.person_id = ce.person_id WHERE e.source_id = ?3 AND ce.tenant_id = ?1 AND ce.person_id = ?2) ORDER BY p.id",
            )?
            .query_map(
                params![input.tenant_id.0, input.person_id.0, input.source_id.0],
                |row| row.get::<_, String>(0),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let review_ids = transaction
            .prepare(
                "SELECT r.id FROM daily_reviews r WHERE r.tenant_id = ?1 AND r.person_id = ?2 AND EXISTS (SELECT 1 FROM json_each(r.evidence_ids) citation JOIN evidence e ON e.id = citation.value WHERE e.source_id = ?3 AND e.tenant_id = ?1 AND e.person_id = ?2) ORDER BY r.id",
            )?
            .query_map(
                params![input.tenant_id.0, input.person_id.0, input.source_id.0],
                |row| row.get::<_, String>(0).map(DailyReviewId),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
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
        ).map_err(|error| match error {
            rusqlite::Error::SqliteFailure(_, Some(message))
                if message == schema::CLAIM_TIME_INTERVAL_ERROR =>
            {
                Error::Invalid(
                    "deleted_at must advance affected claims' recorded intervals".to_owned(),
                )
            }
            error => Error::Sql(error),
        })? as u64;
        let mut changed_claim_ids = Vec::new();
        for claim_id in candidate_claim_ids {
            let changed: bool = transaction.query_row(
                "SELECT status = 'retracted' AND recorded_until = ?1 FROM claims WHERE id = ?2 AND tenant_id = ?3 AND person_id = ?4",
                params![input.deleted_at, claim_id.0, input.tenant_id.0, input.person_id.0],
                |row| row.get(0),
            )?;
            if changed {
                changed_claim_ids.push(claim_id);
            }
        }
        enqueue_projection_repair(
            &transaction,
            &input.tenant_id,
            &input.person_id,
            EmbeddingTarget::Source(input.source_id.clone()),
            "delete_sync",
            input.deleted_at,
        )?;
        for evidence_id in &evidence_ids {
            enqueue_projection_repair(
                &transaction,
                &input.tenant_id,
                &input.person_id,
                EmbeddingTarget::Evidence(evidence_id.clone()),
                "delete_sync",
                input.deleted_at,
            )?;
        }
        for claim_id in &changed_claim_ids {
            enqueue_projection_repair(
                &transaction,
                &input.tenant_id,
                &input.person_id,
                EmbeddingTarget::Claim(claim_id.clone()),
                "delete_sync",
                input.deleted_at,
            )?;
        }
        transaction.execute(
            "DELETE FROM profile_entries WHERE tenant_id = ?1 AND person_id = ?2 AND claim_id IN (SELECT id FROM claims WHERE tenant_id = ?1 AND person_id = ?2 AND status = 'retracted')",
            params![input.tenant_id.0, input.person_id.0],
        )?;
        transaction.execute(
            "DELETE FROM daily_reviews WHERE tenant_id = ?1 AND person_id = ?2 AND EXISTS (SELECT 1 FROM json_each(evidence_ids) citation JOIN evidence e ON e.id = citation.value WHERE e.source_id = ?3 AND e.tenant_id = ?1 AND e.person_id = ?2)",
            params![input.tenant_id.0, input.person_id.0, input.source_id.0],
        )?;
        let mut records = vec![
            ExportRecord::Source(source_record(
                &transaction,
                &input.tenant_id,
                &input.person_id,
                &input.source_id,
            )?),
            ExportRecord::Deletion(DeletionRecord {
                tenant_id: input.tenant_id.clone(),
                person_id: input.person_id.clone(),
                target: MemoryRef::Source(input.source_id.clone()),
                deleted_at: input.deleted_at,
            }),
        ];
        for evidence_id in &evidence_ids {
            records.push(ExportRecord::Evidence(evidence_record(
                &transaction,
                &input.tenant_id,
                &input.person_id,
                evidence_id,
            )?));
            records.push(ExportRecord::Deletion(DeletionRecord {
                tenant_id: input.tenant_id.clone(),
                person_id: input.person_id.clone(),
                target: MemoryRef::Evidence(evidence_id.clone()),
                deleted_at: input.deleted_at,
            }));
        }
        for claim_id in &changed_claim_ids {
            records.push(ExportRecord::Claim(claim_record(
                &transaction,
                &input.tenant_id,
                &input.person_id,
                claim_id,
            )?));
        }
        for profile_id in profile_ids {
            let remains: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM profile_entries WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3)",
                params![profile_id, input.tenant_id.0, input.person_id.0],
                |row| row.get(0),
            )?;
            if !remains {
                records.push(ExportRecord::Deletion(DeletionRecord {
                    tenant_id: input.tenant_id.clone(),
                    person_id: input.person_id.clone(),
                    target: MemoryRef::ProfileEntry(ProfileEntryId(profile_id)),
                    deleted_at: input.deleted_at,
                }));
            }
        }
        records.extend(review_ids.into_iter().map(|id| {
            ExportRecord::Deletion(DeletionRecord {
                tenant_id: input.tenant_id.clone(),
                person_id: input.person_id.clone(),
                target: MemoryRef::DailyReview(id),
                deleted_at: input.deleted_at,
            })
        }));
        append_commit(
            &transaction,
            &input.tenant_id,
            &input.person_id,
            input.deleted_at,
            records,
        )?;
        transaction.commit()?;
        Ok(Deleted {
            source_id: input.source_id,
            evidence_count,
            claim_count,
        })
    }

    pub fn promote(&mut self, input: PromoteInput) -> Result<Promoted> {
        require_scope(&input.tenant_id, &input.person_id)?;
        let transaction = self.connection.transaction()?;
        let current: (String, String, String, i64) = transaction.query_row(
            "SELECT tier, processing_state, status, recorded_from FROM claims WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3",
            params![input.claim_id.0, input.tenant_id.0, input.person_id.0],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        if current.2 != "accepted" {
            return Err(Error::Invalid(
                "promotion requires an active claim".to_owned(),
            ));
        }
        if current.1 != "processed" {
            return Err(Error::Invalid(
                "promotion requires processing_state=processed".to_owned(),
            ));
        }
        if current.0 != "short_term" {
            return Err(Error::Invalid(
                "promotion requires tier=short_term".to_owned(),
            ));
        }
        if input.recorded_at < current.3 {
            return Err(Error::Invalid(
                "promotion recorded_at must not predate claim".to_owned(),
            ));
        }
        transaction.execute(
            "UPDATE claims SET tier = 'long_term' WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3",
            params![input.claim_id.0, input.tenant_id.0, input.person_id.0],
        )?;
        enqueue_projection_repair(
            &transaction,
            &input.tenant_id,
            &input.person_id,
            EmbeddingTarget::Claim(input.claim_id.clone()),
            "tier_changed",
            input.recorded_at,
        )?;
        let record = ExportRecord::Claim(claim_record(
            &transaction,
            &input.tenant_id,
            &input.person_id,
            &input.claim_id,
        )?);
        append_commit(
            &transaction,
            &input.tenant_id,
            &input.person_id,
            input.recorded_at,
            [record],
        )?;
        record_operation(
            &transaction,
            &input.tenant_id,
            &input.person_id,
            "promote",
            "success",
            Some(EmbeddingTarget::Claim(input.claim_id.clone())),
            input.recorded_at,
        )?;
        transaction.commit()?;
        Ok(Promoted {
            claim_id: input.claim_id,
        })
    }

    pub fn archive(&mut self, input: ArchiveInput) -> Result<Archived> {
        require_scope(&input.tenant_id, &input.person_id)?;
        let transaction = self.connection.transaction()?;
        let current: (String, String, String, i64) = transaction.query_row(
            "SELECT tier, processing_state, status, recorded_from FROM claims WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3",
            params![input.claim_id.0, input.tenant_id.0, input.person_id.0],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        if current.0 == "archive" {
            return Ok(Archived {
                claim_id: input.claim_id,
            });
        }
        if current.2 == "superseded" {
            return Err(Error::Invalid("archive cannot be superseded".to_owned()));
        }
        if current.1 != "processed" {
            return Err(Error::Invalid(
                "archive requires processing_state=processed".to_owned(),
            ));
        }
        if input.recorded_at < current.3 {
            return Err(Error::Invalid(
                "archive recorded_at must not predate claim".to_owned(),
            ));
        }
        transaction.execute(
            "UPDATE claims SET tier = 'archive' WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3",
            params![input.claim_id.0, input.tenant_id.0, input.person_id.0],
        )?;
        enqueue_projection_repair(
            &transaction,
            &input.tenant_id,
            &input.person_id,
            EmbeddingTarget::Claim(input.claim_id.clone()),
            "tier_changed",
            input.recorded_at,
        )?;
        let record = ExportRecord::Claim(claim_record(
            &transaction,
            &input.tenant_id,
            &input.person_id,
            &input.claim_id,
        )?);
        append_commit(
            &transaction,
            &input.tenant_id,
            &input.person_id,
            input.recorded_at,
            [record],
        )?;
        record_operation(
            &transaction,
            &input.tenant_id,
            &input.person_id,
            "archive",
            "success",
            Some(EmbeddingTarget::Claim(input.claim_id.clone())),
            input.recorded_at,
        )?;
        transaction.commit()?;
        Ok(Archived {
            claim_id: input.claim_id,
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
        let relation = serde_json::to_string(&input.relation)?;
        let existing = transaction
            .query_row(
                "SELECT relation, confidence_basis_points FROM claim_evidence WHERE tenant_id = ?1 AND person_id = ?2 AND claim_id = ?3 AND evidence_id = ?4",
                params![input.tenant_id.0, input.person_id.0, input.claim_id.0, input.evidence_id.0],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, u16>(1)?)),
            )
            .optional()?;
        if let Some((stored_relation, stored_confidence)) = existing {
            if stored_relation == relation && stored_confidence == input.confidence_basis_points {
                transaction.commit()?;
                return Ok(());
            }
            return Err(Error::Invalid(
                "claim evidence link conflicts with existing payload".to_owned(),
            ));
        }
        transaction.execute(
            "INSERT INTO claim_evidence(tenant_id, person_id, claim_id, evidence_id, relation, confidence_basis_points) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![input.tenant_id.0, input.person_id.0, input.claim_id.0, input.evidence_id.0, relation, input.confidence_basis_points],
        )?;
        let recorded_at = transaction.query_row(
            "SELECT MAX(c.recorded_from, e.recorded_at) FROM claims c JOIN evidence e WHERE c.id = ?1 AND c.tenant_id = ?2 AND c.person_id = ?3 AND e.id = ?4 AND e.tenant_id = ?2 AND e.person_id = ?3",
            params![input.claim_id.0, input.tenant_id.0, input.person_id.0, input.evidence_id.0],
            |row| row.get(0),
        )?;
        append_commit(
            &transaction,
            &input.tenant_id,
            &input.person_id,
            recorded_at,
            [ExportRecord::ClaimEvidence(claim_evidence_record(
                &transaction,
                &input.tenant_id,
                &input.person_id,
                &input.claim_id,
                &input.evidence_id,
            )?)],
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
        append_commit(
            &transaction,
            &profile.tenant_id,
            &profile.person_id,
            profile.recorded_at,
            [ExportRecord::Profile(profile.clone())],
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
        collect_json_page(rows)
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
        let review = DailyReview {
            id: id.clone(),
            tenant_id: input.tenant_id.clone(),
            person_id: input.person_id.clone(),
            day: input.day,
            summary: input.summary,
            evidence_ids: input.evidence_ids,
            recorded_at: input.recorded_at,
        };
        append_commit(
            &transaction,
            &input.tenant_id,
            &input.person_id,
            review.recorded_at,
            [ExportRecord::DailyReview(review)],
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
        collect_json_page(rows)
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
    assert_legal_state(&claim.tier, &ClaimStatus::Accepted, &claim.processing_state)
        .map_err(|error| Error::Invalid(error.to_string()))?;
    let id = ClaimId(new_id(transaction)?);
    transaction.execute(
        "INSERT INTO claims(id, tenant_id, person_id, subject, predicate, value, kind, valid_from, recorded_from, status, tier, processing_state) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'accepted', ?10, ?11)",
        params![id.0, tenant_id.0, person_id.0, claim.subject, claim.predicate, claim.value, claim_kind_name(&claim.kind), claim.valid_from, recorded_at, tier_name(&claim.tier), processing_state_name(&claim.processing_state)],
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

fn tier_name(tier: &MemoryTier) -> &'static str {
    match tier {
        MemoryTier::ShortTerm => "short_term",
        MemoryTier::LongTerm => "long_term",
        MemoryTier::Archive => "archive",
    }
}

fn processing_state_name(state: &MemoryProcessingState) -> &'static str {
    match state {
        MemoryProcessingState::Pending => "pending",
        MemoryProcessingState::Processed => "processed",
        MemoryProcessingState::Blocked => "blocked",
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
