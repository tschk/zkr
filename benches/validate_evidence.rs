use rusqlite::{Connection, params};
use std::time::Instant;

fn main() -> Result<(), rusqlite::Error> {
    let mut connection = Connection::open_in_memory()?;

    connection.execute_batch(
        "CREATE TABLE evidence(
            id TEXT,
            tenant_id TEXT,
            person_id TEXT,
            deleted_at TEXT,
            PRIMARY KEY (id, tenant_id, person_id)
        );",
    )?;

    let num_items = 900; // Chunk size is 900

    // Insert test data (leave the last one out to trigger the error path)
    let tx = connection.transaction()?;
    let mut stmt =
        tx.prepare("INSERT INTO evidence (id, tenant_id, person_id) VALUES(?, 't1', 'p1')")?;
    for i in 0..(num_items - 1) {
        stmt.execute(params![i.to_string()])?;
    }
    drop(stmt);
    tx.commit()?;

    let chunk: Vec<String> = (0..num_items).map(|i| i.to_string()).collect();

    // N+1 baseline
    let start_nplus1 = Instant::now();
    for _ in 0..100 {
        // Loop multiple times to measure small differences
        for id in &chunk {
            let live: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM evidence WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3 AND deleted_at IS NULL)",
                params![id, "t1", "p1"],
                |row| row.get(0),
            )?;
            if !live {
                // println!("missing: {}", id);
                break;
            }
        }
    }
    let nplus1_duration = start_nplus1.elapsed();

    // Single Query optimized
    let start_optimized = Instant::now();
    for _ in 0..100 {
        let json_arr = serde_json::to_string(&chunk).unwrap();

        // This is SQLite's json_each to find missing elements
        let mut stmt = connection.prepare(
            "SELECT value
             FROM json_each(?1)
             WHERE NOT EXISTS (
                 SELECT 1 FROM evidence
                 WHERE id = value
                   AND tenant_id = ?2
                   AND person_id = ?3
                   AND deleted_at IS NULL
             )
             LIMIT 1",
        )?;

        let missing_id: rusqlite::Result<String> =
            stmt.query_row(params![json_arr, "t1", "p1"], |row| row.get(0));

        if let Ok(_id) = missing_id {
            // Found a missing element
        }
    }
    let optimized_duration = start_optimized.elapsed();

    println!("N+1 Duration: {:?}", nplus1_duration);
    println!("Optimized (json_each) Duration: {:?}", optimized_duration);

    Ok(())
}
