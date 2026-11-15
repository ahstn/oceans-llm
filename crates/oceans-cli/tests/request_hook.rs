use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

use serde_json::{Value, json};

fn hook(root: &Path, args: &[&str], input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_oceans"))
        .env_clear()
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_STATE_HOME", root.join("state"))
        // Both the detached sender and explicit send fail before any network access.
        .env("OCEANS_URL", "not-a-gateway-url")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn request_hook_accepts_the_mapping_and_sender_failure_is_fail_open() {
    let root = tempfile::tempdir().unwrap();
    let input = json!({
        "request_id": "1c8f3d6a-5bf0-4e84-8dd2-81c5d27b4da1",
        "session_id": "journal:own-session",
    });
    let output = hook(root.path(), &["hook", "request"], &input.to_string());
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());

    let queue = root.path().join("state/oceans/attribution/queue");
    let files: Vec<_> = fs::read_dir(&queue).unwrap().map(Result::unwrap).collect();
    assert_eq!(files.len(), 1);
    let entry: Value = serde_json::from_slice(&fs::read(files[0].path()).unwrap()).unwrap();
    assert_eq!(
        entry["payload"],
        json!({
            "kind": "request_session",
            "request_id": input["request_id"],
            "session_id": input["session_id"],
        })
    );
    assert_eq!(entry["sequence"], 1);
    assert!(entry["entry_id"].is_string() && entry["installation_id"].is_string());

    let output = hook(root.path(), &["hook", "send"], "");
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("1c8f3d6a"));
    assert!(
        files[0].path().exists(),
        "failed delivery must preserve the mapping"
    );
}

#[test]
fn invalid_request_input_never_fails_the_caller_or_echoes_the_ids() {
    let root = tempfile::tempdir().unwrap();
    for input in [
        r#"{"request_id":"secret-upstream-id","session_id":"journal"}"#,
        r#"{"request_id":"1c8f3d6a-5bf0-4e84-8dd2-81c5d27b4da1","session_id":"secret journal"}"#,
        r#"{"request_id":"secret-malformed-json""#,
    ] {
        let output = hook(root.path(), &["hook", "request"], input);
        assert!(output.status.success());
        assert!(output.stdout.is_empty());
        let errors = String::from_utf8_lossy(&output.stderr);
        assert!(
            !errors.contains("secret") && !errors.contains("1c8f3d6a"),
            "{errors}"
        );
    }
    assert_eq!(
        fs::read_dir(root.path().join("state/oceans/attribution/queue"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn request_hook_argument_errors_are_fail_open_after_global_options() {
    let root = tempfile::tempdir().unwrap();
    for args in [
        vec!["hook", "request", "--unknown"],
        vec![
            "--json",
            "hook",
            "--url=https://oceans.example.com",
            "request",
            "--unknown",
        ],
    ] {
        let output = hook(root.path(), &args, "");
        assert!(output.status.success());
        assert!(output.stdout.is_empty());
    }
}
