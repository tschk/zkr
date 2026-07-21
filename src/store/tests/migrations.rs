use super::*;

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
    assert_eq!(version, 7);

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
    for version in 0..=6 {
        let connection = Connection::open_in_memory().unwrap();
        if version == 0 {
            connection
                .execute_batch("PRAGMA user_version = 0;")
                .unwrap();
        } else {
            let claim_kind = if version >= 5 {
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
            if version >= 5 {
                let (key, value) = if version == 5 {
                    ("company", "ACME Corp")
                } else {
                    ("employer", "Acme")
                };
                connection
                    .execute_batch(&format!(
                        "CREATE TABLE profile_entries(id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL, stability TEXT NOT NULL, claim_id TEXT NOT NULL REFERENCES claims(id), recorded_at INTEGER NOT NULL);
                         UPDATE claims SET kind = 'profile_fact' WHERE id = 'claim';
                         INSERT INTO profile_entries VALUES('profile', 'a', 'sam', '{key}', '{value}', '\"current\"', 'claim', 12);",
                    ))
                    .unwrap();
            }
            if version >= 6 {
                connection
                    .execute_batch(
                        "ALTER TABLE sources ADD COLUMN origin_evidence_id TEXT;
                         ALTER TABLE sources ADD COLUMN origin_claim_id TEXT;
                         UPDATE sources SET origin_evidence_id = 'evidence', origin_claim_id = 'claim' WHERE id = 'source';",
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
        assert_eq!(actual, 7);
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
                if version >= 5 { "profile_fact" } else { "fact" }
            );
            if version >= 5 {
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
fn migration_rejects_invalid_legacy_claim_time_intervals() {
    for column in ["recorded_until", "valid_until"] {
        let mut db = MemoryDb {
            connection: Connection::open_in_memory().unwrap(),
        };
        db.migrate().unwrap();
        let claim_id = db
            .remember(remember("a", "sam", "Acme"))
            .unwrap()
            .claim_id
            .unwrap();
        db.connection
            .execute_batch(
                "DROP TRIGGER claim_time_interval_insert;
                 DROP TRIGGER claim_time_interval_update;
                 PRAGMA user_version = 6;",
            )
            .unwrap();
        db.connection
            .execute(
                &format!("UPDATE claims SET {column} = 5 WHERE id = ?1"),
                [&claim_id.0],
            )
            .unwrap();

        assert!(matches!(db.migrate(), Err(Error::Invalid(_))));
        assert_eq!(
            db.connection
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            6
        );
        assert_eq!(
            db.connection
                .query_row(
                    &format!("SELECT {column} FROM claims WHERE id = ?1"),
                    [&claim_id.0],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            5
        );
    }
}

#[test]
fn schema_rejects_invalid_claim_time_interval_updates() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let claim_id = db
        .remember(remember("a", "sam", "Acme"))
        .unwrap()
        .claim_id
        .unwrap();

    for (id, valid_until, recorded_until) in [
        ("invalid-valid", Some(10), None),
        ("invalid-recorded", None, Some(10)),
    ] {
        assert!(
            db.connection
                .execute(
                    "INSERT INTO claims(id, tenant_id, person_id, subject, predicate, value, kind, valid_from, valid_until, recorded_from, recorded_until, status) VALUES(?1, 'a', 'sam', 'Sam', 'employer', 'Acme', 'fact', 10, ?2, 10, ?3, 'accepted')",
                    params![id, valid_until, recorded_until],
                )
                .is_err()
        );
    }
    for column in ["recorded_until = recorded_from", "valid_until = valid_from"] {
        assert!(
            db.connection
                .execute(
                    &format!("UPDATE claims SET {column} WHERE id = ?1"),
                    [&claim_id.0],
                )
                .is_err()
        );
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
