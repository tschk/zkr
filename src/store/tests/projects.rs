use super::*;

#[test]
fn remember_project_record_requires_cited_claim_and_updates_categories() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();

    // 1. Missing claim
    let input = ProjectRecordInput {
        record: ProjectRecord {
            record_type: "work_item".into(),
            categories: vec!["AI-defined/connectors".into()],
            fields: serde_json::Map::new(),
            connections: vec![],
        },
        memory: ProjectMemoryInput {
            project: Some("Tofu".into()),
            category: Some("launch".into()),
            categories: vec![],
            embedding: None,
            memory: remember_raw("a", "sam", "No claim here"),
        },
    };

    let result = db.remember_project_record(input);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().to_string(),
        "project record requires a cited claim"
    );

    // 2. Success case: claim is updated with record, categories are updated
    let claim = ClaimInput {
        subject: "Tofu".into(),
        predicate: "task:pairing".into(),
        value: "dummy".into(), // Will be overwritten by record
        kind: ClaimKind::Task,
        valid_from: 10,
        tier: MemoryTier::ShortTerm,
        processing_state: MemoryProcessingState::Processed,
    };

    let mut input2 = ProjectRecordInput {
        record: ProjectRecord {
            record_type: "work_item".into(),
            categories: vec!["AI-defined/connectors".into()],
            fields: serde_json::Map::new(),
            connections: vec![],
        },
        memory: ProjectMemoryInput {
            project: Some("Tofu".into()),
            category: Some("launch".into()),
            categories: vec![],
            embedding: None,
            memory: remember("a", "sam", "dummy"),
        },
    };
    input2.memory.memory.claim = Some(claim);

    let result = db.remember_project_record(input2).unwrap();

    let claim_id = result.claim_id.unwrap();
    let row: String = db
        .connection
        .query_row(
            "SELECT value FROM claims WHERE id = ?",
            [&claim_id.0],
            |row| row.get(0),
        )
        .unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&row).unwrap();
    assert_eq!(parsed["record_type"], "work_item");
    assert_eq!(parsed["categories"][0], "AI-defined/connectors");

    let facts = db.project_memories(ProjectQuery {
        tenant_id: TenantId("a".into()),
        person_id: PersonId("sam".into()),
        project: Some("Tofu".into()),
        category: Some("AI-defined/connectors".into()),
        ..ProjectQuery::default()
    }).unwrap();

    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].value, row);
    assert_eq!(facts[0].categories, vec!["launch".to_string(), "AI-defined/connectors".to_string()]);
}
