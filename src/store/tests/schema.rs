use super::*;
use rusqlite::Connection;
use std::collections::HashSet;

#[test]
fn setup_creates_expected_tables() {
    let mut connection = Connection::open_in_memory().unwrap();
    crate::store::schema::migrate(&mut connection).unwrap();

    let mut stmt = connection
        .prepare("SELECT name FROM sqlite_master WHERE type='table'")
        .unwrap();

    let table_iter = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap();

    let tables: HashSet<String> = table_iter.map(|r| r.unwrap()).collect();

    let expected_tables = vec![
        "sources",
        "evidence",
        "claims",
        "claim_evidence",
        "daily_reviews",
        "embeddings",
        "evidence_locators",
        "profile_entries",
        "corrections",
        "memory_commits",
        "memory_export_events",
        "memory_repair_outbox",
        "memory_operations",
        "memory_applied_records",
        "summary_nodes",
        "retrieval_stats",
    ];

    for table in expected_tables {
        assert!(tables.contains(table), "Missing expected table: {}", table);
    }
}

#[test]
fn migrate_fails_if_newer_version() {
    let mut connection = Connection::open_in_memory().unwrap();
    let version = crate::store::schema::SCHEMA_VERSION + 1;
    connection
        .execute(&format!("PRAGMA user_version = {};", version), [])
        .unwrap();

    let result = crate::store::schema::migrate(&mut connection);
    assert!(result.is_err());
    if let Err(Error::Invalid(msg)) = result {
        assert!(msg.contains("is newer than supported version"));
    } else {
        panic!("Expected Error::Invalid");
    }
}
