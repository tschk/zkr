use super::*;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

pub(super) fn validate_embedding(embedding: &Embedding) -> Result<()> {
    require_text("embedding model", &embedding.model)?;
    require_text("embedding version", &embedding.version)?;
    require_text("embedding input_hash", &embedding.input_hash)?;
    validate_embedding_vector(&embedding.vector, &embedding.normalization)
}

pub(super) fn validate_embedding_vector(
    vector: &[f32],
    normalization: &VectorNormalization,
) -> Result<()> {
    if vector.is_empty() || vector.iter().any(|value| !value.is_finite()) {
        return Err(Error::Invalid(
            "embedding vector must contain finite values".to_owned(),
        ));
    }
    if *normalization == VectorNormalization::L2 {
        let magnitude = vector
            .iter()
            .map(|value| f64::from(*value).powi(2))
            .sum::<f64>()
            .sqrt();
        if !magnitude.is_finite() || (magnitude - 1.0).abs() > 1e-4 {
            return Err(Error::Invalid(
                "l2-normalized embedding vector must have unit magnitude".to_owned(),
            ));
        }
    }
    Ok(())
}

pub(super) fn stored_embedding_is_valid(
    dimension: usize,
    normalization: &str,
    distance: &str,
    vector: &str,
) -> bool {
    let normalization = serde_json::from_str::<VectorNormalization>(normalization);
    let distance = serde_json::from_str::<VectorDistance>(distance);
    let vector = serde_json::from_str::<Vec<f32>>(vector);
    match (normalization, distance, vector) {
        (Ok(normalization), Ok(_), Ok(vector)) => {
            vector.len() == dimension && validate_embedding_vector(&vector, &normalization).is_ok()
        }
        _ => false,
    }
}

pub(super) fn input_hash(text: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(text.as_bytes()))
}

pub(super) fn validate_dense_query(query: &DenseQuery) -> Result<()> {
    require_text("query embedding model", &query.model)?;
    require_text("query embedding version", &query.version)?;
    if query.vector.is_empty() || query.vector.iter().any(|value| !value.is_finite()) {
        return Err(Error::Invalid(
            "query embedding vector must contain finite values".to_owned(),
        ));
    }
    Ok(())
}

pub(super) type EmbeddingLane = (usize, VectorNormalization, VectorDistance);

pub(super) fn projection_input_from(
    connection: &Connection,
    tenant_id: &TenantId,
    person_id: &PersonId,
    target: EmbeddingTarget,
) -> Result<ProjectionInput> {
    require_scope(tenant_id, person_id)?;
    let (table, id, expression, revision, live) = match &target {
        EmbeddingTarget::Source(id) => (
            "sources",
            &id.0,
            "content",
            "revision",
            "deleted_at IS NULL",
        ),
        EmbeddingTarget::Evidence(id) => (
            "evidence",
            &id.0,
            "quote",
            "source_revision",
            "deleted_at IS NULL",
        ),
        EmbeddingTarget::Claim(id) => (
            "claims",
            &id.0,
            "subject || ' ' || predicate || ' ' || value",
            "recorded_from",
            "status = 'accepted' AND valid_until IS NULL AND recorded_until IS NULL",
        ),
    };
    let (text, target_revision) = connection
        .query_row(
            &format!("SELECT {expression}, {revision} FROM {table} WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3 AND {live}"),
            params![id, tenant_id.0, person_id.0],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?
        .ok_or(Error::NotFound)?;
    Ok(ProjectionInput {
        target,
        input_hash: input_hash(&text),
        text,
        target_revision,
    })
}

pub(super) fn current_embedding_lane(
    connection: &Connection,
    tenant_id: &TenantId,
    person_id: &PersonId,
    model: &str,
    version: &str,
    replacement_target: Option<(&str, &str)>,
) -> Result<Option<EmbeddingLane>> {
    let mut statement = connection.prepare(
        "SELECT target_kind, target_id, dimension, normalization, distance, vector, target_revision, input_hash FROM embeddings WHERE tenant_id = ?1 AND person_id = ?2 AND model = ?3 AND version = ?4 ORDER BY target_kind, target_id",
    )?;
    let rows = statement.query_map(params![tenant_id.0, person_id.0, model, version], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, usize>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, i64>(6)?,
            row.get::<_, String>(7)?,
        ))
    })?;
    let mut live = Vec::new();
    for row in rows {
        let (kind, id, dimension, normalization, distance, vector, revision, hash) = row?;
        if !stored_embedding_is_valid(dimension, &normalization, &distance, &vector) {
            continue;
        }
        let Ok(target) = embedding_target(&kind, &id) else {
            continue;
        };
        let current = match projection_input_from(connection, tenant_id, person_id, target) {
            Ok(current) => current,
            Err(Error::NotFound) => continue,
            Err(error) => return Err(error),
        };
        if current.target_revision != revision || current.input_hash != hash {
            continue;
        }
        live.push((
            kind,
            id,
            (
                dimension,
                serde_json::from_str(&normalization)?,
                serde_json::from_str(&distance)?,
            ),
        ));
    }
    if live.len() == 1
        && replacement_target.is_some_and(|target| live[0].0 == target.0 && live[0].1 == target.1)
    {
        return Ok(None);
    }
    Ok(live.into_iter().map(|(_, _, lane)| lane).min())
}

pub(super) fn vector_score(
    query: &[f32],
    stored: &[f32],
    distance: &VectorDistance,
) -> Result<f32> {
    let dot = query
        .iter()
        .zip(stored)
        .map(|(left, right)| left * right)
        .sum::<f32>();
    let score = match distance {
        VectorDistance::Dot => dot,
        VectorDistance::Euclidean => -query
            .iter()
            .zip(stored)
            .map(|(left, right)| (left - right).powi(2))
            .sum::<f32>()
            .sqrt(),
        VectorDistance::Cosine => {
            let query_norm = query.iter().map(|value| value * value).sum::<f32>().sqrt();
            let stored_norm = stored.iter().map(|value| value * value).sum::<f32>().sqrt();
            if query_norm == 0.0 || stored_norm == 0.0 {
                return Err(Error::Invalid(
                    "cosine embeddings must have non-zero magnitude".to_owned(),
                ));
            }
            dot / (query_norm * stored_norm)
        }
    };
    if !score.is_finite() {
        return Err(Error::Invalid(
            "embedding similarity is not finite".to_owned(),
        ));
    }
    Ok(score)
}

pub(super) fn reciprocal_rank_fusion(
    lexical: &[RetrievalTarget],
    dense: &[RetrievalTarget],
    limit: usize,
) -> Vec<(RetrievalTarget, u16)> {
    let mut scores = HashMap::<RetrievalTarget, u32>::new();
    for ranking in [lexical, dense] {
        let mut seen = HashSet::new();
        for (offset, id) in ranking.iter().enumerate() {
            if seen.insert(id) {
                *scores.entry(id.clone()).or_default() += 1_000_000 / (61 + offset as u32);
            }
        }
    }
    let maximum = scores.values().copied().max().unwrap_or(1);
    let mut ranked = scores
        .into_iter()
        .map(|(id, score)| (id, ((score as u64 * 10_000) / maximum as u64) as u16))
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    ranked.truncate(limit);
    ranked
}

pub(super) fn embedding_target_parts(target: &EmbeddingTarget) -> (&'static str, &str) {
    match target {
        EmbeddingTarget::Source(id) => ("source", &id.0),
        EmbeddingTarget::Evidence(id) => ("evidence", &id.0),
        EmbeddingTarget::Claim(id) => ("claim", &id.0),
    }
}

pub(super) fn embedding_target(kind: &str, id: &str) -> Result<EmbeddingTarget> {
    Ok(match kind {
        "source" => EmbeddingTarget::Source(SourceId(id.to_owned())),
        "evidence" => EmbeddingTarget::Evidence(EvidenceId(id.to_owned())),
        "claim" => EmbeddingTarget::Claim(ClaimId(id.to_owned())),
        _ => {
            return Err(Error::Invalid(
                "stored embedding target is invalid".to_owned(),
            ));
        }
    })
}

impl MemoryDb {
    pub(super) fn dense_claims(
        &self,
        tenant_id: &TenantId,
        person_id: &PersonId,
        query: &DenseQuery,
    ) -> Result<Vec<RetrievalTarget>> {
        validate_dense_query(query)?;
        let lane = current_embedding_lane(
            &self.connection,
            tenant_id,
            person_id,
            &query.model,
            &query.version,
            None,
        )?;
        let mut statement = self.connection.prepare(
            "SELECT embeddings.target_kind, embeddings.target_id, embeddings.dimension, embeddings.normalization, embeddings.distance, embeddings.vector, embeddings.target_revision, embeddings.input_hash
             FROM embeddings
             WHERE embeddings.tenant_id = ?1 AND embeddings.person_id = ?2 AND embeddings.model = ?3 AND embeddings.version = ?4
             ORDER BY embeddings.target_kind, embeddings.target_id",
        )?;
        let rows = statement.query_map(
            params![tenant_id.0, person_id.0, query.model, query.version],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, usize>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, String>(7)?,
                ))
            },
        )?;
        let mut scores = HashMap::<RetrievalTarget, f32>::new();
        for row in rows {
            let (
                target_kind,
                target_id,
                dimension,
                normalization,
                distance,
                vector,
                target_revision,
                input_hash,
            ) = row?;
            let Ok(target) = embedding_target(&target_kind, &target_id) else {
                continue;
            };
            let current = match self.projection_input(tenant_id, person_id, target) {
                Ok(current) => current,
                Err(Error::NotFound) => continue,
                Err(error) => return Err(error),
            };
            if current.target_revision != target_revision || current.input_hash != input_hash {
                continue;
            }
            if !stored_embedding_is_valid(dimension, &normalization, &distance, &vector) {
                continue;
            }
            let normalization: VectorNormalization = serde_json::from_str(&normalization)?;
            let distance: VectorDistance = serde_json::from_str(&distance)?;
            let vector: Vec<f32> = serde_json::from_str(&vector)?;
            let configuration = (dimension, normalization, distance);
            if lane.as_ref().is_some_and(|lane| lane != &configuration) {
                continue;
            }
            if dimension != query.vector.len() {
                return Err(Error::Invalid(format!(
                    "query embedding dimension {} does not match stored dimension {dimension}",
                    query.vector.len()
                )));
            }
            let score = vector_score(&query.vector, &vector, &configuration.2)?;
            for target in self.retrieval_targets_for_embedding(
                tenant_id,
                person_id,
                &target_kind,
                &target_id,
            )? {
                scores
                    .entry(target)
                    .and_modify(|existing| *existing = existing.max(score))
                    .or_insert(score);
            }
        }
        let mut ranked = scores.into_iter().collect::<Vec<_>>();
        ranked.sort_by(|left, right| {
            right
                .1
                .total_cmp(&left.1)
                .then_with(|| left.0.cmp(&right.0))
        });
        Ok(ranked.into_iter().map(|(target, _)| target).collect())
    }
}

impl MemoryDb {
    pub fn rebuild_embedding<E: Embedder>(
        &mut self,
        tenant_id: TenantId,
        person_id: PersonId,
        target: EmbeddingTarget,
        embedder: &E,
    ) -> Result<StoredEmbedding> {
        require_scope(&tenant_id, &person_id)?;
        let projection = self.projection_input(&tenant_id, &person_id, target.clone())?;
        let embedding = embedder
            .embed(&projection.text)
            .map_err(|error| Error::Invalid(format!("embedder failed: {error}")))?;
        self.upsert_embedding(EmbeddingInput {
            tenant_id,
            person_id,
            target,
            embedding,
        })
    }

    pub fn upsert_embedding(&mut self, input: EmbeddingInput) -> Result<StoredEmbedding> {
        require_scope(&input.tenant_id, &input.person_id)?;
        validate_embedding(&input.embedding)?;
        let (target_kind, target_id) = embedding_target_parts(&input.target);
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let projection = projection_input_from(
            &transaction,
            &input.tenant_id,
            &input.person_id,
            input.target.clone(),
        )?;
        if input.embedding.input_hash != projection.input_hash {
            return Err(Error::Invalid(format!(
                "embedding input_hash does not match current target input; expected {}",
                projection.input_hash
            )));
        }
        let dimension = input.embedding.vector.len();
        let normalization = serde_json::to_string(&input.embedding.normalization)?;
        let distance = serde_json::to_string(&input.embedding.distance)?;
        let lane = current_embedding_lane(
            &transaction,
            &input.tenant_id,
            &input.person_id,
            &input.embedding.model,
            &input.embedding.version,
            Some((target_kind, target_id)),
        )?;
        if lane.as_ref().is_some_and(|lane| {
            lane != &(
                dimension,
                input.embedding.normalization.clone(),
                input.embedding.distance.clone(),
            )
        }) {
            return Err(Error::Invalid(
                "embedding configuration conflicts with the existing model/version lane".to_owned(),
            ));
        }
        let created_at = transaction.query_row("SELECT unixepoch()", [], |row| row.get(0))?;
        transaction.execute(
            "INSERT INTO embeddings(tenant_id, person_id, target_kind, target_id, model, version, dimension, input_hash, target_revision, created_at, normalization, distance, vector)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(tenant_id, person_id, target_kind, target_id, model, version) DO UPDATE SET dimension = excluded.dimension, input_hash = excluded.input_hash, target_revision = excluded.target_revision, created_at = excluded.created_at, normalization = excluded.normalization, distance = excluded.distance, vector = excluded.vector",
            params![input.tenant_id.0, input.person_id.0, target_kind, target_id, input.embedding.model, input.embedding.version, dimension, projection.input_hash, projection.target_revision, created_at, normalization, distance, serde_json::to_string(&input.embedding.vector)?],
        )?;
        transaction.commit()?;
        Ok(StoredEmbedding {
            target: input.target,
            dimension,
            target_revision: projection.target_revision,
            input_hash: projection.input_hash,
            created_at,
        })
    }

    pub fn projection_input(
        &self,
        tenant_id: &TenantId,
        person_id: &PersonId,
        target: EmbeddingTarget,
    ) -> Result<ProjectionInput> {
        projection_input_from(&self.connection, tenant_id, person_id, target)
    }

    pub fn projection_issues(&self, input: ProjectionAuditInput) -> Result<Vec<ProjectionIssue>> {
        require_scope(&input.tenant_id, &input.person_id)?;
        require_text("embedding model", &input.model)?;
        require_text("embedding version", &input.version)?;
        let lane = current_embedding_lane(
            &self.connection,
            &input.tenant_id,
            &input.person_id,
            &input.model,
            &input.version,
            None,
        )?;
        let mut statement = self.connection.prepare(
            "SELECT target_kind, target_id FROM (
                SELECT 'source' AS target_kind, id AS target_id FROM sources WHERE tenant_id = ?1 AND person_id = ?2 AND deleted_at IS NULL
                UNION ALL SELECT 'evidence', e.id FROM evidence e JOIN sources s ON s.id = e.source_id AND s.tenant_id = e.tenant_id AND s.person_id = e.person_id WHERE e.tenant_id = ?1 AND e.person_id = ?2 AND e.deleted_at IS NULL AND s.deleted_at IS NULL
                UNION ALL SELECT 'claim', c.id FROM claims c WHERE c.tenant_id = ?1 AND c.person_id = ?2 AND c.status = 'accepted' AND c.valid_until IS NULL AND c.recorded_until IS NULL
             ) ORDER BY target_kind, target_id",
        )?;
        let targets = statement
            .query_map(params![input.tenant_id.0, input.person_id.0], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut issues = Vec::new();
        let limit = bounded_limit(input.limit) as usize;
        for (kind, id) in targets {
            let projection = self.projection_input(
                &input.tenant_id,
                &input.person_id,
                embedding_target(&kind, &id)?,
            )?;
            let stored = self
                .connection
                .query_row(
                    "SELECT target_revision, input_hash, created_at, dimension, normalization, distance, vector FROM embeddings WHERE tenant_id = ?1 AND person_id = ?2 AND target_kind = ?3 AND target_id = ?4 AND model = ?5 AND version = ?6",
                    params![input.tenant_id.0, input.person_id.0, kind, embedding_target_parts(&projection.target).1, input.model, input.version],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?, row.get::<_, usize>(3)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?, row.get::<_, String>(6)?)),
                )
                .optional()?;
            let state = match &stored {
                None => ProjectionState::Missing,
                Some((revision, hash, _, _, _, _, _))
                    if *revision != projection.target_revision
                        || hash != &projection.input_hash =>
                {
                    ProjectionState::Stale
                }
                Some((_, _, _, dimension, normalization, distance, vector))
                    if !stored_embedding_is_valid(*dimension, normalization, distance, vector) =>
                {
                    ProjectionState::Stale
                }
                Some((_, _, _, dimension, normalization, distance, _)) => {
                    let configuration = (
                        *dimension,
                        serde_json::from_str(normalization)?,
                        serde_json::from_str(distance)?,
                    );
                    if lane
                        .as_ref()
                        .is_some_and(|expected| expected != &configuration)
                    {
                        ProjectionState::Stale
                    } else {
                        continue;
                    }
                }
            };
            issues.push(ProjectionIssue {
                state,
                input: projection,
                stored_target_revision: stored.as_ref().map(|value| value.0),
                stored_input_hash: stored.as_ref().map(|value| value.1.clone()),
                stored_created_at: stored.map(|value| value.2),
            });
            if issues.len() == limit {
                break;
            }
        }
        Ok(issues)
    }
}
