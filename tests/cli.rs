use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::Write,
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

fn run(database: &str, command: &str, input: Value) -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_zkr"))
        .args(["--db", database, command])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    serde_json::from_slice(&output.stdout).unwrap()
}

fn run_failure(database: &str, command: &str, input: Value) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_zkr"))
        .args(["--db", database, command])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    String::from_utf8(output.stderr).unwrap()
}

#[test]
fn json_cli_rejects_oversized_requests_before_deserialization() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("zkr-oversized-{nonce}.db"));
    let failure = run_failure(
        path.to_str().unwrap(),
        "remember",
        json!({ "text": "x".repeat(1024 * 1024) }),
    );
    assert!(failure.contains("request exceeds 1048576 bytes"));
}

#[test]
fn json_cli_remembers_and_returns_cited_search_results() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("zkr-{nonce}.db"));
    let database = path.to_str().unwrap();
    let remembered = run(
        database,
        "remember",
        json!({
            "tenant_id": "tenant",
            "person_id": "person",
            "kind": "conversation",
            "text": "Sam works at Acme",
            "captured_at": 10,
            "recorded_at": 10,
            "claim": { "subject": "Sam", "predicate": "employer", "value": "Acme", "kind": "fact", "valid_from": 10 }
        }),
    );
    let found = run(
        database,
        "search",
        json!({ "tenant_id": "tenant", "person_id": "person", "query": "Acme", "limit": 1 }),
    );
    assert_eq!(
        found["items"][0]["evidence_ids"][0],
        remembered["evidence_id"]
    );
    let exact = run(
        database,
        "get",
        json!({
            "tenant_id": "tenant",
            "person_id": "person",
            "target": found["items"][0]["memory"]
        }),
    );
    assert_eq!(exact["excerpt"], "Sam employer Acme");
    let projections = run(
        database,
        "projections",
        json!({ "tenant_id": "tenant", "person_id": "person", "model": "test/model", "version": "1", "limit": 1 }),
    );
    assert_eq!(projections[0]["state"], "missing");
    assert!(
        projections[0]["input"]["input_hash"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn json_cli_retrieves_raw_capture_without_a_claim() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("zkr-raw-{nonce}.db"));
    let database = path.to_str().unwrap();
    let remembered = run(
        database,
        "remember",
        json!({
            "tenant_id": "tenant",
            "person_id": "person",
            "kind": "screen",
            "text": "Roadmap review is Thursday",
            "captured_at": 10,
            "recorded_at": 10
        }),
    );
    let found = run(
        database,
        "search",
        json!({ "tenant_id": "tenant", "person_id": "person", "query": "Thursday", "limit": 1 }),
    );
    assert_eq!(found["items"][0]["memory"]["kind"], "source");
    assert_eq!(
        found["items"][0]["evidence_ids"][0],
        remembered["evidence_id"]
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn json_cli_stores_a_profile_and_explicit_contradiction() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("zkr-profile-{nonce}.db"));
    let database = path.to_str().unwrap();
    let profile_claim = run(
        database,
        "remember",
        json!({
            "tenant_id": "tenant",
            "person_id": "person",
            "kind": "conversation",
            "text": "Sam works at Acme",
            "captured_at": 10,
            "recorded_at": 10,
            "claim": { "subject": "Sam", "predicate": "employer", "value": "Acme", "kind": "profile_fact", "valid_from": 10 }
        }),
    );
    let profile = run(
        database,
        "profile",
        json!({
            "tenant_id": "tenant",
            "person_id": "person",
            "stability": "current",
            "claim_id": profile_claim["claim_id"],
            "recorded_at": 11
        }),
    );
    assert_eq!(profile["key"], "employer");
    let contradiction = run(
        database,
        "remember",
        json!({
            "tenant_id": "tenant",
            "person_id": "person",
            "kind": "conversation",
            "text": "Sam left Acme",
            "captured_at": 12,
            "recorded_at": 12
        }),
    );
    assert_eq!(
        run(
            database,
            "link",
            json!({
                "tenant_id": "tenant",
                "person_id": "person",
                "claim_id": profile_claim["claim_id"],
                "evidence_id": contradiction["evidence_id"],
                "relation": "contradicts",
                "confidence_basis_points": 9000
            }),
        )["ok"],
        true
    );
    assert_eq!(
        run(
            database,
            "profiles",
            json!({ "tenant_id": "tenant", "person_id": "person", "limit": 10 }),
        )
        .as_array()
        .unwrap()
        .len(),
        1
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn json_cli_rejects_ingestion_key_conflicts_and_deleted_replays() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("zkr-idempotency-{nonce}.db"));
    let database = path.to_str().unwrap();
    let input = json!({
        "tenant_id": "tenant",
        "person_id": "person",
        "ingestion_key": "capture-1",
        "kind": "screen",
        "text": "Roadmap review is Thursday",
        "captured_at": 10,
        "recorded_at": 10
    });
    let remembered = run(database, "remember", input.clone());
    assert_eq!(run(database, "remember", input.clone()), remembered);
    let mut changed = input.clone();
    changed["text"] = json!("Changed replay content");
    assert!(run_failure(database, "remember", changed).contains("ingestion_key conflicts"));
    run(
        database,
        "delete",
        json!({
            "tenant_id": "tenant",
            "person_id": "person",
            "source_id": remembered["source_id"],
            "deleted_at": 20
        }),
    );
    assert!(run_failure(database, "remember", input).contains("record not found"));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn database_help_matches_the_documented_command() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let database = std::env::temp_dir().join(format!("zkr-help-{nonce}.db"));
    let output = Command::new(env!("CARGO_BIN_EXE_zkr"))
        .args(["--db", database.to_str().unwrap(), "help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("get"));
    assert!(help.contains("export"));
    assert!(!database.exists());
}

#[test]
fn json_cli_round_trips_transcript_evidence_locator() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("zkr-locator-{nonce}.db"));
    let database = path.to_str().unwrap();
    let input = json!({
        "tenant_id": "tenant",
        "person_id": "person",
        "ingestion_key": "transcript-1",
        "kind": "audio",
        "text": "Schedule the review tomorrow",
        "captured_at": 10,
        "recorded_at": 10,
        "locator": {
            "device_id": "omi-1",
            "provider": "deepgram",
            "stream_id": "stream-1",
            "segment_id": "segment-4",
            "start_ms": 1200,
            "end_ms": 2900
        }
    });
    let remembered = run(database, "remember", input.clone());
    assert_eq!(run(database, "remember", input), remembered);
    let locator = run(
        database,
        "locator",
        json!({
            "tenant_id": "tenant",
            "person_id": "person",
            "evidence_id": remembered["evidence_id"]
        }),
    );
    assert_eq!(locator["device_id"], "omi-1");
    assert_eq!(locator["provider"], "deepgram");
    assert_eq!(locator["start_ms"], 1200);
    assert_eq!(locator["end_ms"], 2900);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn json_cli_exports_a_frozen_scoped_commit_page() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("zkr-export-{nonce}.db"));
    let database = path.to_str().unwrap();
    run(
        database,
        "remember",
        json!({
            "tenant_id": "tenant",
            "person_id": "person",
            "kind": "conversation",
            "text": "Sam works at Acme",
            "captured_at": 10,
            "recorded_at": 11,
            "claim": { "subject": "Sam", "predicate": "employer", "value": "Acme", "kind": "fact", "valid_from": 10 }
        }),
    );
    let page = run(
        database,
        "export",
        json!({
            "export_format": 1,
            "tenant_id": "tenant",
            "person_id": "person",
            "after_commit": 0,
            "limit": 10
        }),
    );
    assert_eq!(page["complete"], true);
    assert_eq!(page["export_format"], 1);
    assert_eq!(page["high_water_mark"], page["next_after_commit"]);
    assert_eq!(page["commits"].as_array().unwrap().len(), 1);
    let records = page["commits"][0]["records"].as_array().unwrap();
    assert!(records.iter().any(|record| record["kind"] == "source"));
    assert!(records.iter().any(|record| record["kind"] == "evidence"));
    assert!(records.iter().any(|record| record["kind"] == "claim"));
    assert!(
        records
            .iter()
            .any(|record| record["kind"] == "claim_evidence")
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn json_cli_rejects_invalid_export_cursors() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("zkr-export-error-{nonce}.db"));
    let failure = run_failure(
        path.to_str().unwrap(),
        "export",
        json!({
            "export_format": 1,
            "tenant_id": "tenant",
            "person_id": "person",
            "after_commit": -1,
            "limit": 10
        }),
    );
    assert!(failure.contains("after_commit must not be negative"));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn json_cli_split_correction_requires_a_complete_commit_before_cursor_advance() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("zkr-export-correction-{nonce}.db"));
    let database = path.to_str().unwrap();
    let remembered = run(
        database,
        "remember",
        json!({
            "tenant_id": "tenant",
            "person_id": "person",
            "kind": "conversation",
            "text": "Sam works at Acme",
            "captured_at": 10,
            "recorded_at": 11,
            "claim": { "subject": "Sam", "predicate": "employer", "value": "Acme", "kind": "fact", "valid_from": 10 }
        }),
    );
    run(
        database,
        "correct",
        json!({
            "tenant_id": "tenant",
            "person_id": "person",
            "claim_id": remembered["claim_id"],
            "text": "Sam works at Beta",
            "value": "Beta",
            "valid_at": 20,
            "recorded_at": 21
        }),
    );

    let mut request_cursor = (0, -1);
    let mut applied_cursor = (0, -1);
    let mut high_water = None;
    let mut staged = BTreeMap::<i64, (i64, Vec<i64>, Vec<String>)>::new();
    let mut correction_sequence = None;
    loop {
        let page = run(
            database,
            "export",
            json!({
                "export_format": 1,
                "tenant_id": "tenant",
                "person_id": "person",
                "after_commit": request_cursor.0,
                "after_event_index": request_cursor.1,
                "high_water_mark": high_water,
                "limit": 1
            }),
        );
        high_water = Some(page["high_water_mark"].as_i64().unwrap());
        for commit in page["commits"].as_array().unwrap() {
            let sequence = commit["sequence"].as_i64().unwrap();
            let event_count = commit["event_count"].as_i64().unwrap();
            let first_index = commit["first_event_index"].as_i64().unwrap();
            let records = commit["records"].as_array().unwrap();
            let entry = staged
                .entry(sequence)
                .or_insert_with(|| (event_count, Vec::new(), Vec::new()));
            assert_eq!(entry.0, event_count);
            for (offset, record) in records.iter().enumerate() {
                entry.1.push(first_index + offset as i64);
                let kind = record["kind"].as_str().unwrap().to_owned();
                if kind == "correction" {
                    correction_sequence = Some(sequence);
                }
                entry.2.push(kind);
            }
            if entry.1.len() as i64 == event_count {
                assert_eq!(entry.1, (0..event_count).collect::<Vec<_>>());
                applied_cursor = (sequence, event_count - 1);
            } else if correction_sequence == Some(sequence) {
                assert!(applied_cursor.0 < sequence);
            }
        }
        request_cursor = (
            page["next_after_commit"].as_i64().unwrap(),
            page["next_after_event_index"].as_i64().unwrap(),
        );
        if page["complete"] == true {
            break;
        }
    }
    let correction_sequence = correction_sequence.unwrap();
    assert_eq!(applied_cursor.0, correction_sequence);
    assert!(
        staged[&correction_sequence]
            .2
            .contains(&"correction".to_owned())
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn json_cli_applies_an_exported_page_into_an_empty_replica() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let origin_path = std::env::temp_dir().join(format!("zkr-apply-origin-{nonce}.db"));
    let replica_path = std::env::temp_dir().join(format!("zkr-apply-replica-{nonce}.db"));
    let origin = origin_path.to_str().unwrap();
    let replica = replica_path.to_str().unwrap();
    run(
        origin,
        "remember",
        json!({
            "tenant_id": "tenant",
            "person_id": "person",
            "kind": "conversation",
            "text": "Sam works at Acme",
            "captured_at": 10,
            "recorded_at": 10,
            "claim": {
                "subject": "Sam",
                "predicate": "employer",
                "value": "Acme",
                "kind": "fact",
                "valid_from": 10
            }
        }),
    );
    let page = run(
        origin,
        "export",
        json!({
            "export_format": 1,
            "tenant_id": "tenant",
            "person_id": "person",
            "after_commit": 0,
            "limit": 100
        }),
    );
    assert_eq!(page["complete"], true);
    let request = json!({
        "export_format": 1,
        "database_schema_version": page["database_schema_version"],
        "tenant_id": "tenant",
        "person_id": "person",
        "commits": page["commits"]
    });
    let applied = run(replica, "apply", request.clone());
    assert_eq!(applied["commits_applied"], 1);
    assert_eq!(applied["records_applied"], 4);
    let replayed = run(replica, "apply", request);
    assert_eq!(replayed["records_applied"], 0);
    assert_eq!(replayed["records_skipped"], 4);
    assert_eq!(
        run(
            replica,
            "export",
            json!({
                "export_format": 1,
                "tenant_id": "tenant",
                "person_id": "person",
                "after_commit": 0,
                "limit": 100
            }),
        )["commits"],
        page["commits"]
    );
    let pack = run(
        replica,
        "search",
        json!({ "tenant_id": "tenant", "person_id": "person", "query": "Acme", "limit": 5 }),
    );
    assert!(!pack["items"].as_array().unwrap().is_empty());
    std::fs::remove_file(origin_path).unwrap();
    std::fs::remove_file(replica_path).unwrap();
}
