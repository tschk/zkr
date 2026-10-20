use super::*;

fn scope() -> (TenantId, PersonId) {
    (TenantId("a".into()), PersonId("sam".into()))
}

fn wake(db: &MemoryDb) -> WakePack {
    let (tenant_id, person_id) = scope();
    db.wake(WakeInput {
        search: SearchInput {
            tenant_id,
            person_id,
            query: "rollout".into(),
            limit: 10,
            query_embedding: None,
            as_of: None,
            enabled_features: Vec::new(),
        },
        max_bytes: 20,
    })
    .unwrap()
}

#[test]
fn summary_tree_wakes_zooms_stales_and_rebuilds() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let (tenant_id, person_id) = scope();
    let remembered = (0..4)
        .map(|index| {
            let mut input = remember_raw("a", "sam", &format!("rollout item {index}"));
            input.captured_at = 10 + index;
            input.recorded_at = 10 + index;
            db.remember(input).unwrap()
        })
        .collect::<Vec<_>>();
    let leaves = remembered
        .iter()
        .enumerate()
        .map(|(index, memory)| {
            db.nap(NapInput {
                tenant_id: tenant_id.clone(),
                person_id: person_id.clone(),
                summary: format!("item {index}"),
                evidence_ids: vec![memory.evidence_id.clone()],
                recorded_at: 20 + index as i64,
            })
            .unwrap()
            .summary_id
        })
        .collect::<Vec<_>>();
    let first_half = db
        .merge(MergeInput {
            tenant_id: tenant_id.clone(),
            person_id: person_id.clone(),
            summary: "first half".into(),
            child_ids: leaves[..2].to_vec(),
            recorded_at: 30,
        })
        .unwrap()
        .summary_id;
    let second_half = db
        .merge(MergeInput {
            tenant_id: tenant_id.clone(),
            person_id: person_id.clone(),
            summary: "second half".into(),
            child_ids: leaves[2..].to_vec(),
            recorded_at: 31,
        })
        .unwrap()
        .summary_id;
    let root = db
        .merge(MergeInput {
            tenant_id: tenant_id.clone(),
            person_id: person_id.clone(),
            summary: "all rollout".into(),
            child_ids: vec![first_half.clone(), second_half.clone()],
            recorded_at: 32,
        })
        .unwrap()
        .summary_id;
    assert_eq!(
        wake(&db).summaries,
        vec![
            db.zoom(ZoomInput {
                tenant_id: tenant_id.clone(),
                person_id: person_id.clone(),
                summary_id: root.clone()
            })
            .unwrap()
            .summary
        ]
    );
    let zoom = db
        .zoom(ZoomInput {
            tenant_id: tenant_id.clone(),
            person_id: person_id.clone(),
            summary_id: root.clone(),
        })
        .unwrap();
    assert_eq!(zoom.children.len(), 2);
    db.delete_source(DeleteInput {
        tenant_id: tenant_id.clone(),
        person_id: person_id.clone(),
        source_id: remembered[0].source_id.clone(),
        deleted_at: 100,
    })
    .unwrap();
    assert!(matches!(
        db.zoom(ZoomInput {
            tenant_id: tenant_id.clone(),
            person_id: person_id.clone(),
            summary_id: root.clone(),
        }),
        Err(Error::NotFound)
    ));
    assert_eq!(
        db.repair_projections(RepairInput {
            tenant_id: tenant_id.clone(),
            person_id: person_id.clone(),
            limit: 10,
        })
        .unwrap()
        .summaries_stale,
        3
    );
    let replacement = db
        .remember(remember_raw("a", "sam", "rollout replacement"))
        .unwrap();
    let first_leaf = db
        .rebuild(RebuildInput {
            tenant_id: tenant_id.clone(),
            person_id: person_id.clone(),
            summary_id: leaves[0].clone(),
            summary: "item 0 corrected".into(),
            evidence_ids: vec![replacement.evidence_id],
            recorded_at: 101,
        })
        .unwrap()
        .summary_id;
    let rebuilt_half = db
        .rebuild(RebuildInput {
            tenant_id: tenant_id.clone(),
            person_id: person_id.clone(),
            summary_id: first_half,
            summary: "first half corrected".into(),
            evidence_ids: Vec::new(),
            recorded_at: 102,
        })
        .unwrap()
        .summary_id;
    assert_ne!(first_leaf, leaves[0]);
    let rebuilt_root = db
        .rebuild(RebuildInput {
            tenant_id: tenant_id.clone(),
            person_id: person_id.clone(),
            summary_id: root,
            summary: "all fixed".into(),
            evidence_ids: Vec::new(),
            recorded_at: 103,
        })
        .unwrap()
        .summary_id;
    assert_ne!(rebuilt_half, second_half);
    assert_eq!(wake(&db).summaries[0].id, rebuilt_root);
}

#[test]
fn correction_stales_its_cited_summary() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let (tenant_id, person_id) = scope();
    let remembered = db.remember(remember("a", "sam", "Acme")).unwrap();
    let summary_id = db
        .nap(NapInput {
            tenant_id: tenant_id.clone(),
            person_id: person_id.clone(),
            summary: "Sam works at Acme".into(),
            evidence_ids: vec![remembered.evidence_id],
            recorded_at: 11,
        })
        .unwrap()
        .summary_id;
    db.correct(CorrectInput {
        tenant_id: tenant_id.clone(),
        person_id: person_id.clone(),
        claim_id: remembered.claim_id.unwrap(),
        text: "Sam now works at Beta".into(),
        value: "Beta".into(),
        valid_at: 12,
        recorded_at: 12,
    })
    .unwrap();
    assert!(matches!(
        db.zoom(ZoomInput {
            tenant_id,
            person_id,
            summary_id,
        }),
        Err(Error::NotFound)
    ));
}

#[test]
fn nap_validates_inputs() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let (tenant_id, person_id) = scope();

    // 1. Empty scope/text
    assert!(matches!(
        db.nap(NapInput {
            tenant_id: TenantId("".into()),
            person_id: person_id.clone(),
            summary: "summary".into(),
            evidence_ids: vec![EvidenceId("1".into())],
            recorded_at: 1,
        }),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        db.nap(NapInput {
            tenant_id: tenant_id.clone(),
            person_id: PersonId("".into()),
            summary: "summary".into(),
            evidence_ids: vec![EvidenceId("1".into())],
            recorded_at: 1,
        }),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        db.nap(NapInput {
            tenant_id: tenant_id.clone(),
            person_id: person_id.clone(),
            summary: "".into(),
            evidence_ids: vec![EvidenceId("1".into())],
            recorded_at: 1,
        }),
        Err(Error::Invalid(_))
    ));

    // 2. Empty evidence_ids
    assert!(matches!(
        db.nap(NapInput {
            tenant_id: tenant_id.clone(),
            person_id: person_id.clone(),
            summary: "summary".into(),
            evidence_ids: vec![],
            recorded_at: 1,
        }),
        Err(Error::Invalid(_))
    ));

    let memory1 = db.remember(remember_raw("a", "sam", "item 1")).unwrap();
    let memory2 = db.remember(remember_raw("a", "sam", "item 2")).unwrap();

    // 3. Duplicate evidence_ids
    assert!(matches!(
        db.nap(NapInput {
            tenant_id: tenant_id.clone(),
            person_id: person_id.clone(),
            summary: "summary".into(),
            evidence_ids: vec![memory1.evidence_id.clone(), memory1.evidence_id.clone()],
            recorded_at: 1,
        }),
        Err(Error::Invalid(_))
    ));

    // 4. Missing evidence
    assert!(matches!(
        db.nap(NapInput {
            tenant_id: tenant_id.clone(),
            person_id: person_id.clone(),
            summary: "summary".into(),
            evidence_ids: vec![EvidenceId("missing".into())],
            recorded_at: 1,
        }),
        Err(Error::Invalid(_))
    ));

    // 5. Deleted evidence
    db.delete_source(DeleteInput {
        tenant_id: tenant_id.clone(),
        person_id: person_id.clone(),
        source_id: memory1.source_id.clone(),
        deleted_at: 100,
    })
    .unwrap();
    assert!(matches!(
        db.nap(NapInput {
            tenant_id: tenant_id.clone(),
            person_id: person_id.clone(),
            summary: "summary".into(),
            evidence_ids: vec![memory1.evidence_id.clone()],
            recorded_at: 1,
        }),
        Err(Error::Invalid(_))
    ));

    // 6. Success
    let napped = db
        .nap(NapInput {
            tenant_id: tenant_id.clone(),
            person_id: person_id.clone(),
            summary: "summary".into(),
            evidence_ids: vec![memory2.evidence_id.clone()],
            recorded_at: 101,
        })
        .unwrap();

    let zoomed = db
        .zoom(ZoomInput {
            tenant_id,
            person_id,
            summary_id: napped.summary_id,
        })
        .unwrap();

    assert_eq!(zoomed.summary.summary, "summary");
    assert_eq!(zoomed.summary.evidence_ids, vec![memory2.evidence_id]);
}

#[test]
fn zoom_requires_scope() {
    let db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    assert!(matches!(
        db.zoom(ZoomInput {
            tenant_id: TenantId("".into()),
            person_id: PersonId("sam".into()),
            summary_id: SummaryId("sum-1".into()),
        }),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        db.zoom(ZoomInput {
            tenant_id: TenantId("a".into()),
            person_id: PersonId("".into()),
            summary_id: SummaryId("sum-1".into()),
        }),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn zoom_enforces_isolation() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let (tenant_id, person_id) = scope();
    let remembered = db.remember(remember("a", "sam", "Test")).unwrap();
    let summary_id = db
        .nap(NapInput {
            tenant_id: tenant_id.clone(),
            person_id: person_id.clone(),
            summary: "Test summary".into(),
            evidence_ids: vec![remembered.evidence_id],
            recorded_at: 11,
        })
        .unwrap()
        .summary_id;

    assert!(matches!(
        db.zoom(ZoomInput {
            tenant_id: TenantId("other".into()),
            person_id: person_id.clone(),
            summary_id: summary_id.clone(),
        }),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        db.zoom(ZoomInput {
            tenant_id: tenant_id.clone(),
            person_id: PersonId("other".into()),
            summary_id: summary_id.clone(),
        }),
        Err(Error::NotFound)
    ));
}

#[test]
fn zoom_missing_child_returns_error() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let (tenant_id, person_id) = scope();
    let r1 = db.remember(remember("a", "sam", "A")).unwrap();
    let r2 = db.remember(remember("a", "sam", "B")).unwrap();

    let s1 = db
        .nap(NapInput {
            tenant_id: tenant_id.clone(),
            person_id: person_id.clone(),
            summary: "summary A".into(),
            evidence_ids: vec![r1.evidence_id],
            recorded_at: 10,
        })
        .unwrap()
        .summary_id;

    let s2 = db
        .nap(NapInput {
            tenant_id: tenant_id.clone(),
            person_id: person_id.clone(),
            summary: "summary B".into(),
            evidence_ids: vec![r2.evidence_id],
            recorded_at: 11,
        })
        .unwrap()
        .summary_id;

    let root = db
        .merge(MergeInput {
            tenant_id: tenant_id.clone(),
            person_id: person_id.clone(),
            summary: "root".into(),
            child_ids: vec![s1.clone(), s2.clone()],
            recorded_at: 12,
        })
        .unwrap()
        .summary_id;

    // Everything is healthy, zoom works
    let zoomed = db
        .zoom(ZoomInput {
            tenant_id: tenant_id.clone(),
            person_id: person_id.clone(),
            summary_id: root.clone(),
        })
        .unwrap();
    assert_eq!(zoomed.children.len(), 2);

    // Simulate data corruption by manually deleting one child
    db.connection
        .execute(
            "DELETE FROM summary_nodes WHERE id = ?1",
            rusqlite::params![s1.0],
        )
        .unwrap();

    assert!(matches!(
        db.zoom(ZoomInput {
            tenant_id,
            person_id,
            summary_id: root,
        }),
        Err(Error::NotFound)
    ));
}

#[test]
fn wake_budget_expands_summary_tree_and_reports_gaps() {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let (tenant_id, person_id) = scope();

    let remembered = (0..4)
        .map(|index| {
            let mut input = remember_raw("a", "sam", &format!("item {index}"));
            input.captured_at = 10 + index;
            input.recorded_at = 10 + index;
            db.remember(input).unwrap()
        })
        .collect::<Vec<_>>();

    let leaves = remembered
        .iter()
        .enumerate()
        .map(|(index, memory)| {
            db.nap(NapInput {
                tenant_id: tenant_id.clone(),
                person_id: person_id.clone(),
                summary: format!("item {index}"),
                evidence_ids: vec![memory.evidence_id.clone()],
                recorded_at: 20 + index as i64,
            })
            .unwrap()
            .summary_id
        })
        .collect::<Vec<_>>();

    let first_half = db
        .merge(MergeInput {
            tenant_id: tenant_id.clone(),
            person_id: person_id.clone(),
            summary: "first".into(),
            child_ids: leaves[..2].to_vec(),
            recorded_at: 30,
        })
        .unwrap()
        .summary_id;
    let second_half = db
        .merge(MergeInput {
            tenant_id: tenant_id.clone(),
            person_id: person_id.clone(),
            summary: "second".into(),
            child_ids: leaves[2..].to_vec(),
            recorded_at: 31,
        })
        .unwrap()
        .summary_id;

    let root = db
        .merge(MergeInput {
            tenant_id: tenant_id.clone(),
            person_id: person_id.clone(),
            summary: "all".into(),
            child_ids: vec![first_half.clone(), second_half.clone()],
            recorded_at: 32,
        })
        .unwrap()
        .summary_id;

    let wake_with_budget = |max_bytes: u32| -> WakePack {
        db.wake(WakeInput {
            search: SearchInput {
                tenant_id: tenant_id.clone(),
                person_id: person_id.clone(),
                query: "test".into(),
                limit: 10,
                query_embedding: None,
                as_of: None,
                enabled_features: Vec::new(),
            },
            max_bytes,
        })
        .unwrap()
    };

    // "all" = 3 bytes
    // "first" + "second" = 5 + 6 = 11 bytes
    // "first" + "item 2" + "item 3" = 5 + 6 + 6 = 17 bytes
    // "item 0" + "item 1" + "item 2" + "item 3" = 6 + 6 + 6 + 6 = 24 bytes

    // Budget 2: too small for even the root node.
    let w = wake_with_budget(2);
    assert!(w.summaries.is_empty());
    assert_eq!(w.used_bytes, 0);
    assert!(
        w.retrieval
            .gaps
            .contains(&"no live summary matched the wake budget".to_owned())
    );

    // Budget 10: enough for root, but not to expand it (cost 11).
    let w = wake_with_budget(10);
    assert_eq!(w.summaries.len(), 1);
    assert_eq!(w.summaries[0].id, root);
    assert_eq!(w.used_bytes, 3);
    assert!(
        !w.retrieval
            .gaps
            .contains(&"no live summary matched the wake budget".to_owned())
    );

    // Budget 11: exactly enough to expand root into first_half and second_half.
    let w = wake_with_budget(11);
    assert_eq!(w.summaries.len(), 2);
    assert_eq!(w.summaries[0].id, first_half);
    assert_eq!(w.summaries[1].id, second_half);
    assert_eq!(w.used_bytes, 11);

    // Budget 16: enough to expand root, but not enough to expand second_half (needs 17).
    let w = wake_with_budget(16);
    assert_eq!(w.summaries.len(), 2);
    assert_eq!(w.summaries[0].id, first_half);
    assert_eq!(w.summaries[1].id, second_half);
    assert_eq!(w.used_bytes, 11);

    // Budget 17: exactly enough to expand second_half.
    let w = wake_with_budget(17);
    assert_eq!(w.summaries.len(), 3);
    assert_eq!(w.summaries[0].id, first_half);
    assert_eq!(w.summaries[1].id, leaves[2]);
    assert_eq!(w.summaries[2].id, leaves[3]);
    assert_eq!(w.used_bytes, 17);

    // Budget 23: enough to expand second_half, but not first_half (needs 24).
    let w = wake_with_budget(23);
    assert_eq!(w.summaries.len(), 3);
    assert_eq!(w.summaries[0].id, first_half);
    assert_eq!(w.summaries[1].id, leaves[2]);
    assert_eq!(w.summaries[2].id, leaves[3]);
    assert_eq!(w.used_bytes, 17);

    // Budget 24: exactly enough to expand all nodes to leaves.
    let w = wake_with_budget(24);
    assert_eq!(w.summaries.len(), 4);
    assert_eq!(w.summaries[0].id, leaves[0]);
    assert_eq!(w.summaries[1].id, leaves[1]);
    assert_eq!(w.summaries[2].id, leaves[2]);
    assert_eq!(w.summaries[3].id, leaves[3]);
    assert_eq!(w.used_bytes, 24);
}
