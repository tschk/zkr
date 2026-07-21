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
            "claim": { "subject": "Sam", "predicate": "employer", "value": "Acme", "valid_from": 10 }
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
            "captured_at": 10
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
