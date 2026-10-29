use super::*;
use tempfile::tempdir;

#[test]
fn open_fails_with_invalid_path() {
    let dir = tempdir().unwrap();
    let invalid_path = dir.path().join("non_existent_dir").join("db.sqlite");
    let result = MemoryDb::open(&invalid_path);
    assert!(result.is_err());
}

#[test]
fn open_fails_when_migration_fails() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("db.sqlite");

    // Create a connection and set version higher than expected to force migration to fail
    {
        let connection = rusqlite::Connection::open(&db_path).unwrap();
        let version = crate::store::schema::SCHEMA_VERSION + 1;
        connection
            .execute(&format!("PRAGMA user_version = {};", version), [])
            .unwrap();
    }

    let result = MemoryDb::open(&db_path);
    assert!(result.is_err());
}
