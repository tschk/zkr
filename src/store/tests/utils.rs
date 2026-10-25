use crate::store::utils;
use rusqlite::Connection;

#[test]
fn generates_valid_now_iso() {
    let mut db = Connection::open_in_memory().unwrap();
    let tx = db.transaction().unwrap();
    let iso = utils::now_iso(&tx).unwrap();

    assert_eq!(iso.len(), 20, "ISO string length should be exactly 20 characters");
    assert!(iso.ends_with('Z'), "ISO string should end with Z");
    assert_eq!(&iso[4..5], "-", "Year should be 4 digits followed by a hyphen");
    assert_eq!(&iso[7..8], "-", "Month should be 2 digits followed by a hyphen");
    assert_eq!(&iso[10..11], "T", "Date and time should be separated by T");
    assert_eq!(&iso[13..14], ":", "Hours should be 2 digits followed by a colon");
    assert_eq!(&iso[16..17], ":", "Minutes should be 2 digits followed by a colon");

    assert!(iso[0..4].chars().all(|c: char| c.is_ascii_digit()), "Year should be numeric");
    assert!(iso[5..7].chars().all(|c: char| c.is_ascii_digit()), "Month should be numeric");
    assert!(iso[8..10].chars().all(|c: char| c.is_ascii_digit()), "Day should be numeric");
    assert!(iso[11..13].chars().all(|c: char| c.is_ascii_digit()), "Hour should be numeric");
    assert!(iso[14..16].chars().all(|c: char| c.is_ascii_digit()), "Minute should be numeric");
    assert!(iso[17..19].chars().all(|c: char| c.is_ascii_digit()), "Second should be numeric");
}
