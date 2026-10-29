use super::*;
use rusqlite::Connection;
use tempfile::tempdir;

#[test]
fn open_fails_when_path_is_invalid() {
    let dir = tempdir().unwrap();
    let invalid_path = dir.path().join("nonexistent_dir").join("db.sqlite");
    let result = MemoryDb::open(invalid_path);
    assert!(result.is_err());
    assert!(matches!(result, Err(Error::Sql(_))));
}

#[test]
fn open_fails_when_migration_fails() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("db.sqlite");

    // Create database and set a future version to make migration fail
    {
        let connection = Connection::open(&db_path).unwrap();
        let version = crate::store::schema::SCHEMA_VERSION + 1;
        connection
            .execute(&format!("PRAGMA user_version = {};", version), [])
            .unwrap();
    }

    let result = MemoryDb::open(db_path);
    assert!(result.is_err());

    if let Err(Error::Invalid(msg)) = result {
        assert!(msg.contains("is newer than supported version"));
    } else {
        panic!("Expected Error::Invalid but got something else");
    }
}
