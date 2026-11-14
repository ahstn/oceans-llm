//! The attribution routes through the real router: ownership, per-entry outcomes, the
//! ownership check, the body limit and the report.

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, HeaderValue, Request, StatusCode, header::COOKIE},
};
use gateway_core::{ATTRIBUTION_MAX_BATCH_BYTES, AuthMode, GlobalRole, UserStatus};
use gateway_store::GatewayStore;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};
use tower::ServiceExt;
use uuid::Uuid;

use crate::http::{build_router, state::AppState, test_support::app_state};

const REPOSITORY: &str = "github.com/acme/widgets";
const SESSION_ID: &str = "claude-session-raw-7f3a";
const COMMIT_SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const USER_NAME: &str = "Ada Admin";

struct Gateway {
    router: Router,
    user_id: Uuid,
    email: String,
    cookie: HeaderMap,
    key: String,
    _directory: tempfile::TempDir,
}

struct Reply {
    status: StatusCode,
    body: Value,
    text: String,
}

impl Gateway {
    async fn new() -> Self {
        let (directory, state) = app_state().await;
        let (user_id, email, cookie) = signed_in_admin(&state).await;
        let mut gateway = Self {
            router: build_router(state, admin_ui::AdminUiConfig::default()),
            user_id,
            email,
            cookie,
            key: String::new(),
            _directory: directory,
        };
        gateway.key = gateway.create_attribution_key().await;
        gateway
    }

    fn scope(&self) -> String {
        format!("user:{}", self.user_id)
    }

    async fn send(&self, request: Request<Body>) -> Reply {
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("response");
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        Reply {
            status,
            body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
            text: String::from_utf8_lossy(&bytes).into_owned(),
        }
    }

    fn with_cookie(
        &self,
        mut request: axum::http::request::Builder,
    ) -> axum::http::request::Builder {
        for (name, value) in &self.cookie {
            request = request.header(name, value);
        }
        request
    }

    async fn create_attribution_key(&self) -> String {
        let body = json!({
            "name": "hooks",
            "owner_kind": "user",
            "owner_user_id": self.user_id.to_string(),
            "owner_team_id": null,
            "owner_service_account_id": null,
            "model_grant_mode": "all",
            "model_keys": [],
            "scopes": ["attribution:write"],
            "mcp_server_keys": null,
        });
        let request = self
            .with_cookie(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/api-keys"),
            )
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let reply = self.send(request).await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
        reply.body["data"]["raw_key"].as_str().unwrap().to_string()
    }

    async fn ownership(&self) -> Reply {
        self.send(
            Request::builder()
                .uri("/api/v1/me/ownership")
                .header("authorization", format!("Bearer {}", self.key))
                .body(Body::empty())
                .unwrap(),
        )
        .await
    }

    async fn post_raw(&self, body: Body, content_length: Option<usize>) -> Reply {
        let mut request = Request::builder()
            .method("POST")
            .uri("/api/v1/attribution/entries")
            .header("authorization", format!("Bearer {}", self.key))
            .header("content-type", "application/json");
        if let Some(length) = content_length {
            request = request.header("content-length", length);
        }
        self.send(request.body(body).unwrap()).await
    }

    async fn post_entries(&self, scope: &str, entries: Vec<Value>) -> Reply {
        let body = json!({"expected_ownership_scope": scope, "entries": entries});
        self.post_raw(Body::from(body.to_string()), None).await
    }

    async fn report(&self) -> Reply {
        let request = self
            .with_cookie(Request::builder().uri("/api/v1/admin/attribution/report?days=7"))
            .body(Body::empty())
            .unwrap();
        self.send(request).await
    }
}

async fn signed_in_admin(state: &AppState) -> (Uuid, String, HeaderMap) {
    let email = format!("{}@example.test", Uuid::new_v4());
    let user = state
        .store
        .create_identity_user(
            USER_NAME,
            &email,
            &email,
            GlobalRole::PlatformAdmin,
            AuthMode::Password,
            UserStatus::Active,
        )
        .await
        .expect("create user");
    let session_id = Uuid::new_v4();
    let token = format!("{session_id}.attribution-test-token");
    let token_hash = format!("{:x}", Sha256::digest(token.as_bytes()));
    let now = OffsetDateTime::now_utc();
    state
        .store
        .create_user_session(
            session_id,
            user.user_id,
            &token_hash,
            now + Duration::hours(1),
            now,
        )
        .await
        .expect("create session");
    let mut headers = HeaderMap::new();
    headers.insert(
        COOKIE,
        HeaderValue::from_str(&format!("ogw_session={token}")).unwrap(),
    );
    (user.user_id, email, headers)
}

fn entry(entry_id: &str, sequence: i64, payload: Value) -> Value {
    json!({
        "entry_id": entry_id,
        "installation_id": "3f1c7c1e-7d3c-4d8e-9a59-0d6f0f6f2a11",
        "sequence": sequence,
        "observed_at": OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
        "late_sequenced": false,
        "payload": payload,
    })
}

/// A Claude Code session that wrote lines 1-3, by the signed-in user.
fn note_payload(email: &str) -> Value {
    let metadata = json!({
        "schema_version": "authorship/3.0.0",
        "base_commit_sha": "x",
        "prompts": {},
        "sessions": {"s_b1d318a1ca4b3a": {
            "agent_id": {"tool": "claude", "id": SESSION_ID, "model": "claude-sonnet"},
            "human_author": format!("Ada <{email}>"),
        }},
    });
    json!({
        "kind": "note",
        "repository": REPOSITORY,
        "commit_sha": COMMIT_SHA,
        "branch": "main",
        "is_default_branch": true,
        "note_content": format!("src/lib.rs\n  s_b1d318a1ca4b3a::t_f577269eea4edb 1-3\n---\n{metadata}"),
    })
}

fn assert_private(reply: &Reply, email: &str) {
    assert!(!reply.text.contains(SESSION_ID), "{}", reply.text);
    assert!(!reply.text.contains(email), "{}", reply.text);
}

#[tokio::test]
async fn attribution_entries_report_one_outcome_each_and_reach_the_report() {
    let gateway = Gateway::new().await;
    let ownership = gateway.ownership().await;
    assert_eq!(ownership.status, StatusCode::OK, "{}", ownership.body);
    assert_eq!(
        ownership.body,
        json!({"ownership_scope": gateway.scope(), "owner_kind": "user", "owner_id": gateway.user_id.to_string()})
    );

    let session = json!({
        "kind": "session", "harness": "claude_code", "session_id": SESSION_ID, "event": "start",
        "repository": REPOSITORY, "branch": "main", "head_sha": null, "harness_version": "2.1.309",
    });
    let reply = gateway
        .post_entries(
            &gateway.scope(),
            vec![
                entry("note-1", 1, note_payload(&gateway.email)),
                entry("session-1", 2, session),
                json!({"entry_id": "broken-1", "payload": {"kind": "note"}}),
            ],
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(
        reply.body,
        json!({"ownership_scope": gateway.scope(), "results": [
            {"entry_id": "note-1", "status": "applied"},
            {"entry_id": "session-1", "status": "applied"},
            {"entry_id": "broken-1", "status": "rejected", "reason": "invalid_entry"},
        ]})
    );
    let replay = gateway
        .post_entries(
            &gateway.scope(),
            vec![entry("note-1", 1, note_payload(&gateway.email))],
        )
        .await;
    assert_eq!(
        replay.body["results"][0]["status"], "duplicate",
        "{}",
        replay.body
    );

    let report = gateway.report().await;
    assert_eq!(report.status, StatusCode::OK, "{}", report.body);
    let data = &report.body["data"];
    assert_eq!(data["totals"]["commit_count"], 1, "{}", report.body);
    assert_eq!(data["totals"]["ai_lines"], 3);
    assert_eq!(data["totals"]["agent_session_count"], 1);
    assert_eq!(data["changes"][0]["commit_sha"], COMMIT_SHA);
    assert_eq!(data["changes"][0]["author"]["name"], USER_NAME);
    assert_eq!(data["changes"][0]["owner"]["kind"], "user");
    assert_eq!(data["changes"][0]["agent_tools"], json!(["claude"]));
    assert_eq!(data["users"][0]["user"]["id"], gateway.user_id.to_string());
    assert_private(&reply, &gateway.email);
    assert_private(&report, &gateway.email);
}

#[tokio::test]
async fn attribution_batches_for_another_owner_are_refused_whole() {
    let gateway = Gateway::new().await;
    let other = format!("user:{}", Uuid::new_v4());
    let reply = gateway
        .post_entries(
            &other,
            vec![entry("note-1", 1, note_payload(&gateway.email))],
        )
        .await;
    assert_eq!(reply.status, StatusCode::CONFLICT, "{}", reply.body);
    assert_eq!(reply.body["error"]["code"], "conflict");
    assert!(
        !reply.text.contains(&gateway.user_id.to_string()),
        "{}",
        reply.text
    );
    let report = gateway.report().await;
    assert_eq!(
        report.body["data"]["totals"]["commit_count"], 0,
        "{}",
        report.body
    );
}

#[tokio::test]
async fn attribution_bodies_over_two_mebibytes_are_refused_before_parsing() {
    let gateway = Gateway::new().await;
    let padded = |len: usize| {
        let mut body = json!({"expected_ownership_scope": gateway.scope(), "entries": []})
            .to_string()
            .into_bytes();
        body.resize(len, b' ');
        body
    };
    let exact = padded(ATTRIBUTION_MAX_BATCH_BYTES);
    let reply = gateway
        .post_raw(Body::from(exact), Some(ATTRIBUTION_MAX_BATCH_BYTES))
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);

    let over = padded(ATTRIBUTION_MAX_BATCH_BYTES + 1);
    let declared = gateway
        .post_raw(Body::from(over.clone()), Some(over.len()))
        .await;
    assert_eq!(
        declared.status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "{}",
        declared.body
    );
    assert_eq!(declared.body["error"]["code"], "request_body_too_large");

    let chunks: Vec<Result<Vec<u8>, std::io::Error>> = over
        .chunks(64 * 1024)
        .map(|chunk| Ok(chunk.to_vec()))
        .collect();
    let chunked = gateway
        .post_raw(Body::from_stream(futures_util::stream::iter(chunks)), None)
        .await;
    assert_eq!(
        chunked.status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "{}",
        chunked.body
    );
}

#[tokio::test]
async fn attribution_report_rejects_unknown_windows() {
    let gateway = Gateway::new().await;
    let request = gateway
        .with_cookie(Request::builder().uri("/api/v1/admin/attribution/report?days=14"))
        .body(Body::empty())
        .unwrap();
    let reply = gateway.send(request).await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.body);
}
