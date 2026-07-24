use super::*;

#[test]
fn l2_embeddings_require_unit_magnitude() {
    assert!(matches!(
        validate_embedding(&Embedding {
            vector: vec![0.1, 0.2],
            model: "test/model".into(),
            version: "1".into(),
            input_hash: "sha256:test".into(),
            normalization: VectorNormalization::L2,
            distance: VectorDistance::Cosine,
        }),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn malformed_l2_projection_is_stale_for_rebuild() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let remembered = db.remember(remember_raw("a", "sam", "Acme")).unwrap();
    let target = EmbeddingTarget::Source(remembered.source_id.clone());
    db.upsert_embedding(EmbeddingInput {
        tenant_id: TenantId("a".into()),
        person_id: PersonId("sam".into()),
        target: target.clone(),
        embedding: Embedding {
            vector: vec![1.0, 0.0],
            model: "test/model".into(),
            version: "1".into(),
            input_hash: hash_for(&db, target),
            normalization: VectorNormalization::L2,
            distance: VectorDistance::Cosine,
        },
    })
    .unwrap();
    db.connection
        .execute(
            "UPDATE embeddings SET vector = '[0.1, 0.2]' WHERE target_id = ?1",
            [&remembered.source_id.0],
        )
        .unwrap();
    assert!(
        db.projection_issues(ProjectionAuditInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            model: "test/model".into(),
            version: "1".into(),
            limit: 10,
        })
        .unwrap()
        .iter()
        .any(|issue| {
            issue.state == ProjectionState::Stale
                && issue.input.target == EmbeddingTarget::Source(remembered.source_id.clone())
        })
    );
}

#[test]
fn embedding_lanes_reject_mixed_configuration_and_surface_corruption() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let first = db.remember(remember_raw("a", "sam", "First")).unwrap();
    let second = db.remember(remember_raw("a", "sam", "Second")).unwrap();
    let first_target = EmbeddingTarget::Source(first.source_id.clone());
    db.upsert_embedding(EmbeddingInput {
        tenant_id: TenantId("a".into()),
        person_id: PersonId("sam".into()),
        target: first_target.clone(),
        embedding: Embedding {
            vector: vec![1.0],
            model: "test/model".into(),
            version: "1".into(),
            input_hash: hash_for(&db, first_target),
            normalization: VectorNormalization::L2,
            distance: VectorDistance::Cosine,
        },
    })
    .unwrap();
    let second_target = EmbeddingTarget::Source(second.source_id.clone());
    assert!(matches!(
        db.upsert_embedding(EmbeddingInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            target: second_target.clone(),
            embedding: Embedding {
                vector: vec![1.0, 0.0],
                model: "test/model".into(),
                version: "1".into(),
                input_hash: hash_for(&db, second_target),
                normalization: VectorNormalization::L2,
                distance: VectorDistance::Cosine,
            },
        }),
        Err(Error::Invalid(_))
    ));
    db.connection
        .execute(
            "UPDATE embeddings SET dimension = 2, distance = '\"invalid\"' WHERE target_id = ?1",
            [&first.source_id.0],
        )
        .unwrap();
    let issues = db
        .projection_issues(ProjectionAuditInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            model: "test/model".into(),
            version: "1".into(),
            limit: 10,
        })
        .unwrap();
    assert!(issues.iter().any(|issue| {
        issue.state == ProjectionState::Stale
            && issue.input.target == EmbeddingTarget::Source(first.source_id.clone())
    }));
    assert!(
        db.search(SearchInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            query: "missing".into(),
            limit: 5,
            query_embedding: Some(DenseQuery {
                vector: vec![1.0],
                model: "test/model".into(),
                version: "1".into(),
            }),
            as_of: None,
            enabled_features: Vec::new(),
        })
        .is_ok()
    );
}

#[test]
fn mixed_legacy_embedding_lane_can_be_repaired() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let first = db.remember(remember_raw("a", "sam", "First")).unwrap();
    let second = db.remember(remember_raw("a", "sam", "Second")).unwrap();
    let first_target = EmbeddingTarget::Source(first.source_id.clone());
    let second_target = EmbeddingTarget::Source(second.source_id.clone());
    db.upsert_embedding(EmbeddingInput {
        tenant_id: TenantId("a".into()),
        person_id: PersonId("sam".into()),
        target: first_target.clone(),
        embedding: Embedding {
            vector: vec![1.0],
            model: "test/model".into(),
            version: "1".into(),
            input_hash: hash_for(&db, first_target),
            normalization: VectorNormalization::L2,
            distance: VectorDistance::Cosine,
        },
    })
    .unwrap();
    let second_projection = db
        .projection_input(
            &TenantId("a".into()),
            &PersonId("sam".into()),
            second_target.clone(),
        )
        .unwrap();
    db.connection
        .execute(
            "INSERT INTO embeddings(tenant_id, person_id, target_kind, target_id, model, version, dimension, input_hash, target_revision, created_at, normalization, distance, vector) VALUES('a', 'sam', 'source', ?1, 'test/model', '1', 2, ?2, ?3, 1, '\"l2\"', '\"cosine\"', '[1.0,0.0]')",
            params![second.source_id.0, second_projection.input_hash, second_projection.target_revision],
        )
        .unwrap();
    db.upsert_embedding(EmbeddingInput {
        tenant_id: TenantId("a".into()),
        person_id: PersonId("sam".into()),
        target: second_target.clone(),
        embedding: Embedding {
            vector: vec![1.0],
            model: "test/model".into(),
            version: "1".into(),
            input_hash: hash_for(&db, second_target.clone()),
            normalization: VectorNormalization::L2,
            distance: VectorDistance::Cosine,
        },
    })
    .unwrap();
    assert!(
        !db.projection_issues(ProjectionAuditInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            model: "test/model".into(),
            version: "1".into(),
            limit: 10,
        })
        .unwrap()
        .iter()
        .any(|issue| issue.input.target == second_target)
    );
}

#[test]
fn embedding_projection_is_validated_and_scoped() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let remembered = db.remember(remember("a", "sam", "Acme")).unwrap();
    let target = EmbeddingTarget::Evidence(remembered.evidence_id.clone());
    let embedding = Embedding {
        vector: vec![0.447_213_6, 0.894_427_2],
        model: "provider/model".into(),
        version: "1".into(),
        input_hash: hash_for(&db, target.clone()),
        normalization: VectorNormalization::L2,
        distance: VectorDistance::Cosine,
    };
    let stored = db
        .upsert_embedding(EmbeddingInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            target,
            embedding: embedding.clone(),
        })
        .unwrap();
    assert_eq!(stored.dimension, 2);
    assert!(matches!(
        db.upsert_embedding(EmbeddingInput {
            tenant_id: TenantId("b".into()),
            person_id: PersonId("sam".into()),
            target: EmbeddingTarget::Evidence(remembered.evidence_id),
            embedding,
        }),
        Err(Error::NotFound)
    ));
}

#[test]
fn search_fuses_lexical_and_real_dense_ranks_deterministically() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let lexical = db.remember(remember("a", "sam", "Jazz Club")).unwrap();
    let dense = db.remember(remember("a", "sam", "Music Venue")).unwrap();
    for (claim_id, vector) in [
        (
            lexical.claim_id.clone().unwrap(),
            vec![0.993_883_7, 0.110_431_53],
        ),
        (dense.claim_id.unwrap(), vec![1.0, 0.0]),
    ] {
        let target = EmbeddingTarget::Claim(claim_id);
        let input_hash = hash_for(&db, target.clone());
        db.upsert_embedding(EmbeddingInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            target,
            embedding: Embedding {
                vector,
                model: "test/model".into(),
                version: "1".into(),
                input_hash,
                normalization: VectorNormalization::L2,
                distance: VectorDistance::Cosine,
            },
        })
        .unwrap();
    }

    let found = db
        .search(SearchInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            query: "Jazz Club".into(),
            limit: 2,
            query_embedding: Some(DenseQuery {
                vector: vec![1.0, 0.0],
                model: "test/model".into(),
                version: "1".into(),
            }),
            as_of: None,
            enabled_features: Vec::new(),
        })
        .unwrap();

    assert_eq!(
        found.items[0].memory,
        MemoryRef::Claim(lexical.claim_id.unwrap())
    );
    assert_eq!(found.items.len(), 2);
    assert!(!found.items[0].evidence_ids.is_empty());
}

#[test]
fn stale_projections_are_excluded_and_reported_with_current_inputs() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let raw = db
        .remember(remember_raw("a", "sam", "A quiet desk"))
        .unwrap();
    let claimed = db.remember(remember("a", "sam", "Acme")).unwrap();
    let other = db.remember(remember("b", "sam", "Other")).unwrap();
    let targets = [
        EmbeddingTarget::Source(raw.source_id.clone()),
        EmbeddingTarget::Evidence(raw.evidence_id.clone()),
        EmbeddingTarget::Claim(claimed.claim_id.clone().unwrap()),
    ];
    for target in &targets {
        db.upsert_embedding(EmbeddingInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            target: target.clone(),
            embedding: Embedding {
                vector: vec![1.0, 0.0],
                model: "test/model".into(),
                version: "1".into(),
                input_hash: hash_for(&db, target.clone()),
                normalization: VectorNormalization::L2,
                distance: VectorDistance::Cosine,
            },
        })
        .unwrap();
    }
    db.connection
        .execute(
            "UPDATE sources SET content = 'A changed desk', revision = revision + 1 WHERE id = ?1",
            [&raw.source_id.0],
        )
        .unwrap();
    db.connection
        .execute(
            "UPDATE evidence SET quote = 'Changed evidence' WHERE id = ?1",
            [&raw.evidence_id.0],
        )
        .unwrap();
    db.connection
        .execute(
            "UPDATE claims SET value = 'Changed employer' WHERE id = ?1",
            [&claimed.claim_id.as_ref().unwrap().0],
        )
        .unwrap();

    let found = db
        .search(SearchInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            query: "no lexical match".into(),
            limit: 10,
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

    let issues = db
        .projection_issues(ProjectionAuditInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            model: "test/model".into(),
            version: "1".into(),
            limit: 100,
        })
        .unwrap();
    let stale = issues
        .iter()
        .filter(|issue| issue.state == ProjectionState::Stale)
        .map(|issue| &issue.input.target)
        .collect::<HashSet<_>>();
    assert_eq!(stale, targets.iter().collect::<HashSet<_>>());
    assert!(issues.iter().all(|issue| {
        issue.input.target != EmbeddingTarget::Source(other.source_id.clone())
            && issue.input.target != EmbeddingTarget::Evidence(other.evidence_id.clone())
            && issue.input.target != EmbeddingTarget::Claim(other.claim_id.clone().unwrap())
    }));
    assert_eq!(
        db.projection_issues(ProjectionAuditInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            model: "test/model".into(),
            version: "1".into(),
            limit: 2,
        })
        .unwrap()
        .len(),
        2
    );
}

#[test]
fn projection_issues_stay_within_the_page_byte_limit() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    db.remember(remember_raw("a", "sam", &"x".repeat(600_000)))
        .unwrap();
    db.remember(remember_raw("a", "sam", &"y".repeat(600_000)))
        .unwrap();

    let issues = db
        .projection_issues(ProjectionAuditInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            model: "test/model".into(),
            version: "1".into(),
            limit: 100,
        })
        .unwrap();

    assert_eq!(issues.len(), 1);
    assert!(serde_json::to_vec(&issues).unwrap().len() <= MAX_PROJECTION_PAGE_BYTES);
}

#[test]
fn projection_issues_reject_an_individually_oversized_issue() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    db.remember(remember_raw("a", "sam", &"x".repeat(1_000_000)))
        .unwrap();

    assert!(matches!(
        db.projection_issues(ProjectionAuditInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            model: "test/model".into(),
            version: "1".into(),
            limit: 100,
        }),
        Err(Error::Invalid(message)) if message.contains("page limit")
    ));
}

#[test]
fn embedding_rejects_input_hash_for_different_text() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let remembered = db
        .remember(remember_raw("a", "sam", "Current text"))
        .unwrap();
    let result = db.upsert_embedding(EmbeddingInput {
        tenant_id: TenantId("a".into()),
        person_id: PersonId("sam".into()),
        target: EmbeddingTarget::Source(remembered.source_id),
        embedding: Embedding {
            vector: vec![1.0],
            model: "test/model".into(),
            version: "1".into(),
            input_hash: input_hash("different text"),
            normalization: VectorNormalization::L2,
            distance: VectorDistance::Cosine,
        },
    });
    assert!(matches!(result, Err(Error::Invalid(_))));
}
