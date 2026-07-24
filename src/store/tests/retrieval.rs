use super::*;

#[test]
fn raw_sources_are_scoped_cited_and_deleted_from_retrieval() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let raw = db
        .remember(remember_raw("a", "sam", "The launch code is marigold"))
        .unwrap();
    db.remember(remember_raw("b", "sam", "Marigold belongs elsewhere"))
        .unwrap();

    let found = db
        .search(SearchInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            query: "marigold".into(),
            limit: 5,
            query_embedding: None,
            as_of: None,
            enabled_features: Vec::new(),
        })
        .unwrap();
    assert_eq!(found.items.len(), 1);
    assert_eq!(
        found.items[0].memory,
        MemoryRef::Source(raw.source_id.clone())
    );
    assert_eq!(found.items[0].evidence_ids, vec![raw.evidence_id]);

    db.delete_source(DeleteInput {
        tenant_id: TenantId("a".into()),
        person_id: PersonId("sam".into()),
        source_id: raw.source_id.clone(),
        deleted_at: 20,
    })
    .unwrap();
    assert!(matches!(
        db.get(GetInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            target: EmbeddingTarget::Source(raw.source_id),
        }),
        Err(Error::NotFound)
    ));
    assert!(
        db.search(SearchInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            query: "marigold".into(),
            limit: 5,
            query_embedding: None,
            as_of: None,
            enabled_features: Vec::new(),
        })
        .unwrap()
        .items
        .is_empty()
    );
}

#[test]
fn lexical_search_prefers_phrases_then_recalls_natural_language_tokens() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let phrase = db
        .remember(remember_raw("a", "sam", "Alice works at Acme Corporation"))
        .unwrap();
    let fallback = db
        .remember(remember_raw("a", "sam", "Alice moved near the Acme campus"))
        .unwrap();
    db.remember(remember_raw(
        "b",
        "sam",
        "Where does Alice work at another tenant?",
    ))
    .unwrap();

    let exact = db
        .search(SearchInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            query: "Alice works at Acme".into(),
            limit: 5,
            query_embedding: None,
            as_of: None,
            enabled_features: Vec::new(),
        })
        .unwrap();
    assert_eq!(
        exact.items[0].memory,
        MemoryRef::Source(phrase.source_id.clone())
    );

    let search = || {
        db.search(SearchInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            query: "Where does Alice work at Acme?".into(),
            limit: 5,
            query_embedding: None,
            as_of: None,
            enabled_features: Vec::new(),
        })
        .unwrap()
    };
    let first = search();
    let second = search();

    assert_eq!(first.items, second.items);
    assert_eq!(first.items.len(), 2);
    assert!(
        first
            .items
            .iter()
            .any(|item| item.memory == MemoryRef::Source(phrase.source_id.clone()))
    );
    assert!(
        first
            .items
            .iter()
            .any(|item| item.memory == MemoryRef::Source(fallback.source_id.clone()))
    );
}

#[test]
fn lexical_token_fallback_is_bounded_and_handles_fts_punctuation() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    for index in 0..8 {
        db.remember(remember_raw(
            "a",
            "sam",
            &format!("Alice's project number {index}"),
        ))
        .unwrap();
    }

    let found = db
        .search(SearchInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            query: "What's Alice's project?".into(),
            limit: 3,
            query_embedding: None,
            as_of: None,
            enabled_features: Vec::new(),
        })
        .unwrap();

    assert_eq!(found.items.len(), 3);
}

#[test]
fn lexical_token_fallback_bounds_the_query_expression() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    db.remember(remember_raw("a", "sam", "term0 relevant memory"))
        .unwrap();
    let query = (0..1_000)
        .map(|index| format!("term{index}"))
        .collect::<Vec<_>>()
        .join(" ");

    let found = db
        .search(SearchInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            query,
            limit: 3,
            query_embedding: None,
            as_of: None,
            enabled_features: Vec::new(),
        })
        .unwrap();

    assert_eq!(found.items.len(), 1);
}

#[test]
fn retrieval_excerpts_are_utf8_safe_and_byte_bounded() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let text = format!("needle {}", "é".repeat(MAX_EXCERPT_BYTES));
    let raw = db.remember(remember_raw("a", "sam", &text)).unwrap();
    let item = db
        .get(GetInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            target: EmbeddingTarget::Source(raw.source_id),
        })
        .unwrap();
    assert!(item.excerpt.len() <= MAX_EXCERPT_BYTES);
    assert!(item.excerpt.starts_with("needle "));
    assert!(item.excerpt.is_char_boundary(item.excerpt.len()));
}

#[test]
fn accepted_claim_replaces_its_source_in_retrieval() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let remembered = db.remember(remember("a", "sam", "Acme")).unwrap();
    let target = EmbeddingTarget::Source(remembered.source_id);
    let input_hash = hash_for(&db, target.clone());
    db.upsert_embedding(EmbeddingInput {
        tenant_id: TenantId("a".into()),
        person_id: PersonId("sam".into()),
        target,
        embedding: Embedding {
            vector: vec![1.0, 0.0],
            model: "test/model".into(),
            version: "1".into(),
            input_hash,
            normalization: VectorNormalization::L2,
            distance: VectorDistance::Cosine,
        },
    })
    .unwrap();
    let found = db
        .search(SearchInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            query: "Acme".into(),
            limit: 5,
            query_embedding: Some(DenseQuery {
                vector: vec![1.0, 0.0],
                model: "test/model".into(),
                version: "1".into(),
            }),
            as_of: None,
            enabled_features: Vec::new(),
        })
        .unwrap();
    assert_eq!(found.items.len(), 1);
    assert_eq!(
        found.items[0].memory,
        MemoryRef::Claim(remembered.claim_id.unwrap())
    );
}

#[test]
fn dense_evidence_without_a_claim_is_retrievable() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let raw = db
        .remember(remember_raw("a", "sam", "Quiet desk near a window"))
        .unwrap();
    let target = EmbeddingTarget::Evidence(raw.evidence_id.clone());
    let input_hash = hash_for(&db, target.clone());
    db.upsert_embedding(EmbeddingInput {
        tenant_id: TenantId("a".into()),
        person_id: PersonId("sam".into()),
        target,
        embedding: Embedding {
            vector: vec![1.0, 0.0],
            model: "test/model".into(),
            version: "1".into(),
            input_hash,
            normalization: VectorNormalization::L2,
            distance: VectorDistance::Cosine,
        },
    })
    .unwrap();
    let found = db
        .search(SearchInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            query: "unmatched lexical phrase".into(),
            limit: 5,
            query_embedding: Some(DenseQuery {
                vector: vec![1.0, 0.0],
                model: "test/model".into(),
                version: "1".into(),
            }),
            as_of: None,
            enabled_features: Vec::new(),
        })
        .unwrap();
    assert_eq!(found.items.len(), 1);
    assert_eq!(found.items[0].memory, MemoryRef::Evidence(raw.evidence_id));

    db.delete_source(DeleteInput {
        tenant_id: TenantId("a".into()),
        person_id: PersonId("sam".into()),
        source_id: raw.source_id,
        deleted_at: 20,
    })
    .unwrap();
    assert!(
        db.search(SearchInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            query: "unmatched lexical phrase".into(),
            limit: 5,
            query_embedding: Some(DenseQuery {
                vector: vec![1.0, 0.0],
                model: "test/model".into(),
                version: "1".into(),
            }),
            as_of: None,
            enabled_features: Vec::new(),
        })
        .unwrap()
        .items
        .is_empty()
    );
}

#[test]
fn expired_claims_do_not_fall_back_to_stale_raw_evidence() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let remembered = db.remember(remember("a", "sam", "Acme")).unwrap();
    let target = EmbeddingTarget::Claim(remembered.claim_id.clone().unwrap());
    db.upsert_embedding(EmbeddingInput {
        tenant_id: TenantId("a".into()),
        person_id: PersonId("sam".into()),
        target,
        embedding: Embedding {
            vector: vec![1.0, 0.0],
            model: "test/model".into(),
            version: "1".into(),
            input_hash: hash_for(
                &db,
                EmbeddingTarget::Claim(remembered.claim_id.clone().unwrap()),
            ),
            normalization: VectorNormalization::L2,
            distance: VectorDistance::Cosine,
        },
    })
    .unwrap();
    db.connection
        .execute(
            "UPDATE claims SET valid_until = 20, recorded_until = 20 WHERE id = ?1",
            [&remembered.claim_id.unwrap().0],
        )
        .unwrap();

    let found = db
        .search(SearchInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            query: "Acme".into(),
            limit: 5,
            query_embedding: Some(DenseQuery {
                vector: vec![1.0, 0.0],
                model: "test/model".into(),
                version: "1".into(),
            }),
            as_of: None,
            enabled_features: Vec::new(),
        })
        .unwrap();
    assert!(found.items.is_empty());
}
