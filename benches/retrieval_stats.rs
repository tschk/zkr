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

    let num_items = 10_000;

    let start = Instant::now();
    for i in 0..num_items {
        connection.execute(
            "INSERT INTO retrieval_stats(tenant_id, person_id, target_kind, target_id, exposure_count, last_exposed_at)
             VALUES(?1, ?2, ?3, ?4, 1, ?5)
             ON CONFLICT(tenant_id, person_id, target_kind, target_id)
             DO UPDATE SET exposure_count = retrieval_stats.exposure_count + 1, last_exposed_at = excluded.last_exposed_at",
            params!["t1", "p1", "source", i.to_string(), 123456],
        )?;
    }
    let unoptimized = start.elapsed();

    let start = Instant::now();
    let mut statement = connection.prepare(
        "INSERT INTO retrieval_stats(tenant_id, person_id, target_kind, target_id, exposure_count, last_exposed_at)
         VALUES(?1, ?2, ?3, ?4, 1, ?5)
         ON CONFLICT(tenant_id, person_id, target_kind, target_id)
         DO UPDATE SET exposure_count = retrieval_stats.exposure_count + 1, last_exposed_at = excluded.last_exposed_at",
    )?;
    for i in 0..num_items {
        statement.execute(params![
            "t1",
            "p1",
            "source",
            (i + num_items).to_string(),
            123456
        ])?;
    }
    let optimized = start.elapsed();

    println!("Unoptimized: {unoptimized:?}");
    println!("Optimized: {optimized:?}");
    Ok(())
}
