use std::process::{Command, Stdio};

fn get_binary_path() -> String {
    env!("CARGO_BIN_EXE_zkr").to_string()
}

#[test]
fn test_no_arguments_prints_help() {
    let output = Command::new(get_binary_path())
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("zkr --db PATH COMMAND"));
    assert!(stdout.contains("Commands"));
}

#[test]
fn test_help_argument() {
    for arg in ["help", "--help", "-h"] {
        let output = Command::new(get_binary_path())
            .arg(arg)
            .output()
            .expect("Failed to execute command");

        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("zkr --db PATH COMMAND"));
        assert!(stdout.contains("Commands"));
    }
}

#[test]
fn test_db_help_argument() {
    let output = Command::new(get_binary_path())
        .args(["--db", "mock.db", "help"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("zkr --db PATH COMMAND"));
    assert!(stdout.contains("Commands"));
}

#[test]
fn test_invalid_arguments_error() {
    let output = Command::new(get_binary_path())
        .args(["foo", "bar"])
        .output()
        .expect("Failed to execute command");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains(r#"{"error":"usage: zkr --db PATH COMMAND (use --help)"}"#));
}

#[test]
fn test_unknown_command_error() {
    let temp_db = std::env::temp_dir().join(format!(
        "zkr-test-{}.db",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    let child = Command::new(get_binary_path())
        .args(["--db", temp_db.to_str().unwrap(), "unknown_command"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to execute command");

    // We don't need to write to stdin, wait for the error response.
    let output = child.wait_with_output().unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains(r#"{"error":"unknown command \"unknown_command\""}"#));

    if temp_db.exists() {
        std::fs::remove_file(temp_db).unwrap();
    }
}
