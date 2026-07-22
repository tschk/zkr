use super::embeddings::{
    current_embedding_lane, embedding_target, embedding_target_parts, projection_input_from,
    stored_embedding_is_valid,
};
use super::*;
use rusqlite::{Transaction, params};

pub(super) fn enqueue_projection_repair(
    transaction: &Transaction<'_>,
    tenant_id: &TenantId,
    person_id: &PersonId,
    target: EmbeddingTarget,
    reason: &str,
    created_at: Timestamp,
) -> Result<()> {
    let (target_kind, target_id) = embedding_target_parts(&target);
    transaction.execute(
        "INSERT INTO memory_repair_outbox(id, tenant_id, person_id, target_kind, target_id, reason, created_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![new_id(transaction)?, tenant_id.0, person_id.0, target_kind, target_id, reason, created_at],
    )?;
    Ok(())
}

pub(super) fn record_operation(
    transaction: &Transaction<'_>,
    tenant_id: &TenantId,
    person_id: &PersonId,
    operation_type: &str,
    status: &str,
    target: Option<EmbeddingTarget>,
    recorded_at: Timestamp,
) -> Result<()> {
    let (target_kind, target_id) = target.as_ref().map_or((None, None), |target| {
        let (kind, id) = embedding_target_parts(target);
        (Some(kind), Some(id))
    });
    transaction.execute(
        "INSERT INTO memory_operations(id, tenant_id, person_id, operation_type, status, target_kind, target_id, recorded_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![new_id(transaction)?, tenant_id.0, person_id.0, operation_type, status, target_kind, target_id, recorded_at],
    )?;
    Ok(())
}

impl MemoryDb {
    pub fn repair_projections(&mut self, input: RepairInput) -> Result<RepairResult> {
        require_scope(&input.tenant_id, &input.person_id)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut statement = transaction.prepare(
            "SELECT id, target_kind, target_id FROM memory_repair_outbox WHERE tenant_id = ?1 AND person_id = ?2 AND processed_at IS NULL ORDER BY created_at, id LIMIT ?3",
        )?;
        let rows = statement.query_map(
            params![
                input.tenant_id.0,
                input.person_id.0,
                bounded_limit(input.limit) as i64
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )?;
        let rows: Vec<_> = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        drop(statement);
        let processed_at: Timestamp =
            transaction.query_row("SELECT unixepoch()", [], |row| row.get(0))?;
        let mut processed = 0;
        for (id, target_kind, target_id) in rows {
            let target = match embedding_target(&target_kind, &target_id) {
                Ok(target) => target,
                Err(_) => {
                    transaction.execute(
                        "UPDATE memory_repair_outbox SET processed_at = ?1 WHERE id = ?2",
                        params![processed_at, id],
                    )?;
                    continue;
                }
            };
            match projection_input_from(
                &transaction,
                &input.tenant_id,
                &input.person_id,
                target.clone(),
            ) {
                Ok(current) => {
                    let mut statement = transaction.prepare(
                        "SELECT model, version, target_revision, input_hash, dimension, normalization, distance, vector FROM embeddings WHERE tenant_id = ?1 AND person_id = ?2 AND target_kind = ?3 AND target_id = ?4",
                    )?;
                    let embedding_rows = statement.query_map(
                        params![input.tenant_id.0, input.person_id.0, target_kind, target_id],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, i64>(2)?,
                                row.get::<_, String>(3)?,
                                row.get::<_, usize>(4)?,
                                row.get::<_, String>(5)?,
                                row.get::<_, String>(6)?,
                                row.get::<_, String>(7)?,
                            ))
                        },
                    )?;
                    let embedding_rows: Vec<_> =
                        embedding_rows.collect::<std::result::Result<Vec<_>, _>>()?;
                    drop(statement);
                    for (
                        model,
                        version,
                        revision,
                        hash,
                        dimension,
                        normalization,
                        distance,
                        vector,
                    ) in embedding_rows
                    {
                        let lane = (
                            dimension,
                            serde_json::from_str::<VectorNormalization>(&normalization)?,
                            serde_json::from_str::<VectorDistance>(&distance)?,
                        );
                        let expected = current_embedding_lane(
                            &transaction,
                            &input.tenant_id,
                            &input.person_id,
                            &model,
                            &version,
                            Some((&target_kind, &target_id)),
                        )?;
                        let should_delete = revision != current.target_revision
                            || hash != current.input_hash
                            || !stored_embedding_is_valid(
                                dimension,
                                &normalization,
                                &distance,
                                &vector,
                            )
                            || expected.is_some_and(|expected| expected != lane);
                        if should_delete {
                            transaction.execute(
                                "DELETE FROM embeddings WHERE tenant_id = ?1 AND person_id = ?2 AND target_kind = ?3 AND target_id = ?4 AND model = ?5 AND version = ?6",
                                params![input.tenant_id.0, input.person_id.0, target_kind, target_id, model, version],
                            )?;
                        }
                    }
                }
                Err(Error::NotFound) => {
                    transaction.execute(
                        "DELETE FROM embeddings WHERE tenant_id = ?1 AND person_id = ?2 AND target_kind = ?3 AND target_id = ?4",
                        params![input.tenant_id.0, input.person_id.0, target_kind, target_id],
                    )?;
                    if target_kind == "source" {
                        transaction.execute(
                            "DELETE FROM source_fts WHERE source_id = ?1 AND tenant_id = ?2 AND person_id = ?3",
                            params![target_id, input.tenant_id.0, input.person_id.0],
                        )?;
                    }
                }
                Err(error) => return Err(error),
            }
            transaction.execute(
                "UPDATE memory_repair_outbox SET processed_at = ?1 WHERE id = ?2",
                params![processed_at, id],
            )?;
            processed += 1;
        }
        record_operation(
            &transaction,
            &input.tenant_id,
            &input.person_id,
            "repair",
            "success",
            None,
            processed_at,
        )?;
        transaction.commit()?;
        Ok(RepairResult { processed })
    }
}

fn new_id(transaction: &Transaction<'_>) -> Result<String> {
    Ok(transaction.query_row("SELECT lower(hex(randomblob(16)))", [], |row| row.get(0))?)
}
