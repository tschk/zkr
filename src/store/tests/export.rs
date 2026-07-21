use super::*;

fn input(
    after_commit: i64,
    after_event_index: i64,
    high_water_mark: Option<i64>,
    limit: u32,
) -> ExportInput {
    ExportInput {
        export_format: EXPORT_FORMAT_VERSION,
        tenant_id: TenantId("a".into()),
        person_id: PersonId("sam".into()),
        after_commit,
        after_event_index,
        high_water_mark,
        limit,
    }
}

#[test]
fn export_is_scoped_frozen_and_event_bounded() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    db.remember(remember("a", "sam", "Acme")).unwrap();
    db.remember(remember("b", "sam", "Other")).unwrap();

    let first = db.export(input(0, -1, None, 2)).unwrap();
    assert_eq!(first.export_format, EXPORT_FORMAT_VERSION);
    assert_eq!(first.database_schema_version, DATABASE_SCHEMA_VERSION);
    assert_eq!(
        first
            .commits
            .iter()
            .map(|commit| commit.records.len())
            .sum::<usize>(),
        2
    );
    assert!(!first.complete);
    assert_eq!(first.commits[0].event_count, 4);

    db.remember(remember_raw("a", "sam", "future")).unwrap();
    let second = db
        .export(input(
            first.next_after_commit,
            first.next_after_event_index,
            Some(first.high_water_mark),
            2,
        ))
        .unwrap();
    assert!(second.complete);
    assert_eq!(second.high_water_mark, first.high_water_mark);
    assert_eq!(
        second
            .commits
            .iter()
            .map(|commit| commit.records.len())
            .sum::<usize>(),
        2
    );
    assert!(
        second
            .commits
            .iter()
            .flat_map(|commit| &commit.records)
            .all(|record| match record {
                ExportRecord::Source(record) => record.source.tenant_id.0 == "a",
                ExportRecord::Evidence(record) => record.evidence.tenant_id.0 == "a",
                ExportRecord::Claim(record) => record.tenant_id.0 == "a",
                ExportRecord::ClaimEvidence(record) => record.tenant_id.0 == "a",
                _ => true,
            })
    );
}

#[test]
fn export_rejects_forged_watermark_and_version() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    assert!(matches!(
        db.export(input(0, -1, Some(1), 10)),
        Err(Error::Invalid(_))
    ));
    let mut wrong = input(0, -1, None, 10);
    wrong.export_format = 99;
    assert!(matches!(db.export(wrong), Err(Error::Invalid(_))));
}

#[test]
fn replay_and_failed_mutations_emit_no_commit() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let mut memory = remember("a", "sam", "Acme");
    memory.ingestion_key = Some("same".into());
    db.remember(memory).unwrap();
    let before = db.export(input(0, -1, None, 100)).unwrap().high_water_mark;
    let mut replay = remember("a", "sam", "Acme");
    replay.ingestion_key = Some("same".into());
    db.remember(replay).unwrap();
    assert_eq!(
        db.export(input(0, -1, None, 100)).unwrap().high_water_mark,
        before
    );
    let failure = db.delete_source(DeleteInput {
        tenant_id: TenantId("a".into()),
        person_id: PersonId("sam".into()),
        source_id: SourceId("missing".into()),
        deleted_at: 20,
    });
    assert!(failure.is_err());
    assert_eq!(
        db.export(input(0, -1, None, 100)).unwrap().high_water_mark,
        before
    );
}

#[test]
fn correction_and_deletion_are_explicit_and_split_safely() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let remembered = db.remember(remember("a", "sam", "Acme")).unwrap();
    let corrected = db
        .correct(CorrectInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            claim_id: remembered.claim_id.unwrap(),
            text: "Sam works at Beta".into(),
            value: "Beta".into(),
            valid_at: 20,
            recorded_at: 20,
        })
        .unwrap();
    db.delete_source(DeleteInput {
        tenant_id: TenantId("a".into()),
        person_id: PersonId("sam".into()),
        source_id: corrected.source_id.clone(),
        deleted_at: 30,
    })
    .unwrap();

    let mut cursor = (0, -1);
    let mut high_water = None;
    let mut records = Vec::new();
    loop {
        let page = db.export(input(cursor.0, cursor.1, high_water, 2)).unwrap();
        high_water = Some(page.high_water_mark);
        assert!(
            page.commits
                .iter()
                .map(|commit| commit.records.len())
                .sum::<usize>()
                <= 2
        );
        records.extend(page.commits.into_iter().flat_map(|commit| commit.records));
        cursor = (page.next_after_commit, page.next_after_event_index);
        if page.complete {
            break;
        }
    }
    assert!(records.iter().any(|record| matches!(record, ExportRecord::Correction(value) if value.claim_id == corrected.claim_id)));
    assert!(records.iter().any(|record| matches!(record, ExportRecord::Deletion(value) if value.target == MemoryRef::Source(corrected.source_id.clone()))));
}

#[test]
fn correction_scope_triggers_reject_insert_and_update() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let first = db.remember(remember("a", "sam", "Acme")).unwrap();
    let superseded_claim_id = first.claim_id.unwrap();
    let corrected = db
        .correct(CorrectInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            claim_id: superseded_claim_id.clone(),
            text: "Sam works at Beta".into(),
            value: "Beta".into(),
            valid_at: 20,
            recorded_at: 20,
        })
        .unwrap();
    let other = db.remember(remember("b", "sam", "Other")).unwrap();
    let insert = db.connection.execute(
        "INSERT INTO corrections(tenant_id, person_id, superseded_claim_id, claim_id, source_id, evidence_id, valid_at, recorded_at) VALUES('a', 'sam', ?1, ?2, ?3, ?4, 30, 30)",
        params![superseded_claim_id.0, other.claim_id.as_ref().unwrap().0, other.source_id.0, other.evidence_id.0],
    );
    assert!(insert.is_err());
    let result = db.connection.execute(
        "UPDATE corrections SET claim_id = ?1 WHERE tenant_id = 'a' AND person_id = 'sam' AND claim_id = ?2",
        params![other.claim_id.unwrap().0, corrected.claim_id.0],
    );
    assert!(result.is_err());
}

#[test]
fn migration_bootstrap_is_event_bounded() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    db.remember(remember("a", "sam", "Acme")).unwrap();
    db.connection
        .execute_batch(
            "DELETE FROM memory_commits;
             PRAGMA user_version = 7;",
        )
        .unwrap();
    db.migrate().unwrap();

    let first = db.export(input(0, -1, None, 2)).unwrap();
    assert_eq!(first.commits.len(), 1);
    assert_eq!(first.commits[0].event_count, 4);
    assert_eq!(first.commits[0].records.len(), 2);
    assert!(!first.complete);
    let second = db
        .export(input(
            first.next_after_commit,
            first.next_after_event_index,
            Some(first.high_water_mark),
            2,
        ))
        .unwrap();
    assert_eq!(second.commits[0].event_count, 4);
    assert_eq!(second.commits[0].first_event_index, 2);
    assert!(second.complete);
}

#[test]
fn migration_bootstrap_rejects_oversized_legacy_record_atomically() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let remembered = db.remember(remember_raw("a", "sam", "small")).unwrap();
    db.connection
        .execute(
            "UPDATE sources SET content = ?1 WHERE id = ?2",
            params!["x".repeat(MAX_EXPORT_RECORD_BYTES), remembered.source_id.0],
        )
        .unwrap();
    db.connection
        .execute_batch(
            "DELETE FROM memory_commits;
             PRAGMA user_version = 7;",
        )
        .unwrap();

    assert!(matches!(
        db.migrate(),
        Err(Error::Invalid(message)) if message.contains("export compatibility limit")
    ));
    let version: i64 = db
        .connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    let commits: i64 = db
        .connection
        .query_row("SELECT COUNT(*) FROM memory_commits", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 7);
    assert_eq!(commits, 0);
}

#[test]
fn export_rejects_corrupt_cross_scope_payload() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    db.remember(remember_raw("a", "sam", "capture")).unwrap();
    db.connection
        .execute(
            "UPDATE memory_export_events SET payload = json_set(payload, '$.record.source.tenant_id', 'forged') WHERE event_index = 0",
            [],
        )
        .unwrap();
    assert!(matches!(
        db.export(input(0, -1, None, 100)),
        Err(Error::Invalid(message)) if message.contains("scope")
    ));
}

#[test]
fn append_rejects_cross_scope_payload_and_rolls_back_commit() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let transaction = db.connection.transaction().unwrap();
    let commit = crate::store::export::begin_commit(
        &transaction,
        &TenantId("a".into()),
        &PersonId("sam".into()),
        10,
    )
    .unwrap();
    let record = ExportRecord::Profile(ProfileEntry {
        id: ProfileEntryId("profile".into()),
        tenant_id: TenantId("forged".into()),
        person_id: PersonId("sam".into()),
        key: "employer".into(),
        value: "Acme".into(),
        stability: ProfileStability::Current,
        claim_id: ClaimId("claim".into()),
        recorded_at: 10,
    });
    assert!(matches!(
        crate::store::export::append_records(&transaction, commit, [record]),
        Err(Error::Invalid(message)) if message.contains("scope")
    ));
    transaction.rollback().unwrap();
    let count: i64 = db
        .connection
        .query_row("SELECT COUNT(*) FROM memory_commits", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn oversized_record_rolls_back_authoritative_data_and_commit() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let result = db.remember(remember_raw("a", "sam", &"x".repeat(1024 * 1024)));
    assert!(matches!(result, Err(Error::Invalid(message)) if message.contains("export record")));
    for table in [
        "sources",
        "evidence",
        "memory_commits",
        "memory_export_events",
    ] {
        let count: i64 = db
            .connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0, "{table}");
    }
}

#[test]
fn page_byte_budget_splits_a_commit_without_stranding_events() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    db.remember(remember_raw("a", "sam", &"x".repeat(600 * 1024)))
        .unwrap();
    let first = db.export(input(0, -1, None, 100)).unwrap();
    assert_eq!(first.commits[0].records.len(), 1);
    assert_eq!(first.commits[0].event_count, 2);
    assert!(!first.complete);
    let second = db
        .export(input(
            first.next_after_commit,
            first.next_after_event_index,
            Some(first.high_water_mark),
            100,
        ))
        .unwrap();
    assert_eq!(second.commits[0].records.len(), 1);
    assert_eq!(second.commits[0].first_event_index, 1);
    assert!(second.complete);
}

#[test]
fn projection_and_lifecycle_events_cover_links_profiles_reviews_and_deletions() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let mut fact = remember("a", "sam", "Acme");
    fact.claim.as_mut().unwrap().kind = ClaimKind::ProfileFact;
    let remembered = db.remember(fact).unwrap();
    let extra = db
        .remember(remember_raw("a", "sam", "additional evidence"))
        .unwrap();
    let claim_id = remembered.claim_id.unwrap();
    db.link_claim_evidence(ClaimEvidence {
        tenant_id: TenantId("a".into()),
        person_id: PersonId("sam".into()),
        claim_id: claim_id.clone(),
        evidence_id: extra.evidence_id,
        relation: EvidenceRelation::Supports,
        confidence_basis_points: 9_000,
    })
    .unwrap();
    db.store_profile(ProfileInput {
        tenant_id: TenantId("a".into()),
        person_id: PersonId("sam".into()),
        stability: ProfileStability::Current,
        claim_id: claim_id.clone(),
        recorded_at: 11,
    })
    .unwrap();
    db.store_review(ReviewInput {
        tenant_id: TenantId("a".into()),
        person_id: PersonId("sam".into()),
        day: "2026-07-21".into(),
        summary: "Worked at Acme".into(),
        evidence_ids: vec![remembered.evidence_id.clone()],
        recorded_at: 12,
    })
    .unwrap();
    let before_embedding = db.export(input(0, -1, None, 100)).unwrap().high_water_mark;
    let target = EmbeddingTarget::Source(remembered.source_id.clone());
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
    assert_eq!(
        db.export(input(0, -1, None, 100)).unwrap().high_water_mark,
        before_embedding
    );
    db.delete_source(DeleteInput {
        tenant_id: TenantId("a".into()),
        person_id: PersonId("sam".into()),
        source_id: extra.source_id,
        deleted_at: 19,
    })
    .unwrap();
    db.delete_source(DeleteInput {
        tenant_id: TenantId("a".into()),
        person_id: PersonId("sam".into()),
        source_id: remembered.source_id,
        deleted_at: 20,
    })
    .unwrap();

    let records: Vec<_> = db
        .export(input(0, -1, None, 100))
        .unwrap()
        .commits
        .into_iter()
        .flat_map(|commit| commit.records)
        .collect();
    assert!(records.iter().any(|record| matches!(record, ExportRecord::ClaimEvidence(value) if value.confidence_basis_points == 9_000)));
    assert!(
        records
            .iter()
            .any(|record| matches!(record, ExportRecord::Profile(_)))
    );
    assert!(
        records
            .iter()
            .any(|record| matches!(record, ExportRecord::DailyReview(_)))
    );
    assert!(records.iter().any(|record| matches!(record, ExportRecord::Deletion(value) if matches!(value.target, MemoryRef::ProfileEntry(_)))));
    assert!(records.iter().any(|record| matches!(record, ExportRecord::Deletion(value) if matches!(value.target, MemoryRef::DailyReview(_)))));
}
