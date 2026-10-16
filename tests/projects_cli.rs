use serde_json::{Value, json};
use std::io::Write;
use std::process::{Command, Stdio};

fn run(database: &str, command: &str, payload: Value) -> Value {
    let mut process = Command::new(env!("CARGO_BIN_EXE_zkr"))
        .args(["--db", database, command])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    process
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let output = process.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn project_commands_store_dynamic_categories_and_connection_records() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("projects.db");
    let database = path.to_str().unwrap();
    let claim = json!({"predicate":"task:launch:status", "value":"planned", "kind":"task", "valid_from":10});
    let remembered = run(
        database,
        "remember-project",
        json!({
            "tenant_id":"local", "person_id":"me", "project":"Tofu", "category":"launch",
            "kind":"conversation", "text":"Tofu launch is planned", "captured_at":10, "recorded_at":10, "claim":claim,
        }),
    );
    let query =
        json!({"tenant_id":"local", "person_id":"me", "project":"Tofu", "category":"launch"});
    let facts = run(database, "project-memories", query.clone());
    assert_eq!(facts.as_array().unwrap().len(), 1);
    assert_eq!(facts[0]["evidence_ids"][0], remembered["evidence_id"]);
    assert_eq!(facts[0]["value"], "planned");
    let record = json!({"record_type":"work_item", "fields":{"state":"blocked", "owner":"me"}, "connections":[{"relation":"blocked_by", "target":"cupboard:pairing"}]});
    run(
        database,
        "remember-project-record",
        json!({
            "tenant_id":"local", "person_id":"me", "project":"Tofu", "category":"AI-defined/connectors",
            "kind":"conversation", "text":"Launch is blocked by Cupboard pairing", "captured_at":11, "recorded_at":11,
            "claim":{"predicate":"task:pairing", "kind":"task", "valid_from":11, "tier":"short_term"}, "record":record,
        }),
    );
    let mut other = query;
    other["category"] = json!("AI-defined/connectors");
    let records = run(database, "project-memories", other.clone());
    assert_eq!(records[0]["record"], record);
    assert_eq!(
        serde_json::from_str::<Value>(records[0]["value"].as_str().unwrap()).unwrap(),
        record
    );
    let by_target = run(
        database,
        "project-memories",
        json!({
            "tenant_id":"local", "person_id":"me",
            "record_type":"work_item", "relation":"blocked_by", "target":"cupboard:pairing"
        }),
    );
    assert_eq!(by_target.as_array().unwrap().len(), 1);
    assert_eq!(by_target[0]["predicate"], "task:pairing");

    let dream = run(
        database,
        "dream",
        json!({"tenant_id":"local", "person_id":"me"}),
    );
    assert_eq!(dream["unresolved_targets"].as_array().unwrap().len(), 1);
    assert_eq!(
        dream["unresolved_targets"][0]["target"],
        json!("cupboard:pairing")
    );
    assert_eq!(dream["short_term_claims"].as_array().unwrap().len(), 1);
    assert_eq!(
        dream["unsummarized_evidence_ids"].as_array().unwrap().len(),
        2
    );

    other["person_id"] = json!("someone_else");
    assert!(
        run(database, "project-memories", other)
            .as_array()
            .unwrap()
            .is_empty()
    );
}
