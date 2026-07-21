use serde_json::{Value, json};
use std::{
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
    assert!(String::from_utf8(output.stdout).unwrap().contains("get"));
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
