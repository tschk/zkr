use crate::store::utils::new_id;
use crate::store::MemoryDb;
use rusqlite::Connection;

#[test]
fn new_id_returns_random_hex_string() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    // Migrate the schema to ensure all tables (like event_log) are present
    db.migrate().unwrap();
    let transaction = db.connection.transaction().unwrap();

    let id1 = new_id(&transaction).unwrap();
    let id2 = new_id(&transaction).unwrap();

    assert_ne!(id1, id2);
    assert_eq!(id1.len(), 32);
    assert_eq!(id2.len(), 32);

    // Check if it's a valid hex string
    assert!(id1.chars().all(|c| c.is_ascii_hexdigit()));
}
