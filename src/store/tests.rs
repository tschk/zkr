use super::*;
use std::collections::HashSet;

fn remember(tenant: &str, person: &str, value: &str) -> RememberInput {
    RememberInput {
        tenant_id: TenantId(tenant.into()),
        person_id: PersonId(person.into()),
        ingestion_key: None,
        kind: SourceKind::Conversation,
        text: format!("Sam works at {value}"),
        captured_at: 10,
        recorded_at: 10,
        claim: Some(ClaimInput {
            subject: "Sam".into(),
            predicate: "employer".into(),
            value: value.into(),
            kind: ClaimKind::Fact,
            valid_from: 10,
        }),
    }
}

fn remember_raw(tenant: &str, person: &str, text: &str) -> RememberInput {
    RememberInput {
        tenant_id: TenantId(tenant.into()),
        person_id: PersonId(person.into()),
        ingestion_key: None,
        kind: SourceKind::Conversation,
        text: text.into(),
        captured_at: 10,
        recorded_at: 10,
        claim: None,
    }
}

fn hash_for(db: &MemoryDb, target: EmbeddingTarget) -> String {
    db.projection_input(&TenantId("a".into()), &PersonId("sam".into()), target)
        .unwrap()
        .input_hash
}

#[test]
fn ingestion_keys_are_idempotent_within_scope() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let mut first = remember_raw("a", "sam", "Remember once");
    first.ingestion_key = Some("turn-1".into());
    let stored = db.remember(first).unwrap();
    let mut replay = remember_raw("a", "sam", "Remember once");
    replay.ingestion_key = Some("turn-1".into());
    let replayed = db.remember(replay).unwrap();
    assert_eq!(stored.source_id, replayed.source_id);
    assert_eq!(stored.evidence_id, replayed.evidence_id);
    assert_eq!(
        db.connection
            .query_row("SELECT count(*) FROM sources", [], |row| row
                .get::<_, u64>(0))
            .unwrap(),
        1
    );
    let mut other_scope = remember_raw("b", "sam", "Remember once");
    other_scope.ingestion_key = Some("turn-1".into());
    assert_ne!(
        stored.source_id,
        db.remember(other_scope).unwrap().source_id
    );
}

#[test]
fn raw_ingestion_replay_is_not_changed_by_later_claim_links() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let claimed = db.remember(remember("a", "sam", "Acme")).unwrap();
    let mut raw_input = remember_raw("a", "sam", "Acme is mentioned again");
    raw_input.ingestion_key = Some("raw-turn".into());
    let raw = db.remember(raw_input).unwrap();
    db.link_claim_evidence(ClaimEvidence {
        tenant_id: TenantId("a".into()),
        person_id: PersonId("sam".into()),
        claim_id: claimed.claim_id.unwrap(),
        evidence_id: raw.evidence_id.clone(),
        relation: EvidenceRelation::Supports,
        confidence_basis_points: 8_000,
    })
    .unwrap();

    let mut replay = remember_raw("a", "sam", "Acme is mentioned again");
    replay.ingestion_key = Some("raw-turn".into());
    let replayed = db.remember(replay).unwrap();
    assert_eq!(replayed.source_id, raw.source_id);
    assert_eq!(replayed.evidence_id, raw.evidence_id);
    assert_eq!(replayed.claim_id, None);
}

#[test]
fn ingestion_key_rejects_changed_payload() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let mut first = remember_raw("a", "sam", "Remember once");
    first.ingestion_key = Some("turn-1".into());
    db.remember(first).unwrap();
    let changed_text = remember_raw("a", "sam", "Changed replay content");
    let mut changed_kind = remember_raw("a", "sam", "Remember once");
    changed_kind.kind = SourceKind::Screen;
    let mut changed_time = remember_raw("a", "sam", "Remember once");
    changed_time.captured_at = 11;
    let mut changed_claim = remember_raw("a", "sam", "Remember once");
    changed_claim.claim = Some(ClaimInput {
        subject: "Sam".into(),
        predicate: "status".into(),
        value: "focused".into(),
        kind: ClaimKind::Fact,
        valid_from: 10,
    });
    for mut changed in [changed_text, changed_kind, changed_time, changed_claim] {
        changed.ingestion_key = Some("turn-1".into());
        assert!(matches!(db.remember(changed), Err(Error::Invalid(_))));
    }
    let with_claim = |value: &str| {
        let mut input = remember_raw("a", "sam", "Claim capture");
        input.ingestion_key = Some("claim-turn".into());
        input.claim = Some(ClaimInput {
            subject: "Sam".into(),
            predicate: "status".into(),
            value: value.into(),
            kind: ClaimKind::Fact,
            valid_from: 10,
        });
        input
    };
    let stored = db.remember(with_claim("focused")).unwrap();
    assert_eq!(
        stored.claim_id,
        db.remember(with_claim("focused")).unwrap().claim_id
    );
    assert!(matches!(
        db.remember(with_claim("distracted")),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn ingestion_key_rejects_deleted_memory() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let mut first = remember_raw("a", "sam", "Remember once");
    first.ingestion_key = Some("turn-1".into());
    let stored = db.remember(first).unwrap();
    db.delete_source(DeleteInput {
        tenant_id: TenantId("a".into()),
        person_id: PersonId("sam".into()),
        source_id: stored.source_id,
        deleted_at: 20,
    })
    .unwrap();
    let mut replay = remember_raw("a", "sam", "Remember once");
    replay.ingestion_key = Some("turn-1".into());
    assert!(matches!(db.remember(replay), Err(Error::NotFound)));
}

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
        })
        .unwrap()
        .items
        .is_empty()
    );
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
        })
        .unwrap();
    assert!(found.items.is_empty());
}

#[test]
fn lifecycle_is_scoped_cited_and_propagates_deletion() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let mut first_input = remember("a", "sam", "Acme");
    first_input.captured_at = 0;
    first_input.recorded_at = 10;
    first_input.claim.as_mut().unwrap().valid_from = 0;
    let first = db.remember(first_input).unwrap();
    db.remember(remember("b", "sam", "Other")).unwrap();
    let found = db
        .search(SearchInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            query: "Acme".into(),
            limit: 5,
            query_embedding: None,
            as_of: None,
        })
        .unwrap();
    assert_eq!(found.items.len(), 1);
    assert_eq!(found.items[0].evidence_ids, vec![first.evidence_id.clone()]);
    let corrected = db
        .correct(CorrectInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            claim_id: first.claim_id.unwrap(),
            text: "I moved to Beta".into(),
            value: "Beta".into(),
            valid_at: 5,
            recorded_at: 20,
        })
        .unwrap();
    let old_intervals = db
        .connection
        .query_row(
            "SELECT valid_until, recorded_until FROM claims WHERE id = ?1",
            [&corrected.superseded_claim_id.0],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .unwrap();
    assert_eq!(old_intervals, (5, 20));
    assert!(
        db.search(SearchInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            query: "Acme".into(),
            limit: 5,
            query_embedding: None,
            as_of: None,
        })
        .unwrap()
        .items
        .is_empty()
    );
    assert_eq!(
        db.search(SearchInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            query: "Acme".into(),
            limit: 5,
            query_embedding: None,
            as_of: Some(TemporalQuery {
                valid_at: 4,
                recorded_at: 15,
            }),
        })
        .unwrap()
        .items[0]
            .excerpt,
        "Sam employer Acme"
    );
    let deleted = db
        .delete_source(DeleteInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            source_id: corrected.source_id,
            deleted_at: 30,
        })
        .unwrap();
    assert_eq!((deleted.evidence_count, deleted.claim_count), (1, 1));
    assert!(
        db.search(SearchInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            query: "Beta".into(),
            limit: 5,
            query_embedding: None,
            as_of: None,
        })
        .unwrap()
        .items
        .is_empty()
    );
}

#[test]
fn deleting_a_source_does_not_purge_unrelated_retracted_claim_projections() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let unrelated = db.remember(remember("a", "sam", "Acme")).unwrap();
    let claim_id = unrelated.claim_id.unwrap();
    let target = EmbeddingTarget::Claim(claim_id.clone());
    db.upsert_embedding(EmbeddingInput {
        tenant_id: TenantId("a".into()),
        person_id: PersonId("sam".into()),
        target: target.clone(),
        embedding: Embedding {
            vector: vec![1.0],
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
            "UPDATE claims SET status = 'retracted', recorded_until = 20 WHERE id = ?1",
            [&claim_id.0],
        )
        .unwrap();
    let removed = db
        .remember(remember_raw("a", "sam", "Delete only this source"))
        .unwrap();
    db.delete_source(DeleteInput {
        tenant_id: TenantId("a".into()),
        person_id: PersonId("sam".into()),
        source_id: removed.source_id,
        deleted_at: 20,
    })
    .unwrap();
    assert_eq!(
        db.connection
            .query_row(
                "SELECT count(*) FROM embeddings WHERE target_kind = 'claim' AND target_id = ?1",
                [&claim_id.0],
                |row| row.get::<_, u64>(0),
            )
            .unwrap(),
        1
    );
}

#[test]
fn profile_entries_and_claim_evidence_remain_scoped_and_live() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let mut profile_fact = remember("a", "sam", "Acme");
    profile_fact.claim.as_mut().unwrap().kind = ClaimKind::ProfileFact;
    let claimed = db.remember(profile_fact).unwrap();
    assert!(matches!(
        db.store_profile(ProfileInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            stability: ProfileStability::Current,
            claim_id: claimed.claim_id.clone().unwrap(),
            recorded_at: 9,
        }),
        Err(Error::Invalid(_))
    ));
    let entry = db
        .store_profile(ProfileInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            stability: ProfileStability::Current,
            claim_id: claimed.claim_id.clone().unwrap(),
            recorded_at: 11,
        })
        .unwrap();
    assert_eq!(
        db.profiles(ProfilesInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            limit: 10,
        })
        .unwrap(),
        vec![entry.clone()]
    );
    let replay = db
        .store_profile(ProfileInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            stability: ProfileStability::Current,
            claim_id: claimed.claim_id.clone().unwrap(),
            recorded_at: 11,
        })
        .unwrap();
    assert_eq!(replay, entry);
    let mut replacement_fact = remember("a", "sam", "Beta");
    replacement_fact.claim.as_mut().unwrap().kind = ClaimKind::ProfileFact;
    let replacement = db.remember(replacement_fact).unwrap();
    let replacement_source_id = replacement.source_id.clone();
    assert!(matches!(
        db.store_profile(ProfileInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            stability: ProfileStability::Current,
            claim_id: replacement.claim_id.clone().unwrap(),
            recorded_at: 11,
        }),
        Err(Error::Invalid(_))
    ));
    let replaced = db
        .store_profile(ProfileInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            stability: ProfileStability::Current,
            claim_id: replacement.claim_id.unwrap(),
            recorded_at: 12,
        })
        .unwrap();
    assert_eq!(replaced.id, entry.id);
    assert_eq!(
        (replaced.key.as_str(), replaced.value.as_str()),
        ("employer", "Beta")
    );
    assert_eq!(
        db.profiles(ProfilesInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            limit: 10,
        })
        .unwrap()
        .len(),
        1
    );
    let raw = db
        .remember(remember_raw("a", "sam", "Sam left Acme"))
        .unwrap();
    db.link_claim_evidence(ClaimEvidence {
        tenant_id: TenantId("a".into()),
        person_id: PersonId("sam".into()),
        claim_id: claimed.claim_id.clone().unwrap(),
        evidence_id: raw.evidence_id,
        relation: EvidenceRelation::Contradicts,
        confidence_basis_points: 9_000,
    })
    .unwrap();
    assert!(matches!(
        db.link_claim_evidence(ClaimEvidence {
            tenant_id: TenantId("b".into()),
            person_id: PersonId("sam".into()),
            claim_id: claimed.claim_id.clone().unwrap(),
            evidence_id: claimed.evidence_id.clone(),
            relation: EvidenceRelation::Contradicts,
            confidence_basis_points: 9_000,
        }),
        Err(Error::NotFound)
    ));
    db.delete_source(DeleteInput {
        tenant_id: TenantId("a".into()),
        person_id: PersonId("sam".into()),
        source_id: replacement_source_id,
        deleted_at: 20,
    })
    .unwrap();
    assert!(
        db.profiles(ProfilesInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            limit: 10,
        })
        .unwrap()
        .is_empty()
    );
    assert!(
        serde_json::from_value::<ProfileInput>(serde_json::json!({
            "tenant_id": "a",
            "person_id": "sam",
            "key": "employer",
            "value": "Acme",
            "stability": "current",
            "claim_id": "claim",
            "recorded_at": 11
        }))
        .is_err()
    );
}

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
fn reviews_require_live_same_scope_evidence() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let remembered = db.remember(remember("a", "sam", "Acme")).unwrap();
    db.store_review(ReviewInput {
        tenant_id: TenantId("a".into()),
        person_id: PersonId("sam".into()),
        day: "2026-07-21".into(),
        summary: "Worked at Acme".into(),
        evidence_ids: vec![remembered.evidence_id.clone()],
        recorded_at: 20,
    })
    .unwrap();
    assert_eq!(
        db.reviews(ReviewsInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            limit: 1
        })
        .unwrap()
        .len(),
        1
    );
    db.delete_source(DeleteInput {
        tenant_id: TenantId("a".into()),
        person_id: PersonId("sam".into()),
        source_id: remembered.source_id,
        deleted_at: 30,
    })
    .unwrap();
    assert!(
        db.reviews(ReviewsInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            limit: 1
        })
        .unwrap()
        .is_empty()
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

#[test]
fn migration_marks_existing_projections_for_lifecycle_revalidation() {
    let connection = Connection::open_in_memory().unwrap();
    connection
            .execute_batch(
                "CREATE TABLE sources(id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, revision INTEGER NOT NULL, kind TEXT NOT NULL, content TEXT NOT NULL, captured_at INTEGER NOT NULL, recorded_at INTEGER NOT NULL, deleted_at INTEGER);
                 CREATE VIRTUAL TABLE source_fts USING fts5(source_id UNINDEXED, tenant_id UNINDEXED, person_id UNINDEXED, content, tokenize='unicode61');
                 CREATE TABLE evidence(id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, source_id TEXT NOT NULL REFERENCES sources(id), source_revision INTEGER NOT NULL, quote TEXT NOT NULL, recorded_at INTEGER NOT NULL, deleted_at INTEGER);
                 CREATE TABLE claims(id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, subject TEXT NOT NULL, predicate TEXT NOT NULL, value TEXT NOT NULL, valid_from INTEGER NOT NULL, valid_until INTEGER, recorded_from INTEGER NOT NULL, recorded_until INTEGER, status TEXT NOT NULL);
                 CREATE TABLE claim_evidence(tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, claim_id TEXT NOT NULL REFERENCES claims(id), evidence_id TEXT NOT NULL REFERENCES evidence(id), relation TEXT NOT NULL, confidence_basis_points INTEGER NOT NULL, PRIMARY KEY(tenant_id, person_id, claim_id, evidence_id));
                 CREATE TABLE daily_reviews(id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, day TEXT NOT NULL, summary TEXT NOT NULL, evidence_ids TEXT NOT NULL, recorded_at INTEGER NOT NULL);
                 CREATE TABLE embeddings(tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, target_kind TEXT NOT NULL, target_id TEXT NOT NULL, model TEXT NOT NULL, version TEXT NOT NULL, dimension INTEGER NOT NULL, input_hash TEXT NOT NULL, normalization TEXT NOT NULL, distance TEXT NOT NULL, vector TEXT NOT NULL, PRIMARY KEY(tenant_id, person_id, target_kind, target_id, model, version));
                 INSERT INTO sources VALUES('old', 'a', 'sam', 1, '\"conversation\"', 'old text', 10, 10, NULL);
                 INSERT INTO embeddings VALUES('a', 'sam', 'source', 'old', 'model', '1', 1, 'sha256:old', '\"l2\"', '\"cosine\"', '[1.0]');
                 PRAGMA user_version = 1;",
            )
            .unwrap();
    let mut db = MemoryDb { connection };
    db.migrate().unwrap();
    let lifecycle = db
        .connection
        .query_row(
            "SELECT target_revision, created_at FROM embeddings WHERE target_id = 'old'",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .unwrap();
    assert_eq!(lifecycle, (0, 0));
    let remembered = db
        .remember(remember_raw("a", "sam", "Upgraded v1 memory"))
        .unwrap();
    assert!(
        db.projection_input(
            &TenantId("a".into()),
            &PersonId("sam".into()),
            EmbeddingTarget::Source(remembered.source_id),
        )
        .is_ok()
    );
}

#[test]
fn new_database_accepts_point_locators() {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch("PRAGMA user_version = 0;")
        .unwrap();
    let mut db = MemoryDb { connection };
    db.migrate().unwrap();
    let version = db
        .connection
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .unwrap();
    assert_eq!(version, 6);

    let remembered = db
        .remember_with_locator(
            remember_raw("a", "sam", "Untimed final transcript"),
            Some(TranscriptLocator {
                device_id: "omi-1".into(),
                provider: "deepgram".into(),
                stream_id: "stream-1".into(),
                segment_id: "segment-1".into(),
                start_ms: 1000,
                end_ms: 1000,
            }),
        )
        .unwrap();
    assert_eq!(
        db.evidence_locator(EvidenceLocatorInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            evidence_id: remembered.evidence_id,
        })
        .unwrap()
        .unwrap()
        .start_ms,
        1000
    );
}

#[test]
fn migration_preserves_unknown_legacy_superseded_claim_validity() {
    let connection = Connection::open_in_memory().unwrap();
    connection
            .execute_batch(
                "CREATE TABLE claims(id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, subject TEXT NOT NULL, predicate TEXT NOT NULL, value TEXT NOT NULL, valid_from INTEGER NOT NULL, valid_until INTEGER, recorded_from INTEGER NOT NULL, recorded_until INTEGER, status TEXT NOT NULL);
                 INSERT INTO claims VALUES('old', 'a', 'sam', 'sam', 'employer', 'Acme', 10, NULL, 10, 20, 'superseded');
                 PRAGMA user_version = 0;",
            )
            .unwrap();
    let mut db = MemoryDb { connection };
    db.migrate().unwrap();
    let valid_until = db
        .connection
        .query_row(
            "SELECT valid_until FROM claims WHERE id = 'old'",
            [],
            |row| row.get::<_, Option<i64>>(0),
        )
        .unwrap();
    assert_eq!(valid_until, None);
}

#[test]
fn migration_is_idempotent_for_supported_schema_versions() {
    for version in 0..=5 {
        let connection = Connection::open_in_memory().unwrap();
        if version == 0 {
            connection
                .execute_batch("PRAGMA user_version = 0;")
                .unwrap();
        } else {
            let claim_kind = if version == 5 {
                "kind TEXT NOT NULL DEFAULT 'fact',"
            } else {
                ""
            };
            connection
                    .execute_batch(&format!(
                        "CREATE TABLE sources(id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, ingestion_key TEXT, revision INTEGER NOT NULL, kind TEXT NOT NULL, content TEXT NOT NULL, captured_at INTEGER NOT NULL, recorded_at INTEGER NOT NULL, deleted_at INTEGER);
                         CREATE VIRTUAL TABLE source_fts USING fts5(source_id UNINDEXED, tenant_id UNINDEXED, person_id UNINDEXED, content, tokenize='unicode61');
                         CREATE TABLE evidence(id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, source_id TEXT NOT NULL REFERENCES sources(id), source_revision INTEGER NOT NULL, quote TEXT NOT NULL, recorded_at INTEGER NOT NULL, deleted_at INTEGER);
                         CREATE TABLE claims(id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, subject TEXT NOT NULL, predicate TEXT NOT NULL, value TEXT NOT NULL, {claim_kind} valid_from INTEGER NOT NULL, valid_until INTEGER, recorded_from INTEGER NOT NULL, recorded_until INTEGER, status TEXT NOT NULL);
                         CREATE TABLE claim_evidence(tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, claim_id TEXT NOT NULL REFERENCES claims(id), evidence_id TEXT NOT NULL REFERENCES evidence(id), relation TEXT NOT NULL, confidence_basis_points INTEGER NOT NULL, PRIMARY KEY(tenant_id, person_id, claim_id, evidence_id));
                         CREATE TABLE daily_reviews(id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, day TEXT NOT NULL, summary TEXT NOT NULL, evidence_ids TEXT NOT NULL, recorded_at INTEGER NOT NULL);
                         CREATE TABLE embeddings(tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, target_kind TEXT NOT NULL, target_id TEXT NOT NULL, model TEXT NOT NULL, version TEXT NOT NULL, dimension INTEGER NOT NULL, input_hash TEXT NOT NULL, target_revision INTEGER NOT NULL, created_at INTEGER NOT NULL, normalization TEXT NOT NULL, distance TEXT NOT NULL, vector TEXT NOT NULL, PRIMARY KEY(tenant_id, person_id, target_kind, target_id, model, version));
                         INSERT INTO sources VALUES('source', 'a', 'sam', 'turn', 1, '\"conversation\"', 'Sam works at Acme', 10, 11, NULL);
                         INSERT INTO source_fts VALUES('source', 'a', 'sam', 'Sam works at Acme');
                         INSERT INTO evidence VALUES('evidence', 'a', 'sam', 'source', 1, 'Sam works at Acme', 11, NULL);
                         INSERT INTO claims(id, tenant_id, person_id, subject, predicate, value, valid_from, recorded_from, status) VALUES('claim', 'a', 'sam', 'Sam', 'employer', 'Acme', 10, 11, 'accepted');
                         INSERT INTO claim_evidence VALUES('a', 'sam', 'claim', 'evidence', '\"supports\"', 10000);
                         INSERT INTO daily_reviews VALUES('review', 'a', 'sam', '2026-07-21', 'Sam works at Acme', '[\"evidence\"]', 12);
                         INSERT INTO embeddings VALUES('a', 'sam', 'claim', 'claim', 'model', '1', 1, 'sha256:legacy', 11, 12, '\"l2\"', '\"cosine\"', '[1.0]');
                         PRAGMA user_version = {version};"
                    ))
                    .unwrap();
            if version >= 2 {
                connection
                        .execute_batch(
                            "CREATE TABLE evidence_locators(tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, evidence_id TEXT NOT NULL REFERENCES evidence(id), device_id TEXT NOT NULL, provider TEXT NOT NULL, stream_id TEXT NOT NULL, segment_id TEXT NOT NULL, start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL, PRIMARY KEY(tenant_id, person_id, evidence_id));
                             INSERT INTO evidence_locators VALUES('a', 'sam', 'evidence', 'device', 'provider', 'stream', 'segment', 1, 1);",
                        )
                        .unwrap();
            }
            if version == 5 {
                connection
                        .execute_batch(
                            "CREATE TABLE profile_entries(id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL, stability TEXT NOT NULL, claim_id TEXT NOT NULL REFERENCES claims(id), recorded_at INTEGER NOT NULL);
                             UPDATE claims SET kind = 'profile_fact' WHERE id = 'claim';
                             INSERT INTO profile_entries VALUES('profile', 'a', 'sam', 'company', 'ACME Corp', '\"current\"', 'claim', 12);",
                        )
                        .unwrap();
            }
        }
        let mut db = MemoryDb { connection };
        db.migrate().unwrap();
        db.migrate().unwrap();
        let actual = db
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap();
        assert_eq!(actual, 6);
        if version > 0 {
            let preserved = db
                .connection
                .query_row(
                    "SELECT origin_evidence_id, origin_claim_id FROM sources WHERE id = 'source'",
                    [],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .unwrap();
            assert_eq!(preserved, ("evidence".into(), "claim".into()));
            assert_eq!(
                db.connection
                    .query_row("SELECT kind FROM claims WHERE id = 'claim'", [], |row| {
                        row.get::<_, String>(0)
                    })
                    .unwrap(),
                if version == 5 { "profile_fact" } else { "fact" }
            );
            if version == 5 {
                assert_eq!(
                    db.connection
                        .query_row(
                            "SELECT key, value FROM profile_entries WHERE id = 'profile'",
                            [],
                            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                        )
                        .unwrap(),
                    ("employer".into(), "Acme".into())
                );
            }
        }
    }
}

#[test]
fn reopening_current_schema_does_not_write_or_repair_data() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    db.remember(remember_raw("a", "sam", "Keep this unchanged"))
        .unwrap();
    let before = db
        .connection
        .query_row("SELECT total_changes()", [], |row| row.get::<_, i64>(0))
        .unwrap();
    db.migrate().unwrap();
    let after = db
        .connection
        .query_row("SELECT total_changes()", [], |row| row.get::<_, i64>(0))
        .unwrap();
    assert_eq!(after, before);
}

#[test]
fn schema_rejects_cross_scope_references() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let first = db.remember(remember("a", "sam", "Acme")).unwrap();
    let second = db.remember(remember("b", "sam", "Other")).unwrap();

    assert!(db
            .connection
            .execute(
                "INSERT INTO evidence(id, tenant_id, person_id, source_id, source_revision, quote, recorded_at) VALUES(?1, ?2, ?3, ?4, 1, 'cross', 10)",
                params!["cross-evidence", "b", "sam", first.source_id.0],
            )
            .is_err());
    assert!(db
            .connection
            .execute(
                "INSERT INTO evidence_locators(tenant_id, person_id, evidence_id, device_id, provider, stream_id, segment_id, start_ms, end_ms) VALUES('b', 'sam', ?1, 'device', 'provider', 'stream', 'segment', 0, 1)",
                [&first.evidence_id.0],
            )
            .is_err());
    assert!(db
            .connection
            .execute(
                "INSERT INTO claim_evidence(tenant_id, person_id, claim_id, evidence_id, relation, confidence_basis_points) VALUES('b', 'sam', ?1, ?2, '\"supports\"', 10000)",
                params![second.claim_id.unwrap().0, first.evidence_id.0],
            )
            .is_err());
    assert!(db
            .connection
            .execute(
                "INSERT INTO embeddings(tenant_id, person_id, target_kind, target_id, model, version, dimension, input_hash, target_revision, created_at, normalization, distance, vector) VALUES('b', 'sam', 'source', ?1, 'model', '1', 1, 'sha256:input', 1, 1, '\"l2\"', '\"cosine\"', '[1.0]')",
                [&first.source_id.0],
            )
            .is_err());
    let citations = serde_json::to_string(&[first.evidence_id]).unwrap();
    assert!(db
            .connection
            .execute(
                "INSERT INTO daily_reviews(id, tenant_id, person_id, day, summary, evidence_ids, recorded_at) VALUES('cross-review', 'b', 'sam', '2026-07-21', 'cross', ?1, 10)",
                [&citations],
            )
            .is_err());
}

#[test]
fn migration_rejects_legacy_cross_scope_evidence() {
    let connection = Connection::open_in_memory().unwrap();
    connection
            .execute_batch(
                "CREATE TABLE sources(id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, ingestion_key TEXT, revision INTEGER NOT NULL, kind TEXT NOT NULL, content TEXT NOT NULL, captured_at INTEGER NOT NULL, recorded_at INTEGER NOT NULL, deleted_at INTEGER);
                 CREATE TABLE evidence(id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, source_id TEXT NOT NULL, source_revision INTEGER NOT NULL, quote TEXT NOT NULL, recorded_at INTEGER NOT NULL, deleted_at INTEGER);
                 INSERT INTO sources VALUES('source-a', 'a', 'sam', NULL, 1, '\"conversation\"', 'source', 10, 10, NULL);
                 INSERT INTO evidence VALUES('evidence-b', 'b', 'sam', 'source-a', 1, 'cross', 10, NULL);",
            )
            .unwrap();
    let mut db = MemoryDb { connection };
    assert!(matches!(db.migrate(), Err(Error::Invalid(_))));
}

#[test]
fn migration_rejects_legacy_cross_scope_embeddings() {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "CREATE TABLE sources(id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, ingestion_key TEXT, revision INTEGER NOT NULL, kind TEXT NOT NULL, content TEXT NOT NULL, captured_at INTEGER NOT NULL, recorded_at INTEGER NOT NULL, deleted_at INTEGER);
             CREATE TABLE embeddings(tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, target_kind TEXT NOT NULL, target_id TEXT NOT NULL, model TEXT NOT NULL, version TEXT NOT NULL, dimension INTEGER NOT NULL, input_hash TEXT NOT NULL, target_revision INTEGER NOT NULL, created_at INTEGER NOT NULL, normalization TEXT NOT NULL, distance TEXT NOT NULL, vector TEXT NOT NULL, PRIMARY KEY(tenant_id, person_id, target_kind, target_id, model, version));
             INSERT INTO sources VALUES('source-a', 'a', 'sam', NULL, 1, '\"conversation\"', 'private', 10, 10, NULL);
             INSERT INTO embeddings VALUES('b', 'sam', 'source', 'source-a', 'model', '1', 1, 'sha256:legacy', 1, 10, '\"l2\"', '\"cosine\"', '[1.0]');
             PRAGMA user_version = 0;",
        )
        .unwrap();
    let mut db = MemoryDb { connection };
    assert!(matches!(db.migrate(), Err(Error::Invalid(_))));
}
