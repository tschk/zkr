use crate::store::schema::is_valid_identifier;
use rusqlite::Connection;

#[test]
fn is_valid_identifier_accepts_valid_strings() {
    assert!(is_valid_identifier("valid_table_name"));
    assert!(is_valid_identifier("table123"));
    assert!(is_valid_identifier("A_b_3"));
}

#[test]
fn is_valid_identifier_rejects_invalid_strings() {
    assert!(!is_valid_identifier(""));
    assert!(!is_valid_identifier("table name"));
    assert!(!is_valid_identifier("table;DROP"));
    assert!(!is_valid_identifier("table-name"));
}

#[test]
fn schema_setup_and_basic_queries() {
    let mut connection = Connection::open_in_memory().unwrap();
    crate::store::schema::migrate(&mut connection).unwrap();

    // Verify basic tables exist
    let tables: Vec<String> = connection
        .prepare("SELECT name FROM sqlite_master WHERE type='table'")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(|name| name.unwrap())
        .collect();

    assert!(tables.contains(&"sources".to_string()));
    assert!(tables.contains(&"evidence".to_string()));
    assert!(tables.contains(&"claims".to_string()));
    assert!(tables.contains(&"claim_evidence".to_string()));
    assert!(tables.contains(&"daily_reviews".to_string()));
    assert!(tables.contains(&"embeddings".to_string()));

    // Verify the user version is updated to SCHEMA_VERSION
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .unwrap();
    assert_eq!(version, crate::store::schema::SCHEMA_VERSION);
}
