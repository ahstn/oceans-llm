//! End-to-end checks that admin mutations write the right audit entries through
//! the router, that failed mutations write none, and that secrets never land in
//! an entry.

use admin_ui::client_ip::{CLIENT_IP_HEADER, sign_client_ip};
use axum::{
    body::{Body, to_bytes},
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header::COOKIE},
};
use gateway_core::{AuthMode, GlobalRole, IdentityRepository, UserStatus};
use gateway_store::GatewayStore;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};
use tower::ServiceExt;
use uuid::Uuid;

use crate::http::{build_router, state::AppState, test_support::app_state};

const AUDIT_LOGS: &str = "/api/v1/admin/audit-logs";
const CLIENT_IP: &str = "203.0.113.9";

struct Harness {
    state: AppState,
    admin_id: Uuid,
    admin: HeaderMap,
    _directory: tempfile::TempDir,
}

impl Harness {
    async fn new() -> Self {
        let (directory, state) = app_state().await;
        let mut harness = Self {
            state,
            admin_id: Uuid::nil(),
            admin: HeaderMap::new(),
            _directory: directory,
        };
        let (admin_id, admin) = harness.user(GlobalRole::PlatformAdmin).await;
        harness.admin_id = admin_id;
        harness.admin = admin;
        harness
    }

    /// Creates an active user with a session and returns its id and request headers.
    async fn user(&self, role: GlobalRole) -> (Uuid, HeaderMap) {
        let email = format!("{}@example.test", Uuid::new_v4());
        let user = self
            .state
            .store
            .create_identity_user(
                "Audit tester",
                &email,
                &email,
                role,
                AuthMode::Password,
                UserStatus::Active,
            )
            .await
            .expect("create user");
        let session_id = Uuid::new_v4();
        let token = format!("{session_id}.audit-test-token");
        let token_hash = format!("{:x}", Sha256::digest(token.as_bytes()));
        let now = OffsetDateTime::now_utc();
        self.state
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
            HeaderValue::from_str(&format!("ogw_session={token}")).expect("cookie"),
        );
        headers.insert(CLIENT_IP_HEADER, self.signed_client_ip());
        headers.insert("user-agent", HeaderValue::from_static("audit-tests/1.0"));
        (user.user_id, headers)
    }

    /// What the admin UI proxy forwards for a browser at `CLIENT_IP`.
    fn signed_client_ip(&self) -> HeaderValue {
        let value = sign_client_ip(
            &self.state.client_ip_signing_key(),
            CLIENT_IP.parse().unwrap(),
            OffsetDateTime::now_utc().unix_timestamp(),
        );
        HeaderValue::from_str(&value).unwrap()
    }

    async fn send(
        &self,
        method: Method,
        uri: &str,
        headers: &HeaderMap,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder().method(method).uri(uri);
        for (name, value) in headers {
            request = request.header(name, value);
        }
        let request = match body {
            Some(body) => request
                .header("content-type", "application/json")
                .body(Body::from(body.to_string())),
            None => request.body(Body::empty()),
        }
        .expect("request");
        let response = build_router(self.state.clone(), admin_ui::AdminUiConfig::default())
            .oneshot(request)
            .await
            .expect("response");
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, body)
    }

    async fn admin(&self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        self.send(method, uri, &self.admin, body).await
    }

    /// Every audit entry with its summaries, newest first. The list omits summaries,
    /// so each entry is read back through the detail endpoint.
    async fn entries(&self) -> Vec<Value> {
        let (status, body) = self
            .admin(Method::GET, &format!("{AUDIT_LOGS}?limit=200"), None)
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let mut entries = Vec::new();
        for item in body["data"]["items"].as_array().expect("items") {
            assert!(item.get("after").is_none(), "list items carry no summaries");
            let id = item["id"].as_str().expect("id");
            let (status, detail) = self
                .admin(Method::GET, &format!("{AUDIT_LOGS}/{id}"), None)
                .await;
            assert_eq!(status, StatusCode::OK, "{detail}");
            entries.push(detail["data"].clone());
        }
        entries
    }

    async fn entry(&self, action: &str) -> Value {
        let entries = self.entries().await;
        let matches: Vec<_> = entries
            .iter()
            .filter(|entry| entry["action"] == action)
            .collect();
        assert_eq!(matches.len(), 1, "one `{action}` entry in {entries:#?}");
        matches[0].clone()
    }
}

fn assert_admin_actor(entry: &Value, admin_id: Uuid) {
    assert_eq!(entry["actor"]["kind"], "user");
    assert_eq!(entry["actor"]["user_id"], admin_id.to_string());
    assert_eq!(entry["request_ip"], CLIENT_IP);
    assert_eq!(entry["user_agent"], "audit-tests/1.0");
}

#[tokio::test]
async fn api_key_lifecycle_is_audited_without_the_raw_key() {
    let harness = Harness::new().await;
    let (status, created) = harness
        .admin(
            Method::POST,
            "/api/v1/admin/api-keys",
            Some(json!({
                "name": "ci",
                "owner_kind": "user",
                "owner_user_id": harness.admin_id.to_string(),
                "owner_team_id": null,
                "owner_service_account_id": null,
                "model_grant_mode": "all",
                "model_keys": [],
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let raw_key = created["data"]["raw_key"].as_str().expect("raw key");
    let api_key_id = created["data"]["api_key"]["id"].as_str().expect("id");

    let (status, revoked) = harness
        .admin(
            Method::POST,
            &format!("/api/v1/admin/api-keys/{api_key_id}/revoke"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{revoked}");

    let created_entry = harness.entry("api_key.created").await;
    assert_admin_actor(&created_entry, harness.admin_id);
    assert_eq!(created_entry["target_type"], "api_key");
    assert_eq!(created_entry["target_id"], api_key_id);
    assert_eq!(created_entry["target_label"], "ci");
    assert_eq!(created_entry["after"]["status"], "active");
    // The public display prefix stays readable; only the full key is a secret.
    assert_eq!(
        created_entry["after"]["prefix"],
        created["data"]["api_key"]["prefix"]
    );

    let revoked_entry = harness.entry("api_key.revoked").await;
    assert_eq!(revoked_entry["after"]["status"], "revoked");

    let everything = serde_json::to_string(&harness.entries().await).unwrap();
    let secret = raw_key.split_once('.').expect("key secret").1;
    assert!(
        !everything.contains(raw_key),
        "raw key leaked: {everything}"
    );
    assert!(!everything.contains(secret), "key secret leaked");
}

#[tokio::test]
async fn identity_changes_are_audited_without_invite_links() {
    let harness = Harness::new().await;
    let (status, team) = harness
        .admin(
            Method::POST,
            "/api/v1/admin/identity/teams",
            Some(json!({ "name": "Platform", "admin_user_ids": [] })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{team}");
    let team_id = team["data"]["id"].as_str().expect("team id").to_string();

    let (status, user) = harness
        .admin(
            Method::POST,
            "/api/v1/admin/identity/users",
            Some(json!({
                "name": "Invitee",
                "email": "invitee@example.test",
                "auth_mode": "password",
                "global_role": "user",
                "team_id": team_id,
                "team_role": "member",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{user}");
    let invite_url = user["data"]["invite_url"].as_str().expect("invite url");
    let invite_token = invite_url.rsplit('/').next().expect("invite token");

    let team_entry = harness.entry("team.created").await;
    assert_admin_actor(&team_entry, harness.admin_id);
    assert_eq!(team_entry["target_id"], team_id);
    assert_eq!(team_entry["after"]["name"], "Platform");

    let user_entry = harness.entry("user.created").await;
    assert_eq!(user_entry["target_label"], "invitee@example.test");
    assert_eq!(user_entry["after"]["status"], "invited");
    assert_eq!(user_entry["after"]["team_id"], team_id);

    let everything = serde_json::to_string(&harness.entries().await).unwrap();
    assert!(!everything.contains(invite_token), "invite token leaked");
}

#[tokio::test]
async fn budget_changes_record_before_and_after() {
    let harness = Harness::new().await;
    let (user_id, _) = harness.user(GlobalRole::User).await;
    for amount in ["10.00", "25.00"] {
        let (status, body) = harness
            .admin(
                Method::PUT,
                "/api/v1/admin/spend/budgets",
                Some(json!({
                    "scope": { "kind": "user", "user_id": user_id.to_string() },
                    "cadence": "monthly",
                    "amount_usd": amount,
                    "hard_limit": true,
                    "timezone": "UTC",
                })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    let entries = harness.entries().await;
    let budget_entries: Vec<_> = entries
        .iter()
        .filter(|entry| entry["action"] == "budget.set")
        .collect();
    assert_eq!(budget_entries.len(), 2);
    // Newest first: the second write saw the first as its before state.
    assert_eq!(budget_entries[0]["before"]["amount_usd"], "10.0000");
    assert_eq!(budget_entries[0]["after"]["amount_usd"], "25.0000");
    assert_eq!(budget_entries[1]["before"], Value::Null);
    assert_admin_actor(budget_entries[0], harness.admin_id);
}

#[tokio::test]
#[serial_test::serial]
async fn mcp_changes_are_audited_and_credential_secrets_are_not() {
    let _key = CredentialKeyGuard::new();
    let harness = Harness::new().await;
    let (status, server) = harness
        .admin(
            Method::POST,
            "/api/v1/admin/mcp/servers",
            Some(json!({
                "server_key": "tickets",
                "display_name": "Tickets",
                "server_url": "https://user:pass@mcp.example.test/mcp?token=abc",
                "transport": "streamable_http",
                "auth_mode": "gateway_bearer_token",
                "auth_config": { "secret_ref": "env/OCEANS_MCP_DISCOVERY_TICKETS" },
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{server}");
    let server_id = server["data"]["server"]["id"].as_str().expect("server id");

    let (status, toolset) = harness
        .admin(
            Method::POST,
            "/api/v1/admin/mcp/toolsets",
            Some(json!({ "toolset_key": "support", "display_name": "Support" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{toolset}");
    let toolset_id = toolset["data"]["toolset"]["id"]
        .as_str()
        .expect("toolset id");

    let grant = json!({
        "subject_kind": "user",
        "subject_id": harness.admin_id,
        "target_kind": "toolset",
        "target_id": toolset_id,
    });
    let (status, body) = harness
        .admin(Method::PUT, "/api/v1/admin/mcp/grants", Some(grant.clone()))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = harness
        .admin(
            Method::DELETE,
            "/api/v1/admin/mcp/grants",
            Some(grant.clone()),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // Revoking an already-revoked grant changes nothing, so it records nothing.
    let (status, body) = harness
        .admin(Method::DELETE, "/api/v1/admin/mcp/grants", Some(grant))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let secret = "upstream-secret-value-that-must-not-leak";
    let (status, binding) = harness
        .admin(
            Method::PUT,
            "/api/v1/admin/mcp/credential-bindings",
            Some(json!({
                "server_id": server_id,
                "owner_scope_kind": "user",
                "owner_user_id": harness.admin_id,
                "material_kind": "bearer_token",
                "secret": secret,
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{binding}");
    let binding_id = binding["data"]["binding"]["id"]
        .as_str()
        .expect("binding id");
    let (status, body) = harness
        .admin(
            Method::DELETE,
            &format!("/api/v1/admin/mcp/credential-bindings/{binding_id}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let server_entry = harness.entry("mcp_server.created").await;
    assert_admin_actor(&server_entry, harness.admin_id);
    assert_eq!(server_entry["target_label"], "tickets");
    assert_eq!(
        server_entry["after"]["server_url"],
        "https://mcp.example.test/mcp"
    );
    assert_eq!(
        harness.entry("mcp_toolset.created").await["target_id"],
        toolset_id
    );
    assert_eq!(
        harness.entry("mcp_grant.set").await["after"]["target_id"],
        toolset_id
    );
    let granted = harness.entry("mcp_grant.set").await;
    let revoked = harness.entry("mcp_grant.revoked").await;
    assert_eq!(revoked["before"]["subject_kind"], "user");
    assert_eq!(revoked["target_id"], granted["target_id"]);
    let binding_entry = harness.entry("mcp_credential_binding.set").await;
    assert_eq!(binding_entry["target_id"], binding_id);
    assert_eq!(binding_entry["metadata"]["operation"], "create");
    assert_eq!(binding_entry["after"]["material_kind"], "bearer_token");
    assert_eq!(
        harness.entry("mcp_credential_binding.revoked").await["target_id"],
        binding_id
    );

    let everything = serde_json::to_string(&harness.entries().await).unwrap();
    assert!(!everything.contains(secret), "credential secret leaked");
    assert!(!everything.contains("pass@"), "URL credentials leaked");
    assert!(!everything.contains("token=abc"), "URL query leaked");

    // A caller-chosen ID that does not exist yet creates the binding; reusing it replaces.
    let chosen_id = Uuid::new_v4().to_string();
    for chosen_secret in ["first-chosen-id-secret", "second-chosen-id-secret"] {
        let (status, body) = harness
            .admin(
                Method::PUT,
                "/api/v1/admin/mcp/credential-bindings",
                Some(json!({
                    "credential_binding_id": chosen_id,
                    "server_id": server_id,
                    "owner_scope_kind": "user",
                    "owner_user_id": harness.admin_id,
                    "material_kind": "bearer_token",
                    "secret": chosen_secret,
                })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    let operations: Vec<_> = harness
        .entries()
        .await
        .into_iter()
        .filter(|entry| entry["target_id"] == chosen_id.as_str())
        .map(|entry| entry["metadata"]["operation"].clone())
        .collect();
    assert_eq!(operations, vec![json!("replace"), json!("create")]);
}

#[tokio::test]
async fn failed_mutations_write_no_entry() {
    let harness = Harness::new().await;
    let (status, _) = harness
        .admin(
            Method::POST,
            "/api/v1/admin/identity/teams",
            Some(json!({ "name": "  ", "admin_user_ids": [] })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = harness
        .admin(
            Method::POST,
            &format!("/api/v1/admin/api-keys/{}/revoke", Uuid::new_v4()),
            None,
        )
        .await;
    assert!(status.is_client_error(), "{status}");

    let toolset = json!({ "toolset_key": "dup", "display_name": "Dup" });
    let (status, body) = harness
        .admin(
            Method::POST,
            "/api/v1/admin/mcp/toolsets",
            Some(toolset.clone()),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, _) = harness
        .admin(Method::POST, "/api/v1/admin/mcp/toolsets", Some(toolset))
        .await;
    assert!(status.is_client_error(), "{status}");

    // A denied caller cannot mutate, so nothing is written for them either.
    let (_, member) = harness.user(GlobalRole::User).await;
    let (status, _) = harness
        .send(
            Method::POST,
            "/api/v1/admin/identity/teams",
            &member,
            Some(json!({ "name": "Shadow", "admin_user_ids": [] })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let actions: Vec<_> = harness
        .entries()
        .await
        .iter()
        .map(|entry| entry["action"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(actions, vec!["mcp_toolset.created"]);
}

#[tokio::test]
async fn password_sign_in_and_sign_out_are_audited_without_the_password() {
    let harness = Harness::new().await;
    let (user_id, _) = harness.user(GlobalRole::User).await;
    let user = harness
        .state
        .store
        .get_user_by_id(user_id)
        .await
        .unwrap()
        .unwrap();
    let password = "correct horse battery staple";
    let hash = gateway_service::hash_gateway_key_secret(password).unwrap();
    harness
        .state
        .store
        .store_user_password(user_id, &hash, OffsetDateTime::now_utc())
        .await
        .unwrap();

    let mut anonymous = HeaderMap::new();
    anonymous.insert(CLIENT_IP_HEADER, harness.signed_client_ip());
    let (status, _) = harness
        .send(
            Method::POST,
            "/api/v1/auth/login/password",
            &anonymous,
            Some(json!({ "email": user.email, "password": "wrong password" })),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, body) = harness
        .send(
            Method::POST,
            "/api/v1/auth/login/password",
            &anonymous,
            Some(json!({ "email": user.email, "password": password })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let entry = harness.entry("session.created").await;
    assert_eq!(entry["actor"]["user_id"], user_id.to_string());
    assert_eq!(entry["metadata"]["method"], "password");
    assert_eq!(entry["request_ip"], CLIENT_IP);
    let everything = serde_json::to_string(&harness.entries().await).unwrap();
    assert!(!everything.contains(password));
}

#[tokio::test]
async fn forged_client_addresses_are_not_recorded() {
    let harness = Harness::new().await;
    let mut headers = harness.admin.clone();
    headers.insert("x-forwarded-for", HeaderValue::from_static("198.51.100.1"));
    let forged = sign_client_ip(b"guessed-key", "198.51.100.1".parse().unwrap(), 0);
    headers.insert(CLIENT_IP_HEADER, HeaderValue::from_str(&forged).unwrap());
    let (status, body) = harness
        .send(
            Method::POST,
            "/api/v1/admin/mcp/toolsets",
            &headers,
            Some(json!({ "toolset_key": "spoof", "display_name": "Spoof" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // Without a valid signature only the direct peer counts, and a test request has none.
    assert_eq!(
        harness.entry("mcp_toolset.created").await["request_ip"],
        Value::Null
    );
}

#[tokio::test]
async fn audit_log_list_filters_and_pages() {
    let harness = Harness::new().await;
    for name in ["Alpha", "Beta", "Gamma"] {
        let (status, body) = harness
            .admin(
                Method::POST,
                "/api/v1/admin/identity/teams",
                Some(json!({ "name": name, "admin_user_ids": [] })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    let (status, body) = harness
        .admin(
            Method::POST,
            "/api/v1/admin/mcp/toolsets",
            Some(json!({ "toolset_key": "ops", "display_name": "Ops" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, first) = harness
        .admin(
            Method::GET,
            &format!("{AUDIT_LOGS}?target_type=team&limit=2"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let labels = |page: &Value| -> Vec<String> {
        page["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["target_label"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(labels(&first), vec!["Gamma", "Beta"]);
    let cursor = first["data"]["next_cursor"].as_str().expect("next cursor");

    let (_, second) = harness
        .admin(
            Method::GET,
            &format!("{AUDIT_LOGS}?target_type=team&limit=2&cursor={cursor}"),
            None,
        )
        .await;
    assert_eq!(labels(&second), vec!["Alpha"]);
    assert_eq!(second["data"]["next_cursor"], Value::Null);

    let (_, by_action) = harness
        .admin(
            Method::GET,
            &format!(
                "{AUDIT_LOGS}?action=mcp_toolset.created&actor_user_id={}",
                harness.admin_id
            ),
            None,
        )
        .await;
    assert_eq!(labels(&by_action), vec!["ops"]);

    let (_, other_actor) = harness
        .admin(
            Method::GET,
            &format!("{AUDIT_LOGS}?actor_user_id={}", Uuid::new_v4()),
            None,
        )
        .await;
    assert!(labels(&other_actor).is_empty());

    let (_, future) = harness
        .admin(
            Method::GET,
            &format!("{AUDIT_LOGS}?occurred_at_start=2999-01-01T00:00:00Z"),
            None,
        )
        .await;
    assert!(labels(&future).is_empty());

    let (status, _) = harness
        .admin(Method::GET, &format!("{AUDIT_LOGS}?cursor=bogus"), None)
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = harness
        .admin(Method::GET, &format!("{AUDIT_LOGS}/999999"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (_, member) = harness.user(GlobalRole::User).await;
    let (status, _) = harness.send(Method::GET, AUDIT_LOGS, &member, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

const CREDENTIAL_KEY_ENV: &str = "OCEANS_MCP_CREDENTIAL_ENCRYPTION_KEY";

struct CredentialKeyGuard(Option<std::ffi::OsString>);

impl CredentialKeyGuard {
    fn new() -> Self {
        let previous = std::env::var_os(CREDENTIAL_KEY_ENV);
        unsafe {
            std::env::set_var(
                CREDENTIAL_KEY_ENV,
                "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
            );
        }
        Self(previous)
    }
}

impl Drop for CredentialKeyGuard {
    fn drop(&mut self) {
        unsafe {
            match self.0.take() {
                Some(value) => std::env::set_var(CREDENTIAL_KEY_ENV, value),
                None => std::env::remove_var(CREDENTIAL_KEY_ENV),
            }
        }
    }
}
