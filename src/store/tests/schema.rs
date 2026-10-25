use super::*;

#[test]
fn migrate_creates_schema_tables() {
    let mut connection = Connection::open_in_memory().unwrap();
    crate::store::schema::migrate(&mut connection).unwrap();

    let mut stmt = connection
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap();

    let tables: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();

    let expected_tables = vec![
        "claim_evidence",
        "claims",
        "corrections",
        "daily_reviews",
        "embeddings",
        "evidence",
        "evidence_locators",
        "memory_applied_records",
        "memory_commits",
        "memory_export_events",
        "memory_operations",
        "memory_repair_outbox",
        "profile_entries",
        "retrieval_stats",
        "sources",
        "summary_nodes",
    ];

    for expected in expected_tables {
        assert!(
            tables.contains(&expected.to_string()),
            "Table {} should exist",
            expected
        );
    }
}
