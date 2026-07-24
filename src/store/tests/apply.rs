use super::*;

const STATE_QUERIES: [&str; 9] = [
    "SELECT id, person_id, ingestion_key, revision, kind, content, captured_at, recorded_at, deleted_at, origin_evidence_id, origin_claim_id FROM sources WHERE tenant_id = 'a' ORDER BY id",
    "SELECT id, person_id, source_id, source_revision, quote, recorded_at, deleted_at FROM evidence WHERE tenant_id = 'a' ORDER BY id",
    "SELECT person_id, evidence_id, device_id, provider, stream_id, segment_id, start_ms, end_ms FROM evidence_locators WHERE tenant_id = 'a' ORDER BY evidence_id",
    "SELECT id, person_id, subject, predicate, value, kind, valid_from, valid_until, recorded_from, recorded_until, status, tier, processing_state FROM claims WHERE tenant_id = 'a' ORDER BY id",
    "SELECT person_id, claim_id, evidence_id, relation, confidence_basis_points FROM claim_evidence WHERE tenant_id = 'a' ORDER BY claim_id, evidence_id",
    "SELECT person_id, superseded_claim_id, claim_id, source_id, evidence_id, valid_at, recorded_at FROM corrections WHERE tenant_id = 'a' ORDER BY superseded_claim_id, claim_id",
    "SELECT id, person_id, key, value, stability, claim_id, recorded_at FROM profile_entries WHERE tenant_id = 'a' ORDER BY id",
    "SELECT id, person_id, day, summary, evidence_ids, recorded_at FROM daily_reviews WHERE tenant_id = 'a' ORDER BY id",
    "SELECT source_id, content FROM source_fts WHERE tenant_id = 'a' ORDER BY source_id",
];

fn empty() -> MemoryDb {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    db
}

fn locator() -> TranscriptLocator {
    TranscriptLocator {
        device_id: "device".into(),
        provider: "provider".into(),
        stream_id: "stream".into(),
        segment_id: "segment".into(),
        start_ms: 10,
        end_ms: 20,
    }
}

fn scope() -> (TenantId, PersonId) {
    (TenantId("a".into()), PersonId("sam".into()))
}

fn table_rows(db: &MemoryDb, query: &str) -> Vec<String> {
    let mut statement = db.connection.prepare(query).unwrap();
    let columns = statement.column_count();
    statement
        .query_map([], |row| {
            let mut cells = Vec::with_capacity(columns);
            for index in 0..columns {
                cells.push(format!("{:?}", row.get_ref(index)?));
            }
            Ok(cells.join("|"))
        })
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap()
}

fn state(db: &MemoryDb) -> Vec<Vec<String>> {
    STATE_QUERIES
        .iter()
        .map(|query| table_rows(db, query))
        .collect()
}

fn export_all(db: &mut MemoryDb, tenant: &str) -> Vec<ExportCommit> {
    let mut commits: Vec<ExportCommit> = Vec::new();
    let mut after_commit = 0;
    let mut after_event_index = -1;
    let mut high_water_mark = None;
    loop {
        let page = db
            .export(ExportInput {
                export_format: EXPORT_FORMAT_VERSION,
                tenant_id: TenantId(tenant.into()),
                person_id: PersonId("sam".into()),
                after_commit,
                after_event_index,
                high_water_mark,
                limit: 100,
            })
            .unwrap();
        high_water_mark = Some(page.high_water_mark);
        after_commit = page.next_after_commit;
        after_event_index = page.next_after_event_index;
        let complete = page.complete;
        for commit in page.commits {
            match commits.last_mut() {
                Some(last) if last.sequence == commit.sequence => {
                    last.records.extend(commit.records);
                }
                _ => commits.push(commit),
            }
        }
        if complete {
            break;
        }
    }
    commits
}

fn apply_input(commits: Vec<ExportCommit>) -> ApplyInput {
    let (tenant_id, person_id) = scope();
    ApplyInput {
        export_format: EXPORT_FORMAT_VERSION,
        database_schema_version: Some(DATABASE_SCHEMA_VERSION),
        tenant_id,
        person_id,
        commits,
    }
}

fn populated() -> MemoryDb {
    let (tenant_id, person_id) = scope();
    let mut db = empty();
    let employer = db
        .remember_with_locator(remember("a", "sam", "Acme"), Some(locator()))
        .unwrap();
    let mut city = remember_raw("a", "sam", "Sam lives in Berlin");
    city.claim = Some(ClaimInput {
        subject: "Sam".into(),
        predicate: "city".into(),
        value: "Berlin".into(),
        kind: ClaimKind::ProfileFact,
        valid_from: 10,
        tier: MemoryTier::LongTerm,
        processing_state: MemoryProcessingState::Processed,
    });
    let city = db.remember(city).unwrap();
    db.store_profile(ProfileInput {
        tenant_id: tenant_id.clone(),
        person_id: person_id.clone(),
        stability: ProfileStability::Current,
        claim_id: city.claim_id.clone().unwrap(),
        recorded_at: 12,
    })
    .unwrap();
    db.store_review(ReviewInput {
        tenant_id: tenant_id.clone(),
        person_id: person_id.clone(),
        day: "2026-07-21".into(),
        summary: "Sam works at Acme".into(),
        evidence_ids: vec![employer.evidence_id.clone()],
        recorded_at: 13,
    })
    .unwrap();
    let mut language = remember_raw("a", "sam", "Sam speaks Portuguese");
    language.claim = Some(ClaimInput {
        subject: "Sam".into(),
        predicate: "language".into(),
        value: "Portuguese".into(),
        kind: ClaimKind::ProfileFact,
        valid_from: 10,
        tier: MemoryTier::LongTerm,
        processing_state: MemoryProcessingState::Processed,
    });
    let language = db.remember(language).unwrap();
    db.store_profile(ProfileInput {
        tenant_id: tenant_id.clone(),
        person_id: person_id.clone(),
        stability: ProfileStability::Stable,
        claim_id: language.claim_id.clone().unwrap(),
        recorded_at: 12,
    })
    .unwrap();
    let mut running = remember_raw("a", "sam", "Sam started running");
    running.claim = Some(ClaimInput {
        subject: "Sam".into(),
        predicate: "habit".into(),
        value: "running".into(),
        kind: ClaimKind::Fact,
        valid_from: 10,
        tier: MemoryTier::ShortTerm,
        processing_state: MemoryProcessingState::Processed,
    });
    let running = db.remember(running).unwrap();
    db.promote(PromoteInput {
        tenant_id: tenant_id.clone(),
        person_id: person_id.clone(),
        claim_id: running.claim_id.clone().unwrap(),
        recorded_at: 14,
    })
    .unwrap();
    db.link_claim_evidence(ClaimEvidence {
        tenant_id: tenant_id.clone(),
        person_id: person_id.clone(),
        claim_id: employer.claim_id.clone().unwrap(),
        evidence_id: city.evidence_id.clone(),
        relation: EvidenceRelation::Contradicts,
        confidence_basis_points: 5_000,
    })
    .unwrap();
    db.correct(CorrectInput {
        tenant_id: tenant_id.clone(),
        person_id: person_id.clone(),
        claim_id: employer.claim_id.clone().unwrap(),
        text: "Sam works at Globex".into(),
        value: "Globex".into(),
        valid_at: 20,
        recorded_at: 20,
    })
    .unwrap();
    db.delete_source(DeleteInput {
        tenant_id: tenant_id.clone(),
        person_id: person_id.clone(),
        source_id: city.source_id.clone(),
        deleted_at: 30,
    })
    .unwrap();
    db.remember(remember("b", "sam", "Other")).unwrap();
    db
}

#[test]
fn apply_round_trips_an_exported_replica() {
    let mut origin = populated();
    let commits = export_all(&mut origin, "a");
    let mut replica = empty();
    let applied = replica.apply(apply_input(commits.clone())).unwrap();

    assert_eq!(applied.commits_applied, commits.len() as u64);
    assert_eq!(applied.commits_skipped, 0);
    assert_eq!(applied.records_skipped, 0);
    assert_eq!(
        applied.records_applied,
        commits
            .iter()
            .map(|commit| commit.records.len() as u64)
            .sum::<u64>()
    );
    assert_eq!(state(&replica), state(&origin));
    assert_eq!(export_all(&mut replica, "a"), commits);
    assert!(state(&replica).iter().all(|table| !table.is_empty()));

    let (tenant_id, person_id) = scope();
    let located = replica
        .connection
        .query_row(
            "SELECT evidence_id FROM evidence_locators WHERE tenant_id = 'a'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap();
    assert_eq!(
        replica
            .evidence_locator(EvidenceLocatorInput {
                tenant_id: tenant_id.clone(),
                person_id: person_id.clone(),
                evidence_id: EvidenceId(located),
            })
            .unwrap(),
        Some(locator())
    );
    assert!(
        table_rows(
            &replica,
            "SELECT id FROM sources WHERE tenant_id <> 'a' OR person_id <> 'sam'",
        )
        .is_empty()
    );
}

#[test]
fn apply_is_idempotent_across_replays() {
    let mut origin = populated();
    let commits = export_all(&mut origin, "a");
    let mut replica = empty();
    replica.apply(apply_input(commits.clone())).unwrap();
    let first = state(&replica);
    let feed = export_all(&mut replica, "a");

    let replayed = replica.apply(apply_input(commits.clone())).unwrap();
    assert_eq!(replayed.records_applied, 0);
    assert_eq!(replayed.commits_applied, 0);
    assert_eq!(replayed.commits_skipped, commits.len() as u64);
    assert_eq!(state(&replica), first);
    assert_eq!(export_all(&mut replica, "a"), feed);

    let mut halves = commits.clone();
    let tail = halves.split_off(commits.len() / 2);
    let mut incremental = empty();
    incremental.apply(apply_input(halves.clone())).unwrap();
    incremental.apply(apply_input(commits.clone())).unwrap();
    incremental.apply(apply_input(tail)).unwrap();
    assert_eq!(state(&incremental), first);
}

#[test]
fn apply_is_order_independent_inside_a_commit() {
    let mut origin = populated();
    let commits = export_all(&mut origin, "a");
    let reversed = commits
        .iter()
        .map(|commit| {
            let mut commit = commit.clone();
            commit.records.reverse();
            commit
        })
        .collect();
    let mut replica = empty();
    replica.apply(apply_input(reversed)).unwrap();
    assert_eq!(state(&replica), state(&origin));
}

#[test]
fn apply_preserves_evidence_locators_exactly() {
    let mut origin = empty();
    origin
        .remember_with_locator(remember("a", "sam", "Acme"), Some(locator()))
        .unwrap();
    let commits = export_all(&mut origin, "a");
    let mut replica = empty();
    replica.apply(apply_input(commits.clone())).unwrap();
    assert_eq!(
        table_rows(&replica, STATE_QUERIES[2]),
        table_rows(&origin, STATE_QUERIES[2])
    );

    let mut rewritten = commits;
    for commit in &mut rewritten {
        for record in &mut commit.records {
            if let ExportRecord::Evidence(record) = record {
                record.locator = Some(TranscriptLocator {
                    start_ms: 11,
                    ..locator()
                });
            }
        }
    }
    let error = replica.apply(apply_input(rewritten)).unwrap_err();
    assert!(matches!(error, Error::Invalid(message) if message.contains("transcript locator")));
    assert_eq!(
        table_rows(&replica, STATE_QUERIES[2]),
        table_rows(&origin, STATE_QUERIES[2])
    );
}

#[test]
fn apply_rejects_dangling_references_without_partial_writes() {
    let mut origin = populated();
    let commits = export_all(&mut origin, "a");
    let dangling = commits
        .iter()
        .filter(|commit| {
            commit
                .records
                .iter()
                .all(|record| matches!(record, ExportRecord::ClaimEvidence(_)))
        })
        .cloned()
        .collect::<Vec<_>>();
    assert!(!dangling.is_empty());
    let mut replica = empty();
    let error = replica.apply(apply_input(dangling)).unwrap_err();
    assert!(matches!(error, Error::Invalid(message) if message.contains("unknown")));
    assert!(state(&replica).iter().all(|table| table.is_empty()));
    assert_eq!(export_all(&mut replica, "a").len(), 0);
}

#[test]
fn apply_rejects_partial_commits_and_foreign_scope() {
    let mut origin = populated();
    let commits = export_all(&mut origin, "a");
    let mut replica = empty();

    let mut partial = commits.clone();
    partial[0].first_event_index = 1;
    assert!(
        matches!(replica.apply(apply_input(partial)).unwrap_err(), Error::Invalid(message) if message.contains("partial"))
    );

    let mut miscounted = commits.clone();
    miscounted[0].event_count += 1;
    assert!(
        matches!(replica.apply(apply_input(miscounted)).unwrap_err(), Error::Invalid(message) if message.contains("declares"))
    );

    let mut unordered = commits.clone();
    unordered.reverse();
    assert!(
        matches!(replica.apply(apply_input(unordered)).unwrap_err(), Error::Invalid(message) if message.contains("ascending"))
    );

    let mut foreign = apply_input(commits.clone());
    foreign.tenant_id = TenantId("b".into());
    assert!(
        matches!(replica.apply(foreign).unwrap_err(), Error::Invalid(message) if message.contains("scope"))
    );

    let mut stale = apply_input(commits.clone());
    stale.export_format = EXPORT_FORMAT_VERSION + 1;
    assert!(
        matches!(replica.apply(stale).unwrap_err(), Error::Invalid(message) if message.contains("export_format"))
    );

    let mut newer = apply_input(commits);
    newer.database_schema_version = Some(DATABASE_SCHEMA_VERSION + 1);
    assert!(
        matches!(replica.apply(newer).unwrap_err(), Error::Invalid(message) if message.contains("schema"))
    );

    assert!(state(&replica).iter().all(|table| table.is_empty()));
}

#[test]
fn apply_rejects_contradictory_authored_content() {
    let mut origin = populated();
    let commits = export_all(&mut origin, "a");
    let mut replica = empty();
    replica.apply(apply_input(commits.clone())).unwrap();

    let mut rewritten = commits;
    for commit in &mut rewritten {
        for record in &mut commit.records {
            if let ExportRecord::Source(record) = record {
                record.source.content.push_str(" (edited)");
            }
        }
    }
    let error = replica.apply(apply_input(rewritten)).unwrap_err();
    assert!(matches!(error, Error::Invalid(message) if message.contains("conflicts")));
    assert_eq!(state(&replica), state(&origin));
}

#[test]
fn applied_records_stay_searchable_and_repairable() {
    let mut origin = populated();
    let commits = export_all(&mut origin, "a");
    let mut replica = empty();
    replica.apply(apply_input(commits)).unwrap();
    let (tenant_id, person_id) = scope();

    let pack = replica
        .search(SearchInput {
            tenant_id: tenant_id.clone(),
            person_id: person_id.clone(),
            query: "Globex".into(),
            limit: 5,
            query_embedding: None,
            as_of: None,
            enabled_features: Vec::new(),
        })
        .unwrap();
    assert!(!pack.items.is_empty());
    assert!(pack.items.iter().all(|item| !item.evidence_ids.is_empty()));

    let issues = replica
        .projection_issues(ProjectionAuditInput {
            tenant_id,
            person_id,
            model: "model".into(),
            version: "1".into(),
            limit: 100,
        })
        .unwrap();
    assert!(
        issues
            .iter()
            .all(|issue| issue.state == ProjectionState::Missing)
    );
    assert!(!issues.is_empty());
}
