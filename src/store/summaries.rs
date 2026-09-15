use super::*;
use rusqlite::{OptionalExtension, Transaction, params};
use std::collections::{BTreeMap, BTreeSet, HashSet};

impl MemoryDb {
    pub fn nap(&mut self, input: NapInput) -> Result<Napped> {
        require_scope(&input.tenant_id, &input.person_id)?;
        require_text("summary", &input.summary)?;
        let transaction = self.connection.transaction()?;
        validate_evidence(
            &transaction,
            &input.tenant_id,
            &input.person_id,
            &input.evidence_ids,
        )?;
        let next_sequence = transaction.query_row(
            "SELECT COALESCE(MAX(end_sequence) + 1, 0) FROM summary_nodes WHERE tenant_id = ?1 AND person_id = ?2 AND level = 0",
            params![input.tenant_id.0, input.person_id.0],
            |row| row.get::<_, u64>(0),
        )?;
        let id = insert_summary(
            &transaction,
            &input.tenant_id,
            &input.person_id,
            SummaryData {
                summary: &input.summary,
                evidence_ids: &input.evidence_ids,
                child_ids: &[],
                start_sequence: next_sequence,
                end_sequence: next_sequence,
                level: 0,
                supersedes_id: None,
                recorded_at: input.recorded_at,
            },
        )?;
        transaction.commit()?;
        Ok(Napped { summary_id: id })
    }

    pub fn merge(&mut self, input: MergeInput) -> Result<Merged> {
        require_scope(&input.tenant_id, &input.person_id)?;
        require_text("summary", &input.summary)?;
        if input.child_ids.len() != 2 || input.child_ids[0] == input.child_ids[1] {
            return Err(Error::Invalid(
                "merge needs two distinct summary children".to_owned(),
            ));
        }
        let transaction = self.connection.transaction()?;
        let mut children = input
            .child_ids
            .iter()
            .map(|id| live_summary(&transaction, &input.tenant_id, &input.person_id, id))
            .collect::<Result<Vec<_>>>()?;
        children.sort_by_key(|summary| summary.start_sequence);
        let left = &children[0];
        let right = &children[1];
        if left.level != right.level
            || left.end_sequence.checked_add(1) != Some(right.start_sequence)
            || left.start_sequence % (1_u64 << (left.level + 1)) != 0
        {
            return Err(Error::Invalid(
                "merge children must be adjacent aligned summaries at the same level".to_owned(),
            ));
        }
        if input.recorded_at < left.recorded_at.max(right.recorded_at) {
            return Err(Error::Invalid(
                "merge recorded_at cannot predate its children".to_owned(),
            ));
        }
        let placeholders = (3..3 + children.len())
            .map(|index| format!("?{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let query = format!(
            "SELECT EXISTS(SELECT 1 FROM summary_nodes parent JOIN json_each(parent.child_ids) child_id WHERE parent.tenant_id = ?1 AND parent.person_id = ?2 AND parent.stale_at IS NULL AND child_id.value IN ({}))",
            placeholders
        );
        let mut query_params: Vec<&dyn rusqlite::ToSql> =
            vec![&input.tenant_id.0, &input.person_id.0];
        query_params.extend(
            children
                .iter()
                .map(|child| &child.id.0 as &dyn rusqlite::ToSql),
        );

        let has_parent: bool =
            transaction.query_row(&query, rusqlite::params_from_iter(query_params), |row| {
                row.get(0)
            })?;
        if has_parent {
            return Err(Error::Invalid(
                "summary child already has a live parent".to_owned(),
            ));
        }
        let evidence_ids = unique_evidence(&children);
        let child_ids = children
            .iter()
            .map(|child| child.id.clone())
            .collect::<Vec<_>>();
        let id = insert_summary(
            &transaction,
            &input.tenant_id,
            &input.person_id,
            SummaryData {
                summary: &input.summary,
                evidence_ids: &evidence_ids,
                child_ids: &child_ids,
                start_sequence: left.start_sequence,
                end_sequence: right.end_sequence,
                level: left.level + 1,
                supersedes_id: None,
                recorded_at: input.recorded_at,
            },
        )?;
        transaction.commit()?;
        Ok(Merged { summary_id: id })
    }

    pub fn rebuild(&mut self, input: RebuildInput) -> Result<Rebuilt> {
        require_scope(&input.tenant_id, &input.person_id)?;
        require_text("summary", &input.summary)?;
        let transaction = self.connection.transaction()?;
        let stale = stale_summary(
            &transaction,
            &input.tenant_id,
            &input.person_id,
            &input.summary_id,
        )?;
        if input.recorded_at < stale.recorded_at {
            return Err(Error::Invalid(
                "rebuild recorded_at cannot predate the stale summary".to_owned(),
            ));
        }
        let (evidence_ids, child_ids) = if stale.level == 0 {
            validate_evidence(
                &transaction,
                &input.tenant_id,
                &input.person_id,
                &input.evidence_ids,
            )?;
            (input.evidence_ids, Vec::new())
        } else {
            if !input.evidence_ids.is_empty() {
                return Err(Error::Invalid(
                    "parent rebuild derives citations from its children".to_owned(),
                ));
            }
            let children = stale
                .child_ids
                .iter()
                .map(|child_id| {
                    let child = summary_any(&transaction, &input.tenant_id, &input.person_id, child_id)?;
                    transaction
                        .query_row(
                            "SELECT id FROM summary_nodes WHERE tenant_id = ?1 AND person_id = ?2 AND start_sequence = ?3 AND end_sequence = ?4 AND level = ?5 AND stale_at IS NULL",
                            params![input.tenant_id.0, input.person_id.0, child.start_sequence, child.end_sequence, child.level],
                            |row| row.get::<_, String>(0).map(SummaryId),
                        )
                        .optional()?
                        .ok_or(Error::NotFound)
                })
                .collect::<Result<Vec<_>>>()?;
            let children = children
                .iter()
                .map(|id| live_summary(&transaction, &input.tenant_id, &input.person_id, id))
                .collect::<Result<Vec<_>>>()?;
            (
                unique_evidence(&children),
                children.into_iter().map(|child| child.id).collect(),
            )
        };
        let id = insert_summary(
            &transaction,
            &input.tenant_id,
            &input.person_id,
            SummaryData {
                summary: &input.summary,
                evidence_ids: &evidence_ids,
                child_ids: &child_ids,
                start_sequence: stale.start_sequence,
                end_sequence: stale.end_sequence,
                level: stale.level,
                supersedes_id: Some(&stale.id),
                recorded_at: input.recorded_at,
            },
        )?;
        transaction.commit()?;
        Ok(Rebuilt { summary_id: id })
    }

    pub fn wake(&self, input: WakeInput) -> Result<WakePack> {
        let tenant_id = input.search.tenant_id.clone();
        let person_id = input.search.person_id.clone();
        let mut retrieval = self.search(input.search)?;
        let summaries = live_summaries(&self.connection, &tenant_id, &person_id, input.max_bytes)?;
        let used_bytes = summaries
            .iter()
            .map(|summary| summary.summary.len() as u32)
            .sum();
        if summaries.is_empty() {
            retrieval
                .gaps
                .push("no live summary matched the wake budget".to_owned());
        }
        Ok(WakePack {
            retrieval,
            summaries,
            used_bytes,
        })
    }

    pub fn zoom(&self, input: ZoomInput) -> Result<Zoomed> {
        require_scope(&input.tenant_id, &input.person_id)?;
        let summary = live_summary(
            &self.connection,
            &input.tenant_id,
            &input.person_id,
            &input.summary_id,
        )?;
        let children = summary
            .child_ids
            .iter()
            .map(|id| live_summary(&self.connection, &input.tenant_id, &input.person_id, id))
            .collect::<Result<Vec<_>>>()?;
        Ok(Zoomed { summary, children })
    }
}

pub(super) fn invalidate_summaries_for_evidence(
    transaction: &Transaction<'_>,
    tenant_id: &TenantId,
    person_id: &PersonId,
    evidence_ids: &[EvidenceId],
    stale_at: Timestamp,
) -> Result<()> {
    if evidence_ids.is_empty() {
        return Ok(());
    }
    let values = evidence_ids
        .iter()
        .map(|id| id.0.as_str())
        .collect::<Vec<_>>();
    let placeholders = (3..3 + values.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let query = format!(
        "WITH RECURSIVE stale(id) AS (
           SELECT id FROM summary_nodes
           WHERE tenant_id = ?1 AND person_id = ?2 AND stale_at IS NULL
             AND EXISTS(SELECT 1 FROM json_each(evidence_ids) citation WHERE citation.value IN ({placeholders}))
           UNION
           SELECT parent.id FROM summary_nodes parent JOIN stale child
             ON EXISTS(SELECT 1 FROM json_each(parent.child_ids) child_id WHERE child_id.value = child.id)
           WHERE parent.tenant_id = ?1 AND parent.person_id = ?2 AND parent.stale_at IS NULL
         )
         UPDATE summary_nodes SET stale_at = ?{} WHERE id IN (SELECT id FROM stale)",
        3 + values.len()
    );
    let mut params: Vec<&dyn rusqlite::ToSql> = vec![&tenant_id.0, &person_id.0];
    params.extend(values.iter().map(|value| value as &dyn rusqlite::ToSql));
    params.push(&stale_at);
    transaction.execute(&query, rusqlite::params_from_iter(params))?;
    Ok(())
}

pub(super) fn stale_summary_count(
    transaction: &Transaction<'_>,
    tenant_id: &TenantId,
    person_id: &PersonId,
) -> Result<u32> {
    transaction.query_row(
        "SELECT COUNT(*) FROM summary_nodes WHERE tenant_id = ?1 AND person_id = ?2 AND stale_at IS NOT NULL",
        params![tenant_id.0, person_id.0],
        |row| row.get(0),
    )
    .map_err(Error::from)
}

fn validate_evidence(
    transaction: &Transaction<'_>,
    tenant_id: &TenantId,
    person_id: &PersonId,
    evidence_ids: &[EvidenceId],
) -> Result<()> {
    if evidence_ids.is_empty() {
        return Err(Error::Invalid("summary needs evidence_ids".to_owned()));
    }
    let mut seen = HashSet::new();
    for evidence_id in evidence_ids {
        if !seen.insert(&evidence_id.0) {
            return Err(Error::Invalid(
                "summary evidence_ids must be unique".to_owned(),
            ));
        }
    }

    for chunk in evidence_ids.chunks(900) {
        let placeholders = (3..3 + chunk.len())
            .map(|index| format!("?{index}"))
            .collect::<Vec<_>>()
            .join(", ");

        let query = format!(
            "SELECT COUNT(*) FROM evidence WHERE tenant_id = ?1 AND person_id = ?2 AND deleted_at IS NULL AND id IN ({placeholders})"
        );

        let mut params: Vec<&dyn rusqlite::ToSql> = vec![&tenant_id.0, &person_id.0];
        params.extend(chunk.iter().map(|id| &id.0 as &dyn rusqlite::ToSql));

        let count: usize =
            transaction.query_row(&query, rusqlite::params_from_iter(params), |row| row.get(0))?;

        if count != chunk.len() {
            // Find the specific missing evidence to produce the exact error message
            for evidence_id in chunk {
                let live: bool = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM evidence WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3 AND deleted_at IS NULL)",
                    params![evidence_id.0, tenant_id.0, person_id.0],
                    |row| row.get(0),
                )?;
                if !live {
                    return Err(Error::Invalid(format!(
                        "evidence {} is unavailable",
                        evidence_id.0
                    )));
                }
            }
        }
    }

    Ok(())
}

struct SummaryData<'a> {
    summary: &'a str,
    evidence_ids: &'a [EvidenceId],
    child_ids: &'a [SummaryId],
    start_sequence: u64,
    end_sequence: u64,
    level: u32,
    supersedes_id: Option<&'a SummaryId>,
    recorded_at: Timestamp,
}

fn insert_summary(
    transaction: &Transaction<'_>,
    tenant_id: &TenantId,
    person_id: &PersonId,
    data: SummaryData<'_>,
) -> Result<SummaryId> {
    let id = SummaryId(new_id(transaction)?);
    transaction.execute(
        "INSERT INTO summary_nodes(id, tenant_id, person_id, summary, evidence_ids, child_ids, start_sequence, end_sequence, level, supersedes_id, recorded_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![id.0, tenant_id.0, person_id.0, data.summary, serde_json::to_string(data.evidence_ids)?, serde_json::to_string(data.child_ids)?, data.start_sequence, data.end_sequence, data.level, data.supersedes_id.map(|id| &id.0), data.recorded_at],
    )?;
    Ok(id)
}

fn live_summaries(
    connection: &Connection,
    tenant_id: &TenantId,
    person_id: &PersonId,
    max_bytes: u32,
) -> Result<Vec<MemorySummary>> {
    let mut statement = connection.prepare(
        "SELECT id, summary, evidence_ids, child_ids, start_sequence, end_sequence, level, recorded_at FROM summary_nodes WHERE tenant_id = ?1 AND person_id = ?2 AND stale_at IS NULL ORDER BY start_sequence, level, id",
    )?;
    let rows = statement.query_map(params![tenant_id.0, person_id.0], |row| {
        summary_from_row_at(SummaryId(row.get(0)?), row, 1)
    })?;
    let summaries = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    let by_id = summaries
        .iter()
        .cloned()
        .map(|summary| (summary.id.clone(), summary))
        .collect::<BTreeMap<_, _>>();
    let children = summaries
        .iter()
        .flat_map(|summary| summary.child_ids.iter().cloned())
        .collect::<HashSet<_>>();
    let mut roots = summaries
        .iter()
        .filter(|summary| !children.contains(&summary.id))
        .cloned()
        .collect::<Vec<_>>();
    roots.sort_by_key(|summary| summary.start_sequence);
    let budget = max_bytes.clamp(1, MAX_READ_PAGE_BYTES as u32);
    let mut selected = Vec::new();
    let mut used = 0_u32;
    for summary in roots.into_iter().rev() {
        let bytes = summary.summary.len() as u32;
        if bytes <= budget.saturating_sub(used) {
            used += bytes;
            selected.push(summary);
        }
    }
    selected.sort_by_key(|summary| summary.start_sequence);
    while let Some(index) = selected
        .iter()
        .rposition(|summary| !summary.child_ids.is_empty())
    {
        let parent = selected[index].clone();
        let Some(children) = parent
            .child_ids
            .iter()
            .map(|id| by_id.get(id).cloned())
            .collect::<Option<Vec<_>>>()
        else {
            break;
        };
        let child_bytes = children
            .iter()
            .map(|summary| summary.summary.len() as u32)
            .sum::<u32>();
        let replacement = used - parent.summary.len() as u32 + child_bytes;
        if replacement > budget {
            break;
        }
        used = replacement;
        selected.splice(index..=index, children);
        selected.sort_by_key(|summary| summary.start_sequence);
    }
    Ok(selected)
}

fn live_summary(
    connection: &Connection,
    tenant_id: &TenantId,
    person_id: &PersonId,
    id: &SummaryId,
) -> Result<MemorySummary> {
    summary_row(connection, tenant_id, person_id, id, true)
}

fn stale_summary(
    transaction: &Transaction<'_>,
    tenant_id: &TenantId,
    person_id: &PersonId,
    id: &SummaryId,
) -> Result<MemorySummary> {
    let summary = summary_row(transaction, tenant_id, person_id, id, false)?;
    let stale: bool = transaction.query_row(
        "SELECT stale_at IS NOT NULL FROM summary_nodes WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3",
        params![id.0, tenant_id.0, person_id.0],
        |row| row.get(0),
    )?;
    if stale {
        Ok(summary)
    } else {
        Err(Error::Invalid("summary is not stale".to_owned()))
    }
}

fn summary_any(
    transaction: &Transaction<'_>,
    tenant_id: &TenantId,
    person_id: &PersonId,
    id: &SummaryId,
) -> Result<MemorySummary> {
    summary_row(transaction, tenant_id, person_id, id, false)
}

fn summary_row(
    connection: &Connection,
    tenant_id: &TenantId,
    person_id: &PersonId,
    id: &SummaryId,
    live: bool,
) -> Result<MemorySummary> {
    let stale = if live { " AND stale_at IS NULL" } else { "" };
    connection
        .query_row(
            &format!("SELECT summary, evidence_ids, child_ids, start_sequence, end_sequence, level, recorded_at FROM summary_nodes WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3{stale}"),
            params![id.0, tenant_id.0, person_id.0],
            |row| summary_from_row(id.clone(), row),
        )
        .optional()?
        .ok_or(Error::NotFound)
}

fn summary_from_row(id: SummaryId, row: &rusqlite::Row<'_>) -> rusqlite::Result<MemorySummary> {
    summary_from_row_at(id, row, 0)
}

fn summary_from_row_at(
    id: SummaryId,
    row: &rusqlite::Row<'_>,
    offset: usize,
) -> rusqlite::Result<MemorySummary> {
    let evidence_ids: String = row.get(offset + 1)?;
    let child_ids: String = row.get(offset + 2)?;
    Ok(MemorySummary {
        id,
        summary: row.get(offset)?,
        evidence_ids: serde_json::from_str(&evidence_ids).map_err(json_error)?,
        child_ids: serde_json::from_str(&child_ids).map_err(json_error)?,
        start_sequence: row.get(offset + 3)?,
        end_sequence: row.get(offset + 4)?,
        level: row.get(offset + 5)?,
        recorded_at: row.get(offset + 6)?,
    })
}

fn unique_evidence(children: &[MemorySummary]) -> Vec<EvidenceId> {
    children
        .iter()
        .flat_map(|child| child.evidence_ids.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn json_error(error: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

fn new_id(transaction: &Transaction<'_>) -> Result<String> {
    Ok(transaction.query_row("SELECT lower(hex(randomblob(16)))", [], |row| row.get(0))?)
}
