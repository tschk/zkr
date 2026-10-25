use rusqlite::Connection;

#[test]
fn setup_creates_core_tables() {
    let mut connection = Connection::open_in_memory().unwrap();
    crate::store::schema::migrate(&mut connection).unwrap();

    let mut stmt = connection
        .prepare("SELECT name FROM sqlite_master WHERE type='table'")
        .unwrap();

    let tables: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();

    let expected_core_tables = ["sources", "evidence", "claims"];
    for table in expected_core_tables {
        assert!(
            tables.contains(&table.to_string()),
            "Missing core table: {}",
            table
        );
    }
}
