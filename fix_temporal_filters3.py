import re

with open("src/store/retrieval.rs", "r") as f:
    content = f.read()

# Original retrieval_item doesn't have temporal filters for source and evidence.
# So I should remove them for source and evidence in Some(as_of) branch!
# Let's fix retrieval_items code.

old_retrieval_items = content[content.find("fn retrieval_items("):content.find("fn rerank_candidates(")]

new_retrieval_items = """fn retrieval_items(
        &self,
        tenant_id: &TenantId,
        person_id: &PersonId,
        targets: &[(RetrievalTarget, u16)],
        as_of: Option<&TemporalQuery>,
    ) -> Result<Vec<RetrievalItem>> {
        if targets.is_empty() {
            return Ok(Vec::new());
        }

        let mut inputs = Vec::with_capacity(targets.len());
        for (i, (target, _relevance)) in targets.iter().enumerate() {
            let (kind, id) = match target {
                RetrievalTarget::Claim(id) => ("claim", id.0.clone()),
                RetrievalTarget::Source(id) => ("source", id.0.clone()),
                RetrievalTarget::Evidence(id) => ("evidence", id.0.clone()),
            };
            inputs.push((kind, id, i));
        }

        let inputs_json =
            serde_json::to_string(&inputs).map_err(|error| Error::Invalid(error.to_string()))?;

        let (sql, values): (String, Vec<&dyn rusqlite::ToSql>) = match as_of {
            None => (
                "
                WITH inputs AS (
                    SELECT CAST(json_extract(value, '$[2]') AS INTEGER) as idx,
                           json_extract(value, '$[0]') as kind,
                           json_extract(value, '$[1]') as id
                    FROM json_each(?1)
                )
                SELECT i.idx, i.kind, i.id,
                       c.subject || ' ' || c.predicate || ' ' || c.value as excerpt,
                       MIN(ce.evidence_id) as evidence_id
                FROM inputs i
                JOIN claims c ON c.id = i.id AND c.tenant_id = ?2 AND c.person_id = ?3 AND c.status = 'accepted' AND c.valid_until IS NULL AND c.recorded_until IS NULL AND c.tier IN ('short_term', 'long_term') AND c.processing_state = 'processed'
                JOIN claim_evidence ce ON ce.claim_id = c.id AND ce.tenant_id = ?2 AND ce.person_id = ?3 AND ce.relation = '\\\"supports\\\"'
                JOIN evidence e ON e.id = ce.evidence_id AND e.tenant_id = ?2 AND e.person_id = ?3 AND e.deleted_at IS NULL
                JOIN sources s ON s.id = e.source_id AND s.tenant_id = ?2 AND s.person_id = ?3 AND s.deleted_at IS NULL
                WHERE i.kind = 'claim'
                GROUP BY i.idx

                UNION ALL

                SELECT i.idx, i.kind, i.id, s.content as excerpt, MIN(e.id) as evidence_id
                FROM inputs i
                JOIN sources s ON s.id = i.id AND s.tenant_id = ?2 AND s.person_id = ?3 AND s.deleted_at IS NULL
                JOIN evidence e ON e.source_id = s.id AND e.tenant_id = ?2 AND e.person_id = ?3 AND e.deleted_at IS NULL
                WHERE i.kind = 'source'
                GROUP BY i.idx

                UNION ALL

                SELECT i.idx, i.kind, i.id, e.quote as excerpt, MIN(e.id) as evidence_id
                FROM inputs i
                JOIN evidence e ON e.id = i.id AND e.tenant_id = ?2 AND e.person_id = ?3 AND e.deleted_at IS NULL
                JOIN sources s ON s.id = e.source_id AND s.tenant_id = ?2 AND s.person_id = ?3 AND s.deleted_at IS NULL
                WHERE i.kind = 'evidence'
                GROUP BY i.idx

                ORDER BY idx
                ".to_string(),
                vec![&inputs_json, &tenant_id.0, &person_id.0],
            ),
            Some(as_of) => (
                "
                WITH inputs AS (
                    SELECT CAST(json_extract(value, '$[2]') AS INTEGER) as idx,
                           json_extract(value, '$[0]') as kind,
                           json_extract(value, '$[1]') as id
                    FROM json_each(?1)
                )
                SELECT i.idx, i.kind, i.id,
                       c.subject || ' ' || c.predicate || ' ' || c.value as excerpt,
                       MIN(ce.evidence_id) as evidence_id
                FROM inputs i
                JOIN claims c ON c.id = i.id AND c.tenant_id = ?2 AND c.person_id = ?3 AND c.status IN ('accepted', 'superseded') AND c.valid_from <= ?4 AND (c.valid_until IS NULL OR c.valid_until > ?4) AND c.recorded_from <= ?5 AND (c.recorded_until IS NULL OR c.recorded_until > ?5) AND c.processing_state = 'processed'
                JOIN claim_evidence ce ON ce.claim_id = c.id AND ce.tenant_id = ?2 AND ce.person_id = ?3 AND ce.relation = '\\\"supports\\\"'
                JOIN evidence e ON e.id = ce.evidence_id AND e.tenant_id = ?2 AND e.person_id = ?3 AND e.deleted_at IS NULL AND e.recorded_at <= ?5
                JOIN sources s ON s.id = e.source_id AND s.tenant_id = ?2 AND s.person_id = ?3 AND s.deleted_at IS NULL AND s.captured_at <= ?4 AND s.recorded_at <= ?5
                WHERE i.kind = 'claim'
                GROUP BY i.idx

                UNION ALL

                SELECT i.idx, i.kind, i.id, s.content as excerpt, MIN(e.id) as evidence_id
                FROM inputs i
                JOIN sources s ON s.id = i.id AND s.tenant_id = ?2 AND s.person_id = ?3 AND s.deleted_at IS NULL
                JOIN evidence e ON e.source_id = s.id AND e.tenant_id = ?2 AND e.person_id = ?3 AND e.deleted_at IS NULL
                WHERE i.kind = 'source'
                GROUP BY i.idx

                UNION ALL

                SELECT i.idx, i.kind, i.id, e.quote as excerpt, MIN(e.id) as evidence_id
                FROM inputs i
                JOIN evidence e ON e.id = i.id AND e.tenant_id = ?2 AND e.person_id = ?3 AND e.deleted_at IS NULL
                JOIN sources s ON s.id = e.source_id AND s.tenant_id = ?2 AND s.person_id = ?3 AND s.deleted_at IS NULL
                WHERE i.kind = 'evidence'
                GROUP BY i.idx

                ORDER BY idx
                ".to_string(),
                vec![
                    &inputs_json,
                    &tenant_id.0,
                    &person_id.0,
                    &as_of.valid_at,
                    &as_of.recorded_at,
                ],
            ),
        };

        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(rusqlite::params_from_iter(values), |row| {
            Ok((
                row.get::<_, usize>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;

        let mut items = Vec::with_capacity(targets.len());
        for row in rows {
            let (idx, _kind, _id, excerpt, evidence_id) = row?;
            let (target, relevance_basis_points) = &targets[idx];
            let memory = match target {
                RetrievalTarget::Claim(id) => MemoryRef::Claim(id.clone()),
                RetrievalTarget::Source(id) => MemoryRef::Source(id.clone()),
                RetrievalTarget::Evidence(id) => MemoryRef::Evidence(id.clone()),
            };

            items.push(RetrievalItem {
                memory,
                excerpt: bounded_excerpt(excerpt),
                relevance_basis_points: *relevance_basis_points,
                evidence_ids: vec![EvidenceId(evidence_id)],
            });
        }

        if items.len() < targets.len() {
            return Err(Error::NotFound);
        }

        Ok(items)
    }

"""

new_content = content.replace(old_retrieval_items, new_retrieval_items)
with open("src/store/retrieval.rs", "w") as f:
    f.write(new_content)
print("Fixed temporal filters and missing ranked targets")
