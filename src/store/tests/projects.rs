use super::*;

fn scope() -> (TenantId, PersonId) {
    (TenantId("a".into()), PersonId("sam".into()))
}

#[test]
fn dreams_summarize_project_categories() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let (tenant_id, person_id) = scope();

    db.remember_project_record(ProjectRecordInput {
        memory: ProjectMemoryInput {
            project: Some("build".into()),
            category: None,
            categories: Vec::new(),
            embedding: None,
            memory: remember("a", "sam", "started frontend task"),
        },
        record: ProjectRecord {
            record_type: "task".into(),
            categories: vec!["frontend".into()],
            fields: serde_json::Map::new(),
            connections: vec![],
        },
    })
    .unwrap();

    db.remember_project_record(ProjectRecordInput {
        memory: ProjectMemoryInput {
            project: Some("build".into()),
            category: None,
            categories: Vec::new(),
            embedding: None,
            memory: remember("a", "sam", "working on ui task"),
        },
        record: ProjectRecord {
            record_type: "task".into(),
            categories: vec!["frontend".into(), "ui".into()],
            fields: serde_json::Map::new(),
            connections: vec![],
        },
    })
    .unwrap();

    let pack = db
        .dream(DreamInput {
            tenant_id,
            person_id,
            model: None,
            version: None,
            min_similarity_basis_points: 0,
            limit: 10,
        })
        .unwrap();

    let frontend = pack
        .categories
        .iter()
        .find(|c| c.category == "frontend")
        .unwrap();
    assert_eq!(frontend.project, "build");
    assert_eq!(frontend.memory_count, 2);
    assert!(frontend.co_occurring_categories.contains(&"ui".to_string()));
    assert!(frontend.record_types.contains(&"task".to_string()));

    let ui = pack.categories.iter().find(|c| c.category == "ui").unwrap();
    assert_eq!(ui.project, "build");
    assert_eq!(ui.memory_count, 1);
    assert!(ui.co_occurring_categories.contains(&"frontend".to_string()));
}

#[test]
fn dreams_extract_struct_schemas() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let (tenant_id, person_id) = scope();

    db.remember_project_record(ProjectRecordInput {
        memory: ProjectMemoryInput {
            project: Some("build".into()),
            category: None,
            categories: Vec::new(),
            embedding: None,
            memory: remember("a", "sam", "started frontend task"),
        },
        record: ProjectRecord {
            record_type: "task".into(),
            categories: vec!["frontend".into()],
            fields: {
                let mut map = serde_json::Map::new();
                map.insert("status".into(), serde_json::Value::String("todo".into()));
                map.insert("priority".into(), serde_json::Value::String("high".into()));
                map
            },
            connections: vec![ProjectConnection {
                relation: "blocks".into(),
                target: "other_task".into(),
                weight_basis_points: None,
                note: None,
            }],
        },
    })
    .unwrap();

    let pack = db
        .dream(DreamInput {
            tenant_id,
            person_id,
            model: None,
            version: None,
            min_similarity_basis_points: 0,
            limit: 10,
        })
        .unwrap();

    let task_schema = pack
        .schemas
        .iter()
        .find(|s| s.record_type == "task")
        .unwrap();
    assert_eq!(task_schema.record_count, 1);
    assert!(task_schema.fields.contains(&"status".to_string()));
    assert!(task_schema.fields.contains(&"priority".to_string()));
    assert!(task_schema.relations.contains(&"blocks".to_string()));
    assert!(task_schema.categories.contains(&"frontend".to_string()));
}

#[test]
fn dreams_extract_graph_hubs_and_unresolved_targets() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let (tenant_id, person_id) = scope();

    db.remember_project_record(ProjectRecordInput {
        memory: ProjectMemoryInput {
            project: Some("build".into()),
            category: None,
            categories: Vec::new(),
            embedding: None,
            memory: remember("a", "sam", "started frontend task"),
        },
        record: ProjectRecord {
            record_type: "task".into(),
            categories: vec!["frontend".into()],
            fields: serde_json::Map::new(),
            connections: vec![
                ProjectConnection {
                    relation: "depends_on".into(),
                    target: "design_system".into(), // Unresolved target!
                    weight_basis_points: None,
                    note: None,
                },
                ProjectConnection {
                    relation: "blocks".into(),
                    target: "launch".into(), // Unresolved target!
                    weight_basis_points: None,
                    note: None,
                },
            ],
        },
    })
    .unwrap();

    let pack = db
        .dream(DreamInput {
            tenant_id,
            person_id,
            model: None,
            version: None,
            min_similarity_basis_points: 0,
            limit: 10,
        })
        .unwrap();

    // Check hubs
    let frontend_task = pack
        .hubs
        .iter()
        .find(|h| h.identifier == "employer")
        .unwrap(); // predicate of remember
    assert_eq!(frontend_task.outbound_count, 2); // 2 connections
    assert_eq!(frontend_task.inbound_count, 0);

    let launch_hub = pack.hubs.iter().find(|h| h.identifier == "launch").unwrap();
    assert_eq!(launch_hub.inbound_count, 1);
    assert_eq!(launch_hub.outbound_count, 0);

    let design_hub = pack
        .hubs
        .iter()
        .find(|h| h.identifier == "design_system")
        .unwrap();
    assert_eq!(design_hub.inbound_count, 1);
    assert_eq!(design_hub.outbound_count, 0);

    // Check unresolved targets
    assert_eq!(pack.unresolved_targets.len(), 2);
    let launch_target = pack
        .unresolved_targets
        .iter()
        .find(|t| t.target == "launch")
        .unwrap();
    assert_eq!(launch_target.relation, "blocks");

    let design_target = pack
        .unresolved_targets
        .iter()
        .find(|t| t.target == "design_system")
        .unwrap();
    assert_eq!(design_target.relation, "depends_on");
}

#[test]
fn dreams_surface_contradictions_and_short_term_claims() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let (tenant_id, person_id) = scope();

    // Create a regular claim
    let _c1 = db
        .remember_project_record(ProjectRecordInput {
            memory: ProjectMemoryInput {
                project: Some("build".into()),
                category: None,
                categories: Vec::new(),
                embedding: None,
                memory: remember("a", "sam", "Frontend takes 2 weeks"),
            },
            record: ProjectRecord {
                record_type: "estimate".into(),
                categories: vec!["test".into()],
                fields: serde_json::Map::new(),
                connections: vec![],
            },
        })
        .unwrap();

    // Create a contradicting claim
    db.connection
        .execute(
            "UPDATE claim_evidence SET relation = '\"contradicts\"' WHERE claim_id = ?1",
            params![_c1.claim_id.unwrap().0],
        )
        .unwrap();

    // Create a short-term claim
    let mut mem = remember("a", "sam", "Temporary note");
    mem.claim.as_mut().unwrap().tier = MemoryTier::ShortTerm;

    let c2 = db
        .remember_project_record(ProjectRecordInput {
            memory: ProjectMemoryInput {
                project: Some("build".into()),
                category: None,
                categories: Vec::new(),
                embedding: None,
                memory: mem,
            },
            record: ProjectRecord {
                record_type: "note".into(),
                categories: vec!["test".into()],
                fields: serde_json::Map::new(),
                connections: vec![],
            },
        })
        .unwrap();

    let pack = db
        .dream(DreamInput {
            tenant_id,
            person_id,
            model: None,
            version: None,
            min_similarity_basis_points: 0,
            limit: 10,
        })
        .unwrap();

    assert_eq!(pack.contradictions.len(), 1);

    assert_eq!(pack.short_term_claims.len(), 1);
    assert_eq!(pack.short_term_claims[0], c2.claim_id.unwrap());
}

#[test]
fn dreams_surface_unsummarized_evidence_and_stale_summaries() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let (tenant_id, person_id) = scope();

    // Remember a claim. This creates evidence automatically.
    let _c1 = db
        .remember_project_record(ProjectRecordInput {
            memory: ProjectMemoryInput {
                project: Some("build".into()),
                category: None,
                categories: Vec::new(),
                embedding: None,
                memory: remember("a", "sam", "started frontend task"),
            },
            record: ProjectRecord {
                record_type: "task".into(),
                categories: vec!["test".into()],
                fields: serde_json::Map::new(),
                connections: vec![],
            },
        })
        .unwrap();

    // Create a stale summary node
    db.connection.execute(
        "INSERT INTO summary_nodes (id, tenant_id, person_id, level, start_sequence, end_sequence, summary, evidence_ids, child_ids, recorded_at, stale_at)
         VALUES ('s1', ?1, ?2, 0, 0, 0, 'A summary', json_array(?3), '[]', 10, 20)",
        params![tenant_id.0, person_id.0, _c1.evidence_id.0],
    ).unwrap();

    let pack = db
        .dream(DreamInput {
            tenant_id,
            person_id,
            model: None,
            version: None,
            min_similarity_basis_points: 0,
            limit: 10,
        })
        .unwrap();

    assert_eq!(pack.unsummarized_evidence_ids.len(), 1);
    assert_eq!(pack.unsummarized_evidence_ids[0], _c1.evidence_id);

    assert_eq!(pack.stale_summary_ids.len(), 1);
    assert_eq!(pack.stale_summary_ids[0].0, "s1");
}
