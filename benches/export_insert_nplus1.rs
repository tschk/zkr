use rusqlite::{Connection, params};
use std::time::Instant;

fn main() -> Result<(), rusqlite::Error> {
    let mut connection = Connection::open_in_memory()?;

    connection.execute_batch(
        "CREATE TABLE memory_commits(
            sequence INTEGER PRIMARY KEY,
            tenant_id TEXT,
            person_id TEXT,
            recorded_at INTEGER
        );
        CREATE TABLE memory_export_events(
            commit_sequence INTEGER,
            event_index INTEGER,
            payload TEXT,
            PRIMARY KEY (commit_sequence, event_index)
        );",
    )?;

    connection.execute(
        "INSERT INTO memory_commits (sequence, tenant_id, person_id) VALUES (1, 't1', 'p1')",
        [],
    )?;

    let num_items = 1000;
    let mut payloads = Vec::new();
    for i in 0..num_items {
        payloads.push(format!("{{\"dummy\":\"data_{}\"}}", i));
    }

    // N+1 baseline
    let tx = connection.transaction()?;
    let start_nplus1 = Instant::now();
    let mut statement = tx.prepare_cached(
        "INSERT INTO memory_export_events(commit_sequence, event_index, payload) VALUES(?1, ?2, ?3)",
    )?;
    for (index, payload) in payloads.iter().enumerate() {
        // Mocking the length validation
        if payload.len() > 1024 * 1024 {
            panic!("too large");
        }
        statement.execute(params![1, index as i64, payload])?;
    }
    drop(statement);
    tx.commit()?;
    let nplus1_duration = start_nplus1.elapsed();

    connection.execute("DELETE FROM memory_export_events", [])?;

    // Optimized baseline: pure json_each with simpler structure
    let tx3 = connection.transaction()?;
    let start_json = Instant::now();
    for payload in &payloads {
        if payload.len() > 1024 * 1024 {
            panic!("too large");
        }
    }
    let json_str = serde_json::to_string(&payloads).unwrap();
    let mut stmt = tx3.prepare_cached("INSERT INTO memory_export_events(commit_sequence, event_index, payload) SELECT ?1, key, value FROM json_each(?2)")?;
    stmt.execute(params![1, json_str])?;
    drop(stmt);
    tx3.commit()?;
    let json_duration = start_json.elapsed();

    println!("N+1 Duration: {:?}", nplus1_duration);
    println!("JSON Duration: {:?}", json_duration);

    Ok(())
}
