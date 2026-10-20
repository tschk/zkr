use rusqlite::{Connection, params};
use std::time::Instant;

fn main() -> Result<(), rusqlite::Error> {
    let mut connection = Connection::open_in_memory()?;

    connection.execute_batch(
        "
        CREATE TABLE evidence(
            id TEXT,
            tenant_id TEXT,
            person_id TEXT,
            source_id TEXT,
            source_revision TEXT,
            quote TEXT,
            recorded_at INTEGER,
            deleted_at INTEGER,
            PRIMARY KEY (id, tenant_id, person_id)
        );
        CREATE TABLE evidence_locators(
            evidence_id TEXT,
            tenant_id TEXT,
            person_id TEXT,
            device_id TEXT,
            provider TEXT,
            stream_id TEXT,
            segment_id TEXT,
            start_ms INTEGER,
            end_ms INTEGER,
            PRIMARY KEY (evidence_id, tenant_id, person_id)
        );
        ",
    )?;

    let num_items = 1000;
    let mut ids = Vec::new();

    let tx = connection.transaction()?;
    let mut stmt = tx.prepare("INSERT INTO evidence (id, tenant_id, person_id, source_id, source_revision, quote, recorded_at, deleted_at) VALUES (?, 't1', 'p1', 's1', 'r1', 'q', 0, NULL)")?;
    for i in 0..num_items {
        stmt.execute(params![i.to_string()])?;
        ids.push(i.to_string());
    }
    drop(stmt);
    tx.commit()?;

    // N+1 baseline
    let start_nplus1 = Instant::now();
    let mut read_stmt = connection.prepare(
        "SELECT e.source_id, e.source_revision, e.quote, e.recorded_at, e.deleted_at, l.device_id, l.provider, l.stream_id, l.segment_id, l.start_ms, l.end_ms FROM evidence e LEFT JOIN evidence_locators l ON l.evidence_id = e.id AND l.tenant_id = e.tenant_id AND l.person_id = e.person_id WHERE e.id = ?1 AND e.tenant_id = ?2 AND e.person_id = ?3"
    )?;
    let mut count = 0;
    for id in &ids {
        read_stmt.query_row(params![id, "t1", "p1"], |row| {
            let _: String = row.get(0)?;
            Ok(())
        })?;
        count += 1;
    }
    let nplus1_duration = start_nplus1.elapsed();

    // Using json_each approach
    let start_json_each = Instant::now();
    let ids_json = serde_json::to_string(&ids).unwrap();
    let mut batch_stmt = connection.prepare(
        "SELECT e.id, e.source_id, e.source_revision, e.quote, e.recorded_at, e.deleted_at, l.device_id, l.provider, l.stream_id, l.segment_id, l.start_ms, l.end_ms FROM json_each(?3) j JOIN evidence e ON e.id = j.value AND e.tenant_id = ?1 AND e.person_id = ?2 LEFT JOIN evidence_locators l ON l.evidence_id = e.id AND l.tenant_id = e.tenant_id AND l.person_id = e.person_id"
    )?;

    let mut opt_count = 0;
    let mut rows = batch_stmt.query(params!["t1", "p1", ids_json])?;
    while let Some(row) = rows.next()? {
        let _: String = row.get(1)?;
        opt_count += 1;
    }
    let json_each_duration = start_json_each.elapsed();

    assert_eq!(count, num_items);
    assert_eq!(opt_count, num_items);

    println!("N+1 Duration: {:?}", nplus1_duration);
    println!("json_each Duration: {:?}", json_each_duration);

    Ok(())
}
