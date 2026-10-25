1. **Understand `remember_project` test gap**
    The user is asking to "Add test for remember_project".
    Line 457 of `src/store/projects.rs`: `pub fn remember_project(&mut self, mut input: ProjectMemoryInput) -> Result<Remembered>`

    The test needs to cover:
    - Failing cases: missing claim, no categories, category too long, >32 categories.
    - Success cases: with and without inline embedding.
    - Verification: check that embeddings were inserted. `embedding_target_parts` maps `EmbeddingTarget::Claim(id)` to `("claim", id.0)`.

    I will write a test `remember_project_validates_inputs_and_embeds` in `src/store/projects.rs`.

2. **Drafting the test**
    ```rust
    #[test]
    fn remember_project_validates_inputs_and_embeds() {
        let mut db = MemoryDb::open(":memory:").unwrap();

        // 1. Missing claim
        let mut missing_claim = memory("Tofu", "work", "me", "active");
        missing_claim.memory.claim = None;
        assert!(db.remember_project(missing_claim).is_err());

        // 2. No category
        let mut no_cat = memory("Tofu", "work", "me", "active");
        no_cat.category = None;
        no_cat.categories = vec![];
        assert!(db.remember_project(no_cat).is_err());

        // 3. Category > 256 bytes
        let mut long_cat = memory("Tofu", "work", "me", "active");
        long_cat.category = Some("a".repeat(257));
        assert!(db.remember_project(long_cat).is_err());

        // 4. Categories > 32
        let mut too_many_cats = memory("Tofu", "work", "me", "active");
        too_many_cats.category = None;
        too_many_cats.categories = (0..33).map(|i| format!("cat{}", i)).collect();
        assert!(db.remember_project(too_many_cats).is_err());

        // 5. Successful remember project with inline embeddings
        let mut with_emb = memory("Tofu", "work", "me", "active");
        with_emb.embedding = Some(InlineEmbedding {
            vector: vec![0.1, 0.2, 0.3],
            model: "test-model".into(),
            version: "v1".into(),
            normalization: VectorNormalization::L2,
            distance: VectorDistance::Cosine,
        });
        let rem = db.remember_project(with_emb).unwrap();

        // Assert embeddings were created.
        let claim_id = rem.claim_id.unwrap();
        let target_kind = "claim";
        let target_id = claim_id.0;

        let auto: usize = db.connection.query_row(
            "SELECT count(*) FROM embeddings WHERE target_kind = ? AND target_id = ? AND model = ?",
            [target_kind, target_id.as_str(), AUTO_EMBEDDING_MODEL],
            |row| row.get(0)
        ).unwrap();
        assert_eq!(auto, 1);

        let inline: usize = db.connection.query_row(
            "SELECT count(*) FROM embeddings WHERE target_kind = ? AND target_id = ? AND model = ?",
            [target_kind, target_id.as_str(), "test-model"],
            |row| row.get(0)
        ).unwrap();
        assert_eq!(inline, 1);
    }
    ```

3. **Pre-commit step**
    Complete pre-commit steps to ensure proper testing, verification, review, and reflection are done.

4. **Submit PR**
