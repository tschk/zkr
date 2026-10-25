use super::*;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

pub const AUTO_EMBEDDING_MODEL: &str = "zkr-auto";
pub const AUTO_EMBEDDING_VERSION: &str = "v1";
const AUTO_EMBEDDING_DIM: usize = 64;

fn default_embedding_model() -> String {
    "default".to_owned()
}

fn default_embedding_version() -> String {
    "v1".to_owned()
}

const fn default_vector_normalization() -> VectorNormalization {
    VectorNormalization::L2
}

const fn default_vector_distance() -> VectorDistance {
    VectorDistance::Cosine
}

/// Optional inline vector attached directly when remembering a project memory or record.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InlineEmbedding {
    pub vector: Vec<f32>,
    #[serde(default = "default_embedding_model")]
    pub model: String,
    #[serde(default = "default_embedding_version")]
    pub version: String,
    #[serde(default = "default_vector_normalization")]
    pub normalization: VectorNormalization,
    #[serde(default = "default_vector_distance")]
    pub distance: VectorDistance,
}

/// Deterministic, zero-dependency L2-normalized semantic feature-hashing vectorizer
/// over word tokens, token bigrams, and character n-grams.
pub fn auto_vectorize(text: &str) -> Vec<f32> {
    let mut accum = [0.0_f64; AUTO_EMBEDDING_DIM];
    let mut add_feature = |feature: &str, weight: f64| {
        let digest = Sha256::digest(feature.as_bytes());
        let idx1 = (digest[0] as usize) % AUTO_EMBEDDING_DIM;
        let idx2 = (digest[1] as usize) % AUTO_EMBEDDING_DIM;
        accum[idx1] += weight;
        accum[idx2] += weight * 0.5;
    };

    const STOPWORDS: &[&str] = &[
        "zkr",
        "project",
        "record",
        "record_type",
        "type",
        "fields",
        "connections",
        "relation",
        "target",
        "categories",
        "weight_basis_points",
        "the",
        "and",
        "for",
        "with",
        "into",
        "from",
        "that",
        "this",
        "are",
        "was",
    ];

    let words: Vec<String> = text
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| s.len() >= 2 && !STOPWORDS.contains(s))
        .map(str::to_owned)
        .collect();

    for word in &words {
        add_feature(&format!("w:{word}"), 3.0);
        let chars: Vec<char> = word.chars().collect();
        if chars.len() >= 4 {
            let stem4: String = chars[..4].iter().collect();
            add_feature(&format!("s4:{stem4}"), 1.8);
        }
        if chars.len() >= 5 {
            let stem5: String = chars[..5].iter().collect();
            add_feature(&format!("s5:{stem5}"), 2.2);
        }
        if chars.len() >= 4 {
            for win in chars.windows(4) {
                let ngram: String = win.iter().collect();
                add_feature(&format!("c4:{ngram}"), 0.35);
            }
        }
    }
    for pair in words.windows(2) {
        add_feature(&format!("b:{} {}", pair[0], pair[1]), 1.5);
    }

    let norm = accum.iter().map(|v| v * v).sum::<f64>().sqrt();
    if norm > 1e-9 {
        accum.into_iter().map(|v| (v / norm) as f32).collect()
    } else {
        let mut fallback = vec![0.0_f32; AUTO_EMBEDDING_DIM];
        fallback[0] = 1.0;
        fallback
    }
}

/// Categorize a cited fact or task across one or more categories without introducing a second memory lifecycle.
#[derive(Debug, Deserialize)]
pub struct ProjectMemoryInput {
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub embedding: Option<InlineEmbedding>,
    #[serde(flatten)]
    pub memory: RememberInput,
}

#[derive(Debug, Deserialize)]
pub struct ProjectQuery {
    pub tenant_id: TenantId,
    pub person_id: PersonId,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub projects: Vec<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub all_categories: Vec<String>,
    #[serde(default)]
    pub predicate: Option<String>,
    #[serde(default)]
    pub record_type: Option<String>,
    #[serde(default)]
    pub relation: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub where_fields: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub connected_to: Option<String>,
    #[serde(default)]
    pub max_hops: Option<u32>,
    #[serde(default)]
    pub query_embedding: Option<DenseQuery>,
    #[serde(default)]
    pub vector_query: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

impl Default for ProjectQuery {
    fn default() -> Self {
        Self {
            tenant_id: TenantId("local".to_owned()),
            person_id: PersonId("me".to_owned()),
            project: None,
            projects: Vec::new(),
            category: None,
            categories: Vec::new(),
            all_categories: Vec::new(),
            predicate: None,
            record_type: None,
            relation: None,
            target: None,
            where_fields: serde_json::Map::new(),
            connected_to: None,
            max_hops: None,
            query_embedding: None,
            vector_query: None,
            limit: default_limit(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectFact {
    pub project: String,
    pub category: String,
    pub categories: Vec<String>,
    pub claim_id: ClaimId,
    pub kind: ClaimKind,
    pub predicate: String,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record: Option<ProjectRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub similarity_basis_points: Option<i64>,
    pub evidence_ids: Vec<EvidenceId>,
}

/// An AI or caller can name a record type, its categories, its fields, and its graph connections without a schema migration.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProjectRecord {
    pub record_type: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,
    #[serde(default)]
    pub fields: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub connections: Vec<ProjectConnection>,
}

/// The target is an exact named subject, not an executable instruction or a foreign key.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProjectConnection {
    pub relation: String,
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight_basis_points: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ProjectRecordInput {
    pub record: ProjectRecord,
    #[serde(flatten)]
    pub memory: ProjectMemoryInput,
}

#[derive(Debug, Deserialize)]
pub struct DreamInput {
    pub tenant_id: TenantId,
    pub person_id: PersonId,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default = "default_dream_similarity")]
    pub min_similarity_basis_points: i64,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CategorySummary {
    pub project: String,
    pub category: String,
    pub memory_count: u32,
    pub co_occurring_categories: Vec<String>,
    pub record_types: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub centroid: Option<Vec<f32>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CategorySuggestion {
    pub claim_id: ClaimId,
    pub predicate: String,
    pub suggested_project: String,
    pub suggested_category: String,
    pub similarity_basis_points: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StructSchemaSummary {
    pub record_type: String,
    pub record_count: u32,
    pub fields: Vec<String>,
    pub relations: Vec<String>,
    pub categories: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MemoryHub {
    pub identifier: String,
    pub inbound_count: u32,
    pub outbound_count: u32,
    pub categories: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DreamAssociation {
    pub left_claim_id: ClaimId,
    pub left_subject: String,
    pub left_predicate: String,
    pub right_claim_id: ClaimId,
    pub right_subject: String,
    pub right_predicate: String,
    pub similarity_basis_points: i64,
    pub reason: String,
    pub evidence_ids: Vec<EvidenceId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UnresolvedTarget {
    pub claim_id: ClaimId,
    pub subject: String,
    pub predicate: String,
    pub relation: String,
    pub target: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DreamPack {
    pub categories: Vec<CategorySummary>,
    pub category_suggestions: Vec<CategorySuggestion>,
    pub schemas: Vec<StructSchemaSummary>,
    pub hubs: Vec<MemoryHub>,
    pub associations: Vec<DreamAssociation>,
    pub unresolved_targets: Vec<UnresolvedTarget>,
    pub contradictions: Vec<ClaimId>,
    pub short_term_claims: Vec<ClaimId>,
    pub unsummarized_evidence_ids: Vec<EvidenceId>,
    pub stale_summary_ids: Vec<SummaryId>,
}

const fn default_dream_similarity() -> i64 {
    5_000
}

fn validate_record(record: &ProjectRecord) -> Result<String> {
    let label = |name: &str, value: &str| -> Result<()> {
        require_text(name, value)?;
        if value.len() > 256 {
            return Err(Error::Invalid(format!("{name} exceeds 256 bytes")));
        }
        Ok(())
    };
    label("record_type", &record.record_type)?;
    if record.categories.len() > 32 {
        return Err(Error::Invalid("record exceeds 32 categories".into()));
    }
    for cat in &record.categories {
        label("category", cat)?;
    }
    if record.fields.len() > 64 || record.connections.len() > 64 {
        return Err(Error::Invalid(
            "record exceeds 64 fields or connections".into(),
        ));
    }
    for name in record.fields.keys() {
        label("field name", name)?;
    }
    for connection in &record.connections {
        label("relation", &connection.relation)?;
        label("target", &connection.target)?;
        if let Some(weight) = connection.weight_basis_points
            && !(1..=10_000).contains(&weight)
        {
            return Err(Error::Invalid(
                "connection weight_basis_points must be between 1 and 10000".into(),
            ));
        }
        if let Some(note) = &connection.note {
            label("connection note", note)?;
        }
    }
    let value = serde_json::to_string(record)?;
    if value.len() > 16 * 1024 {
        return Err(Error::Invalid("record exceeds 16384 bytes".into()));
    }
    Ok(value)
}

fn parse_valid_record(value: &str) -> Option<ProjectRecord> {
    serde_json::from_str::<ProjectRecord>(value)
        .ok()
        .filter(|record| validate_record(record).is_ok())
}

fn parse_project_subject(subject: &str) -> Option<(String, Vec<String>)> {
    let rest = subject.strip_prefix("zkr:project:")?;
    let parts = serde_json::from_str::<Vec<String>>(rest).ok()?;
    if parts.len() < 2 || parts.iter().any(|s| s.trim().is_empty()) {
        return None;
    }
    let mut iter = parts.into_iter();
    let project = iter.next()?;
    let mut categories = Vec::new();
    for cat in iter {
        if !categories.contains(&cat) {
            categories.push(cat);
        }
    }
    if categories.is_empty() {
        return None;
    }
    Some((project, categories))
}

fn fact_matches_node(fact: &ProjectFact, subject: &str, ident: &str) -> bool {
    fact.claim_id.0 == ident
        || fact.predicate == ident
        || subject == ident
        || fact.project == ident
        || fact.categories.iter().any(|c| {
            c == ident
                || format!("{}:{c}", fact.project) == ident
                || format!("zkr:project:[\"{}\",\"{c}\"]", fact.project) == ident
        })
}

struct LiveDreamClaim {
    id: ClaimId,
    subject: String,
    predicate: String,
    value: String,
    recorded_from: i64,
    tier: String,
    evidence_id: EvidenceId,
    record: Option<ProjectRecord>,
    project_categories: Option<(String, Vec<String>)>,
}

fn claim_points_to(from: &LiveDreamClaim, to: &LiveDreamClaim) -> bool {
    from.record.as_ref().is_some_and(|rec| {
        rec.connections.iter().any(|conn| {
            conn.target == to.id.0 || conn.target == to.subject || conn.target == to.predicate
        })
    })
}

fn claims_explicitly_connected(left: &LiveDreamClaim, right: &LiveDreamClaim) -> bool {
    claim_points_to(left, right) || claim_points_to(right, left)
}

impl MemoryDb {
    /// Store a caller-defined JSON record as the cited claim's complete value.
    /// Generic `correct` updates this serialized value; connections never grant authority.
    pub fn remember_project_record(&mut self, mut input: ProjectRecordInput) -> Result<Remembered> {
        let value = validate_record(&input.record)?;
        for cat in &input.record.categories {
            if !input.memory.categories.contains(cat) && input.memory.category.as_ref() != Some(cat)
            {
                input.memory.categories.push(cat.clone());
            }
        }
        let claim = input
            .memory
            .memory
            .claim
            .as_mut()
            .ok_or_else(|| Error::Invalid("project record requires a cited claim".into()))?;
        claim.value = value;
        self.remember_project(input.memory)
    }

    /// Use a stable predicate per fact/task (for example `task:launch:status`).
    /// Update it through `correct`; deletion, replication and evidence stay ordinary ZKR operations.
    pub fn remember_project(&mut self, mut input: ProjectMemoryInput) -> Result<Remembered> {
        require_scope(&input.memory.tenant_id, &input.memory.person_id)?;
        let project = match input.project {
            Some(p) => {
                require_text("project", &p)?;
                p
            }
            None => "general".to_owned(),
        };
        let mut categories = Vec::new();
        let mut push_category = |cat: String| -> Result<()> {
            require_text("category", &cat)?;
            if cat.len() > 256 {
                return Err(Error::Invalid("category exceeds 256 bytes".to_owned()));
            }
            if !categories.contains(&cat) {
                categories.push(cat);
            }
            Ok(())
        };
        if let Some(cat) = input.category {
            push_category(cat)?;
        }
        for cat in input.categories {
            push_category(cat)?;
        }
        if categories.is_empty() {
            return Err(Error::Invalid("category must not be empty".to_owned()));
        }
        if categories.len() > 32 {
            return Err(Error::Invalid("categories exceed 32 entries".to_owned()));
        }
        if let Some(emb) = &input.embedding {
            require_text("embedding model", &emb.model)?;
            require_text("embedding version", &emb.version)?;
            validate_embedding_vector(&emb.vector, &emb.normalization)?;
        }
        let claim =
            input.memory.claim.as_mut().ok_or_else(|| {
                Error::Invalid("project memory requires a cited claim".to_owned())
            })?;
        let mut subject_parts = Vec::with_capacity(1 + categories.len());
        subject_parts.push(project.as_str());
        for cat in &categories {
            subject_parts.push(cat.as_str());
        }
        // JSON preserves exact labels containing separators and quotes.
        claim.subject = format!("zkr:project:{}", serde_json::to_string(&subject_parts)?);
        let tenant_id = input.memory.tenant_id.clone();
        let person_id = input.memory.person_id.clone();
        let source_text = input.memory.text.clone();
        let inline_embedding = input.embedding;
        let remembered = self.remember(input.memory)?;
        if let Some(claim_id) = remembered.claim_id.clone() {
            if let Some(emb) = inline_embedding {
                let proj = projection_input_from(
                    &self.connection,
                    &tenant_id,
                    &person_id,
                    EmbeddingTarget::Claim(claim_id.clone()),
                )?;
                self.upsert_embedding(EmbeddingInput {
                    tenant_id: tenant_id.clone(),
                    person_id: person_id.clone(),
                    target: EmbeddingTarget::Claim(claim_id.clone()),
                    embedding: Embedding {
                        vector: emb.vector,
                        model: emb.model,
                        version: emb.version,
                        input_hash: proj.input_hash,
                        normalization: emb.normalization,
                        distance: emb.distance,
                    },
                })?;
            }
            if let Ok(proj) = projection_input_from(
                &self.connection,
                &tenant_id,
                &person_id,
                EmbeddingTarget::Claim(claim_id.clone()),
            ) {
                let auto_vec = auto_vectorize(&format!("{} {source_text}", proj.text));
                let _ = self.upsert_embedding(EmbeddingInput {
                    tenant_id,
                    person_id,
                    target: EmbeddingTarget::Claim(claim_id),
                    embedding: Embedding {
                        vector: auto_vec,
                        model: AUTO_EMBEDDING_MODEL.to_owned(),
                        version: AUTO_EMBEDDING_VERSION.to_owned(),
                        input_hash: proj.input_hash,
                        normalization: VectorNormalization::L2,
                        distance: VectorDistance::Cosine,
                    },
                });
            }
        }
        Ok(remembered)
    }

    /// Current, processed project facts and tasks, optionally filtered by exact category/project,
    /// multi-category sets, custom struct fields, multi-hop graph connections, and vector similarity.
    pub fn project_memories(&self, input: ProjectQuery) -> Result<Vec<ProjectFact>> {
        require_scope(&input.tenant_id, &input.person_id)?;
        if input.limit == 0 {
            return Ok(vec![]);
        }
        if let Some(project) = &input.project {
            require_text("project", project)?;
        }
        for project in &input.projects {
            require_text("project", project)?;
        }
        if let Some(category) = &input.category {
            require_text("category", category)?;
        }
        for category in &input.categories {
            require_text("category", category)?;
        }
        for category in &input.all_categories {
            require_text("category", category)?;
        }
        if let Some(predicate) = &input.predicate {
            require_text("predicate", predicate)?;
        }
        if let Some(record_type) = &input.record_type {
            require_text("record_type", record_type)?;
        }
        if let Some(relation) = &input.relation {
            require_text("relation", relation)?;
        }
        if let Some(target) = &input.target {
            require_text("target", target)?;
        }
        for key in input.where_fields.keys() {
            require_text("where_fields key", key)?;
        }
        if let Some(connected_to) = &input.connected_to {
            require_text("connected_to", connected_to)?;
        }
        if let Some(hops) = input.max_hops
            && !(1..=4).contains(&hops)
        {
            return Err(Error::Invalid("max_hops must be between 1 and 4".into()));
        }
        if let Some(dq) = &input.query_embedding {
            validate_dense_query(dq)?;
        }
        if let Some(vq) = &input.vector_query {
            require_text("vector_query", vq)?;
        }

        let mut statement = self.connection.prepare(
            "SELECT c.subject,c.id,c.kind,c.predicate,c.value,min(e.id),c.recorded_from,min(e.quote) FROM claims c
             JOIN claim_evidence ce ON ce.claim_id=c.id AND ce.tenant_id=c.tenant_id AND ce.person_id=c.person_id
             JOIN evidence e ON e.id=ce.evidence_id AND e.tenant_id=ce.tenant_id AND e.person_id=ce.person_id
             JOIN sources s ON s.id=e.source_id AND s.tenant_id=e.tenant_id AND s.person_id=e.person_id
             WHERE c.tenant_id=?1 AND c.person_id=?2 AND substr(c.subject,1,12)='zkr:project:'
             AND c.status='accepted' AND c.valid_until IS NULL AND c.recorded_until IS NULL
             AND c.tier IN ('short_term','long_term') AND c.processing_state='processed'
             AND ce.relation='\"supports\"' AND e.deleted_at IS NULL AND s.deleted_at IS NULL
             AND s.feature_flag IS NULL
             GROUP BY c.id ORDER BY c.recorded_from DESC,c.id",
        )?;
        let rows = statement.query_map(params![input.tenant_id.0, input.person_id.0], |row| {
            let subject: String = row.get(0)?;
            let Some((project, mut categories)) = parse_project_subject(&subject) else {
                return Ok(None);
            };
            let kind: String = row.get(2)?;
            let predicate: String = row.get(3)?;
            let value: String = row.get(4)?;
            let record = parse_valid_record(&value);
            if let Some(rec) = &record {
                for cat in &rec.categories {
                    if !categories.contains(cat) {
                        categories.push(cat.clone());
                    }
                }
            }
            let category = categories[0].clone();
            let recorded_from: i64 = row.get(6)?;
            let quote: String = row.get(7)?;
            Ok(Some((
                ProjectFact {
                    project,
                    category,
                    categories,
                    claim_id: ClaimId(row.get(1)?),
                    kind: serde_json::from_value(serde_json::Value::String(kind))
                        .map_err(sql_json_error)?,
                    predicate,
                    value,
                    record,
                    similarity_basis_points: None,
                    evidence_ids: vec![EvidenceId(row.get(5)?)],
                },
                subject,
                recorded_from,
                quote,
            )))
        })?;

        let mut all_candidates = Vec::new();
        for row in rows {
            if let Some(item) = row? {
                all_candidates.push(item);
            }
        }

        let activated_ids: Option<HashSet<ClaimId>> = if let Some(seed) = &input.connected_to {
            let hops = input.max_hops.unwrap_or(1);
            let mut frontier = HashSet::from([seed.clone()]);
            for (fact, subject, _, _) in &all_candidates {
                if fact_matches_node(fact, subject, seed) {
                    frontier.insert(fact.claim_id.0.clone());
                    frontier.insert(fact.predicate.clone());
                    frontier.insert(subject.clone());
                }
            }
            let mut activated = HashSet::new();
            for _ in 0..hops {
                let mut next_frontier = frontier.clone();
                let mut outgoing_targets = HashSet::new();
                for (fact, subject, _, _) in &all_candidates {
                    let is_in_frontier = frontier
                        .iter()
                        .any(|id| fact_matches_node(fact, subject, id));
                    if is_in_frontier && let Some(rec) = &fact.record {
                        for conn in &rec.connections {
                            outgoing_targets.insert(conn.target.clone());
                            next_frontier.insert(conn.target.clone());
                        }
                    }
                }
                for (fact, subject, _, _) in &all_candidates {
                    if fact_matches_node(fact, subject, seed) {
                        continue;
                    }
                    let matched_by_outgoing = outgoing_targets
                        .iter()
                        .any(|tgt| fact_matches_node(fact, subject, tgt));
                    let points_into_frontier = fact.record.as_ref().is_some_and(|rec| {
                        rec.connections.iter().any(|conn| {
                            frontier.contains(&conn.target)
                                || all_candidates.iter().any(|(f, s, _, _)| {
                                    frontier.iter().any(|id| fact_matches_node(f, s, id))
                                        && fact_matches_node(f, s, &conn.target)
                                })
                        })
                    });
                    if matched_by_outgoing || points_into_frontier {
                        activated.insert(fact.claim_id.clone());
                        next_frontier.insert(fact.claim_id.0.clone());
                        next_frontier.insert(fact.predicate.clone());
                        next_frontier.insert(subject.clone());
                    }
                }
                frontier = next_frontier;
            }
            Some(activated)
        } else {
            None
        };

        let record_type_filter = input.record_type.as_deref();
        let relation_filter = input.relation.as_deref();
        let target_filter = input.target.as_deref();

        let mut filtered = Vec::new();
        for (fact, subject, recorded_from, quote) in all_candidates {
            if let Some(active) = &activated_ids
                && !active.contains(&fact.claim_id)
            {
                continue;
            }
            if let Some(proj) = &input.project
                && &fact.project != proj
            {
                continue;
            }
            if !input.projects.is_empty() && !input.projects.iter().any(|p| p == &fact.project) {
                continue;
            }
            if let Some(cat) = &input.category
                && !fact.categories.iter().any(|c| c == cat)
            {
                continue;
            }
            if !input.categories.is_empty()
                && !input
                    .categories
                    .iter()
                    .any(|c| fact.categories.iter().any(|fc| fc == c))
            {
                continue;
            }
            if !input.all_categories.is_empty()
                && !input
                    .all_categories
                    .iter()
                    .all(|c| fact.categories.iter().any(|fc| fc == c))
            {
                continue;
            }
            if let Some(pred) = &input.predicate
                && &fact.predicate != pred
            {
                continue;
            }
            if record_type_filter.is_some() || relation_filter.is_some() || target_filter.is_some()
            {
                let Some(rec) = &fact.record else {
                    continue;
                };
                if let Some(rt) = record_type_filter
                    && rec.record_type != rt
                {
                    continue;
                }
                let matches_connection = match (relation_filter, target_filter) {
                    (Some(rel), Some(tgt)) => rec
                        .connections
                        .iter()
                        .any(|conn| conn.relation == rel && conn.target == tgt),
                    (Some(rel), None) => rec.connections.iter().any(|conn| conn.relation == rel),
                    (None, Some(tgt)) => rec.connections.iter().any(|conn| conn.target == tgt),
                    (None, None) => true,
                };
                if !matches_connection {
                    continue;
                }
            }
            if !input.where_fields.is_empty() {
                let Some(rec) = &fact.record else {
                    continue;
                };
                if !input
                    .where_fields
                    .iter()
                    .all(|(k, v)| rec.fields.get(k) == Some(v))
                {
                    continue;
                }
            }
            filtered.push((fact, subject, recorded_from, quote));
        }

        if let Some(dq) = &input.query_embedding {
            let lane = current_embedding_lane(
                &self.connection,
                &input.tenant_id,
                &input.person_id,
                &dq.model,
                &dq.version,
                None,
            )?;
            let mut emb_stmt = self.connection.prepare(
                "SELECT target_id, dimension, normalization, distance, vector, target_revision, input_hash
                 FROM embeddings
                 WHERE tenant_id = ?1 AND person_id = ?2 AND target_kind = 'claim' AND model = ?3 AND version = ?4",
            )?;
            let emb_rows = emb_stmt.query_map(
                params![input.tenant_id.0, input.person_id.0, &dq.model, &dq.version],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, usize>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, String>(6)?,
                    ))
                },
            )?;
            let mut stored_map = HashMap::new();
            for row in emb_rows {
                let (id, dim, norm_str, dist_str, vec_str, rev, hash) = row?;
                if !stored_embedding_is_valid(dim, &norm_str, &dist_str, &vec_str) {
                    continue;
                }
                let norm: VectorNormalization = serde_json::from_str(&norm_str)?;
                let dist: VectorDistance = serde_json::from_str(&dist_str)?;
                let vec: Vec<f32> = serde_json::from_str(&vec_str)?;
                if lane
                    .as_ref()
                    .is_some_and(|l| l != &(dim, norm.clone(), dist.clone()))
                {
                    continue;
                }
                stored_map.insert(id, (dim, dist, vec, rev, hash));
            }
            let mut scored = Vec::new();
            for (mut fact, subject, recorded_from, quote) in filtered {
                let Some((dim, dist, vec, rev, hash)) = stored_map.get(&fact.claim_id.0) else {
                    continue;
                };
                let expected_text = format!("{subject} {} {}", fact.predicate, fact.value);
                if *rev != recorded_from || *hash != input_hash(&expected_text) {
                    continue;
                }
                if *dim != dq.vector.len() {
                    return Err(Error::Invalid(format!(
                        "query embedding dimension {} does not match stored dimension {dim}",
                        dq.vector.len()
                    )));
                }
                let score = vector_score(&dq.vector, vec, dist)?;
                fact.similarity_basis_points = Some((f64::from(score) * 10_000.0).round() as i64);
                scored.push((fact, subject, recorded_from, quote));
            }
            scored.sort_by(|a, b| {
                b.0.similarity_basis_points
                    .cmp(&a.0.similarity_basis_points)
                    .then_with(|| b.2.cmp(&a.2))
                    .then_with(|| a.0.claim_id.cmp(&b.0.claim_id))
            });
            filtered = scored;
        } else if let Some(vq) = &input.vector_query {
            let query_vec = auto_vectorize(vq);
            for (fact, subject, _, quote) in &mut filtered {
                let text = format!("{subject} {} {} {quote}", fact.predicate, fact.value);
                let claim_vec = auto_vectorize(&text);
                let score = vector_score(&query_vec, &claim_vec, &VectorDistance::Cosine)?;
                fact.similarity_basis_points = Some((f64::from(score) * 10_000.0).round() as i64);
            }
            filtered.sort_by(|a, b| {
                b.0.similarity_basis_points
                    .cmp(&a.0.similarity_basis_points)
                    .then_with(|| b.2.cmp(&a.2))
                    .then_with(|| a.0.claim_id.cmp(&b.0.claim_id))
            });
        }

        collect_json_page(
            filtered
                .into_iter()
                .map(|(fact, _, _, _)| Ok(fact))
                .take(bounded_limit(input.limit) as usize),
        )
    }

    /// Brain-like offline consolidation and associative synthesis across live memories.
    /// Surfaces vectorized category centroids, auto-categorization suggestions, emergent struct schemas,
    /// graph hubs, semantic/structural/triadic associations, unresolved targets, contradictions,
    /// short-term claims ready for promotion, unsummarized evidence, and stale summaries.
    pub fn dream(&self, input: DreamInput) -> Result<DreamPack> {
        require_scope(&input.tenant_id, &input.person_id)?;
        let requested_lane = match (&input.model, &input.version) {
            (Some(model), Some(version)) => {
                require_text("embedding model", model)?;
                require_text("embedding version", version)?;
                Some((model.clone(), version.clone()))
            }
            (None, None) => None,
            _ => {
                return Err(Error::Invalid(
                    "dream embedding model and version must be provided together".into(),
                ));
            }
        };
        let min_similarity = input.min_similarity_basis_points.clamp(0, 10_000);
        let limit = bounded_limit(input.limit) as usize;

        let mut claim_stmt = self.connection.prepare(
            "SELECT c.id, c.subject, c.predicate, c.value, c.recorded_from, c.tier, min(e.id)
             FROM claims c
             JOIN claim_evidence ce ON ce.claim_id = c.id AND ce.tenant_id = c.tenant_id AND ce.person_id = c.person_id
             JOIN evidence e ON e.id = ce.evidence_id AND e.tenant_id = ce.tenant_id AND e.person_id = ce.person_id
             JOIN sources s ON s.id = e.source_id AND s.tenant_id = e.tenant_id AND s.person_id = e.person_id
             WHERE c.tenant_id = ?1 AND c.person_id = ?2
               AND c.status = 'accepted' AND c.valid_until IS NULL AND c.recorded_until IS NULL
               AND c.tier IN ('short_term', 'long_term') AND c.processing_state = 'processed'
               AND ce.relation = '\"supports\"' AND e.deleted_at IS NULL AND s.deleted_at IS NULL
               AND s.feature_flag IS NULL
             GROUP BY c.id
             ORDER BY c.recorded_from DESC, c.id",
        )?;
        let claims = claim_stmt
            .query_map(params![input.tenant_id.0, input.person_id.0], |row| {
                let subject: String = row.get(1)?;
                let value: String = row.get(3)?;
                let record = parse_valid_record(&value);
                let mut project_categories = parse_project_subject(&subject);
                if let (Some((_, cats)), Some(rec)) = (&mut project_categories, &record) {
                    for cat in &rec.categories {
                        if !cats.contains(cat) {
                            cats.push(cat.clone());
                        }
                    }
                }
                Ok(LiveDreamClaim {
                    id: ClaimId(row.get(0)?),
                    subject,
                    predicate: row.get(2)?,
                    value,
                    recorded_from: row.get(4)?,
                    tier: row.get(5)?,
                    evidence_id: EvidenceId(row.get(6)?),
                    record,
                    project_categories,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut known_identifiers = HashSet::new();
        for claim in &claims {
            known_identifiers.insert(claim.id.0.clone());
            known_identifiers.insert(claim.subject.clone());
            known_identifiers.insert(claim.predicate.clone());
            if let Some((proj, cats)) = &claim.project_categories {
                known_identifiers.insert(proj.clone());
                for cat in cats {
                    known_identifiers.insert(cat.clone());
                    known_identifiers.insert(format!("{proj}:{cat}"));
                    known_identifiers.insert(format!("zkr:project:[\"{proj}\",\"{cat}\"]"));
                }
            }
        }

        let mut associations = Vec::new();
        let mut paired_claims = HashSet::<(ClaimId, ClaimId)>::new();

        let active_model_version = match requested_lane {
            Some(pair) => Some(pair),
            None => self
                .connection
                .query_row(
                    "SELECT model, version FROM embeddings
                     WHERE tenant_id = ?1 AND person_id = ?2
                     ORDER BY (model != 'zkr-auto') DESC, (target_kind = 'claim') DESC, model, version
                     LIMIT 1",
                    params![input.tenant_id.0, input.person_id.0],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?
                .or_else(|| {
                    Some((
                        AUTO_EMBEDDING_MODEL.to_owned(),
                        AUTO_EMBEDDING_VERSION.to_owned(),
                    ))
                }),
        };

        let mut claim_vectors = HashMap::<ClaimId, Vec<f32>>::new();
        let mut active_distance = VectorDistance::Cosine;

        if let Some((model, version)) = active_model_version {
            let is_auto = model == AUTO_EMBEDDING_MODEL && version == AUTO_EMBEDDING_VERSION;
            let lane_opt = current_embedding_lane(
                &self.connection,
                &input.tenant_id,
                &input.person_id,
                &model,
                &version,
                None,
            )?;
            let effective_lane = lane_opt.or_else(|| {
                is_auto.then_some((
                    AUTO_EMBEDDING_DIM,
                    VectorNormalization::L2,
                    VectorDistance::Cosine,
                ))
            });

            if let Some((expected_dim, expected_norm, expected_dist)) = effective_lane {
                active_distance = expected_dist.clone();
                let claim_by_id: HashMap<&str, &LiveDreamClaim> =
                    claims.iter().map(|c| (c.id.0.as_str(), c)).collect();
                let mut emb_stmt = self.connection.prepare(
                    "SELECT target_id, dimension, normalization, distance, vector, target_revision, input_hash
                     FROM embeddings
                     WHERE tenant_id = ?1 AND person_id = ?2 AND target_kind = 'claim' AND model = ?3 AND version = ?4
                     ORDER BY target_id",
                )?;
                let emb_rows = emb_stmt.query_map(
                    params![input.tenant_id.0, input.person_id.0, &model, &version],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, usize>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, i64>(5)?,
                            row.get::<_, String>(6)?,
                        ))
                    },
                )?;
                for row in emb_rows {
                    let (target_id, dim, norm_str, dist_str, vec_str, rev, hash) = row?;
                    let Some(claim) = claim_by_id.get(target_id.as_str()) else {
                        continue;
                    };
                    if !stored_embedding_is_valid(dim, &norm_str, &dist_str, &vec_str) {
                        continue;
                    }
                    let expected_text =
                        format!("{} {} {}", claim.subject, claim.predicate, claim.value);
                    if rev != claim.recorded_from || hash != input_hash(&expected_text) {
                        continue;
                    }
                    let norm: VectorNormalization = serde_json::from_str(&norm_str)?;
                    let dist: VectorDistance = serde_json::from_str(&dist_str)?;
                    if (dim, norm, dist)
                        != (expected_dim, expected_norm.clone(), expected_dist.clone())
                    {
                        continue;
                    }
                    let vec: Vec<f32> = serde_json::from_str(&vec_str)?;
                    claim_vectors.insert(claim.id.clone(), vec);
                }

                if is_auto && expected_dim == AUTO_EMBEDDING_DIM {
                    for claim in &claims {
                        claim_vectors.entry(claim.id.clone()).or_insert_with(|| {
                            let text =
                                format!("{} {} {}", claim.subject, claim.predicate, claim.value);
                            auto_vectorize(&text)
                        });
                    }
                }

                let reason = format!("vector_similarity:{model}:{version}");
                for i in 0..claims.len() {
                    let left = &claims[i];
                    let Some(vec_left) = claim_vectors.get(&left.id) else {
                        continue;
                    };
                    for right in &claims[(i + 1)..] {
                        if left.subject == right.subject && left.predicate == right.predicate {
                            continue;
                        }
                        if claims_explicitly_connected(left, right) {
                            continue;
                        }
                        let Some(vec_right) = claim_vectors.get(&right.id) else {
                            continue;
                        };
                        let score = vector_score(vec_left, vec_right, &expected_dist)?;
                        let basis_points = (f64::from(score) * 10_000.0).round() as i64;
                        if basis_points >= min_similarity {
                            paired_claims.insert((left.id.clone(), right.id.clone()));
                            let evidence_ids = if left.evidence_id == right.evidence_id {
                                vec![left.evidence_id.clone()]
                            } else {
                                vec![left.evidence_id.clone(), right.evidence_id.clone()]
                            };
                            associations.push(DreamAssociation {
                                left_claim_id: left.id.clone(),
                                left_subject: left.subject.clone(),
                                left_predicate: left.predicate.clone(),
                                right_claim_id: right.id.clone(),
                                right_subject: right.subject.clone(),
                                right_predicate: right.predicate.clone(),
                                similarity_basis_points: basis_points,
                                reason: reason.clone(),
                                evidence_ids,
                            });
                        }
                    }
                }
            }
        }

        if 8_500 >= min_similarity {
            for i in 0..claims.len() {
                let left = &claims[i];
                let Some(left_rec) = &left.record else {
                    continue;
                };
                for right in &claims[(i + 1)..] {
                    if left.subject == right.subject && left.predicate == right.predicate {
                        continue;
                    }
                    if paired_claims.contains(&(left.id.clone(), right.id.clone()))
                        || claims_explicitly_connected(left, right)
                    {
                        continue;
                    }
                    let Some(right_rec) = &right.record else {
                        continue;
                    };
                    if let Some(shared) = left_rec.connections.iter().find(|lc| {
                        right_rec
                            .connections
                            .iter()
                            .any(|rc| rc.target == lc.target)
                    }) {
                        paired_claims.insert((left.id.clone(), right.id.clone()));
                        let evidence_ids = if left.evidence_id == right.evidence_id {
                            vec![left.evidence_id.clone()]
                        } else {
                            vec![left.evidence_id.clone(), right.evidence_id.clone()]
                        };
                        associations.push(DreamAssociation {
                            left_claim_id: left.id.clone(),
                            left_subject: left.subject.clone(),
                            left_predicate: left.predicate.clone(),
                            right_claim_id: right.id.clone(),
                            right_subject: right.subject.clone(),
                            right_predicate: right.predicate.clone(),
                            similarity_basis_points: 8_500,
                            reason: format!("shared_target:{}", shared.target),
                            evidence_ids,
                        });
                    }
                }
            }
        }

        if 8_000 >= min_similarity {
            for i in 0..claims.len() {
                let left = &claims[i];
                for right in &claims[(i + 1)..] {
                    if left.subject == right.subject && left.predicate == right.predicate {
                        continue;
                    }
                    if paired_claims.contains(&(left.id.clone(), right.id.clone()))
                        || claims_explicitly_connected(left, right)
                    {
                        continue;
                    }
                    if let Some(bridge) = claims.iter().find(|b| {
                        b.id != left.id
                            && b.id != right.id
                            && ((claim_points_to(left, b) && claim_points_to(b, right))
                                || (claim_points_to(right, b) && claim_points_to(b, left)))
                    }) {
                        paired_claims.insert((left.id.clone(), right.id.clone()));
                        let evidence_ids = if left.evidence_id == right.evidence_id {
                            vec![left.evidence_id.clone()]
                        } else {
                            vec![left.evidence_id.clone(), right.evidence_id.clone()]
                        };
                        associations.push(DreamAssociation {
                            left_claim_id: left.id.clone(),
                            left_subject: left.subject.clone(),
                            left_predicate: left.predicate.clone(),
                            right_claim_id: right.id.clone(),
                            right_subject: right.subject.clone(),
                            right_predicate: right.predicate.clone(),
                            similarity_basis_points: 8_000,
                            reason: format!("triadic_closure:{}", bridge.predicate),
                            evidence_ids,
                        });
                    }
                }
            }
        }

        let cross_subject = |a: &DreamAssociation| u8::from(a.left_subject != a.right_subject);
        associations.sort_by(|a, b| {
            cross_subject(b)
                .cmp(&cross_subject(a))
                .then_with(|| b.similarity_basis_points.cmp(&a.similarity_basis_points))
                .then_with(|| a.left_claim_id.cmp(&b.left_claim_id))
                .then_with(|| a.right_claim_id.cmp(&b.right_claim_id))
        });
        associations.truncate(limit);

        // Aggregate category summaries and compute prototype centroids
        #[derive(Default)]
        struct CatAccum {
            count: u32,
            co_occurring: BTreeSet<String>,
            record_types: BTreeSet<String>,
            vectors: Vec<Vec<f32>>,
        }
        let mut cat_map = BTreeMap::<(String, String), CatAccum>::new();
        for claim in &claims {
            if let Some((proj, cats)) = &claim.project_categories {
                for cat in cats {
                    let entry = cat_map.entry((proj.clone(), cat.clone())).or_default();
                    entry.count += 1;
                    for other in cats {
                        if other != cat {
                            entry.co_occurring.insert(other.clone());
                        }
                    }
                    if let Some(rec) = &claim.record {
                        entry.record_types.insert(rec.record_type.clone());
                    }
                    if let Some(vec) = claim_vectors.get(&claim.id) {
                        entry.vectors.push(vec.clone());
                    }
                }
            }
        }
        let mut categories: Vec<CategorySummary> = cat_map
            .into_iter()
            .map(|((project, category), acc)| {
                let centroid = if !acc.vectors.is_empty() {
                    let dim = acc.vectors[0].len();
                    let mut mean = vec![0.0_f64; dim];
                    let n = acc.vectors.len() as f64;
                    for vec in &acc.vectors {
                        for (m, v) in mean.iter_mut().zip(vec.iter()) {
                            *m += f64::from(*v) / n;
                        }
                    }
                    let norm = mean.iter().map(|v| v * v).sum::<f64>().sqrt();
                    if norm > 1e-9 {
                        Some(mean.into_iter().map(|v| (v / norm) as f32).collect())
                    } else {
                        Some(mean.into_iter().map(|v| v as f32).collect())
                    }
                } else {
                    None
                };
                CategorySummary {
                    project,
                    category,
                    memory_count: acc.count,
                    co_occurring_categories: acc.co_occurring.into_iter().collect(),
                    record_types: acc.record_types.into_iter().collect(),
                    centroid,
                }
            })
            .collect();
        categories.sort_by(|a, b| {
            b.memory_count
                .cmp(&a.memory_count)
                .then_with(|| a.project.cmp(&b.project))
                .then_with(|| a.category.cmp(&b.category))
        });

        // Vectorized auto-categorization suggestions against category centroids
        let mut category_suggestions = Vec::new();
        for claim in &claims {
            let Some(claim_vec) = claim_vectors.get(&claim.id) else {
                continue;
            };
            let existing_cats: &[String] = claim
                .project_categories
                .as_ref()
                .map_or(&[], |(_, cats)| cats.as_slice());
            for cat_summary in &categories {
                if existing_cats.iter().any(|c| c == &cat_summary.category) {
                    continue;
                }
                let Some(centroid) = &cat_summary.centroid else {
                    continue;
                };
                if claim_vec.len() != centroid.len() {
                    continue;
                }
                let score = vector_score(claim_vec, centroid, &active_distance)?;
                let bps = (f64::from(score) * 10_000.0).round() as i64;
                if bps >= min_similarity {
                    category_suggestions.push(CategorySuggestion {
                        claim_id: claim.id.clone(),
                        predicate: claim.predicate.clone(),
                        suggested_project: cat_summary.project.clone(),
                        suggested_category: cat_summary.category.clone(),
                        similarity_basis_points: bps,
                    });
                }
            }
        }
        category_suggestions.sort_by(|a, b| {
            b.similarity_basis_points
                .cmp(&a.similarity_basis_points)
                .then_with(|| a.claim_id.cmp(&b.claim_id))
                .then_with(|| a.suggested_project.cmp(&b.suggested_project))
                .then_with(|| a.suggested_category.cmp(&b.suggested_category))
        });
        category_suggestions.truncate(limit);
        categories.truncate(limit);

        // Aggregate struct schema summaries
        #[derive(Default)]
        struct SchemaAccum {
            count: u32,
            fields: BTreeSet<String>,
            relations: BTreeSet<String>,
            categories: BTreeSet<String>,
        }
        let mut schema_map = BTreeMap::<String, SchemaAccum>::new();
        for claim in &claims {
            if let Some(rec) = &claim.record {
                let entry = schema_map.entry(rec.record_type.clone()).or_default();
                entry.count += 1;
                for field in rec.fields.keys() {
                    entry.fields.insert(field.clone());
                }
                for conn in &rec.connections {
                    entry.relations.insert(conn.relation.clone());
                }
                if let Some((_, cats)) = &claim.project_categories {
                    for cat in cats {
                        entry.categories.insert(cat.clone());
                    }
                }
            }
        }
        let mut schemas: Vec<StructSchemaSummary> = schema_map
            .into_iter()
            .map(|(record_type, acc)| StructSchemaSummary {
                record_type,
                record_count: acc.count,
                fields: acc.fields.into_iter().collect(),
                relations: acc.relations.into_iter().collect(),
                categories: acc.categories.into_iter().collect(),
            })
            .collect();
        schemas.sort_by(|a, b| {
            b.record_count
                .cmp(&a.record_count)
                .then_with(|| a.record_type.cmp(&b.record_type))
        });
        schemas.truncate(limit);

        // Aggregate graph hubs (inbound & outbound connection degree)
        #[derive(Default)]
        struct HubAccum {
            inbound: u32,
            outbound: u32,
            categories: BTreeSet<String>,
        }
        let mut hub_map = BTreeMap::<String, HubAccum>::new();
        for claim in &claims {
            if let Some(rec) = &claim.record
                && !rec.connections.is_empty()
            {
                let src_entry = hub_map.entry(claim.predicate.clone()).or_default();
                src_entry.outbound += rec.connections.len() as u32;
                if let Some((_, cats)) = &claim.project_categories {
                    for cat in cats {
                        src_entry.categories.insert(cat.clone());
                    }
                }
                for conn in &rec.connections {
                    let tgt_entry = hub_map.entry(conn.target.clone()).or_default();
                    tgt_entry.inbound += 1;
                }
            }
        }
        for claim in &claims {
            if let Some((_, cats)) = &claim.project_categories {
                if let Some(entry) = hub_map.get_mut(&claim.predicate) {
                    for cat in cats {
                        entry.categories.insert(cat.clone());
                    }
                }
                if let Some(entry) = hub_map.get_mut(&claim.id.0) {
                    for cat in cats {
                        entry.categories.insert(cat.clone());
                    }
                }
            }
        }
        let mut hubs: Vec<MemoryHub> = hub_map
            .into_iter()
            .filter(|(_, acc)| acc.inbound + acc.outbound > 0)
            .map(|(identifier, acc)| MemoryHub {
                identifier,
                inbound_count: acc.inbound,
                outbound_count: acc.outbound,
                categories: acc.categories.into_iter().collect(),
            })
            .collect();
        hubs.sort_by(|a, b| {
            (b.inbound_count + b.outbound_count)
                .cmp(&(a.inbound_count + a.outbound_count))
                .then_with(|| b.inbound_count.cmp(&a.inbound_count))
                .then_with(|| a.identifier.cmp(&b.identifier))
        });
        hubs.truncate(limit);

        let mut unresolved_targets = Vec::new();
        'outer: for claim in &claims {
            if let Some(rec) = &claim.record {
                for conn in &rec.connections {
                    if !known_identifiers.contains(&conn.target) {
                        unresolved_targets.push(UnresolvedTarget {
                            claim_id: claim.id.clone(),
                            subject: claim.subject.clone(),
                            predicate: claim.predicate.clone(),
                            relation: conn.relation.clone(),
                            target: conn.target.clone(),
                        });
                        if unresolved_targets.len() == limit {
                            break 'outer;
                        }
                    }
                }
            }
        }

        let mut contradiction_stmt = self.connection.prepare(
            "SELECT c.id
             FROM claims c
             JOIN claim_evidence ce ON ce.claim_id = c.id AND ce.tenant_id = c.tenant_id AND ce.person_id = c.person_id
             JOIN evidence e ON e.id = ce.evidence_id AND e.tenant_id = ce.tenant_id AND e.person_id = ce.person_id
             JOIN sources s ON s.id = e.source_id AND s.tenant_id = e.tenant_id AND s.person_id = e.person_id
             WHERE c.tenant_id = ?1 AND c.person_id = ?2
               AND c.status = 'accepted' AND c.valid_until IS NULL AND c.recorded_until IS NULL
               AND c.tier IN ('short_term', 'long_term') AND c.processing_state = 'processed'
               AND ce.relation = '\"contradicts\"' AND e.deleted_at IS NULL AND s.deleted_at IS NULL
               AND s.feature_flag IS NULL
             GROUP BY c.id
             ORDER BY c.recorded_from DESC, c.id
             LIMIT ?3",
        )?;
        let contradictions = contradiction_stmt
            .query_map(
                params![input.tenant_id.0, input.person_id.0, limit as i64],
                |row| row.get::<_, String>(0).map(ClaimId),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let short_term_claims = claims
            .iter()
            .filter(|c| c.tier == "short_term")
            .take(limit)
            .map(|c| c.id.clone())
            .collect();

        let mut unsummarized_stmt = self.connection.prepare(
            "SELECT e.id
             FROM evidence e
             JOIN sources s ON s.id = e.source_id AND s.tenant_id = e.tenant_id AND s.person_id = e.person_id
             WHERE e.tenant_id = ?1 AND e.person_id = ?2
               AND e.deleted_at IS NULL AND s.deleted_at IS NULL AND s.feature_flag IS NULL
               AND NOT EXISTS (
                   SELECT 1 FROM summary_nodes sn JOIN json_each(sn.evidence_ids) cited
                   WHERE sn.tenant_id = e.tenant_id AND sn.person_id = e.person_id
                     AND sn.stale_at IS NULL AND cited.value = e.id
               )
             ORDER BY e.recorded_at DESC, e.id
             LIMIT ?3",
        )?;
        let unsummarized_evidence_ids = unsummarized_stmt
            .query_map(
                params![input.tenant_id.0, input.person_id.0, limit as i64],
                |row| row.get::<_, String>(0).map(EvidenceId),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut stale_summary_stmt = self.connection.prepare(
            "SELECT sn.id
             FROM summary_nodes sn
             WHERE sn.tenant_id = ?1 AND sn.person_id = ?2
               AND sn.stale_at IS NOT NULL
               AND NOT EXISTS (
                   SELECT 1 FROM summary_nodes live
                   WHERE live.tenant_id = sn.tenant_id AND live.person_id = sn.person_id
                     AND live.start_sequence = sn.start_sequence
                     AND live.end_sequence = sn.end_sequence
                     AND live.level = sn.level
                     AND live.stale_at IS NULL
               )
             ORDER BY sn.level ASC, sn.start_sequence ASC, sn.recorded_at DESC, sn.id
             LIMIT ?3",
        )?;
        let stale_summary_ids = stale_summary_stmt
            .query_map(
                params![input.tenant_id.0, input.person_id.0, limit as i64],
                |row| row.get::<_, String>(0).map(SummaryId),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(DreamPack {
            categories,
            category_suggestions,
            schemas,
            hubs,
            associations,
            unresolved_targets,
            contradictions,
            short_term_claims,
            unsummarized_evidence_ids,
            stale_summary_ids,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn memory(project: &str, category: &str, person: &str, value: &str) -> ProjectMemoryInput {
        serde_json::from_value(json!({
            "project":project,"category":category,"tenant_id":"local","person_id":person,
            "kind":"conversation","text":format!("Launch status: {value}"),"captured_at":10,"recorded_at":10,
            "claim":{"subject":"ignored","predicate":"task:launch:status","value":value,"kind":"task","valid_from":10}
        })).unwrap()
    }

    fn query(person: &str, project: Option<&str>, category: Option<&str>) -> ProjectQuery {
        ProjectQuery {
            tenant_id: TenantId("local".into()),
            person_id: PersonId(person.into()),
            project: project.map(str::to_owned),
            category: category.map(str::to_owned),
            limit: 100,
            ..Default::default()
        }
    }

    #[test]
    fn projects_and_categories_are_exact_scoped_and_cited() {
        let mut db = MemoryDb::open(":memory:").unwrap();
        let remembered = db
            .remember_project(memory("Tofu", "work", "me", "planned"))
            .unwrap();
        db.remember_project(memory("Tofu", "personal", "me", "blocked"))
            .unwrap();
        db.remember_project(memory("Other", "work", "me", "active"))
            .unwrap();
        db.remember_project(memory("Tofu", "work", "someone", "private"))
            .unwrap();
        let facts = db
            .project_memories(query("me", Some("Tofu"), Some("work")))
            .unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].value, "planned");
        assert_eq!(facts[0].categories, vec!["work"]);
        assert_eq!(facts[0].evidence_ids, vec![remembered.evidence_id]);
        assert_eq!(facts[0].kind, ClaimKind::Task);
        assert_eq!(
            db.project_memories(query("me", Some("Tofu"), None))
                .unwrap()
                .len(),
            2
        );
        let mut other_tenant = query("me", None, None);
        other_tenant.tenant_id = TenantId("elsewhere".into());
        assert!(db.project_memories(other_tenant).unwrap().is_empty());
        assert_eq!(
            db.project_memories(query("me", None, Some("work")))
                .unwrap()
                .len(),
            2
        );
    }



    #[test]
    fn project_memories_core_functionality_and_filters() {
        let mut db = MemoryDb::open(":memory:").unwrap();

        // Seed database
        db.remember_project(memory("CoreProject", "backend", "me", "active")).unwrap();
        db.remember_project(memory("CoreProject", "frontend", "me", "planned")).unwrap();
        db.remember_project(memory("OtherProject", "backend", "me", "done")).unwrap();

        // Test basic retrieval by project
        let mut q = query("me", Some("CoreProject"), None);
        let facts = db.project_memories(query("me", Some("CoreProject"), None)).unwrap();
        assert_eq!(facts.len(), 2, "Should retrieve all facts for CoreProject");

        // Test limit
        q.limit = 1;
        let facts_limited = db.project_memories(q).unwrap();
        assert_eq!(facts_limited.len(), 1, "Should respect limit");

        // Test category filtering
        let q_cat = query("me", Some("CoreProject"), Some("frontend"));
        let facts_cat = db.project_memories(q_cat).unwrap();
        assert_eq!(facts_cat.len(), 1, "Should filter by category");
        assert_eq!(facts_cat[0].value, "planned");

        // Test filtering by non-existent project
        let q_none = query("me", Some("NonExistent"), None);
        let facts_none = db.project_memories(q_none).unwrap();
        assert!(facts_none.is_empty(), "Should return empty for non-existent project");
    }

    #[test]
    fn project_memories_limit_zero_returns_empty_and_respects_authorization() {
        let db = MemoryDb::open(":memory:").unwrap();

        let mut unauth_input = query("me", None, None);
        unauth_input.tenant_id = TenantId("".into());
        unauth_input.limit = 0;

        assert!(db.project_memories(unauth_input).is_err(), "Authorization check should fail before limit=0 early return");

        let mut auth_input = query("me", None, None);
        auth_input.limit = 0;

        let result = db.project_memories(auth_input).unwrap();
        assert!(result.is_empty(), "Result should be empty when limit=0");
    }

    #[test]
    fn correction_and_source_deletion_preserve_project_lifecycle() {
        let mut db = MemoryDb::open(":memory:").unwrap();
        let original = db
            .remember_project(memory("Tofu", "work", "me", "planned"))
            .unwrap();
        let corrected = db
            .correct(CorrectInput {
                tenant_id: TenantId("local".into()),
                person_id: PersonId("me".into()),
                claim_id: original.claim_id.unwrap(),
                text: "Launch is completed".into(),
                value: "completed".into(),
                valid_at: 20,
                recorded_at: 20,
            })
            .unwrap();
        let facts = db
            .project_memories(query("me", Some("Tofu"), None))
            .unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].value, "completed");
        assert_eq!(facts[0].claim_id, corrected.claim_id);
        db.delete_source(DeleteInput {
            tenant_id: TenantId("local".into()),
            person_id: PersonId("me".into()),
            source_id: corrected.source_id,
            deleted_at: 30,
        })
        .unwrap();
        assert!(
            db.project_memories(query("me", Some("Tofu"), None))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn project_memory_replicates_through_existing_export_apply() {
        let mut source = MemoryDb::open(":memory:").unwrap();
        source
            .remember_project(memory("Tofu", "work", "me", "planned"))
            .unwrap();
        let page = source
            .export(ExportInput {
                export_format: EXPORT_FORMAT_VERSION,
                tenant_id: TenantId("local".into()),
                person_id: PersonId("me".into()),
                after_commit: 0,
                after_event_index: -1,
                high_water_mark: None,
                limit: 100,
            })
            .unwrap();
        assert!(page.complete);
        let mut replica = MemoryDb::open(":memory:").unwrap();
        replica
            .apply(ApplyInput {
                export_format: page.export_format,
                database_schema_version: Some(page.database_schema_version),
                tenant_id: TenantId("local".into()),
                person_id: PersonId("me".into()),
                commits: page.commits,
            })
            .unwrap();
        let facts = replica
            .project_memories(query("me", Some("Tofu"), Some("work")))
            .unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].value, "planned");
    }

    #[test]
    fn ai_defined_records_and_connections_keep_citations_and_lifecycle() {
        let mut db = MemoryDb::open(":memory:").unwrap();
        let record: ProjectRecord = serde_json::from_value(json!({
            "record_type":"work_item",
            "fields":{"title":"Launch", "priority":2, "tags":["work"], "details":{"owner":"me"}},
            "connections":[{"relation":"blocked_by", "target":"cupboard:pairing"}]
        }))
        .unwrap();
        let remembered = db
            .remember_project_record(ProjectRecordInput {
                record: record.clone(),
                memory: memory("Tofu", "AI-created/launch", "me", "replaced"),
            })
            .unwrap();
        let facts = db
            .project_memories(query("me", Some("Tofu"), Some("AI-created/launch")))
            .unwrap();
        let stored: ProjectRecord = serde_json::from_str(&facts[0].value).unwrap();
        assert_eq!(stored, record);
        assert_eq!(facts[0].evidence_ids, vec![remembered.evidence_id]);
        let mut updated = stored;
        updated.fields.insert("priority".into(), json!(1));
        let corrected = db
            .correct(CorrectInput {
                tenant_id: TenantId("local".into()),
                person_id: PersonId("me".into()),
                claim_id: remembered.claim_id.unwrap(),
                text: "Launch is now priority one".into(),
                value: validate_record(&updated).unwrap(),
                valid_at: 20,
                recorded_at: 20,
            })
            .unwrap();
        let facts = db
            .project_memories(query("me", Some("Tofu"), None))
            .unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(
            serde_json::from_str::<ProjectRecord>(&facts[0].value).unwrap(),
            updated
        );
        assert!(
            db.project_memories(query("other", Some("Tofu"), None))
                .unwrap()
                .is_empty()
        );
        db.delete_source(DeleteInput {
            tenant_id: TenantId("local".into()),
            person_id: PersonId("me".into()),
            source_id: corrected.source_id,
            deleted_at: 30,
        })
        .unwrap();
        assert!(
            db.project_memories(query("me", Some("Tofu"), None))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn malformed_subjects_do_not_break_or_starve_valid_project_memories() {
        let mut db = MemoryDb::open(":memory:").unwrap();
        db.remember_project(memory("Tofu", "work", "me", "visible"))
            .unwrap();
        for subject in [
            r#"zkr:project:["\ud800","work"]"#,
            r#"zkr:project:[" ",""]"#,
        ] {
            let mut input = memory("unused", "unused", "me", "malformed").memory;
            input.recorded_at = 20;
            input.claim.as_mut().unwrap().subject = subject.into();
            db.remember(input).unwrap();
        }
        let mut input = query("me", None, None);
        input.limit = 1;
        let facts = db.project_memories(input).unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].project, "Tofu");
        assert_eq!(facts[0].value, "visible");
    }

    #[test]
    fn record_boundaries_reject_invalid_labels_and_oversized_data_before_writing() {
        let mut db = MemoryDb::open(":memory:").unwrap();
        for record in [
            json!({"record_type":" ","fields":{}}),
            json!({"record_type":"meeting","fields":{"":"invalid"}}),
            json!({"record_type":"meeting","fields":{"notes":"x".repeat(16384)}}),
            json!({"record_type":"meeting","fields":{},"connections":[{"relation":" ","target":"someone"}]}),
            json!({"record_type":"meeting","fields":{},"connections":[{"relation":"with","target":""}]}),
            json!({"record_type":"meeting","fields":{},"connections":[{"relation":"with","target":"someone","weight_basis_points":0}]}),
            json!({"record_type":"meeting","fields":{},"connections":[{"relation":"with","target":"someone","weight_basis_points":10001}]}),
            json!({"record_type":"meeting","fields":{},"connections":[{"relation":"with","target":"someone","note":" "}]}),
            json!({"record_type":"meeting","fields":{},"connections":vec![json!({"relation":"with","target":"someone"});65]}),
        ] {
            let input = ProjectRecordInput {
                record: serde_json::from_value(record).unwrap(),
                memory: memory("Tofu", "work", "me", "ignored"),
            };
            assert!(db.remember_project_record(input).is_err());
            assert!(
                db.project_memories(query("me", None, None))
                    .unwrap()
                    .is_empty()
            );
        }
    }

    #[test]
    fn labels_do_not_collide_and_ineligible_memory_is_excluded() {
        let mut db = MemoryDb::open(":memory:").unwrap();
        db.remember_project(memory("a:b", "c", "me", "one"))
            .unwrap();
        db.remember_project(memory("a", "b:c", "me", "two"))
            .unwrap();
        for (tier, state, flag) in [
            ("archive", "processed", None),
            ("short_term", "pending", None),
            ("long_term", "processed", Some("private")),
        ] {
            let input = json!({
                "project":"hidden","category":"work","tenant_id":"local","person_id":"me",
                "kind":"conversation","text":"not visible","captured_at":10,"recorded_at":10,"feature_flag":flag,
                "claim":{"subject":"ignored","predicate":"status","value":"hidden","kind":"fact","valid_from":10,"tier":tier,"processing_state":state}
            });
            db.remember_project(serde_json::from_value(input).unwrap())
                .unwrap();
        }
        assert_eq!(
            db.project_memories(query("me", Some("a:b"), Some("c")))
                .unwrap()[0]
                .value,
            "one"
        );
        assert!(
            db.project_memories(query("me", Some("hidden"), None))
                .unwrap()
                .is_empty()
        );
        let mut missing = memory("Tofu", "work", "me", "none");
        missing.memory.claim = None;
        assert!(db.remember_project(missing).is_err());
        assert!(
            db.remember_project(memory(" ", "work", "me", "bad"))
                .is_err()
        );
    }

    #[test]
    fn omitting_redundant_claim_subject_and_value_works_for_project_helpers() {
        let mut db = MemoryDb::open(":memory:").unwrap();
        let proj_input: ProjectMemoryInput = serde_json::from_value(json!({
            "project":"Tofu","category":"work","tenant_id":"local","person_id":"me",
            "kind":"conversation","text":"Status is active","captured_at":10,"recorded_at":10,
            "claim":{"predicate":"task:status","value":"active","kind":"task","valid_from":10}
        }))
        .unwrap();
        db.remember_project(proj_input).unwrap();

        let rec_input: ProjectRecordInput = serde_json::from_value(json!({
            "project":"Tofu","category":"work","tenant_id":"local","person_id":"me",
            "kind":"conversation","text":" Kickoff meeting","captured_at":11,"recorded_at":11,
            "record":{
                "record_type":"meeting",
                "fields":{"topic":"Kickoff"},
                "connections":[{"relation":"with","target":"alice"}]
            },
            "claim":{"predicate":"meeting:kickoff","kind":"fact","valid_from":11}
        }))
        .unwrap();
        db.remember_project_record(rec_input).unwrap();

        let raw_missing_subject: RememberInput = serde_json::from_value(json!({
            "tenant_id":"local","person_id":"me","kind":"conversation","text":"bad",
            "captured_at":12,"recorded_at":12,
            "claim":{"predicate":"p","value":"v","kind":"fact","valid_from":12}
        }))
        .unwrap();
        assert!(db.remember(raw_missing_subject).is_err());

        let raw_missing_value: RememberInput = serde_json::from_value(json!({
            "tenant_id":"local","person_id":"me","kind":"conversation","text":"bad",
            "captured_at":12,"recorded_at":12,
            "claim":{"subject":"s","predicate":"p","kind":"fact","valid_from":12}
        }))
        .unwrap();
        assert!(db.remember(raw_missing_value).is_err());

        let facts = db
            .project_memories(query("me", Some("Tofu"), Some("work")))
            .unwrap();
        assert_eq!(facts.len(), 2);
        assert!(facts[0].record.is_some());
        assert!(facts[1].record.is_none());
        let serialized_plain = serde_json::to_value(&facts[1]).unwrap();
        assert!(serialized_plain.get("record").is_none());
    }

    #[test]
    fn project_query_filters_by_predicate_record_type_and_graph_connections() {
        let mut db = MemoryDb::open(":memory:").unwrap();
        db.remember_project(memory("Tofu", "work", "me", "plain"))
            .unwrap();

        for (predicate, rec, recorded_at) in [
            (
                "task:auth",
                json!({
                    "record_type":"work_item",
                    "fields":{"title":"Auth"},
                    "connections":[
                        {"relation":"blocked_by","target":"task:db"},
                        {"relation":"assigned_to","target":"person:alice"}
                    ]
                }),
                20,
            ),
            (
                "task:ui",
                json!({
                    "record_type":"work_item",
                    "fields":{"title":"UI"},
                    "connections":[
                        {"relation":"depends_on","target":"task:auth"},
                        {"relation":"assigned_to","target":"person:bob"}
                    ]
                }),
                30,
            ),
            (
                "note:arch",
                json!({
                    "record_type":"decision",
                    "fields":{"summary":"Use SQLite"},
                    "connections":[
                        {"relation":"affects","target":"task:db"}
                    ]
                }),
                40,
            ),
        ] {
            let input: ProjectRecordInput = serde_json::from_value(json!({
                "project":"Tofu","category":"work","tenant_id":"local","person_id":"me",
                "kind":"conversation","text":predicate,"captured_at":recorded_at,"recorded_at":recorded_at,
                "record":rec,
                "claim":{"predicate":predicate,"kind":"task","valid_from":recorded_at}
            }))
            .unwrap();
            db.remember_project_record(input).unwrap();
        }

        let mut by_pred = query("me", Some("Tofu"), None);
        by_pred.predicate = Some("task:ui".into());
        let facts = db.project_memories(by_pred).unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].predicate, "task:ui");
        assert_eq!(facts[0].record.as_ref().unwrap().record_type, "work_item");

        let mut by_type = query("me", Some("Tofu"), None);
        by_type.record_type = Some("work_item".into());
        let facts = db.project_memories(by_type).unwrap();
        assert_eq!(facts.len(), 2);

        let mut by_rel = query("me", Some("Tofu"), None);
        by_rel.relation = Some("blocked_by".into());
        let facts = db.project_memories(by_rel).unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].predicate, "task:auth");

        let mut by_target = query("me", Some("Tofu"), None);
        by_target.target = Some("task:db".into());
        let facts = db.project_memories(by_target).unwrap();
        assert_eq!(facts.len(), 2);

        // Both relation and target must match on the SAME connection entry
        let mut split_mismatch = query("me", Some("Tofu"), None);
        split_mismatch.relation = Some("blocked_by".into());
        split_mismatch.target = Some("person:alice".into());
        assert!(db.project_memories(split_mismatch).unwrap().is_empty());

        let mut exact_conn = query("me", Some("Tofu"), None);
        exact_conn.relation = Some("blocked_by".into());
        exact_conn.target = Some("task:db".into());
        let facts = db.project_memories(exact_conn).unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].predicate, "task:auth");

        let mut bad = query("me", None, None);
        bad.record_type = Some(" ".into());
        assert!(db.project_memories(bad).is_err());
    }

    #[test]
    fn dream_consolidates_associations_unresolved_targets_short_term_and_unsummarized() {
        let mut db = MemoryDb::open(":memory:").unwrap();

        // Claim 1: short-term structured claim in project Tofu with a shared target and an unresolved target
        let c1 = db
            .remember_project_record(
                serde_json::from_value(json!({
                    "project":"Tofu","category":"work","tenant_id":"local","person_id":"me",
                    "kind":"conversation","text":"Auth service needs schema migration","captured_at":10,"recorded_at":10,
                    "record":{
                        "record_type":"work_item",
                        "fields":{"title":"Auth migration"},
                        "connections":[
                            {"relation":"depends_on","target":"shared:db_migration"},
                            {"relation":"owned_by","target":"team:missing_squad"}
                        ]
                    },
                    "claim":{"predicate":"task:auth_migration","kind":"task","valid_from":10,"tier":"short_term"}
                }))
                .unwrap(),
            )
            .unwrap();

        // Claim 2: long-term structured claim in project Other sharing target `shared:db_migration`
        // and also pointing to known predicate `task:auth_migration` on a third claim
        let c2 = db
            .remember_project_record(
                serde_json::from_value(json!({
                    "project":"Other","category":"ops","tenant_id":"local","person_id":"me",
                    "kind":"conversation","text":"Billing cutover awaits schema migration","captured_at":20,"recorded_at":20,
                    "record":{
                        "record_type":"work_item",
                        "fields":{"title":"Billing cutover"},
                        "connections":[
                            {"relation":"depends_on","target":"shared:db_migration"},
                            {"relation":"relates_to","target":"Tofu:work"}
                        ]
                    },
                    "claim":{"predicate":"task:billing_cutover","kind":"task","valid_from":20}
                }))
                .unwrap(),
            )
            .unwrap();

        // Claim 3: semantically similar via vector embeddings to Claim 1, no explicit connection
        let c3 = db
            .remember_project(
                serde_json::from_value(json!({
                    "project":"Infra","category":"db","tenant_id":"local","person_id":"me",
                    "kind":"conversation","text":"Postgres migration runbook","captured_at":30,"recorded_at":30,
                    "claim":{"predicate":"doc:pg_runbook","value":"runbook v1","kind":"fact","valid_from":30}
                }))
                .unwrap(),
            )
            .unwrap();

        // Attach cosine embeddings to c1 and c3 with high similarity (1.0 -> 10000 bps)
        for (claim_id, vec) in [
            (c1.claim_id.clone().unwrap(), vec![1.0_f32, 0.0]),
            (c3.claim_id.clone().unwrap(), vec![1.0_f32, 0.0]),
        ] {
            let proj = db
                .projection_input(
                    &TenantId("local".into()),
                    &PersonId("me".into()),
                    EmbeddingTarget::Claim(claim_id.clone()),
                )
                .unwrap();
            db.upsert_embedding(EmbeddingInput {
                tenant_id: TenantId("local".into()),
                person_id: PersonId("me".into()),
                target: EmbeddingTarget::Claim(claim_id),
                embedding: Embedding {
                    vector: vec,
                    model: "test-model".into(),
                    version: "v1".into(),
                    input_hash: proj.input_hash,
                    normalization: VectorNormalization::L2,
                    distance: VectorDistance::Cosine,
                },
            })
            .unwrap();
        }

        // Summarize c1's evidence via nap so only c2 and c3 remain unsummarized
        db.nap(NapInput {
            tenant_id: TenantId("local".into()),
            person_id: PersonId("me".into()),
            summary: "Discussed auth schema migration".into(),
            evidence_ids: vec![c1.evidence_id.clone()],
            recorded_at: 40,
        })
        .unwrap();

        let pack = db
            .dream(DreamInput {
                tenant_id: TenantId("local".into()),
                person_id: PersonId("me".into()),
                model: None,
                version: None,
                min_similarity_basis_points: 5_000,
                limit: 10,
            })
            .unwrap();

        // 1. Vector similarity association (c3 <-> c1, 10000 bps) and shared target association (c2 <-> c1, 8500 bps)
        assert_eq!(pack.associations.len(), 2);
        assert_eq!(pack.associations[0].similarity_basis_points, 10_000);
        assert_eq!(
            pack.associations[0].reason,
            "vector_similarity:test-model:v1"
        );
        assert_eq!(pack.associations[1].similarity_basis_points, 8_500);
        assert_eq!(
            pack.associations[1].reason,
            "shared_target:shared:db_migration"
        );

        // 2. Unresolved targets: `shared:db_migration` (on c2 and c1) and `team:missing_squad` (on c1),
        // while `Tofu:work` is resolved because `zkr:project:["Tofu","work"]` is live!
        assert!(
            pack.unresolved_targets
                .iter()
                .any(|u| u.target == "team:missing_squad" && u.relation == "owned_by")
        );
        assert!(
            !pack
                .unresolved_targets
                .iter()
                .any(|u| u.target == "Tofu:work")
        );

        // 3. Short-term claims: only c1 is short_term
        assert_eq!(pack.short_term_claims, vec![c1.claim_id.unwrap()]);

        // 4. Unsummarized evidence: c3 and c2 (c1 was summarized by nap)
        assert_eq!(
            pack.unsummarized_evidence_ids,
            vec![c3.evidence_id, c2.evidence_id]
        );

        // Model without version is rejected
        assert!(
            db.dream(DreamInput {
                tenant_id: TenantId("local".into()),
                person_id: PersonId("me".into()),
                model: Some("test-model".into()),
                version: None,
                min_similarity_basis_points: 5_000,
                limit: 10,
            })
            .is_err()
        );
    }

    #[test]
    fn multi_category_custom_fields_and_multihop_graph_traversal_work() {
        let mut db = MemoryDb::open(":memory:").unwrap();

        // Node 1: database schema migration (multi-category via `categories` + `record.categories`)
        db.remember_project_record(
            serde_json::from_value(json!({
                "project":"Tofu",
                "categories":["backend","database"],
                "tenant_id":"local","person_id":"me",
                "kind":"conversation","text":"Create users table in Postgres","captured_at":10,"recorded_at":10,
                "record":{
                    "record_type":"migration",
                    "categories":["urgent"],
                    "fields":{"engine":"postgres","priority":1},
                    "connections":[]
                },
                "claim":{"predicate":"task:db_schema","kind":"task","valid_from":10}
            }))
            .unwrap(),
        )
        .unwrap();

        // Node 2: auth service (connects to task:db_schema with weight + note)
        db.remember_project_record(
            serde_json::from_value(json!({
                "project":"Tofu",
                "categories":["backend","security"],
                "tenant_id":"local","person_id":"me",
                "kind":"conversation","text":"Implement OAuth token exchange","captured_at":20,"recorded_at":20,
                "record":{
                    "record_type":"service",
                    "fields":{"owner":"alice","priority":1},
                    "connections":[
                        {
                            "relation":"depends_on",
                            "target":"task:db_schema",
                            "weight_basis_points":9000,
                            "note":"Requires users table first"
                        }
                    ]
                },
                "claim":{"predicate":"task:auth_service","kind":"task","valid_from":20}
            }))
            .unwrap(),
        )
        .unwrap();

        // Node 3: mobile login screen (connects to task:auth_service -> 2 hops from task:db_schema)
        db.remember_project_record(
            serde_json::from_value(json!({
                "project":"Tofu",
                "category":"frontend",
                "tenant_id":"local","person_id":"me",
                "kind":"conversation","text":"Build login UI","captured_at":30,"recorded_at":30,
                "record":{
                    "record_type":"screen",
                    "fields":{"owner":"bob","priority":2},
                    "connections":[
                        {"relation":"calls","target":"task:auth_service","weight_basis_points":7500}
                    ]
                },
                "claim":{"predicate":"task:login_ui","kind":"task","valid_from":30}
            }))
            .unwrap(),
        )
        .unwrap();

        // 1. Query by `all_categories` (intersection)
        let mut all_cat_q = query("me", Some("Tofu"), None);
        all_cat_q.all_categories = vec!["backend".into(), "database".into(), "urgent".into()];
        let facts = db.project_memories(all_cat_q).unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].predicate, "task:db_schema");
        assert_eq!(facts[0].categories, vec!["backend", "database", "urgent"]);

        // 2. Query by `categories` (union / ANY)
        let mut any_cat_q = query("me", Some("Tofu"), None);
        any_cat_q.categories = vec!["security".into(), "frontend".into()];
        let facts = db.project_memories(any_cat_q).unwrap();
        assert_eq!(facts.len(), 2);

        // 3. Query by `where_fields`
        let mut field_q = query("me", Some("Tofu"), None);
        field_q.where_fields.insert("priority".into(), json!(1));
        field_q.where_fields.insert("owner".into(), json!("alice"));
        let facts = db.project_memories(field_q).unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].predicate, "task:auth_service");
        assert_eq!(
            facts[0].record.as_ref().unwrap().connections[0].weight_basis_points,
            Some(9000)
        );

        // 4. Multi-hop spreading activation: 1 hop from task:db_schema finds task:auth_service
        let mut hop1_q = query("me", Some("Tofu"), None);
        hop1_q.connected_to = Some("task:db_schema".into());
        hop1_q.max_hops = Some(1);
        let hop1_facts = db.project_memories(hop1_q).unwrap();
        assert_eq!(hop1_facts.len(), 1);
        assert_eq!(hop1_facts[0].predicate, "task:auth_service");

        // 2 hops from task:db_schema finds both task:auth_service and task:login_ui
        let mut hop2_q = query("me", Some("Tofu"), None);
        hop2_q.connected_to = Some("task:db_schema".into());
        hop2_q.max_hops = Some(2);
        let hop2_facts = db.project_memories(hop2_q).unwrap();
        assert_eq!(hop2_facts.len(), 2);

        // Reverse direction: 2 hops from task:login_ui (outgoing edges) also reaches task:db_schema
        let mut rev_hop2_q = query("me", Some("Tofu"), None);
        rev_hop2_q.connected_to = Some("task:login_ui".into());
        rev_hop2_q.max_hops = Some(2);
        let rev_facts = db.project_memories(rev_hop2_q).unwrap();
        assert_eq!(rev_facts.len(), 2);
    }

    #[test]
    fn inline_and_automatic_vectorization_power_queries_centroids_and_dream_synthesis() {
        let mut db = MemoryDb::open(":memory:").unwrap();

        // Store two memories in category "database" with inline embeddings near [1.0, 0.0]
        db.remember_project_record(
            serde_json::from_value(json!({
                "project":"Tofu",
                "categories":["database","storage"],
                "tenant_id":"local","person_id":"me",
                "kind":"conversation","text":"Postgres WAL tuning and indexing","captured_at":10,"recorded_at":10,
                "embedding":{"vector":[1.0, 0.0],"model":"brain-vec","version":"v1"},
                "record":{
                    "record_type":"tuning_note",
                    "fields":{"engine":"postgres"},
                    "connections":[{"relation":"optimizes","target":"task:query_planner"}]
                },
                "claim":{"predicate":"note:pg_wal","kind":"fact","valid_from":10}
            }))
            .unwrap(),
        )
        .unwrap();

        // Second claim forms a triadic chain: note:pg_wal -> task:query_planner -> service:analytics
        db.remember_project_record(
            serde_json::from_value(json!({
                "project":"Tofu",
                "category":"database",
                "tenant_id":"local","person_id":"me",
                "kind":"conversation","text":"Query planner statistics","captured_at":20,"recorded_at":20,
                "embedding":{"vector":[1.0, 0.0],"model":"brain-vec","version":"v1"},
                "record":{
                    "record_type":"tuning_note",
                    "fields":{"engine":"postgres","cost_based":true},
                    "connections":[{"relation":"feeds","target":"service:analytics"}]
                },
                "claim":{"predicate":"task:query_planner","kind":"task","valid_from":20}
            }))
            .unwrap(),
        )
        .unwrap();

        // Third claim in category "analytics" ONLY, but with vector [1.0, 0.0] matching the "database" centroid!
        db.remember_project_record(
            serde_json::from_value(json!({
                "project":"Tofu",
                "category":"analytics",
                "tenant_id":"local","person_id":"me",
                "kind":"conversation","text":"Analytics rollup service","captured_at":30,"recorded_at":30,
                "embedding":{"vector":[0.0, 1.0],"model":"brain-vec","version":"v1"},
                "record":{
                    "record_type":"service",
                    "fields":{"owner":"data-team"},
                    "connections":[]
                },
                "claim":{"predicate":"service:analytics","kind":"fact","valid_from":30}
            }))
            .unwrap(),
        )
        .unwrap();

        // Fourth claim in category "misc" with vector [1.0, 0.0] -> dream should suggest "database" category!
        db.remember_project(
            serde_json::from_value(json!({
                "project":"Tofu",
                "category":"misc",
                "tenant_id":"local","person_id":"me",
                "kind":"conversation","text":"B-tree page split notes","captured_at":40,"recorded_at":40,
                "embedding":{"vector":[1.0, 0.0],"model":"brain-vec","version":"v1"},
                "claim":{"predicate":"note:btree","value":"page split factor 0.9","kind":"fact","valid_from":40}
            }))
            .unwrap(),
        )
        .unwrap();

        // Vector query using `query_embedding`
        let mut vq = query("me", Some("Tofu"), None);
        vq.query_embedding = Some(DenseQuery {
            vector: vec![0.0, 1.0],
            model: "brain-vec".into(),
            version: "v1".into(),
        });
        let ranked = db.project_memories(vq).unwrap();
        assert_eq!(ranked[0].predicate, "service:analytics");
        assert_eq!(ranked[0].similarity_basis_points, Some(10_000));

        // Automatic `vector_query` using built-in `auto_vectorize`
        let mut auto_q = query("me", Some("Tofu"), None);
        auto_q.vector_query = Some("Postgres WAL tuning".into());
        let auto_ranked = db.project_memories(auto_q).unwrap();
        assert_eq!(auto_ranked[0].predicate, "note:pg_wal");
        assert!(auto_ranked[0].similarity_basis_points.unwrap() > 3_000);

        // Attach a contradicting evidence link to `note:btree` and a stale summary node
        let btree_claim_id = ranked
            .iter()
            .find(|f| f.predicate == "note:btree")
            .unwrap()
            .claim_id
            .clone();
        let btree_ev_id = ranked
            .iter()
            .find(|f| f.predicate == "note:btree")
            .unwrap()
            .evidence_ids[0]
            .clone();
        let other_ev_id = ranked[0].evidence_ids[0].clone();
        db.connection
            .execute(
                "INSERT INTO claim_evidence (tenant_id, person_id, claim_id, evidence_id, relation, confidence_basis_points) VALUES ('local', 'me', ?1, ?2, '\"contradicts\"', 10000)",
                params![btree_claim_id.0, other_ev_id.0],
            )
            .unwrap();
        let napped = db
            .nap(NapInput {
                tenant_id: TenantId("local".into()),
                person_id: PersonId("me".into()),
                summary: "Temporary summary to mark stale".into(),
                evidence_ids: vec![btree_ev_id],
                recorded_at: 50,
            })
            .unwrap();
        db.connection
            .execute(
                "UPDATE summary_nodes SET stale_at = 60 WHERE id = ?1",
                params![napped.summary_id.0],
            )
            .unwrap();

        // Run dream and inspect categories (with centroids), category_suggestions, schemas, hubs, triadic_closure, contradictions, and stale_summary_ids
        let pack = db
            .dream(DreamInput {
                tenant_id: TenantId("local".into()),
                person_id: PersonId("me".into()),
                model: Some("brain-vec".into()),
                version: Some("v1".into()),
                min_similarity_basis_points: 7_000,
                limit: 20,
            })
            .unwrap();

        let db_cat = pack
            .categories
            .iter()
            .find(|c| c.category == "database")
            .unwrap();
        assert_eq!(db_cat.memory_count, 2);
        assert_eq!(db_cat.co_occurring_categories, vec!["storage"]);
        assert_eq!(db_cat.record_types, vec!["tuning_note"]);
        assert!(db_cat.centroid.is_some());

        // `note:btree` (in "misc") has vector [1.0, 0.0], matching the "database" centroid [1.0, 0.0]
        assert!(pack.category_suggestions.iter().any(|s| {
            s.predicate == "note:btree"
                && s.suggested_category == "database"
                && s.similarity_basis_points == 10_000
        }));

        // `schemas` has `tuning_note` (2 records) with merged fields and relations
        let tuning_schema = pack
            .schemas
            .iter()
            .find(|s| s.record_type == "tuning_note")
            .unwrap();
        assert_eq!(tuning_schema.record_count, 2);
        assert_eq!(tuning_schema.fields, vec!["cost_based", "engine"]);
        assert_eq!(tuning_schema.relations, vec!["feeds", "optimizes"]);

        // `hubs` includes `task:query_planner` with 1 inbound and 1 outbound connection
        let planner_hub = pack
            .hubs
            .iter()
            .find(|h| h.identifier == "task:query_planner")
            .unwrap();
        assert_eq!(planner_hub.inbound_count, 1);
        assert_eq!(planner_hub.outbound_count, 1);

        // Triadic closure: `note:pg_wal` -> `task:query_planner` -> `service:analytics`
        assert!(pack.associations.iter().any(|a| {
            a.reason == "triadic_closure:task:query_planner" && a.similarity_basis_points == 8_000
        }));

        // Contradictions and stale summaries surfaced
        assert_eq!(pack.contradictions, vec![btree_claim_id]);
        assert_eq!(pack.stale_summary_ids, vec![napped.summary_id]);
    }
}
