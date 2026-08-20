use rusqlite::{Connection, params, Row};

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

    // test duplicate within same statement
    let res = connection.execute(
        "INSERT INTO retrieval_stats(tenant_id, person_id, target_kind, target_id, exposure_count, last_exposed_at)
         VALUES
            ('t1', 'p1', 'source', '1', 1, 123),
            ('t1', 'p1', 'source', '1', 1, 123)
         ON CONFLICT(tenant_id, person_id, target_kind, target_id)
         DO UPDATE SET exposure_count = retrieval_stats.exposure_count + 1, last_exposed_at = excluded.last_exposed_at",
        [],
    );

    println!("Duplicate insert result: {:?}", res);

    let cnt: i64 = connection.query_row("SELECT exposure_count FROM retrieval_stats", [], |r| r.get(0)).unwrap_or(-1);
    println!("Exposure count: {}", cnt);

    Ok(())
}
