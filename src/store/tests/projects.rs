use super::*;

#[test]
fn test_remember_project_missing_claim() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();

    let mut memory = remember("a", "sam", "Acme");
    memory.claim = None;
    let input = ProjectMemoryInput {
        project: Some("alpha".into()),
        category: Some("milestone".into()),
        categories: vec![],
        embedding: None,
        memory,
    };

    let result = db.remember_project(input);
    assert!(matches!(result, Err(Error::Invalid(_))));
}

#[test]
fn test_remember_project_no_categories() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();

    let memory = remember("a", "sam", "Acme");
    let input = ProjectMemoryInput {
        project: Some("alpha".into()),
        category: None,
        categories: vec![],
        embedding: None,
        memory,
    };

    let result = db.remember_project(input);
    assert!(
        matches!(result, Err(Error::Invalid(msg)) if msg.contains("category must not be empty"))
    );
}

#[test]
fn test_remember_project_too_many_categories() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();

    let memory = remember("a", "sam", "Acme");
    let categories: Vec<String> = (0..33).map(|i| format!("cat{}", i)).collect();
    let input = ProjectMemoryInput {
        project: Some("alpha".into()),
        category: None,
        categories,
        embedding: None,
        memory,
    };

    let result = db.remember_project(input);
    assert!(matches!(result, Err(Error::Invalid(msg)) if msg.contains("exceed 32 entries")));
}

#[test]
fn test_remember_project_category_too_long() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();

    let memory = remember("a", "sam", "Acme");
    let long_cat = "a".repeat(257);
    let input = ProjectMemoryInput {
        project: Some("alpha".into()),
        category: Some(long_cat),
        categories: vec![],
        embedding: None,
        memory,
    };

    let result = db.remember_project(input);
    assert!(matches!(result, Err(Error::Invalid(msg)) if msg.contains("exceeds 256 bytes")));
}

#[test]
fn test_remember_project_with_embedding() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();

    let memory = remember("a", "sam", "Acme");
    let input = ProjectMemoryInput {
        project: Some("alpha".into()),
        category: Some("milestone".into()),
        categories: vec![],
        embedding: Some(InlineEmbedding {
            vector: vec![1.0, 0.0, 0.0],
            model: "test-model".into(),
            version: "v1".into(),
            normalization: VectorNormalization::L2,
            distance: VectorDistance::Cosine,
        }),
        memory,
    };

    let result = db.remember_project(input).unwrap();
    assert!(result.claim_id.is_some());

    // Test that the embedding was stored
    let claim_id = result.claim_id.unwrap();
    let count: i64 = db.connection.query_row(
        "SELECT count(*) FROM embeddings WHERE target_kind = 'claim' AND target_id = ?1 AND model = 'test-model'",
        [claim_id.0.as_str()],
        |row| row.get(0)
    ).unwrap();

    assert_eq!(count, 1);
}

#[test]
fn test_remember_project_basic() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();

    let memory = remember("a", "sam", "Acme");
    let input = ProjectMemoryInput {
        project: Some("alpha".into()),
        category: Some("milestone".into()),
        categories: vec![],
        embedding: None,
        memory,
    };
    let result = db.remember_project(input).unwrap();

    assert!(result.claim_id.is_some());

    // Check if the claim subject was modified as expected
    let claim_id = result.claim_id.unwrap();
    let claim_subject: String = db
        .connection
        .query_row(
            "SELECT subject FROM claims WHERE id = ?1",
            [claim_id.0.as_str()],
            |row| row.get(0),
        )
        .unwrap();

    assert!(claim_subject.starts_with("zkr:project:"));
    assert!(claim_subject.contains("alpha"));
    assert!(claim_subject.contains("milestone"));
}

#[test]
fn test_remember_project_default_project_name() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();

    let memory = remember("a", "sam", "Acme");
    let input = ProjectMemoryInput {
        project: None, // This should default to "general"
        category: Some("milestone".into()),
        categories: vec![],
        embedding: None,
        memory,
    };
    let result = db.remember_project(input).unwrap();

    let claim_id = result.claim_id.unwrap();
    let claim_subject: String = db
        .connection
        .query_row(
            "SELECT subject FROM claims WHERE id = ?1",
            [claim_id.0.as_str()],
            |row| row.get(0),
        )
        .unwrap();

    assert!(claim_subject.contains("general"));
}

#[test]
fn test_remember_project_multiple_categories() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();

    let memory = remember("a", "sam", "Acme");
    let input = ProjectMemoryInput {
        project: Some("alpha".into()),
        category: Some("main_cat".into()),
        categories: vec!["sub_cat1".into(), "sub_cat2".into(), "main_cat".into()], // Duplicate should be ignored
        embedding: None,
        memory,
    };
    let result = db.remember_project(input).unwrap();

    let claim_id = result.claim_id.unwrap();
    let claim_subject: String = db
        .connection
        .query_row(
            "SELECT subject FROM claims WHERE id = ?1",
            [claim_id.0.as_str()],
            |row| row.get(0),
        )
        .unwrap();

    assert!(claim_subject.contains("alpha"));
    assert!(claim_subject.contains("main_cat"));
    assert!(claim_subject.contains("sub_cat1"));
    assert!(claim_subject.contains("sub_cat2"));

    // Check that "main_cat" only appears once in the serialized parts
    // It will be serialized with quotes inside the json string like "main_cat"
    // Since subject is like zkr:project:["alpha","main_cat","sub_cat1","sub_cat2"]
    let expected = format!(
        "zkr:project:{}",
        serde_json::to_string(&vec!["alpha", "main_cat", "sub_cat1", "sub_cat2"]).unwrap()
    );
    assert_eq!(claim_subject, expected);
}
