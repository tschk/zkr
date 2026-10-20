use super::*;
use rusqlite::TransactionBehavior;

#[test]
fn generates_random_hex_string() {
    let mut db = MemoryDb::open(":memory:").unwrap();
    let tx = db
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let id1 = crate::store::utils::new_id(&tx).unwrap();
    let id2 = crate::store::utils::new_id(&tx).unwrap();

    assert_eq!(id1.len(), 32);
    assert!(
        id1.chars()
            .all(|c| (c.is_ascii_hexdigit() && c.is_ascii_lowercase()) || c.is_numeric())
    );

    assert_eq!(id2.len(), 32);
    assert!(
        id2.chars()
            .all(|c| (c.is_ascii_hexdigit() && c.is_ascii_lowercase()) || c.is_numeric())
    );

    assert_ne!(id1, id2);
}
