use std::{fs, process::Command};

use serde_json::json;

use super::*;
use crate::attribution::{
    payload::{MAX_NOTE_BYTES, SessionEvent},
    queue::{Entry, read_entry},
};

const SHA_A: &str = "f6fb7a1c0d6f5e4b3a2918273645546372819a0b";
const SHA_B: &str = "0123456789abcdef0123456789abcdef01234567";

fn state(root: &tempfile::TempDir) -> State {
    State::open(root.path().join("attribution")).unwrap()
}

fn queued(state: &State) -> Vec<Entry> {
    state
        .queued_files()
        .unwrap()
        .iter()
        .map(|path| read_entry(path).unwrap())
        .collect()
}

fn git_note(sha: &str, repo_url: &str, content: &str) -> serde_json::Value {
    json!({
        "branch": "main",
        "commit_sha": sha,
        "is_default_branch": true,
        "note_content": content,
        "repo_name": "widgets",
        "repo_url": repo_url,
    })
}

#[test]
fn git_ai_notes_are_queued_with_normalised_repositories() {
    let root = tempfile::tempdir().unwrap();
    let state = state(&root);
    let input = json!([git_note(
        SHA_A,
        "https://user:tok@github.com/acme/widgets.git",
        "b.txt\n  s_b1d318a1ca4b3a::t_f577269eea4edb 1-3\n---\n{}"
    )]);
    let report = notes(&state, input.to_string().as_bytes()).unwrap();
    assert_eq!((report.queued, report.quarantined), (1, 0));
    let entries = queued(&state);
    let Payload::Note(note) = &entries[0].payload else {
        panic!("expected a note");
    };
    assert_eq!(note.repository, "github.com/acme/widgets");
    assert_eq!(note.commit_sha, SHA_A);
    assert_eq!(
        (note.branch.as_str(), note.is_default_branch),
        ("main", true)
    );
    let stored = fs::read_to_string(&state.queued_files().unwrap()[0]).unwrap();
    assert!(!stored.contains("tok@"), "{stored}");
}

#[test]
fn notes_the_gateway_would_refuse_go_to_quarantine() {
    let root = tempfile::tempdir().unwrap();
    let state = state(&root);
    let input = json!([
        git_note(SHA_A, "https://github.com/acme/widgets", &"x".repeat(MAX_NOTE_BYTES + 1)),
        git_note(SHA_B, "/srv/git/user:secret@widgets", "note"),
        git_note("not-a-sha", "git@github.com:acme/widgets.git", "note"),
        {"commit_sha": SHA_B},
        git_note(SHA_B, "", "note"),
    ]);
    let report = notes(&state, input.to_string().as_bytes()).unwrap();
    assert_eq!((report.queued, report.quarantined), (0, 4));
    let mut reasons: Vec<_> = state
        .listing()
        .unwrap()
        .quarantined
        .into_iter()
        .map(|summary| (summary.reason.unwrap(), summary.repository))
        .collect();
    reasons.sort();
    assert_eq!(
        reasons,
        [
            (
                "invalid_entry".to_string(),
                Some("github.com/acme/widgets".to_string())
            ),
            ("invalid_repository".to_string(), None),
            ("invalid_repository".to_string(), None),
            (
                "oversized_note".to_string(),
                Some("github.com/acme/widgets".to_string())
            ),
        ]
    );
    for file in fs::read_dir(state.dir("quarantine")).unwrap() {
        let stored = fs::read_to_string(file.unwrap().path()).unwrap();
        assert!(!stored.contains("secret"), "raw remotes are never stored");
    }
}

#[test]
fn notes_before_a_truncated_or_malformed_tail_are_kept() {
    let first = json!([git_note(SHA_A, "https://github.com/acme/widgets", "first")]).to_string();
    let truncated = format!(
        "{},{}",
        first.trim_end_matches(']'),
        json!(git_note(
            SHA_B,
            "https://github.com/acme/widgets",
            &"y".repeat(4096)
        ))
    );
    let notes = read_notes(truncated.as_bytes(), first.len() as u64 + 100);
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].commit_sha, SHA_A);
    assert!(read_notes("not json".as_bytes(), 1024).is_empty());
    assert!(read_notes("{}".as_bytes(), 1024).is_empty());
}

fn git(directory: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}");
}

#[test]
fn claude_session_events_record_the_repository_context() {
    let root = tempfile::tempdir().unwrap();
    let state = state(&root);
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "--quiet", "--initial-branch=main"]);
    git(
        repo.path(),
        &["remote", "add", "origin", "git@github.com:acme/widgets.git"],
    );
    let input = json!({
        "session_id": "0b9d3f8e-8a59-4f5e-9c61-3f1c1d2a7b10",
        "cwd": repo.path(),
        "hook_event_name": "SessionStart",
        "source": "startup",
    });
    let queued_session = claude(&state, input.to_string().as_bytes())
        .unwrap()
        .unwrap();
    assert_eq!(queued_session.event, SessionEvent::Start);
    assert!(queued_session.has_repository);
    let entries = queued(&state);
    let Payload::Session(session) = &entries[0].payload else {
        panic!("expected a session");
    };
    assert_eq!(session.harness, Harness::ClaudeCode);
    assert_eq!(
        session.repository.as_deref(),
        Some("github.com/acme/widgets")
    );
    assert_eq!(session.branch.as_deref(), Some("main"));
    assert_eq!(session.head_sha, None);
    assert_eq!(session.harness_version, None);

    let stop = json!({"session_id": "abc", "cwd": repo.path(), "hook_event_name": "Stop"});
    assert!(
        claude(&state, stop.to_string().as_bytes())
            .unwrap()
            .is_none()
    );
    assert_eq!(queued(&state).len(), 1);
}

#[test]
fn session_hooks_reject_bad_input_without_echoing_it() {
    let root = tempfile::tempdir().unwrap();
    let state = state(&root);
    let bad_id =
        json!({"session_id": "secret id with spaces", "cwd": "/", "hook_event_name": "SessionEnd"});
    let error = claude(&state, bad_id.to_string().as_bytes())
        .unwrap_err()
        .to_string();
    assert!(!error.contains("secret"), "{error}");
    let wrong_type = json!({"session_id": "s1", "cwd": "/", "event": "secret-event"});
    let error = omp(&state, wrong_type.to_string().as_bytes())
        .unwrap_err()
        .to_string();
    assert!(!error.contains("secret"), "{error}");
    assert!(queued(&state).is_empty());
}

#[test]
fn omp_session_events_carry_the_extension_version() {
    let root = tempfile::tempdir().unwrap();
    let state = state(&root);
    let outside = tempfile::tempdir().unwrap();
    let input = json!({
        "session_id": "omp-session:42",
        "cwd": outside.path(),
        "event": "end",
        "harness_version": "14.2.0",
    });
    let queued_session = omp(&state, input.to_string().as_bytes()).unwrap().unwrap();
    assert_eq!(queued_session.event, SessionEvent::End);
    assert!(!queued_session.has_repository);
    let Payload::Session(session) = &queued(&state)[0].payload else {
        panic!("expected a session");
    };
    assert_eq!(session.harness, Harness::OhMyPi);
    assert_eq!(session.session_id, "omp-session:42");
    assert_eq!(session.repository, None);
    assert_eq!(session.harness_version.as_deref(), Some("14.2.0"));
}

#[test]
fn request_mappings_use_the_existing_owned_queue_envelope() {
    let root = tempfile::tempdir().unwrap();
    let state = state(&root);
    let scope = "user:0b9d3f8e-8a59-4f5e-9c61-3f1c1d2a7b10";
    state
        .record_ownership(
            "https://oceans.example.com/",
            "key-fingerprint",
            &crate::attribution::ownership::Ownership {
                ownership_scope: scope.into(),
                owner_kind: "user".into(),
                owner_id: "0b9d3f8e-8a59-4f5e-9c61-3f1c1d2a7b10".into(),
            },
        )
        .unwrap();
    let input = json!({
        "request_id": "1c8f3d6a-5bf0-4e84-8dd2-81c5d27b4da1",
        "session_id": "journal:own-session.42",
    });
    assert_eq!(
        request(&state, input.to_string().as_bytes())
            .unwrap()
            .queued,
        1
    );
    let entries = queued(&state);
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry.ownership_scope.as_deref(), Some(scope));
    assert_eq!(entry.sequence, Some(1));
    assert_eq!(entry.payload.kind(), "request_session");
    assert_eq!(entry.payload.listing(), (None, None));
    assert_eq!(entry.payload.local_rejection(), None);
    let wire = serde_json::to_value(entry.wire().unwrap()).unwrap();
    assert_eq!(
        wire["payload"],
        json!({
            "kind": "request_session",
            "request_id": input["request_id"],
            "session_id": input["session_id"],
        })
    );
    assert!(wire.get("ownership_scope").is_none());
    let listing = serde_json::to_string(&state.listing().unwrap().queued).unwrap();
    assert!(!listing.contains("journal") && !listing.contains("1c8f3d6a"));
}

#[test]
fn request_mappings_reject_invalid_ids_without_echoing_input() {
    let root = tempfile::tempdir().unwrap();
    let state = state(&root);
    let valid_id = "1c8f3d6a-5bf0-4e84-8dd2-81c5d27b4da1";
    for input in [
        json!({"request_id": "secret-upstream-id", "session_id": "journal"}),
        json!({"request_id": "1c8f3d6a5bf04e848dd281c5d27b4da1", "session_id": "journal"}),
        json!({"request_id": format!("{valid_id}\n"), "session_id": "journal"}),
        json!({"request_id": valid_id, "session_id": "secret journal"}),
        json!({"request_id": valid_id, "session_id": "secret\n"}),
        json!({"request_id": valid_id, "session_id": "x".repeat(257)}),
        json!({"request_id": valid_id, "session_id": ""}),
        json!({"request_id": valid_id}),
        json!({"session_id": "journal"}),
        json!({"request_id": {"secret": true}, "session_id": "journal"}),
    ] {
        let error = request(&state, input.to_string().as_bytes())
            .unwrap_err()
            .to_string();
        assert!(
            !error.contains("secret") && !error.contains(valid_id),
            "{error}"
        );
    }
    assert!(queued(&state).is_empty());
    assert!(state.listing().unwrap().quarantined.is_empty());
}
