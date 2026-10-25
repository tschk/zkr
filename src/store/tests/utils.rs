use crate::store::MemoryDb;
use rusqlite::Connection;
use crate::store::utils::new_id;

#[test]
fn test_new_id() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();

    let tx = db.connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).unwrap();

    let id = new_id(&tx).unwrap();

    assert_eq!(id.len(), 32, "ID should be exactly 32 characters long, got: {}", id.len());
    assert!(id.chars().all(|c| c.is_ascii_hexdigit()), "ID should contain only hexadecimal characters, got: {}", id);
    assert!(id.chars().all(|c| !c.is_uppercase()), "ID should contain only lowercase hexadecimal characters, got: {}", id);
}
