use super::*;
use tempfile::NamedTempFile;
use std::iter;


#[test]
fn test_require_text() {
    assert!(require_text("field", "valid text").is_ok());

    let err = require_text("field", "").unwrap_err();
    assert!(matches!(err, Error::Invalid(msg) if msg == "field must not be empty"));

    let err = require_text("field", "   \n").unwrap_err();
    assert!(matches!(err, Error::Invalid(msg) if msg == "field must not be empty"));
}

#[test]
fn test_require_scope() {
    let valid_tenant = TenantId("tenant1".into());
    let valid_person = PersonId("person1".into());
    let empty_tenant = TenantId("".into());
    let empty_person = PersonId("".into());
    let ws_tenant = TenantId("   ".into());

    assert!(require_scope(&valid_tenant, &valid_person).is_ok());

    let err = require_scope(&empty_tenant, &valid_person).unwrap_err();
    assert!(matches!(err, Error::Invalid(msg) if msg == "tenant_id must not be empty"));

    let err = require_scope(&valid_tenant, &empty_person).unwrap_err();
    assert!(matches!(err, Error::Invalid(msg) if msg == "person_id must not be empty"));

    let err = require_scope(&ws_tenant, &valid_person).unwrap_err();
    assert!(matches!(err, Error::Invalid(msg) if msg == "tenant_id must not be empty"));
}

#[test]
fn test_require_aliases() {
    // Valid aliases
    assert!(require_aliases(&["alias1".into(), "alias2".into()]).is_ok());

    // Empty array
    assert!(require_aliases(&[]).is_ok());

    // Too many aliases
    let too_many: Vec<String> = iter::repeat("a".to_string()).take(33).collect();
    let err = require_aliases(&too_many).unwrap_err();
    assert!(matches!(err, Error::Invalid(msg) if msg == "aliases cannot exceed 32 entries"));

    // Alias too long
    let too_long = iter::repeat('a').take(65).collect::<String>();
    let err = require_aliases(&[too_long]).unwrap_err();
    assert!(matches!(err, Error::Invalid(msg) if msg == "alias exceeds 64 characters"));

    // Empty alias
    let err = require_aliases(&["".into()]).unwrap_err();
    assert!(matches!(err, Error::Invalid(msg) if msg == "aliases must not be empty"));

    // Whitespace alias
    let err = require_aliases(&["   ".into()]).unwrap_err();
    assert!(matches!(err, Error::Invalid(msg) if msg == "aliases must not be empty"));
}

#[test]
fn test_fts_content() {
    assert_eq!(fts_content("hello", &[]), "hello");

    assert_eq!(
        fts_content("hello", &["alias1".into(), "alias2".into()]),
        "hello\nalias1\nalias2"
    );

    // Skips empty or whitespace aliases
    assert_eq!(
        fts_content("hello", &["alias1".into(), "".into(), "   ".into(), "alias2".into()]),
        "hello\nalias1\nalias2"
    );
}

#[test]
fn test_bounded_limit() {
    assert_eq!(bounded_limit(0), 10);
    assert_eq!(bounded_limit(5), 5);
    assert_eq!(bounded_limit(100), 100);
    assert_eq!(bounded_limit(101), 100);
    assert_eq!(bounded_limit(1000), 100);
}

#[test]
fn test_collect_json_page() {
    let rows: Vec<rusqlite::Result<String>> = vec![
        Ok("item1".into()),
        Ok("item2".into()),
    ];
    let page = collect_json_page(rows.into_iter()).unwrap();
    assert_eq!(page, vec!["item1".to_string(), "item2".to_string()]);

    // Test large item exceeding MAX_READ_PAGE_BYTES (1_000_000)
    let large_item = iter::repeat('a').take(1_000_000).collect::<String>();
    let rows: Vec<rusqlite::Result<String>> = vec![Ok(large_item)];
    let err = collect_json_page(rows.into_iter()).unwrap_err();
    assert!(matches!(err, Error::Invalid(msg) if msg.contains("read record exceeds")));
}

#[test]
fn test_collect_json_page_exceeds_page_limit() {
    // Each string will be about 600_000 bytes when serialized (with quotes)
    let large_item = iter::repeat('a').take(600_000).collect::<String>();
    let rows: Vec<rusqlite::Result<String>> = vec![
        Ok(large_item.clone()),
        Ok(large_item), // Second item will push it over 1_000_000
    ];
    let err = collect_json_page(rows.into_iter()).unwrap_err();
    assert!(matches!(err, Error::Invalid(msg) if msg.contains("read page exceeds")));
}

#[test]
fn test_memorydb_open() {
    let temp_file = NamedTempFile::new().unwrap();
    let db = MemoryDb::open(temp_file.path()).unwrap();

    // Verify migration ran (e.g. check tables exist)
    let count: i64 = db.connection
        .query_row("SELECT count(*) FROM sqlite_master WHERE type='table'", [], |row| row.get(0))
        .unwrap();
    assert!(count > 0, "Database should have tables created after migration");
}
