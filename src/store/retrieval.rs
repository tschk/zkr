use super::*;
use std::collections::HashSet;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) enum RetrievalTarget {
    Source(SourceId),
    Evidence(EvidenceId),
    Claim(ClaimId),
}

impl MemoryDb {
    pub fn search(&self, input: SearchInput) -> Result<RetrievalPack> {
        require_scope(&input.tenant_id, &input.person_id)?;
        require_text("query", &input.query)?;
        let limit = bounded_limit(input.limit);
        let candidate_limit = limit * 4;
        let lexical = self.search_lexical(&input, candidate_limit)?;
        let dense = if input.enabled_features.is_empty() {
            input
                .query_embedding
                .as_ref()
                .filter(|_| input.as_of.is_none())
                .map(|query| self.dense_claims(&input.tenant_id, &input.person_id, query))
                .transpose()?
                .unwrap_or_default()
        } else if input.query_embedding.is_some() {
            return Err(Error::Invalid(
                "feature filtering is not supported with dense queries".to_owned(),
            ));
        } else {
            Vec::new()
        };
        let ranked = reciprocal_rank_fusion(&lexical, &dense, candidate_limit as usize);
        let ranked =
            self.rerank_candidates(&input.tenant_id, &input.person_id, ranked, limit as usize)?;
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
        self.record_exposures(&input.tenant_id, &input.person_id, &items)?;
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

    fn search_lexical(
        &self,
        input: &SearchInput,
        candidate_limit: u32,
    ) -> Result<Vec<RetrievalTarget>> {
        let (phrase_query, token_query) = lexical_queries(&input.query);
        let mut lexical = self.lexical_targets(
            &input.tenant_id,
            &input.person_id,
            &phrase_query,
            candidate_limit,
            input.as_of.as_ref(),
            &input.enabled_features,
        )?;
        let mut seen = HashSet::new();
        lexical.retain(|target| seen.insert(target.clone()));
        if lexical.len() < candidate_limit as usize {
            if let Some(token_query) = token_query {
                for target in self.lexical_targets(
                    &input.tenant_id,
                    &input.person_id,
                    &token_query,
                    candidate_limit,
                    input.as_of.as_ref(),
                    &input.enabled_features,
                )? {
                    if seen.insert(target.clone()) {
                        lexical.push(target);
                        if lexical.len() >= candidate_limit as usize {
                            break;
                        }
                    }
                }
            }
        }
        Ok(lexical)
    }

    fn lexical_targets(
        &self,
        tenant_id: &TenantId,
        person_id: &PersonId,
        query: &str,
        candidate_limit: u32,
        as_of: Option<&TemporalQuery>,
        enabled_features: &[String],
    ) -> Result<Vec<RetrievalTarget>> {
        let feature_condition = if enabled_features.is_empty() {
            "s.feature_flag IS NULL".to_string()
        } else {
            let placeholders = (5..5 + enabled_features.len())
                .map(|i| format!("?{i}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("(s.feature_flag IS NULL OR s.feature_flag IN ({placeholders}))")
        };
        let as_of_feature_condition = if enabled_features.is_empty() {
            "s.feature_flag IS NULL".to_string()
        } else {
            let placeholders = (7..7 + enabled_features.len())
                .map(|i| format!("?{i}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("(s.feature_flag IS NULL OR s.feature_flag IN ({placeholders}))")
        };
        let (sql, mut values): (&str, Vec<&dyn rusqlite::ToSql>) = match as_of {
            None => (
                &format!("SELECT s.id, c.id
                 FROM source_fts
                 JOIN sources s ON s.id = source_fts.source_id AND s.tenant_id = source_fts.tenant_id AND s.person_id = source_fts.person_id
                 JOIN evidence e ON e.source_id = s.id AND e.tenant_id = s.tenant_id AND e.person_id = s.person_id AND e.deleted_at IS NULL
                 LEFT JOIN claim_evidence ce ON ce.evidence_id = e.id AND ce.tenant_id = e.tenant_id AND ce.person_id = e.person_id AND ce.relation = '\"supports\"'
                 LEFT JOIN claims c ON c.id = ce.claim_id AND c.tenant_id = ce.tenant_id AND c.person_id = ce.person_id AND c.status = 'accepted' AND c.valid_until IS NULL AND c.recorded_until IS NULL AND c.tier IN ('short_term', 'long_term') AND c.processing_state = 'processed'
                 WHERE source_fts MATCH ?1 AND source_fts.tenant_id = ?2 AND source_fts.person_id = ?3 AND s.deleted_at IS NULL AND {feature_condition}
                 AND (c.id IS NOT NULL OR NOT EXISTS (
                     SELECT 1 FROM evidence live_e
                     JOIN claim_evidence live_ce ON live_ce.evidence_id = live_e.id AND live_ce.tenant_id = live_e.tenant_id AND live_ce.person_id = live_e.person_id AND live_ce.relation = '\"supports\"'

                     WHERE live_e.source_id = s.id AND live_e.tenant_id = s.tenant_id AND live_e.person_id = s.person_id AND live_e.deleted_at IS NULL
                 ))
                 ORDER BY bm25(source_fts), s.id, c.id LIMIT ?4"),
                vec![&query, &tenant_id.0, &person_id.0, &candidate_limit],
            ),
            Some(as_of) => (
                &format!("SELECT s.id, c.id
                 FROM source_fts
                 JOIN sources s ON s.id = source_fts.source_id AND s.tenant_id = source_fts.tenant_id AND s.person_id = source_fts.person_id
                 JOIN evidence e ON e.source_id = s.id AND e.tenant_id = s.tenant_id AND e.person_id = s.person_id AND e.deleted_at IS NULL AND e.recorded_at <= ?5
                 LEFT JOIN claim_evidence ce ON ce.evidence_id = e.id AND ce.tenant_id = e.tenant_id AND ce.person_id = e.person_id AND ce.relation = '\"supports\"'
                 LEFT JOIN claims c ON c.id = ce.claim_id AND c.tenant_id = ce.tenant_id AND c.person_id = ce.person_id AND c.status IN ('accepted', 'superseded') AND c.valid_from <= ?4 AND (c.valid_until IS NULL OR c.valid_until > ?4) AND c.recorded_from <= ?5 AND (c.recorded_until IS NULL OR c.recorded_until > ?5) AND c.processing_state = 'processed'
                 WHERE source_fts MATCH ?1 AND source_fts.tenant_id = ?2 AND source_fts.person_id = ?3 AND s.deleted_at IS NULL AND s.captured_at <= ?4 AND s.recorded_at <= ?5 AND {as_of_feature_condition}
                 AND (c.id IS NOT NULL OR NOT EXISTS (
                     SELECT 1 FROM evidence live_e
                     JOIN claim_evidence live_ce ON live_ce.evidence_id = live_e.id AND live_ce.tenant_id = live_e.tenant_id AND live_ce.person_id = live_e.person_id AND live_ce.relation = '\"supports\"'
                     JOIN claims live_c ON live_c.id = live_ce.claim_id AND live_c.tenant_id = live_ce.tenant_id AND live_c.person_id = live_ce.person_id
                     WHERE live_e.source_id = s.id AND live_e.tenant_id = s.tenant_id AND live_e.person_id = s.person_id AND live_e.deleted_at IS NULL AND live_e.recorded_at <= ?5 AND live_c.status IN ('accepted', 'superseded') AND live_c.valid_from <= ?4 AND (live_c.valid_until IS NULL OR live_c.valid_until > ?4) AND live_c.recorded_from <= ?5 AND (live_c.recorded_until IS NULL OR live_c.recorded_until > ?5) AND live_c.processing_state = 'processed'
                 ))
                 ORDER BY bm25(source_fts), s.id, c.id LIMIT ?6"),
                vec![&query, &tenant_id.0, &person_id.0, &as_of.valid_at, &as_of.recorded_at, &candidate_limit],
            ),
        };
        for feature in enabled_features {
            values.push(feature);
        }
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

    pub(super) fn retrieval_targets_for_embedding(
        &self,
        tenant_id: &TenantId,
        person_id: &PersonId,
        target_kind: &str,
        target_id: &str,
    ) -> Result<Vec<RetrievalTarget>> {
        let sql = match target_kind {
            "claim" => {
                "SELECT id FROM claims WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3 AND status = 'accepted' AND valid_until IS NULL AND recorded_until IS NULL AND tier IN ('short_term', 'long_term') AND processing_state = 'processed'"
            }
            "evidence" => {
                "SELECT c.id FROM evidence e JOIN sources s ON s.id = e.source_id AND s.tenant_id = e.tenant_id AND s.person_id = e.person_id LEFT JOIN claim_evidence ce ON ce.evidence_id = e.id AND ce.tenant_id = e.tenant_id AND ce.person_id = e.person_id AND ce.relation = '\"supports\"' LEFT JOIN claims c ON c.id = ce.claim_id AND c.tenant_id = ce.tenant_id AND c.person_id = ce.person_id AND c.status = 'accepted' AND c.valid_until IS NULL AND c.recorded_until IS NULL AND c.tier IN ('short_term', 'long_term') AND c.processing_state = 'processed' WHERE e.id = ?1 AND e.tenant_id = ?2 AND e.person_id = ?3 AND e.deleted_at IS NULL AND s.deleted_at IS NULL ORDER BY c.id"
            }
            "source" => {
                "SELECT DISTINCT c.id FROM sources s JOIN evidence e ON e.source_id = s.id AND e.tenant_id = s.tenant_id AND e.person_id = s.person_id LEFT JOIN claim_evidence ce ON ce.evidence_id = e.id AND ce.tenant_id = e.tenant_id AND ce.person_id = e.person_id AND ce.relation = '\"supports\"' LEFT JOIN claims c ON c.id = ce.claim_id AND c.tenant_id = ce.tenant_id AND c.person_id = ce.person_id AND c.status = 'accepted' AND c.valid_until IS NULL AND c.recorded_until IS NULL AND c.tier IN ('short_term', 'long_term') AND c.processing_state = 'processed' WHERE s.id = ?1 AND s.tenant_id = ?2 AND s.person_id = ?3 AND s.deleted_at IS NULL AND e.deleted_at IS NULL ORDER BY c.id"
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
                "SELECT EXISTS(SELECT 1 FROM claim_evidence ce JOIN evidence e ON e.id = ce.evidence_id AND e.tenant_id = ce.tenant_id AND e.person_id = ce.person_id JOIN claims c ON c.id = ce.claim_id AND c.tenant_id = ce.tenant_id AND c.person_id = ce.person_id AND c.status = 'accepted' AND c.valid_until IS NULL AND c.recorded_until IS NULL AND c.tier IN ('short_term', 'long_term') AND c.processing_state = 'processed' WHERE ce.relation = '\"supports\"' AND e.source_id = ?1 AND e.tenant_id = ?2 AND e.person_id = ?3)"
            }
            "evidence" => {
                "SELECT EXISTS(SELECT 1 FROM claim_evidence ce JOIN claims c ON c.id = ce.claim_id AND c.tenant_id = ce.tenant_id AND c.person_id = ce.person_id AND c.status = 'accepted' AND c.valid_until IS NULL AND c.recorded_until IS NULL AND c.tier IN ('short_term', 'long_term') AND c.processing_state = 'processed' WHERE ce.relation = '\"supports\"' AND ce.evidence_id = ?1 AND ce.tenant_id = ?2 AND ce.person_id = ?3)"
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
                 WHERE c.id = ?1 AND c.tenant_id = ?2 AND c.person_id = ?3 AND c.status = 'accepted' AND c.valid_until IS NULL AND c.recorded_until IS NULL AND c.tier IN ('short_term', 'long_term') AND c.processing_state = 'processed' AND e.deleted_at IS NULL AND s.deleted_at IS NULL
                 ORDER BY ce.evidence_id LIMIT 1",
                    vec![&id.0, &tenant_id.0, &person_id.0],
                ),
                Some(as_of) => (
                    "SELECT c.subject || ' ' || c.predicate || ' ' || c.value, ce.evidence_id
                 FROM claims c
                 JOIN claim_evidence ce ON ce.claim_id = c.id AND ce.tenant_id = c.tenant_id AND ce.person_id = c.person_id AND ce.relation = '\"supports\"'
                 JOIN evidence e ON e.id = ce.evidence_id AND e.tenant_id = ce.tenant_id AND e.person_id = ce.person_id
                 JOIN sources s ON s.id = e.source_id AND s.tenant_id = e.tenant_id AND s.person_id = e.person_id
                 WHERE c.id = ?1 AND c.tenant_id = ?2 AND c.person_id = ?3 AND c.status IN ('accepted', 'superseded') AND c.valid_from <= ?4 AND (c.valid_until IS NULL OR c.valid_until > ?4) AND c.recorded_from <= ?5 AND (c.recorded_until IS NULL OR c.recorded_until > ?5) AND c.processing_state = 'processed' AND e.deleted_at IS NULL AND e.recorded_at <= ?5 AND s.deleted_at IS NULL AND s.captured_at <= ?4 AND s.recorded_at <= ?5
                 ORDER BY ce.evidence_id LIMIT 1",
                    vec![&id.0, &tenant_id.0, &person_id.0, &as_of.valid_at, &as_of.recorded_at],
                ),
            },
            RetrievalTarget::Source(id) => (
                "SELECT s.content, e.id FROM sources s JOIN evidence e ON e.source_id = s.id AND e.tenant_id = s.tenant_id AND e.person_id = s.person_id WHERE s.id = ?1 AND s.tenant_id = ?2 AND s.person_id = ?3 AND s.deleted_at IS NULL AND e.deleted_at IS NULL ORDER BY e.id LIMIT 1",
                vec![&id.0, &tenant_id.0, &person_id.0],
            ),
            RetrievalTarget::Evidence(id) => (
                "SELECT e.quote, e.id FROM evidence e JOIN sources s ON s.id = e.source_id AND s.tenant_id = e.tenant_id AND e.person_id = s.person_id WHERE e.id = ?1 AND e.tenant_id = ?2 AND e.person_id = ?3 AND e.deleted_at IS NULL AND s.deleted_at IS NULL",
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
    fn rerank_candidates(
        &self,
        tenant_id: &TenantId,
        person_id: &PersonId,
        candidates: Vec<(RetrievalTarget, u16)>,
        limit: usize,
    ) -> Result<Vec<(RetrievalTarget, u16)>> {
        let mut candidates = candidates
            .into_iter()
            .map(|(target, relevance)| {
                let (recorded_at, exposure_count) =
                    self.retrieval_rank_metadata(tenant_id, person_id, &target)?;
                Ok((target, relevance, recorded_at, exposure_count))
            })
            .collect::<Result<Vec<_>>>()?;
        let newest_recorded_at = candidates
            .iter()
            .map(|(_, _, recorded_at, _)| *recorded_at)
            .max()
            .unwrap_or_default();
        candidates.sort_by(|left, right| {
            let left_score = rerank_score_basis_points(
                left.1,
                newest_recorded_at.saturating_sub(left.2),
                left.3,
            );
            let right_score = rerank_score_basis_points(
                right.1,
                newest_recorded_at.saturating_sub(right.2),
                right.3,
            );
            right_score
                .cmp(&left_score)
                .then_with(|| right.1.cmp(&left.1))
                .then_with(|| left.0.cmp(&right.0))
        });
        candidates.truncate(limit);
        Ok(candidates
            .into_iter()
            .map(|(target, relevance, recorded_at, exposure_count)| {
                (
                    target,
                    rerank_score_basis_points(
                        relevance,
                        newest_recorded_at.saturating_sub(recorded_at),
                        exposure_count,
                    ),
                )
            })
            .collect())
    }

    fn retrieval_rank_metadata(
        &self,
        tenant_id: &TenantId,
        person_id: &PersonId,
        target: &RetrievalTarget,
    ) -> Result<(i64, i64)> {
        let (sql, target_id) = match target {
            RetrievalTarget::Source(id) => (
                "SELECT s.recorded_at, COALESCE(rs.exposure_count, 0) FROM sources s LEFT JOIN retrieval_stats rs ON rs.tenant_id = s.tenant_id AND rs.person_id = s.person_id AND rs.target_kind = 'source' AND rs.target_id = s.id WHERE s.id = ?1 AND s.tenant_id = ?2 AND s.person_id = ?3",
                &id.0,
            ),
            RetrievalTarget::Evidence(id) => (
                "SELECT e.recorded_at, COALESCE(rs.exposure_count, 0) FROM evidence e LEFT JOIN retrieval_stats rs ON rs.tenant_id = e.tenant_id AND rs.person_id = e.person_id AND rs.target_kind = 'evidence' AND rs.target_id = e.id WHERE e.id = ?1 AND e.tenant_id = ?2 AND e.person_id = ?3",
                &id.0,
            ),
            RetrievalTarget::Claim(id) => (
                "SELECT c.recorded_from, COALESCE(rs.exposure_count, 0) FROM claims c LEFT JOIN retrieval_stats rs ON rs.tenant_id = c.tenant_id AND rs.person_id = c.person_id AND rs.target_kind = 'claim' AND rs.target_id = c.id WHERE c.id = ?1 AND c.tenant_id = ?2 AND c.person_id = ?3",
                &id.0,
            ),
        };
        self.connection
            .query_row(sql, params![target_id, tenant_id.0, person_id.0], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .optional()?
            .ok_or(Error::NotFound)
    }

    fn record_exposures(
        &self,
        tenant_id: &TenantId,
        person_id: &PersonId,
        items: &[RetrievalItem],
    ) -> Result<()> {
        let exposed_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .min(i64::MAX as u64) as i64;
        if items.is_empty() {
            return Ok(());
        }
        let mut stmt = self.connection.prepare(
            "INSERT INTO retrieval_stats(tenant_id, person_id, target_kind, target_id, exposure_count, last_exposed_at)
             VALUES(?1, ?2, ?3, ?4, 1, ?5)
             ON CONFLICT(tenant_id, person_id, target_kind, target_id)
             DO UPDATE SET exposure_count = retrieval_stats.exposure_count + 1, last_exposed_at = excluded.last_exposed_at"
        )?;
        for item in items {
            let Some((target_kind, target_id)) = (match &item.memory {
                MemoryRef::Source(id) => Some(("source", &id.0)),
                MemoryRef::Evidence(id) => Some(("evidence", &id.0)),
                MemoryRef::Claim(id) => Some(("claim", &id.0)),
                MemoryRef::ProfileEntry(_) | MemoryRef::DailyReview(_) => None,
            }) else {
                continue;
            };
            stmt.execute(params![
                tenant_id.0,
                person_id.0,
                target_kind,
                target_id,
                exposed_at
            ])?;
        }
        Ok(())
    }
}

pub(super) fn rerank_score_basis_points(
    relevance_basis_points: u16,
    age_seconds: i64,
    exposure_count: i64,
) -> u16 {
    const YEAR_SECONDS: i64 = 365 * 86_400;
    const AGE_PENALTY: i64 = 2_500;
    const EXPOSURE_PENALTY: i64 = 1_500;
    const MAX_EXPOSURES: i64 = 4;

    let age_penalty = age_seconds.clamp(0, YEAR_SECONDS) * AGE_PENALTY / YEAR_SECONDS;
    let reuse_penalty = exposure_count.clamp(0, MAX_EXPOSURES) * EXPOSURE_PENALTY / MAX_EXPOSURES;
    let multiplier = (10_000 - age_penalty - reuse_penalty).max(6_000);
    (i64::from(relevance_basis_points) * multiplier / 10_000) as u16
}

pub(super) fn lexical_queries(query: &str) -> (String, Option<String>) {
    let quote = |value: &str| format!("\"{}\"", value.replace('"', "\"\""));
    let phrase = quote(query);
    let mut seen = HashSet::new();
    let terms = query
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .filter(|term| seen.insert(term.to_lowercase()))
        .take(MAX_LEXICAL_TERMS)
        .map(quote)
        .collect::<Vec<_>>();
    let tokens = (terms.len() > 1).then(|| terms.join(" OR "));
    (phrase, tokens)
}

const MAX_LEXICAL_TERMS: usize = 32;
pub(super) const MAX_EXCERPT_BYTES: usize = 4096;

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
