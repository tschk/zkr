use super::*;
use tempfile::tempdir;

#[test]
fn open_fails_on_invalid_path() {
    let result = MemoryDb::open("/root/invalid/path/that/does/not/exist.db");
    assert!(result.is_err());
}

#[test]
fn open_fails_on_migration_error() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    // Create a database and intentionally corrupt the schema version or create conflicting tables
    let conn = Connection::open(&db_path).unwrap();
    conn.execute("PRAGMA user_version = 999999;", []).unwrap();
    drop(conn);

    let result = MemoryDb::open(&db_path);
    assert!(result.is_err());

    if let Err(Error::Invalid(msg)) = result {
        assert!(msg.contains("is newer than supported version"), "Expected version error, got: {}", msg);
    } else {
        panic!("Expected Error::Invalid, got: {:?}", result.err());
    }
}
