use rusqlite::{Connection, params};
use std::time::Instant;

fn new_id(connection: &Connection) -> Result<String, rusqlite::Error> {
    connection.query_row("SELECT lower(hex(randomblob(16)))", [], |row| row.get(0))
}

fn unoptimized(connection: &Connection, num_items: usize) -> Result<(), rusqlite::Error> {
    for i in 0..num_items {
        connection.execute(
            "INSERT INTO memory_repair_outbox(id, tenant_id, person_id, target_kind, target_id, reason, created_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![new_id(connection)?, "t1", "p1", "evidence", i.to_string(), "delete_sync", 123456],
        )?;
    }
    Ok(())
}

fn optimized(connection: &Connection, num_items: usize) -> Result<(), rusqlite::Error> {
    let mut stmt = connection.prepare_cached(
        "INSERT INTO memory_repair_outbox(id, tenant_id, person_id, target_kind, target_id, reason, created_at) VALUES(lower(hex(randomblob(16))), ?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    for i in 0..num_items {
        stmt.execute(params![
            "t1",
            "p1",
            "evidence",
            (i + num_items).to_string(),
            "delete_sync",
            123456
        ])?;
    }
    Ok(())
}

fn optimized_json_each(connection: &Connection, num_items: usize) -> Result<(), rusqlite::Error> {
    let mut payload = Vec::with_capacity(num_items);
    for i in 0..num_items {
        payload.push(("evidence", (i + num_items * 2).to_string()));
    }
    let json_payload = serde_json::to_string(&payload).unwrap();
    connection.execute(
        "INSERT INTO memory_repair_outbox(id, tenant_id, person_id, target_kind, target_id, reason, created_at) SELECT lower(hex(randomblob(16))), ?1, ?2, json_extract(value, '$[0]'), json_extract(value, '$[1]'), ?3, ?4 FROM json_each(?5)",
        params![
            "t1",
            "p1",
            "delete_sync",
            123456,
            json_payload
        ]
    )?;
    Ok(())
}

fn main() -> Result<(), rusqlite::Error> {
    let connection = Connection::open_in_memory()?;

    connection.execute(
        "CREATE TABLE memory_repair_outbox(
            id TEXT PRIMARY KEY,
            tenant_id TEXT,
            person_id TEXT,
            target_kind TEXT,
            target_id TEXT,
            reason TEXT,
            created_at INTEGER,
            processed_at INTEGER
        )",
        [],
    )?;

    let num_items = 10_000;

    let start = Instant::now();
    unoptimized(&connection, num_items)?;
    let unoptimized_time = start.elapsed();

    let start = Instant::now();
    optimized(&connection, num_items)?;
    let optimized_time = start.elapsed();

    let start = Instant::now();
    optimized_json_each(&connection, num_items)?;
    let optimized_json_each_time = start.elapsed();

    println!("Unoptimized: {unoptimized_time:?}");
    println!("Optimized (loop): {optimized_time:?}");
    println!("Optimized (json_each): {optimized_json_each_time:?}");
    Ok(())
}
