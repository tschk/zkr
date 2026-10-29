import re

with open("src/store/projects.rs", "r") as f:
    content = f.read()

helper_funcs = """
fn extract_project_categories(
    project: Option<String>,
    category: Option<String>,
    input_categories: Vec<String>,
) -> Result<(String, Vec<String>)> {
    let project = match project {
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
    if let Some(cat) = category {
        push_category(cat)?;
    }
    for cat in input_categories {
        push_category(cat)?;
    }
    if categories.is_empty() {
        return Err(Error::Invalid("category must not be empty".to_owned()));
    }
    if categories.len() > 32 {
        return Err(Error::Invalid("categories exceed 32 entries".to_owned()));
    }
    Ok((project, categories))
}
"""

original_remember_project = """    /// Use a stable predicate per fact/task (for example `task:launch:status`).
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
    }"""

new_remember_project = """    /// Use a stable predicate per fact/task (for example `task:launch:status`).
    /// Update it through `correct`; deletion, replication and evidence stay ordinary ZKR operations.
    pub fn remember_project(&mut self, mut input: ProjectMemoryInput) -> Result<Remembered> {
        require_scope(&input.memory.tenant_id, &input.memory.person_id)?;
        let (project, categories) = extract_project_categories(
            input.project.take(),
            input.category.take(),
            std::mem::take(&mut input.categories),
        )?;

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
            self.attach_project_embeddings(&tenant_id, &person_id, &claim_id, inline_embedding, &source_text)?;
        }
        Ok(remembered)
    }

    fn attach_project_embeddings(
        &mut self,
        tenant_id: &TenantId,
        person_id: &PersonId,
        claim_id: &ClaimId,
        inline_embedding: Option<InlineEmbedding>,
        source_text: &str,
    ) -> Result<()> {
        if let Some(emb) = inline_embedding {
            let proj = projection_input_from(
                &self.connection,
                tenant_id,
                person_id,
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
            tenant_id,
            person_id,
            EmbeddingTarget::Claim(claim_id.clone()),
        ) {
            let auto_vec = auto_vectorize(&format!("{} {source_text}", proj.text));
            let _ = self.upsert_embedding(EmbeddingInput {
                tenant_id: tenant_id.clone(),
                person_id: person_id.clone(),
                target: EmbeddingTarget::Claim(claim_id.clone()),
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
        Ok(())
    }"""

if original_remember_project not in content:
    print("Could not find the original function!")
else:
    new_content = content.replace(original_remember_project, new_remember_project)

    # inject the helper function outside the impl block
    # search for impl MemoryDb
    impl_idx = new_content.find("impl MemoryDb {")
    if impl_idx == -1:
        print("Could not find impl MemoryDb")
    else:
        final_content = new_content[:impl_idx] + helper_funcs + "\n" + new_content[impl_idx:]
        with open("src/store/projects.rs", "w") as f:
            f.write(final_content)
        print("Refactored successfully")
