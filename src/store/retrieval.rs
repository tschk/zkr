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
        let (phrase_query, token_query) = lexical_queries(&input.query);
        let mut lexical = self.lexical_targets(
            &input.tenant_id,
            &input.person_id,
            &phrase_query,
            candidate_limit,
            input.as_of.as_ref(),
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

    pub(super) fn retrieval_targets_for_embedding(
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
}

fn lexical_queries(query: &str) -> (String, Option<String>) {
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
