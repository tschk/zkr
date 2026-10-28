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

    // The automatic embedding is written alongside the inline one.
    let auto_count: i64 = db
        .connection
        .query_row(
            "SELECT count(*) FROM embeddings WHERE target_kind = 'claim' AND target_id = ?1 AND model = ?2",
            [claim_id.0.as_str(), AUTO_EMBEDDING_MODEL],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(auto_count, 1);
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

#[test]
fn remember_project_record_stores_record_and_keeps_primary_category_first() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();

    let claim = ClaimInput {
        subject: "Tofu".into(),
        predicate: "task:pairing".into(),
        value: "dummy".into(), // Will be overwritten by record
        kind: ClaimKind::Task,
        valid_from: 10,
        tier: MemoryTier::ShortTerm,
        processing_state: MemoryProcessingState::Processed,
    };

    let mut input = ProjectRecordInput {
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
    input.memory.memory.claim = Some(claim);

    let result = db.remember_project_record(input).unwrap();

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

    // Query by the record-only category; the memory's own category stays first.
    let facts = db
        .project_memories(ProjectQuery {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("sam".into()),
            project: Some("Tofu".into()),
            category: Some("AI-defined/connectors".into()),
            ..ProjectQuery::default()
        })
        .unwrap();

    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].value, row);
    assert_eq!(
        facts[0].categories,
        vec!["launch".to_string(), "AI-defined/connectors".to_string()]
    );
}
