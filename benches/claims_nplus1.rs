use rusqlite::{Connection, params};
use std::time::Instant;

fn main() -> Result<(), rusqlite::Error> {
    let mut connection = Connection::open_in_memory()?;

    connection.execute_batch(
        "CREATE TABLE claims(
            id TEXT,
            tenant_id TEXT,
            person_id TEXT,
            subject TEXT,
            predicate TEXT,
            value TEXT,
            kind TEXT,
            valid_from INTEGER,
            valid_until INTEGER,
            recorded_from INTEGER,
            recorded_until INTEGER,
            status TEXT,
            tier TEXT,
            processing_state TEXT,
            PRIMARY KEY (id, tenant_id, person_id)
        );",
    )?;

    let num_items = 10_000;

    // Insert test data
    let tx = connection.transaction()?;
    let mut stmt = tx.prepare("INSERT INTO claims VALUES(?, 't1', 'p1', 's', 'p', 'v', '\"fact\"', 0, 0, 0, 0, '\"open\"', '\"default\"', '\"idle\"')")?;
    for i in 0..num_items {
        stmt.execute(params![i.to_string()])?;
    }
    drop(stmt);
    tx.commit()?;

    // N+1 baseline
    let start_nplus1 = Instant::now();
    let mut scoped_ids = connection
        .prepare("SELECT id FROM claims WHERE tenant_id = 't1' AND person_id = 'p1' ORDER BY id")?;
    let ids: Vec<String> = scoped_ids
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;

    let mut read_stmt = connection.prepare("SELECT subject, predicate, value, kind, valid_from, valid_until, recorded_from, recorded_until, status, tier, processing_state FROM claims WHERE id = ?1 AND tenant_id = 't1' AND person_id = 'p1'")?;

    let mut num_read = 0;
    for id in ids {
        read_stmt.query_row(params![id], |row| {
            let s: String = row.get(0)?;
            Ok(s)
        })?;
        num_read += 1;
    }
    let nplus1_duration = start_nplus1.elapsed();

    // Single Query optimized
    let start_optimized = Instant::now();
    let mut single_query = connection.prepare("SELECT id, subject, predicate, value, kind, valid_from, valid_until, recorded_from, recorded_until, status, tier, processing_state FROM claims WHERE tenant_id = 't1' AND person_id = 'p1' ORDER BY id")?;

    let rows = single_query.query_map([], |row| {
        let _id: String = row.get(0)?;
        let s: String = row.get(1)?;
        Ok(s)
    })?;

    let mut num_read_opt = 0;
    for _row in rows {
        num_read_opt += 1;
    }
    let optimized_duration = start_optimized.elapsed();

    assert_eq!(num_read, num_items);
    assert_eq!(num_read_opt, num_items);

    println!("N+1 Duration: {:?}", nplus1_duration);
    println!("Optimized Duration: {:?}", optimized_duration);

    Ok(())
}
