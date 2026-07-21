use super::*;

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
fn deletion_rejects_zero_width_claim_recorded_interval_atomically() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let remembered = db.remember(remember("a", "sam", "Acme")).unwrap();
    let claim_id = remembered.claim_id.unwrap();

    let error = db
        .delete_source(DeleteInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            source_id: remembered.source_id.clone(),
            deleted_at: 10,
        })
        .unwrap_err();
    assert!(matches!(
        error,
        Error::Invalid(message)
            if message == "deleted_at must advance affected claims' recorded intervals"
    ));
    assert_eq!(
        db.connection
            .query_row(
                "SELECT s.deleted_at, e.deleted_at, c.status, c.recorded_until FROM sources s JOIN evidence e ON e.source_id = s.id JOIN claim_evidence ce ON ce.evidence_id = e.id JOIN claims c ON c.id = ce.claim_id WHERE s.id = ?1 AND c.id = ?2",
                params![remembered.source_id.0, claim_id.0],
                |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<i64>>(3)?)),
            )
            .unwrap(),
        (None, None, "accepted".to_owned(), None)
    );
}

#[test]
fn deletion_rejects_older_last_support_for_newer_claim_atomically() {
    let (mut db, source_id, claim_id) = claim_with_older_last_support();

    let error = db
        .delete_source(DeleteInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            source_id: source_id.clone(),
            deleted_at: 15,
        })
        .unwrap_err();
    assert!(matches!(
        error,
        Error::Invalid(message)
            if message == "deleted_at must advance affected claims' recorded intervals"
    ));
    assert_eq!(
        db.connection
            .query_row(
                "SELECT s.deleted_at, c.status, c.recorded_from, c.recorded_until FROM sources s, claims c WHERE s.id = ?1 AND c.id = ?2",
                params![source_id.0, claim_id.0],
                |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?, row.get::<_, Option<i64>>(3)?)),
            )
            .unwrap(),
        (None, "accepted".to_owned(), 20, None)
    );
}

#[test]
fn deletion_closes_newer_claim_at_a_valid_recorded_time() {
    let (mut db, source_id, claim_id) = claim_with_older_last_support();

    let deleted = db
        .delete_source(DeleteInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            source_id,
            deleted_at: 30,
        })
        .unwrap();
    assert_eq!((deleted.evidence_count, deleted.claim_count), (1, 1));
    assert_eq!(
        db.connection
            .query_row(
                "SELECT status, recorded_from, recorded_until FROM claims WHERE id = ?1",
                [&claim_id.0],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .unwrap(),
        ("retracted".to_owned(), 20, 30)
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
fn profile_and_review_pages_are_byte_bounded() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    for (index, key) in ["first", "second"].into_iter().enumerate() {
        let mut input = remember("a", "sam", &format!("{key}-{}", "x".repeat(600_000)));
        input.claim.as_mut().unwrap().kind = ClaimKind::ProfileFact;
        input.claim.as_mut().unwrap().predicate = key.into();
        input.captured_at = index as i64 + 10;
        input.recorded_at = index as i64 + 10;
        input.claim.as_mut().unwrap().valid_from = input.recorded_at;
        let remembered = db.remember(input).unwrap();
        db.store_profile(ProfileInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            stability: ProfileStability::Current,
            claim_id: remembered.claim_id.unwrap(),
            recorded_at: index as i64 + 10,
        })
        .unwrap();
    }
    assert!(matches!(
        db.profiles(ProfilesInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            limit: 100,
        })
        ,
        Err(Error::Invalid(message)) if message.contains("page limit")
    ));

    let evidence = db.remember(remember("a", "sam", "support")).unwrap();
    for (index, day) in ["2026-07-20", "2026-07-21"].into_iter().enumerate() {
        db.store_review(ReviewInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            day: day.into(),
            summary: "y".repeat(600_000),
            evidence_ids: vec![evidence.evidence_id.clone()],
            recorded_at: index as i64 + 20,
        })
        .unwrap();
    }
    assert!(matches!(
        db.reviews(ReviewsInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            limit: 100,
        })
        ,
        Err(Error::Invalid(message)) if message.contains("page limit")
    ));
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
