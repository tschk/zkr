use crate::store::*;
use crate::{PersonId, TenantId};
use rusqlite::Connection;

#[test]
fn require_text_accepts_valid_text() {
    assert!(require_text("field", "valid").is_ok());
    assert!(require_text("field", " valid ").is_ok());
}

#[test]
fn require_text_rejects_empty_or_whitespace() {
    let err = require_text("field", "").unwrap_err();
    assert!(matches!(err, Error::Invalid(msg) if msg == "field must not be empty"));

    let err = require_text("field", "   ").unwrap_err();
    assert!(matches!(err, Error::Invalid(msg) if msg == "field must not be empty"));
}

#[test]
fn require_scope_accepts_valid_scope() {
    let tenant = TenantId("t1".into());
    let person = PersonId("p1".into());
    assert!(require_scope(&tenant, &person).is_ok());
}

#[test]
fn require_scope_rejects_invalid_tenant() {
    let tenant = TenantId("".into());
    let person = PersonId("p1".into());
    let err = require_scope(&tenant, &person).unwrap_err();
    assert!(matches!(err, Error::Invalid(msg) if msg == "tenant_id must not be empty"));
}

#[test]
fn require_scope_rejects_invalid_person() {
    let tenant = TenantId("t1".into());
    let person = PersonId("   ".into());
    let err = require_scope(&tenant, &person).unwrap_err();
    assert!(matches!(err, Error::Invalid(msg) if msg == "person_id must not be empty"));
}

#[test]
fn require_aliases_enforces_limits() {
    assert!(require_aliases(&[]).is_ok());

    let valid_aliases = vec!["a".to_owned(), "b".to_owned()];
    assert!(require_aliases(&valid_aliases).is_ok());

    let too_many = vec!["a".to_owned(); MAX_ALIASES + 1];
    let err = require_aliases(&too_many).unwrap_err();
    assert!(matches!(err, Error::Invalid(msg) if msg.contains("cannot exceed")));

    let too_long = vec!["a".repeat(MAX_ALIAS_CHARS + 1)];
    let err = require_aliases(&too_long).unwrap_err();
    assert!(matches!(err, Error::Invalid(msg) if msg.contains("exceeds")));

    let empty = vec![" ".to_owned()];
    let err = require_aliases(&empty).unwrap_err();
    assert!(matches!(err, Error::Invalid(msg) if msg == "aliases must not be empty"));
}

#[test]
fn fts_content_formats_correctly() {
    assert_eq!(fts_content("hello", &[]), "hello");
    assert_eq!(
        fts_content("hello", &["".to_owned(), "  ".to_owned()]),
        "hello"
    );

    let aliases = vec!["world".to_owned(), " test ".to_owned()];
    assert_eq!(fts_content("hello", &aliases), "hello\nworld\ntest");
}

#[test]
fn bounded_limit_works() {
    assert_eq!(bounded_limit(0), 10);
    assert_eq!(bounded_limit(5), 5);
    assert_eq!(bounded_limit(10), 10);
    assert_eq!(bounded_limit(100), 100);
    assert_eq!(bounded_limit(105), 100);
}

#[test]
fn memory_db_open_migrates_schema() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    assert!(db.migrate().is_ok());

    let user_version: i64 = db
        .connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert!(user_version > 0);
}
