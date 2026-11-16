use std::{collections::BTreeMap, io::Read, sync::Arc};

use axum::{
    Form, Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
};
use serde_json::json;
use tokio::sync::{Mutex, Semaphore};

use super::*;

const FIXTURE: &str = include_str!("testdata/credential.json");

struct MockState {
    requests: Mutex<Vec<BTreeMap<String, String>>>,
    started: Semaphore,
    release: Option<Semaphore>,
    status: StatusCode,
    body: Value,
}

struct MockServer {
    url: String,
    state: Arc<MockState>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl MockServer {
    async fn start(status: StatusCode, body: Value, paused: bool) -> Self {
        async fn handler(
            State(state): State<Arc<MockState>>,
            headers: HeaderMap,
            Form(form): Form<BTreeMap<String, String>>,
        ) -> (StatusCode, Json<Value>) {
            assert!(
                headers["content-type"]
                    .to_str()
                    .unwrap()
                    .starts_with("application/x-www-form-urlencoded")
            );
            for header in [
                "authorization",
                "chatgpt-account-id",
                "originator",
                "openai-beta",
            ] {
                assert!(!headers.contains_key(header));
            }
            state.requests.lock().await.push(form);
            state.started.add_permits(1);
            if let Some(release) = &state.release {
                release.acquire().await.unwrap().forget();
            }
            (state.status, Json(state.body.clone()))
        }
        let state = Arc::new(MockState {
            requests: Mutex::new(Vec::new()),
            started: Semaphore::new(0),
            release: paused.then(|| Semaphore::new(0)),
            status,
            body,
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/token", listener.local_addr().unwrap());
        let router = Router::new()
            .route("/token", post(handler))
            .with_state(Arc::clone(&state));
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self { url, state, task }
    }

    fn source(&self, path: &Path) -> ChatGptPlanTokenSource {
        ChatGptPlanTokenSource::with_token_url(path.to_path_buf(), self.url.clone())
    }

    async fn wait_started(&self) {
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.state.started.acquire(),
        )
        .await
        .unwrap()
        .unwrap()
        .forget();
    }
}

fn fresh_response() -> Value {
    json!({
        "access_token": "fake-rotated-access",
        "refresh_token": "fake-rotated-refresh",
        "id_token": "fake-rotated-id",
        "token_type": "Bearer",
        "expires_in": 3600,
        "scope": "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct",
        "earliest_refresh_at": "ignored-server-extension"
    })
}

fn fixture_value(remaining_seconds: i64) -> Value {
    let mut value: Value = serde_json::from_str(FIXTURE).unwrap();
    value["saved_at"] = json!(
        (OffsetDateTime::now_utc() - Duration::seconds(3600 - remaining_seconds))
            .format(&Rfc3339)
            .unwrap()
    );
    value
}

fn save_fixture(directory: &tempfile::TempDir, remaining_seconds: i64) -> PathBuf {
    let path = directory.path().join("credential.json");
    fs::write(
        &path,
        serde_json::to_vec(&fixture_value(remaining_seconds)).unwrap(),
    )
    .unwrap();
    path
}

fn saved_value(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn fixture_uses_exact_shared_siwc_schema() {
    let value: Value = serde_json::from_str(FIXTURE).unwrap();
    let mut keys: Vec<_> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "access_token",
            "client_id",
            "email",
            "expires_in",
            "ext_agent_host_id",
            "id_token",
            "issuer",
            "refresh_token",
            "saved_at",
            "scopes",
            "subject",
            "token_type",
        ]
    );
    let credentials: Credentials = serde_json::from_value(value).unwrap();
    assert!(credentials.extra.is_empty());
    assert!(credentials.email.ends_with(".invalid"));
    assert!(credentials.access_token.starts_with("fake-"));
    credentials.token().unwrap();
}

#[tokio::test]
async fn fresh_credentials_do_not_contact_token_endpoint() {
    let server = MockServer::start(StatusCode::OK, fresh_response(), false).await;
    let directory = tempfile::tempdir().unwrap();
    let path = save_fixture(&directory, 301);
    let before = fs::read(&path).unwrap();
    let source = server.source(&path);
    for _ in 0..2 {
        let token = source.fetch_token().await.unwrap();
        assert_eq!(token.token, "fake-access-token-not-a-credential");
        assert!(token.is_usable(REFRESH_BEFORE));
    }
    assert!(server.state.requests.lock().await.is_empty());
    assert_eq!(
        source
            .credential_reads
            .load(std::sync::atomic::Ordering::Relaxed),
        1,
        "repeated fresh fetches must reuse the parsed token"
    );
    assert_eq!(fs::read(path).unwrap(), before);
}

#[tokio::test]
async fn fresh_cached_fetch_does_not_wait_for_the_refresh_lock() {
    let server = MockServer::start(StatusCode::OK, fresh_response(), false).await;
    let directory = tempfile::tempdir().unwrap();
    let path = save_fixture(&directory, 301);
    let source = server.source(&path);
    source.fetch_token().await.unwrap();
    let _refresh_guard = source.refreshing.lock().await;
    let token = tokio::time::timeout(std::time::Duration::from_secs(5), source.fetch_token())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(token.token, "fake-access-token-not-a-credential");
    assert!(server.state.requests.lock().await.is_empty());
}

#[tokio::test]
async fn external_atomic_login_replacement_invalidates_cached_metadata() {
    let server = MockServer::start(StatusCode::OK, fresh_response(), false).await;
    let directory = tempfile::tempdir().unwrap();
    let path = save_fixture(&directory, 301);
    let source = server.source(&path);
    source.fetch_token().await.unwrap();
    for (index, preserve_modified) in [false, true].into_iter().enumerate() {
        let before = fs::metadata(&path).unwrap();
        let mut replacement = saved_value(&path);
        let previous = replacement["access_token"].as_str().unwrap();
        let token = if preserve_modified {
            format!("{previous}-external-login")
        } else {
            previous.replace("access", "loginx")
        };
        replacement["access_token"] = json!(token);
        let temporary = directory.path().join("external-login.json");
        fs::write(&temporary, serde_json::to_vec(&replacement).unwrap()).unwrap();
        let modified = before.modified().unwrap()
            + if preserve_modified {
                std::time::Duration::ZERO
            } else {
                std::time::Duration::from_secs(1)
            };
        File::options()
            .write(true)
            .open(&temporary)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(modified))
            .unwrap();
        fs::rename(&temporary, &path).unwrap();
        let after = fs::metadata(&path).unwrap();
        assert_eq!(after.modified().unwrap(), modified);
        if preserve_modified {
            assert_ne!(after.len(), before.len());
        } else {
            assert_eq!(after.len(), before.len());
        }
        assert_eq!(source.fetch_token().await.unwrap().token, token);
        assert_eq!(
            source
                .credential_reads
                .load(std::sync::atomic::Ordering::Relaxed),
            index + 2
        );
    }
    assert!(server.state.requests.lock().await.is_empty());
}

#[tokio::test]
async fn near_expiry_cached_token_rereads_even_when_metadata_is_unchanged() {
    let server = MockServer::start(StatusCode::OK, fresh_response(), false).await;
    let directory = tempfile::tempdir().unwrap();
    let path = save_fixture(&directory, 301);
    let source = server.source(&path);
    source.fetch_token().await.unwrap();
    // Advance only the cached expiry instead of waiting for wall-clock time.
    // The on-disk metadata stays identical and the fresh disk token wins.
    source
        .cached
        .lock()
        .await
        .as_mut()
        .unwrap()
        .token
        .expires_at = OffsetDateTime::now_utc() + Duration::seconds(239);
    let token = source.fetch_token().await.unwrap();
    assert!(token.is_usable(REFRESH_BEFORE));
    assert_eq!(
        source
            .credential_reads
            .load(std::sync::atomic::Ordering::Relaxed),
        2,
        "near-expiry tokens must be reread under the process lock"
    );
    assert!(server.state.requests.lock().await.is_empty());
}

#[tokio::test]
async fn near_expiry_rotates_atomically_and_preserves_registration_and_extensions() {
    let server = MockServer::start(StatusCode::OK, fresh_response(), false).await;
    let directory = tempfile::tempdir().unwrap();
    let path = save_fixture(&directory, 239);
    let mut before = saved_value(&path);
    before["future_field"] = json!({"nested": [1, "preserved", true]});
    fs::write(&path, serde_json::to_vec(&before).unwrap()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    }
    let mut old_file = File::open(&path).unwrap();
    let token = server.source(&path).fetch_token().await.unwrap();
    assert_eq!(token.token, "fake-rotated-access");
    assert!(token.is_usable(Duration::minutes(59)));
    let after = saved_value(&path);
    for field in [
        "email",
        "issuer",
        "subject",
        "client_id",
        "ext_agent_host_id",
        "future_field",
    ] {
        assert_eq!(after[field], before[field], "field {field} changed");
    }
    assert_eq!(after["access_token"], "fake-rotated-access");
    assert_eq!(after["refresh_token"], "fake-rotated-refresh");
    assert_eq!(after["id_token"], "fake-rotated-id");
    assert_eq!(after["token_type"], "Bearer");
    assert_eq!(after["expires_in"], 3600);
    assert_eq!(
        after["scopes"],
        json!([
            "openid",
            "profile",
            "email",
            "offline_access",
            "resource.invoke",
            DIRECT_SCOPE
        ])
    );
    let saved_at = OffsetDateTime::parse(after["saved_at"].as_str().unwrap(), &Rfc3339).unwrap();
    assert!((OffsetDateTime::now_utc() - saved_at).abs() < Duration::seconds(10));
    let mut old_contents = String::new();
    old_file.read_to_string(&mut old_contents).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&old_contents).unwrap(),
        before
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        assert_ne!(
            old_file.metadata().unwrap().ino(),
            fs::metadata(&path).unwrap().ino()
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert_eq!(
        fs::read_dir(directory.path()).unwrap().count(),
        2,
        "only credentials and the persistent lock remain"
    );
    let requests = server.state.requests.lock().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0],
        BTreeMap::from([
            ("grant_type".into(), "refresh_token".into()),
            (
                "client_id".into(),
                "oaiapp_fixture_not_a_real_client".into()
            ),
            (
                "refresh_token".into(),
                "fake-refresh-token-not-a-credential".into()
            ),
            ("resource".into(), RESOURCE.into()),
        ])
    );
}

#[tokio::test]
async fn subsequent_refresh_uses_latest_persisted_rotating_token() {
    let server = MockServer::start(StatusCode::OK, fresh_response(), false).await;
    let directory = tempfile::tempdir().unwrap();
    let path = save_fixture(&directory, -1);
    let source = server.source(&path);
    source.fetch_token().await.unwrap();
    let mut after = saved_value(&path);
    after["saved_at"] = fixture_value(-1)["saved_at"].clone();
    fs::write(&path, serde_json::to_vec(&after).unwrap()).unwrap();
    source.fetch_token().await.unwrap();
    let requests = server.state.requests.lock().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1]["refresh_token"], "fake-rotated-refresh");
    assert!(!requests[1].contains_key("scope"));
}

#[tokio::test]
async fn concurrent_callers_and_independent_sources_share_one_rotation() {
    let server = MockServer::start(StatusCode::OK, fresh_response(), true).await;
    let directory = tempfile::tempdir().unwrap();
    let path = save_fixture(&directory, 120);
    let shared = Arc::new(server.source(&path));
    let mut tasks = tokio::task::JoinSet::new();
    for index in 0..16 {
        let source = if index % 2 == 0 {
            Arc::clone(&shared)
        } else {
            Arc::new(server.source(&path))
        };
        tasks.spawn(async move { source.fetch_token().await });
    }
    server.wait_started().await;
    server.state.release.as_ref().unwrap().add_permits(16);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(result) = tasks.join_next().await {
            assert_eq!(result.unwrap().unwrap().token, "fake-rotated-access");
        }
    })
    .await
    .unwrap();
    assert_eq!(server.state.requests.lock().await.len(), 1);
}

#[tokio::test]
async fn cancelling_request_does_not_cancel_refresh_or_lose_rotated_credentials() {
    let server = MockServer::start(StatusCode::OK, fresh_response(), true).await;
    let directory = tempfile::tempdir().unwrap();
    let path = save_fixture(&directory, 10);
    let source = Arc::new(server.source(&path));
    let requesting_source = Arc::clone(&source);
    let request = tokio::spawn(async move { requesting_source.fetch_token().await });
    server.wait_started().await;
    request.abort();
    assert!(request.await.unwrap_err().is_cancelled());
    server.state.release.as_ref().unwrap().add_permits(1);
    let token = tokio::time::timeout(std::time::Duration::from_secs(5), source.fetch_token())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(token.token, "fake-rotated-access");
    assert_eq!(saved_value(&path)["refresh_token"], "fake-rotated-refresh");
    assert_eq!(server.state.requests.lock().await.len(), 1);
}

#[tokio::test]
async fn terminal_refresh_errors_clear_only_tokens_and_require_login_without_retry() {
    for (index, code) in [
        "invalid_grant",
        "invalid_refresh_token",
        "token_expired",
        "refresh_token_expired",
        "refresh_token_invalidated",
        "refresh_token_reused",
    ]
    .into_iter()
    .enumerate()
    {
        let body = if index % 2 == 0 {
            json!({"error": code})
        } else {
            json!({"error": {"code": code}})
        };
        let server = MockServer::start(StatusCode::BAD_REQUEST, body, false).await;
        let directory = tempfile::tempdir().unwrap();
        let path = save_fixture(&directory, -1);
        let mut before = saved_value(&path);
        before["custom"] = json!({"retain": true});
        fs::write(&path, serde_json::to_vec(&before).unwrap()).unwrap();
        let source = server.source(&path);
        let error = source.fetch_token().await.unwrap_err().to_string();
        assert!(error.contains(code));
        assert!(error.contains("oceans providers login-chatgpt"));
        let after = saved_value(&path);
        for field in ["access_token", "refresh_token"] {
            assert_eq!(after[field], "");
        }
        for field in [
            "id_token",
            "token_type",
            "saved_at",
            "client_id",
            "subject",
            "email",
            "issuer",
            "ext_agent_host_id",
            "scopes",
            "custom",
        ] {
            assert_eq!(after[field], before[field]);
        }
        assert_eq!(after["expires_in"], 0);
        assert!(
            source
                .fetch_token()
                .await
                .unwrap_err()
                .to_string()
                .contains("oceans providers login-chatgpt")
        );
        assert_eq!(server.state.requests.lock().await.len(), 1);
    }
}

#[tokio::test]
async fn invalid_client_is_a_configuration_error_and_does_not_clear_credentials() {
    let server = MockServer::start(
        StatusCode::UNAUTHORIZED,
        json!({"error": "invalid_client"}),
        false,
    )
    .await;
    let directory = tempfile::tempdir().unwrap();
    let path = save_fixture(&directory, 1);
    let before = fs::read(&path).unwrap();
    let error = server.source(&path).fetch_token().await.unwrap_err();
    assert!(matches!(error, ProviderError::InvalidRequest(_)));
    let message = error.to_string();
    assert!(message.contains("invalid_client"));
    assert!(message.contains("configuration"));
    assert!(!message.contains("login-chatgpt"));
    assert_eq!(fs::read(path).unwrap(), before);
    assert_eq!(server.state.requests.lock().await.len(), 1);
}

#[tokio::test]
async fn temporary_and_malformed_failures_preserve_credentials_and_never_retry_post() {
    for (status, body) in [
        (
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"error": "temporarily_unavailable", "secret": "must-not-leak"}),
        ),
        (StatusCode::OK, json!({"access_token": "must-not-leak"})),
    ] {
        let server = MockServer::start(status, body, false).await;
        let directory = tempfile::tempdir().unwrap();
        let path = save_fixture(&directory, 1);
        let before = fs::read(&path).unwrap();
        let error = server
            .source(&path)
            .fetch_token()
            .await
            .unwrap_err()
            .to_string();
        assert!(!error.contains("must-not-leak"));
        assert_eq!(fs::read(path).unwrap(), before);
        assert_eq!(server.state.requests.lock().await.len(), 1);
    }
}

#[tokio::test]
async fn missing_direct_scope_rejects_fresh_and_expiring_tokens_before_http() {
    for remaining in [3600, -1] {
        let server = MockServer::start(StatusCode::OK, fresh_response(), false).await;
        let directory = tempfile::tempdir().unwrap();
        let path = save_fixture(&directory, remaining);
        let mut value = saved_value(&path);
        value["scopes"] = json!(["openid", "email", "offline_access", "resource.invoke"]);
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let error = server
            .source(&path)
            .fetch_token()
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains(DIRECT_SCOPE));
        assert!(error.contains("oceans providers login-chatgpt"));
        assert!(server.state.requests.lock().await.is_empty());
    }
}

#[tokio::test]
async fn revoked_direct_scope_is_persisted_with_rotated_tokens_before_rejecting_inference() {
    let mut response = fresh_response();
    response["scope"] = json!("openid email offline_access resource.invoke");
    let server = MockServer::start(StatusCode::OK, response, false).await;
    let directory = tempfile::tempdir().unwrap();
    let path = save_fixture(&directory, 1);
    let source = server.source(&path);
    let error = source.fetch_token().await.unwrap_err().to_string();
    assert!(error.contains(DIRECT_SCOPE));
    let value = saved_value(&path);
    assert_eq!(value["refresh_token"], "fake-rotated-refresh");
    assert_eq!(value["access_token"], "fake-rotated-access");
    assert_eq!(
        value["scopes"],
        json!(["openid", "email", "offline_access", "resource.invoke"])
    );
    assert!(source.fetch_token().await.is_err());
    assert_eq!(server.state.requests.lock().await.len(), 1);
}

#[tokio::test]
async fn omitted_refresh_scope_and_id_token_retain_previous_values() {
    let mut response = fresh_response();
    response.as_object_mut().unwrap().remove("scope");
    response.as_object_mut().unwrap().remove("id_token");
    response.as_object_mut().unwrap().remove("token_type");
    let server = MockServer::start(StatusCode::OK, response, false).await;
    let directory = tempfile::tempdir().unwrap();
    let path = save_fixture(&directory, 1);
    let before = saved_value(&path);
    server.source(&path).fetch_token().await.unwrap();
    let after = saved_value(&path);
    for field in ["scopes", "id_token", "token_type"] {
        assert_eq!(after[field], before[field]);
    }
}

#[test]
fn failed_atomic_rename_removes_temporary_file_without_modifying_destination() {
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("existing-directory");
    fs::create_dir(&destination).unwrap();
    let credentials: Credentials = serde_json::from_str(FIXTURE).unwrap();
    assert!(write_credentials(&destination, &credentials).is_err());
    assert!(destination.is_dir());
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}
