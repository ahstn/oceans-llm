use std::{collections::BTreeSet, path::PathBuf, sync::Arc, thread, time::Duration};

use super::*;
use crate::attribution::{
    ownership::Ownership,
    payload::{NotePayload, Payload},
};

const OWNER_ID: &str = "0b9d3f8e-8a59-4f5e-9c61-3f1c1d2a7b10";

fn state(root: &tempfile::TempDir) -> State {
    State::open(root.path().join("attribution")).unwrap()
}

fn note(digit: char, content: &str) -> Payload {
    Payload::Note(NotePayload {
        repository: "github.com/acme/widgets".into(),
        commit_sha: digit.to_string().repeat(40),
        branch: "main".into(),
        is_default_branch: true,
        note_content: content.into(),
    })
}

fn queued(state: &State) -> Vec<Entry> {
    state
        .queued_files()
        .unwrap()
        .iter()
        .map(|path| read_entry(path).unwrap())
        .collect()
}

fn owner() -> Ownership {
    Ownership {
        ownership_scope: format!("user:{OWNER_ID}"),
        owner_kind: "user".into(),
        owner_id: OWNER_ID.into(),
    }
}

#[test]
fn concurrent_hooks_get_unique_increasing_sequences() {
    let root = tempfile::tempdir().unwrap();
    let path: Arc<PathBuf> = Arc::new(root.path().join("attribution"));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let path = Arc::clone(&path);
            thread::spawn(move || {
                let state = State::open(path.to_path_buf()).unwrap();
                let payloads = (0..5).map(|_| note('a', "note")).collect();
                assert_eq!(state.enqueue(payloads).unwrap().queued, 5);
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    let state = state(&root);
    let entries = queued(&state);
    let sequences: Vec<u64> = entries
        .iter()
        .map(|entry| entry.sequence.unwrap())
        .collect();
    assert_eq!(sequences, (1..=40).collect::<Vec<_>>());
    let installations: BTreeSet<_> = entries
        .iter()
        .map(|entry| entry.installation_id.clone())
        .collect();
    assert_eq!(installations.len(), 1);
    assert!(entries.iter().all(|entry| !entry.late_sequenced));
    assert_eq!(state.read_counter().unwrap(), 40);
}

#[test]
fn a_busy_lock_defers_sequencing_to_the_sender() {
    let root = tempfile::tempdir().unwrap();
    let mut state = state(&root);
    state.enqueue(vec![note('a', "first")]).unwrap();
    state.lock_timeout = Duration::from_millis(20);
    let held = state.lock_queue(Duration::from_secs(1)).unwrap().unwrap();
    let report = state
        .enqueue(vec![note('b', "late"), note('c', "later")])
        .unwrap();
    assert_eq!((report.queued, report.unsequenced), (0, 2));
    assert_eq!(state.listing().unwrap().queued.len(), 3);
    assert_eq!(
        state
            .sequence_unsequenced(Duration::from_millis(20))
            .unwrap(),
        0
    );
    drop(held);

    assert_eq!(
        state.sequence_unsequenced(Duration::from_secs(1)).unwrap(),
        2
    );
    let entries = queued(&state);
    let order: Vec<_> = entries
        .iter()
        .map(|entry| (entry.sequence.unwrap(), entry.late_sequenced))
        .collect();
    assert_eq!(order, [(1, false), (2, true), (3, true)]);
    assert!(state.has_pending().unwrap());
}

#[test]
fn entries_past_the_cap_are_dropped_and_counted() {
    let root = tempfile::tempdir().unwrap();
    let mut state = state(&root);
    state.queue_cap_bytes = 1;
    let report = state.enqueue(vec![note('a', "x"), note('b', "y")]).unwrap();
    assert_eq!((report.queued, report.dropped), (0, 2));
    assert_eq!(state.dropped().unwrap(), 2);
    assert_eq!(state.listing().unwrap().dropped, 2);
}

#[test]
fn entries_carry_the_cached_ownership_scope() {
    let root = tempfile::tempdir().unwrap();
    let state = state(&root);
    state.enqueue(vec![note('a', "before login")]).unwrap();
    state
        .record_ownership("https://oceans.example.com/", "0123456789abcdef", &owner())
        .unwrap();
    state.enqueue(vec![note('b', "after login")]).unwrap();
    let scopes: Vec<_> = queued(&state)
        .into_iter()
        .map(|entry| entry.ownership_scope)
        .collect();
    assert_eq!(scopes, [None, Some(format!("user:{OWNER_ID}"))]);
    let wire = serde_json::to_value(queued(&state)[1].wire().unwrap()).unwrap();
    assert!(wire.get("ownership_scope").is_none());
    assert_eq!(wire["payload"]["kind"], "note");
    assert_eq!(wire["sequence"], 2);
}

#[test]
fn listings_show_kind_repository_commit_and_reason_only() {
    let root = tempfile::tempdir().unwrap();
    let state = state(&root);
    state
        .enqueue(vec![note('a', "queued secret note")])
        .unwrap();
    state
        .quarantine_payloads(vec![(
            note('b', "quarantined secret note"),
            "oversized_note",
        )])
        .unwrap();
    let listing = state.listing().unwrap();
    assert_eq!(
        listing.quarantined,
        [EntrySummary {
            kind: "note",
            repository: Some("github.com/acme/widgets".into()),
            commit: Some("b".repeat(40)),
            reason: Some("oversized_note".into()),
        }]
    );
    let json = serde_json::to_string(&listing).unwrap();
    assert!(!json.contains("secret"), "{json}");
    assert!(!json.contains(&state.installation_id().unwrap()), "{json}");
}

#[test]
fn resent_entries_keep_their_sequence_and_owner() {
    let root = tempfile::tempdir().unwrap();
    let state = state(&root);
    state
        .enqueue(vec![note('a', "one"), note('b', "two")])
        .unwrap();
    let path = state.queued_files().unwrap().remove(1);
    let entry = read_entry(&path).unwrap();
    state
        .quarantine_file(&path, entry.clone(), "invalid_note")
        .unwrap();
    assert_eq!(queued(&state).len(), 1);

    state
        .record_ownership("https://oceans.example.com/", "0123456789abcdef", &owner())
        .unwrap();
    assert_eq!(state.resend_quarantine().unwrap(), 1);
    let resent = queued(&state).pop().unwrap();
    assert_eq!(resent.sequence, Some(2));
    assert_eq!(resent.entry_id, entry.entry_id);
    assert_eq!(resent.ownership_scope, Some(format!("user:{OWNER_ID}")));
    assert!(state.listing().unwrap().quarantined.is_empty());

    state
        .quarantine_payloads(vec![(note('c', "three"), "oversized_note")])
        .unwrap();
    assert_eq!(state.discard_quarantine().unwrap(), 1);
    assert!(state.listing().unwrap().quarantined.is_empty());
}

#[test]
fn ownership_cache_tracks_lookups_per_gateway_and_key() {
    let root = tempfile::tempdir().unwrap();
    let state = state(&root);
    assert_eq!(state.current_scope(), None);
    state
        .record_ownership("https://a.example.com/", "aaaa", &owner())
        .unwrap();
    let other = Ownership {
        ownership_scope: "service_account:7d1c6a52-2a7e-4c55-9d0e-6f0b3c9a1e22".into(),
        owner_kind: "service_account".into(),
        owner_id: "7d1c6a52-2a7e-4c55-9d0e-6f0b3c9a1e22".into(),
    };
    state
        .record_ownership("https://a.example.com/", "bbbb", &other)
        .unwrap();
    assert_eq!(state.current_scope(), Some(other.ownership_scope.clone()));
    let first = state
        .cached_ownership("https://a.example.com/", "aaaa")
        .unwrap();
    state.make_current(first).unwrap();
    assert_eq!(state.current_scope(), Some(format!("user:{OWNER_ID}")));
    state
        .forget_ownership("https://a.example.com/", "aaaa")
        .unwrap();
    assert!(
        state
            .cached_ownership("https://a.example.com/", "aaaa")
            .is_none()
    );
    assert!(
        state
            .cached_ownership("https://a.example.com/", "bbbb")
            .is_some()
    );
}

#[cfg(unix)]
#[test]
fn state_directories_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let state = state(&root);
    state.enqueue(vec![note('a', "x")]).unwrap();
    for name in ["", QUEUE_DIR, UNSEQUENCED_DIR, QUARANTINE_DIR] {
        let mode = std::fs::metadata(state.dir(name))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700, "{name}");
    }
    let file = &state.queued_files().unwrap()[0];
    assert_eq!(
        std::fs::metadata(file).unwrap().permissions().mode() & 0o777,
        0o600
    );

    let shared = root.path().join("shared");
    std::fs::create_dir(&shared).unwrap();
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o755)).unwrap();
    let error = State::open(shared).err().unwrap().to_string();
    assert!(error.contains("chmod 700"), "{error}");
}
