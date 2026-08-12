use super::*;
use rusqlite::params;

#[test]
fn repair_projections_handles_empty_outbox() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();

    let result = db
        .repair_projections(RepairInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            limit: 10,
        })
        .unwrap();

    assert_eq!(result.processed, 0);
}

#[test]
fn repair_projections_deletes_stale_embeddings_when_target_modified() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();

    let raw = db
        .remember(remember_raw("a", "sam", "A quiet desk"))
        .unwrap();
    let target = EmbeddingTarget::Source(raw.source_id.clone());

    // Insert embedding
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

    // Modify source text to make projection stale
    db.connection
        .execute(
            "UPDATE sources SET content = 'A changed desk', revision = revision + 1 WHERE id = ?1",
            [&raw.source_id.0],
        )
        .unwrap();

    // Insert to memory_repair_outbox
    let (target_kind, target_id) = embedding_target_parts(&target);
    db.connection.execute(
        "INSERT INTO memory_repair_outbox(id, tenant_id, person_id, target_kind, target_id, reason, created_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params!["outbox_id_1", "a", "sam", target_kind, target_id, "test", 100],
    ).unwrap();

    let result = db
        .repair_projections(RepairInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            limit: 10,
        })
        .unwrap();

    assert_eq!(result.processed, 1);

    // Verify embedding was deleted
    let count: i64 = db
        .connection
        .query_row(
            "SELECT COUNT(*) FROM embeddings WHERE target_id = ?1",
            [&raw.source_id.0],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);

    // Verify outbox was marked as processed
    let processed_at: i64 = db
        .connection
        .query_row(
            "SELECT processed_at FROM memory_repair_outbox WHERE id = 'outbox_id_1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(processed_at > 0);
}

#[test]
fn repair_projections_deletes_embeddings_when_target_deleted() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();

    let raw = db
        .remember(remember_raw("a", "sam", "A quiet desk"))
        .unwrap();
    let target = EmbeddingTarget::Source(raw.source_id.clone());

    // Insert embedding
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

    // Verify source_fts exists
    let count: i64 = db
        .connection
        .query_row(
            "SELECT COUNT(*) FROM source_fts WHERE source_id = ?1",
            [&raw.source_id.0],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);

    // Hard delete source to trigger NotFound in projection_input_from
    db.connection
        .execute("PRAGMA foreign_keys = OFF", [])
        .unwrap();
    db.connection
        .execute("DELETE FROM sources WHERE id = ?1", [&raw.source_id.0])
        .unwrap();
    db.connection
        .execute("PRAGMA foreign_keys = ON", [])
        .unwrap();

    // Insert to memory_repair_outbox
    let (target_kind, target_id) = embedding_target_parts(&target);
    db.connection.execute(
        "INSERT INTO memory_repair_outbox(id, tenant_id, person_id, target_kind, target_id, reason, created_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params!["outbox_id_2", "a", "sam", target_kind, target_id, "test_not_found", 100],
    ).unwrap();

    let result = db
        .repair_projections(RepairInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            limit: 10,
        })
        .unwrap();

    assert_eq!(result.processed, 1);

    // Verify embedding was deleted
    let count: i64 = db
        .connection
        .query_row(
            "SELECT COUNT(*) FROM embeddings WHERE target_id = ?1",
            [&raw.source_id.0],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);

    // Verify source_fts was deleted
    let count: i64 = db
        .connection
        .query_row(
            "SELECT COUNT(*) FROM source_fts WHERE source_id = ?1",
            [&raw.source_id.0],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);

    // Verify outbox was marked as processed
    let processed_at: i64 = db
        .connection
        .query_row(
            "SELECT processed_at FROM memory_repair_outbox WHERE id = 'outbox_id_2'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(processed_at > 0);
}

#[test]
fn repair_projections_handles_invalid_target_kind() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();

    // Insert to memory_repair_outbox with invalid target kind
    db.connection.execute(
        "INSERT INTO memory_repair_outbox(id, tenant_id, person_id, target_kind, target_id, reason, created_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params!["outbox_id_3", "a", "sam", "invalid_kind", "invalid_id", "test_invalid", 100],
    ).unwrap();

    let result = db
        .repair_projections(RepairInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            limit: 10,
        })
        .unwrap();

    // Should return 0 as processed items count is only incremented when valid target is evaluated
    assert_eq!(result.processed, 0);

    // Verify outbox was marked as processed anyway due to the error branch for invalid embedding_target
    let processed_at: i64 = db
        .connection
        .query_row(
            "SELECT processed_at FROM memory_repair_outbox WHERE id = 'outbox_id_3'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(processed_at > 0);
}
