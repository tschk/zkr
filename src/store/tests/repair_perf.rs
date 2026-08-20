use super::*;
use rusqlite::params;
use std::time::Instant;

#[test]
fn bench_repair_projections_n_plus_one() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();

    let tenant_id = TenantId("a".into());
    let person_id = PersonId("sam".into());
    // Use 100 since `bounded_limit` truncates to 100 max in `RepairInput`
    let n: u32 = 100;

    db.connection.execute("BEGIN TRANSACTION", []).unwrap();

    // Create many rows in outbox and memory db
    for i in 0..n {
        let text = format!("A quiet desk {}", i);
        db.connection.execute(
            "INSERT INTO sources (id, tenant_id, person_id, revision, kind, content, captured_at, recorded_at) VALUES (?1, ?2, ?3, 1, 'raw', ?4, 100, 100)",
            params![format!("source_{}", i), tenant_id.0, person_id.0, text]
        ).unwrap();

        db.connection.execute(
            "INSERT INTO embeddings (tenant_id, person_id, target_kind, target_id, model, version, dimension, input_hash, normalization, distance, vector)
             VALUES (?1, ?2, 'source', ?3, 'test/model', '1', 2, 'hash', '\"l2\"', '\"cosine\"', '[1.0, 0.0]')",
            params![tenant_id.0, person_id.0, format!("source_{}", i)]
        ).unwrap();

        db.connection.execute(
            "INSERT INTO memory_repair_outbox(id, tenant_id, person_id, target_kind, target_id, reason, created_at) VALUES(?1, ?2, ?3, 'source', ?4, 'test', 100)",
            params![format!("outbox_{}", i), tenant_id.0, person_id.0, format!("source_{}", i)],
        ).unwrap();
    }

    db.connection.execute("COMMIT", []).unwrap();

    let start = Instant::now();
    let result = db
        .repair_projections(RepairInput {
            tenant_id: tenant_id.clone(),
            person_id: person_id.clone(),
            limit: n,
        })
        .unwrap();
    let elapsed = start.elapsed();
    println!("Processed {} items in {:?}", result.processed, elapsed);
    assert_eq!(result.processed, n);
}
