use rusqlite::{Connection, params};

use std::time::Instant;

fn main() -> Result<(), rusqlite::Error> {
    let mut connection = Connection::open_in_memory()?;

    connection.execute_batch(
        "CREATE TABLE evidence(
            id TEXT,
            tenant_id TEXT,
            person_id TEXT,
            deleted_at INTEGER,
            PRIMARY KEY (id, tenant_id, person_id)
        );",
    )?;

    let num_items = 1000;
    let mut ids = Vec::new();

    let tx = connection.transaction()?;
    let mut stmt = tx.prepare("INSERT INTO evidence VALUES(?, 't1', 'p1', NULL)")?;
    for i in 0..num_items {
        stmt.execute(params![i.to_string()])?;
        ids.push(i.to_string());
    }
    drop(stmt);
    tx.commit()?;

    // N+1 baseline (Happy path)
    let start_nplus1 = Instant::now();
    let mut read_stmt = connection.prepare("SELECT EXISTS(SELECT 1 FROM evidence WHERE id = ?1 AND tenant_id = ?2 AND person_id = ?3 AND deleted_at IS NULL)")?;

    let mut missing_ids_nplus1 = Vec::new();
    for id in &ids {
        let found: bool = read_stmt.query_row(params![id, "t1", "p1"], |row| row.get(0))?;
        if !found {
            missing_ids_nplus1.push(id.clone());
        }
    }
    let nplus1_duration = start_nplus1.elapsed();

    // Using EXCEPT approach
    let start_except = Instant::now();
    let ids_json = serde_json::to_string(&ids).unwrap();
    let mut missing_stmt = connection.prepare(
        "SELECT value FROM json_each(?1) \
         EXCEPT \
         SELECT id FROM evidence WHERE tenant_id = ?2 AND person_id = ?3 AND deleted_at IS NULL",
    )?;
    let missing_ids: Vec<String> = missing_stmt
        .query_map(params![ids_json, "t1", "p1"], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    let except_duration = start_except.elapsed();

    assert_eq!(missing_ids_nplus1.len(), 0);
    assert_eq!(missing_ids.len(), 0);

    println!("N+1 Duration (Happy Path): {:?}", nplus1_duration);
    println!("EXCEPT Duration (Happy Path): {:?}", except_duration);

    // Bench with one missing
    let mut ids_with_missing = ids.clone();
    ids_with_missing.push("missing_id".to_string());

    // N+1 baseline (Missing)
    let start_nplus1_miss = Instant::now();
    let mut missing_ids_nplus1_miss = Vec::new();
    for id in &ids_with_missing {
        let found: bool = read_stmt.query_row(params![id, "t1", "p1"], |row| row.get(0))?;
        if !found {
            missing_ids_nplus1_miss.push(id.clone());
            break; // Stop at first miss, like the code
        }
    }
    let nplus1_duration_miss = start_nplus1_miss.elapsed();

    // Using EXCEPT approach (Missing)
    let start_except_miss = Instant::now();
    let ids_json_miss = serde_json::to_string(&ids_with_missing).unwrap();
    let mut missing_ids_miss = missing_stmt
        .query_map(params![ids_json_miss, "t1", "p1"], |row| {
            row.get::<_, String>(0)
        })?;
    let mut found_missing = false;
    if let Some(_missing_id) = missing_ids_miss.next() {
        found_missing = true;
    }
    let except_duration_miss = start_except_miss.elapsed();

    assert!(missing_ids_nplus1_miss.contains(&"missing_id".to_string()));
    assert!(found_missing);

    println!("N+1 Duration (1 Missing): {:?}", nplus1_duration_miss);
    println!("EXCEPT Duration (1 Missing): {:?}", except_duration_miss);

    Ok(())
}
