use rusqlite::{Connection, params};
use std::time::Instant;

fn main() -> Result<(), rusqlite::Error> {
    let connection = Connection::open_in_memory()?;

    connection.execute(
        "CREATE TABLE retrieval_stats(
            tenant_id TEXT,
            person_id TEXT,
            target_kind TEXT,
            target_id TEXT,
            exposure_count INTEGER,
            last_exposed_at INTEGER,
            PRIMARY KEY (tenant_id, person_id, target_kind, target_id)
        )",
        [],
    )?;

    // We can assume items.len() < 100 since the max candidates is around 400.
    // Let's test with 50 and 400 items.
    let num_items = 400;

    let start = Instant::now();
    let mut stmt = connection.prepare(
        "INSERT INTO retrieval_stats(tenant_id, person_id, target_kind, target_id, exposure_count, last_exposed_at)
         VALUES(?1, ?2, ?3, ?4, 1, ?5)
         ON CONFLICT(tenant_id, person_id, target_kind, target_id)
         DO UPDATE SET exposure_count = retrieval_stats.exposure_count + 1, last_exposed_at = excluded.last_exposed_at"
    )?;
    for i in 0..num_items {
        stmt.execute(params!["t1", "p1", "source", i.to_string(), 123456])?;
    }
    drop(stmt);
    let unoptimized = start.elapsed();
    println!(
        "Unoptimized (Prepared loop implicit tx) for {}: {:?}",
        num_items, unoptimized
    );

    // json bulk without TX
    let start = Instant::now();
    let mut stmt = connection.prepare("
        INSERT INTO retrieval_stats(tenant_id, person_id, target_kind, target_id, exposure_count, last_exposed_at)
        SELECT ?1, ?2, value->>'$[0]', value->>'$[1]', 1, ?3
        FROM json_each(?4)
        WHERE true
        ON CONFLICT(tenant_id, person_id, target_kind, target_id)
        DO UPDATE SET exposure_count = retrieval_stats.exposure_count + 1, last_exposed_at = excluded.last_exposed_at
    ")?;

    let mut json_arr = "[".to_string();
    for i in 0..num_items {
        if i > 0 {
            json_arr.push(',');
        }
        json_arr.push_str(&format!("[\"source\",\"{}\"]", i + num_items));
    }
    json_arr.push(']');

    stmt.execute(params!["t1", "p1", 123456i64, json_arr])?;
    drop(stmt);
    let optimized_json = start.elapsed();
    println!(
        "Optimized (json_each) for {}: {:?}",
        num_items, optimized_json
    );

    Ok(())
}
