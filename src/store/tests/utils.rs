use super::*;

#[test]
fn generates_valid_id() {
    let mut db = MemoryDb {
        connection: rusqlite::Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let tx = db.connection.transaction().unwrap();
    let id = crate::store::utils::new_id(&tx).unwrap();
    assert_eq!(id.len(), 32);
    assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
}
