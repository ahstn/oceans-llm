use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Json, Router,
    extract::State as AxumState,
    http::StatusCode as AxumStatus,
    routing::{get, post},
};
use serde_json::{Value, json};
use url::Url;

use super::*;
use crate::attribution::{
    ownership::{Ownership, key_fingerprint},
    payload::{NotePayload, Payload},
};

const API_KEY: &str = "gwk_test.secret";
const OWNER_A: &str = "0b9d3f8e-8a59-4f5e-9c61-3f1c1d2a7b10";
const OWNER_B: &str = "7d1c6a52-2a7e-4c55-9d0e-6f0b3c9a1e22";

type Respond = dyn Fn(&Value) -> (AxumStatus, Value) + Send + Sync;

#[derive(Clone)]
struct Gateway {
    owner: &'static str,
    lookups: Arc<AtomicUsize>,
    batches: Arc<Mutex<Vec<Value>>>,
    respond: Arc<Respond>,
}

struct Server {
    url: Url,
    gateway: Gateway,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Server {
    fn connection(&self) -> anyhow::Result<Connection> {
        Ok(Connection {
            url: self.url.clone(),
            api_key: API_KEY.into(),
        })
    }

    fn batches(&self) -> Vec<Value> {
        self.gateway.batches.lock().unwrap().clone()
    }

    fn lookups(&self) -> usize {
        self.gateway.lookups.load(Ordering::SeqCst)
    }
}

async fn ownership(AxumState(gateway): AxumState<Gateway>) -> Json<Value> {
    gateway.lookups.fetch_add(1, Ordering::SeqCst);
    let owner = gateway.owner;
    Json(
        json!({"ownership_scope": format!("user:{owner}"), "owner_kind": "user", "owner_id": owner}),
    )
}

async fn entries(
    AxumState(gateway): AxumState<Gateway>,
    Json(body): Json<Value>,
) -> (AxumStatus, Json<Value>) {
    gateway.batches.lock().unwrap().push(body.clone());
    let (status, response) = (gateway.respond)(&body);
    (status, Json(response))
}

async fn server(
    owner: &'static str,
    respond: impl Fn(&Value) -> (AxumStatus, Value) + Send + Sync + 'static,
) -> Server {
    let gateway = Gateway {
        owner,
        lookups: Arc::default(),
        batches: Arc::default(),
        respond: Arc::new(respond),
    };
    let router = Router::new()
        .route("/api/v1/me/ownership", get(ownership))
        .route("/api/v1/attribution/entries", post(entries))
        .with_state(gateway.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    Server { url, gateway, task }
}

/// Answers every entry with `status(entry)`.
fn answer(
    status: impl Fn(&Value) -> &'static str + Send + Sync + 'static,
) -> impl Fn(&Value) -> (AxumStatus, Value) + Send + Sync + 'static {
    move |body: &Value| {
        let results: Vec<Value> = body["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| match status(entry) {
                "rejected" => json!({"entry_id": entry["entry_id"], "status": "rejected", "reason": "invalid_note"}),
                status => json!({"entry_id": entry["entry_id"], "status": status}),
            })
            .collect();
        (
            AxumStatus::OK,
            json!({"ownership_scope": body["expected_ownership_scope"], "results": results}),
        )
    }
}

fn options() -> SendOptions {
    SendOptions {
        backoff: vec![Duration::from_millis(1), Duration::from_millis(1)],
    }
}

fn owner(id: &str) -> Ownership {
    Ownership {
        ownership_scope: format!("user:{id}"),
        owner_kind: "user".into(),
        owner_id: id.into(),
    }
}

fn note(digit: char) -> Payload {
    Payload::Note(NotePayload {
        repository: "github.com/acme/widgets".into(),
        commit_sha: digit.to_string().repeat(40),
        branch: "main".into(),
        is_default_branch: true,
        note_content: format!("note {digit}"),
    })
}

fn sha_of(entry: &Value) -> char {
    entry["payload"]["commit_sha"]
        .as_str()
        .unwrap()
        .chars()
        .next()
        .unwrap()
}

/// A state whose entries are stamped for `stamp`, logged in to `server` as `cached` if given.
fn state_for(
    root: &tempfile::TempDir,
    server: &Server,
    stamp: &str,
    cached: Option<&str>,
) -> State {
    let state = State::open(root.path().join("attribution")).unwrap();
    state
        .record_ownership("https://elsewhere.example.com/", "0000", &owner(stamp))
        .unwrap();
    if let Some(cached) = cached {
        state
            .record_ownership(
                server.url.as_str(),
                &key_fingerprint(API_KEY),
                &owner(cached),
            )
            .unwrap();
    }
    state
}

fn quarantine_reasons(state: &State) -> Vec<String> {
    let mut reasons: Vec<_> = state
        .listing()
        .unwrap()
        .quarantined
        .into_iter()
        .filter_map(|summary| summary.reason)
        .collect();
    reasons.sort();
    reasons
}

#[tokio::test]
async fn entries_are_sent_in_sequence_order_and_settled_per_result() {
    let server = server(
        OWNER_A,
        answer(|entry| match sha_of(entry) {
            'b' => "duplicate",
            'c' => "rejected",
            _ => "applied",
        }),
    )
    .await;
    let root = tempfile::tempdir().unwrap();
    let state = state_for(&root, &server, OWNER_A, None);
    state
        .enqueue(vec![note('a'), note('b'), note('c')])
        .unwrap();

    let report = send(&state, || server.connection(), &options())
        .await
        .unwrap();
    assert_eq!(
        (report.delivered, report.quarantined, report.remaining),
        (2, 1, 0)
    );
    assert_eq!(report.stopped, None);
    assert_eq!(server.lookups(), 1);
    let batches = server.batches();
    assert_eq!(batches.len(), 1);
    assert_eq!(
        batches[0]["expected_ownership_scope"],
        format!("user:{OWNER_A}")
    );
    let sent = batches[0]["entries"].as_array().unwrap();
    let sequences: Vec<_> = sent
        .iter()
        .map(|entry| entry["sequence"].as_u64().unwrap())
        .collect();
    assert_eq!(sequences, [1, 2, 3]);
    assert!(
        sent.iter()
            .all(|entry| entry.get("ownership_scope").is_none())
    );
    assert_eq!(sent[0]["late_sequenced"], false);
    assert_eq!(quarantine_reasons(&state), ["invalid_note"]);

    // The lookup is cached per gateway and key.
    state.enqueue(vec![note('d')]).unwrap();
    send(&state, || server.connection(), &options())
        .await
        .unwrap();
    assert_eq!(server.lookups(), 1);
}

#[tokio::test]
async fn transient_failures_back_off_then_keep_the_queue_in_order() {
    let server = server(OWNER_A, |_: &Value| {
        (AxumStatus::SERVICE_UNAVAILABLE, json!({}))
    })
    .await;
    let root = tempfile::tempdir().unwrap();
    let state = state_for(&root, &server, OWNER_A, Some(OWNER_A));
    state.enqueue(vec![note('a'), note('b')]).unwrap();
    let before = state.queued_files().unwrap();

    let report = send(&state, || server.connection(), &options())
        .await
        .unwrap();
    assert_eq!(report.remaining, 2);
    assert!(report.stopped.unwrap().contains("503"));
    assert_eq!(server.batches().len(), 3, "one attempt plus two retries");
    assert_eq!(state.queued_files().unwrap(), before);
}

#[tokio::test]
async fn fatal_statuses_stop_without_touching_the_queue() {
    let server = server(OWNER_A, |_: &Value| (AxumStatus::UNAUTHORIZED, json!({}))).await;
    let root = tempfile::tempdir().unwrap();
    let state = state_for(&root, &server, OWNER_A, Some(OWNER_A));
    state.enqueue(vec![note('a')]).unwrap();
    let report = send(&state, || server.connection(), &options())
        .await
        .unwrap();
    assert!(report.stopped.unwrap().contains("401"));
    assert_eq!((report.remaining, server.batches().len()), (1, 1));
}

#[tokio::test]
async fn entries_for_another_owner_are_quarantined_locally() {
    let server = server(OWNER_B, answer(|_| "applied")).await;
    let root = tempfile::tempdir().unwrap();
    let state = State::open(root.path().join("attribution")).unwrap();
    state.enqueue(vec![note('a')]).unwrap();
    state
        .record_ownership("https://elsewhere.example.com/", "0000", &owner(OWNER_A))
        .unwrap();
    state.enqueue(vec![note('b')]).unwrap();
    state
        .record_ownership(
            server.url.as_str(),
            &key_fingerprint(API_KEY),
            &owner(OWNER_B),
        )
        .unwrap();
    state.enqueue(vec![note('c')]).unwrap();

    let report = send(&state, || server.connection(), &options())
        .await
        .unwrap();
    assert_eq!((report.delivered, report.quarantined), (1, 2));
    let batches = server.batches();
    let sent: Vec<_> = batches[0]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(sha_of)
        .collect();
    assert_eq!(sent, ['c']);
    assert_eq!(
        quarantine_reasons(&state),
        ["missing_ownership_scope", "ownership_scope_mismatch"]
    );
}

#[tokio::test]
async fn an_owner_change_quarantines_the_batch_and_looks_the_owner_up_again() {
    // The cached owner is A; the gateway now reports B for the key.
    let server = server(OWNER_B, |body: &Value| {
        if body["expected_ownership_scope"] == format!("user:{OWNER_A}") {
            (AxumStatus::CONFLICT, json!({}))
        } else {
            answer(|_| "applied")(body)
        }
    })
    .await;
    let root = tempfile::tempdir().unwrap();
    let state = state_for(&root, &server, OWNER_A, Some(OWNER_A));
    state.enqueue(vec![note('a'), note('b')]).unwrap();

    let report = send(&state, || server.connection(), &options())
        .await
        .unwrap();
    assert_eq!((report.quarantined, report.remaining), (2, 0));
    assert_eq!(report.stopped, None);
    assert_eq!(
        quarantine_reasons(&state),
        ["owner_changed", "owner_changed"]
    );
    assert_eq!(
        server.lookups(),
        1,
        "the cached owner was dropped and looked up again"
    );
    assert_eq!(state.current_scope(), Some(format!("user:{OWNER_B}")));

    // Resent entries keep their owner, so they wait for that owner's key.
    assert_eq!(state.resend_quarantine().unwrap(), 2);
    let report = send(&state, || server.connection(), &options())
        .await
        .unwrap();
    assert_eq!(
        quarantine_reasons(&state),
        ["ownership_scope_mismatch", "ownership_scope_mismatch"]
    );
    assert_eq!(report.delivered, 0);
}

#[tokio::test]
async fn a_refused_batch_is_split_to_find_the_bad_entry() {
    let server = server(OWNER_A, |body: &Value| {
        let entries = body["entries"].as_array().unwrap();
        if entries.len() > 1 || sha_of(&entries[0]) == 'b' {
            (AxumStatus::UNPROCESSABLE_ENTITY, json!({}))
        } else {
            answer(|_| "applied")(body)
        }
    })
    .await;
    let root = tempfile::tempdir().unwrap();
    let state = state_for(&root, &server, OWNER_A, Some(OWNER_A));
    state
        .enqueue(vec![note('a'), note('b'), note('c')])
        .unwrap();

    let report = send(&state, || server.connection(), &options())
        .await
        .unwrap();
    assert_eq!(
        (report.delivered, report.quarantined, report.remaining),
        (2, 1, 0)
    );
    assert_eq!(quarantine_reasons(&state), ["invalid_entry"]);
    let sizes: Vec<_> = server
        .batches()
        .iter()
        .map(|batch| batch["entries"].as_array().unwrap().len())
        .collect();
    assert_eq!(sizes, [3, 1, 1, 1]);
}

#[tokio::test]
async fn a_running_sender_is_left_alone() {
    let root = tempfile::tempdir().unwrap();
    let state = State::open(root.path().join("attribution")).unwrap();
    state.enqueue(vec![note('a')]).unwrap();
    let _held = state.try_lock_sender().unwrap().unwrap();
    let report = send(&state, || panic!("must not resolve the key"), &options())
        .await
        .unwrap();
    assert!(report.already_running);
    assert_eq!(report.remaining, 1);
}

#[tokio::test]
async fn an_empty_queue_never_resolves_the_key() {
    let root = tempfile::tempdir().unwrap();
    let state = State::open(root.path().join("attribution")).unwrap();
    let report = send(&state, || panic!("must not resolve the key"), &options())
        .await
        .unwrap();
    assert_eq!(report, SendReport::default());
}
