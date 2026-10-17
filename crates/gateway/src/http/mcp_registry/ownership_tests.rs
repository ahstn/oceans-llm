//! Authorization boundaries for delegated MCP server ownership. See
//! `docs/adr/2026-10-17-delegated-mcp-server-ownership.md`.

use axum::{
    body::to_bytes,
    http::{HeaderValue, StatusCode, header::COOKIE},
    response::IntoResponse,
};
use gateway_core::{
    AdminApiKeyRepository, ApiKeyModelGrantMode, ApiKeyOwnerKind, AuditLogQuery,
    AuditLogRepository, AuthMode, ExternalMcpAuthMode, ExternalMcpDiscoveryRunRecord,
    ExternalMcpDiscoveryStatus, ExternalMcpTransport, GlobalRole, McpServerOwnership,
    McpServerVisibility, MembershipRole, NewApiKeyRecord, NewExternalMcpServerRecord,
    UpsertExternalMcpToolRecord, UserStatus,
};
use gateway_service::McpOrganisationPublishing;
use gateway_store::GatewayStore;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use time::Duration;

use super::*;

struct Actor {
    user_id: Uuid,
    headers: HeaderMap,
}

struct Server {
    id: Uuid,
    tool_id: Uuid,
}

struct TestContext {
    state: AppState,
    _directory: tempfile::TempDir,
    team_a: Uuid,
    team_b: Uuid,
}

impl TestContext {
    async fn new() -> Self {
        Self::with_publishing(McpOrganisationPublishing::PlatformAdmins).await
    }

    async fn with_publishing(publishing: McpOrganisationPublishing) -> Self {
        let (directory, mut state) = crate::http::test_support::app_state().await;
        state.mcp_organisation_publishing = publishing;
        let team_a = state.store.create_team("team-a", "Team A").await.unwrap();
        let team_b = state.store.create_team("team-b", "Team B").await.unwrap();
        Self {
            state,
            _directory: directory,
            team_a: team_a.team_id,
            team_b: team_b.team_id,
        }
    }

    async fn actor(&self, role: GlobalRole, team: Option<(Uuid, MembershipRole)>) -> Actor {
        let email = format!("{}@example.test", Uuid::new_v4());
        let user = self
            .state
            .store
            .create_identity_user(
                "MCP owner",
                &email,
                &email,
                role,
                AuthMode::Password,
                UserStatus::Active,
            )
            .await
            .unwrap();
        if let Some((team_id, role)) = team {
            self.state
                .store
                .assign_team_membership(user.user_id, team_id, role)
                .await
                .unwrap();
        }
        let session_id = Uuid::new_v4();
        let token = format!("{session_id}.mcp-ownership-test");
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
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_str(&format!("ogw_session={token}")).unwrap(),
        );
        Actor {
            user_id: user.user_id,
            headers,
        }
    }

    async fn team_admin(&self, team_id: Uuid) -> Actor {
        self.actor(GlobalRole::User, Some((team_id, MembershipRole::Admin)))
            .await
    }

    async fn platform_admin(&self) -> Actor {
        self.actor(GlobalRole::PlatformAdmin, None).await
    }

    /// A server with one discovered tool.
    async fn server(&self, key: &str, ownership: McpServerOwnership) -> Server {
        let store = &self.state.store;
        let now = OffsetDateTime::now_utc();
        let server = store
            .create_external_mcp_server(&NewExternalMcpServerRecord {
                server_key: key.to_string(),
                display_name: key.to_string(),
                description: None,
                transport: ExternalMcpTransport::StreamableHttp,
                server_url: format!("https://{key}.example.test/mcp"),
                auth_mode: ExternalMcpAuthMode::None,
                auth_config: Map::new(),
                timeout_ms: 5_000,
                ownership,
                created_at: now,
            })
            .await
            .unwrap();
        let tools = store
            .record_external_mcp_discovery_success(
                &ExternalMcpDiscoveryRunRecord {
                    discovery_run_id: Uuid::new_v4(),
                    mcp_server_id: server.mcp_server_id,
                    status: ExternalMcpDiscoveryStatus::Success,
                    started_at: now,
                    finished_at: now,
                    discovered_tool_count: 1,
                    active_tool_count: 1,
                    schema_set_hash: None,
                    error_summary: None,
                    details: Map::new(),
                },
                &[UpsertExternalMcpToolRecord {
                    mcp_server_id: server.mcp_server_id,
                    upstream_name: "search".to_string(),
                    display_name: "search".to_string(),
                    description: None,
                    input_schema: json!({"type": "object"}),
                    metadata: gateway_core::McpToolMetadata::default(),
                    schema_hash: "hash".to_string(),
                }],
            )
            .await
            .unwrap();
        Server {
            id: server.mcp_server_id,
            tool_id: tools[0].mcp_tool_id,
        }
    }

    fn team_owned(&self, team_id: Uuid, visibility: McpServerVisibility) -> McpServerOwnership {
        McpServerOwnership {
            owner_team_id: Some(team_id),
            owner_contact: None,
            visibility,
        }
    }

    async fn member(&self, team_id: Uuid) -> Uuid {
        self.actor(GlobalRole::User, Some((team_id, MembershipRole::Member)))
            .await
            .user_id
    }

    async fn service_account(&self, team_id: Uuid) -> Uuid {
        self.state
            .store
            .create_service_account(
                team_id,
                &format!("sa-{}", Uuid::new_v4().simple()),
                "Bot",
                OffsetDateTime::now_utc(),
            )
            .await
            .unwrap()
            .service_account_id
    }

    async fn user_api_key(&self, user_id: Uuid) -> Uuid {
        self.state
            .store
            .create_api_key(&NewApiKeyRecord {
                name: "MCP test".into(),
                public_id: Uuid::new_v4().simple().to_string(),
                secret_hash: gateway_core::hash_gateway_key_secret("test-secret").unwrap(),
                model_grant_mode: ApiKeyModelGrantMode::Explicit,
                owner_kind: ApiKeyOwnerKind::User,
                owner_user_id: Some(user_id),
                owner_team_id: None,
                owner_service_account_id: None,
                access: Default::default(),
                created_at: OffsetDateTime::now_utc(),
            })
            .await
            .unwrap()
            .id
    }

    async fn list(&self, actor: &Actor, owner: Option<&str>) -> (StatusCode, Value) {
        read(
            list_mcp_servers(
                State(self.state.clone()),
                actor.headers.clone(),
                Query(McpServersQuery {
                    include_disabled: true,
                    owner: owner.map(str::to_string),
                }),
            )
            .await,
        )
        .await
    }

    async fn create(&self, actor: &Actor, body: Value) -> (StatusCode, Value) {
        read(
            create_mcp_server(
                State(self.state.clone()),
                actor.headers.clone(),
                AuditRequestMeta::default(),
                Json(serde_json::from_value(body).unwrap()),
            )
            .await,
        )
        .await
    }

    async fn update(&self, actor: &Actor, server_id: Uuid, extra: Value) -> (StatusCode, Value) {
        let mut body = json!({
            "display_name": "Renamed",
            "server_url": "https://renamed.example.test/mcp",
            "auth_mode": "none",
        });
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        read(
            update_mcp_server(
                State(self.state.clone()),
                actor.headers.clone(),
                AuditRequestMeta::default(),
                Path(server_id.to_string()),
                Json(serde_json::from_value(body).unwrap()),
            )
            .await,
        )
        .await
    }

    async fn disable(&self, actor: &Actor, server_id: Uuid) -> StatusCode {
        read(
            disable_mcp_server(
                State(self.state.clone()),
                actor.headers.clone(),
                AuditRequestMeta::default(),
                Path(server_id.to_string()),
            )
            .await,
        )
        .await
        .0
    }

    async fn grant(
        &self,
        actor: &Actor,
        subject_kind: &str,
        subject_id: Uuid,
        target_kind: &str,
        target_id: Uuid,
    ) -> (StatusCode, Value) {
        read(
            upsert_mcp_grant(
                State(self.state.clone()),
                actor.headers.clone(),
                AuditRequestMeta::default(),
                Json(grant_request(
                    subject_kind,
                    subject_id,
                    target_kind,
                    target_id,
                )),
            )
            .await,
        )
        .await
    }

    async fn revoke(
        &self,
        actor: &Actor,
        subject_kind: &str,
        subject_id: Uuid,
        tool_id: Uuid,
    ) -> StatusCode {
        read(
            revoke_mcp_grant(
                State(self.state.clone()),
                actor.headers.clone(),
                AuditRequestMeta::default(),
                Json(grant_request(subject_kind, subject_id, "tool", tool_id)),
            )
            .await,
        )
        .await
        .0
    }

    async fn transfer(&self, actor: &Actor, server_id: Uuid, body: Value) -> (StatusCode, Value) {
        read(
            transfer_mcp_server_ownership(
                State(self.state.clone()),
                actor.headers.clone(),
                AuditRequestMeta::default(),
                Path(server_id.to_string()),
                Json(serde_json::from_value(body).unwrap()),
            )
            .await,
        )
        .await
    }

    async fn bind_credential(&self, actor: &Actor, body: Value) -> StatusCode {
        read(
            upsert_mcp_credential_binding(
                State(self.state.clone()),
                actor.headers.clone(),
                AuditRequestMeta::default(),
                Json(serde_json::from_value(body).unwrap()),
            )
            .await,
        )
        .await
        .0
    }

    /// The newest audit entry for `action`, with its metadata.
    async fn last_audit(&self, action: &str) -> gateway_core::AuditLogRecord {
        let page = self
            .state
            .store
            .list_audit_logs(&AuditLogQuery {
                action: Some(action.to_string()),
                limit: 1,
                ..AuditLogQuery::default()
            })
            .await
            .unwrap();
        let id = page.items.first().expect("audit entry").id;
        self.state.store.get_audit_log(id).await.unwrap().unwrap()
    }
}

fn grant_request(
    subject_kind: &str,
    subject_id: Uuid,
    target_kind: &str,
    target_id: Uuid,
) -> UpsertMcpGrantRequest {
    serde_json::from_value(json!({
        "subject_kind": subject_kind,
        "subject_id": subject_id,
        "target_kind": target_kind,
        "target_id": target_id,
    }))
    .unwrap()
}

async fn read(response: impl IntoResponse) -> (StatusCode, Value) {
    let response = response.into_response();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

fn create_body(key: &str) -> Value {
    json!({
        "server_key": key,
        "display_name": key,
        "server_url": format!("https://{key}.example.test/mcp"),
        "auth_mode": "none",
    })
}

#[tokio::test]
async fn members_and_users_without_a_team_cannot_reach_mcp_admin() {
    let context = TestContext::new().await;
    let member = context
        .actor(
            GlobalRole::User,
            Some((context.team_a, MembershipRole::Member)),
        )
        .await;
    let loner = context.actor(GlobalRole::User, None).await;
    for actor in [&member, &loner] {
        assert_eq!(context.list(actor, None).await.0, StatusCode::FORBIDDEN);
        assert_eq!(
            context.create(actor, create_body("docs")).await.0,
            StatusCode::FORBIDDEN
        );
    }
}

#[tokio::test]
async fn team_owner_and_admin_register_servers_for_their_own_team() {
    let context = TestContext::new().await;
    for (role, key) in [
        (MembershipRole::Owner, "owner-docs"),
        (MembershipRole::Admin, "admin-docs"),
    ] {
        let actor = context
            .actor(GlobalRole::User, Some((context.team_a, role)))
            .await;
        let (status, body) = context.create(&actor, create_body(key)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let server = &body["data"]["server"];
        assert_eq!(server["owner_team_id"], context.team_a.to_string());
        assert_eq!(server["owner_team_name"], "Team A");
        assert_eq!(server["visibility"], "team");
        assert_eq!(server["can_manage"], true);
        assert!(
            server["owner_contact"]
                .as_str()
                .unwrap()
                .ends_with("@example.test")
        );

        let entry = context.last_audit("mcp_server.created").await;
        assert_eq!(entry.metadata["actor_access"], "team_admin");
        assert_eq!(entry.metadata["actor_team_role"], role.as_str());
        assert_eq!(entry.metadata["actor_team_id"], context.team_a.to_string());
        assert_eq!(entry.after.as_ref().unwrap()["visibility"], "team");
    }
}

#[tokio::test]
async fn team_admin_cannot_register_for_another_team() {
    let context = TestContext::new().await;
    let admin = context.team_admin(context.team_a).await;
    let mut body = create_body("docs");
    body["owner_team_id"] = json!(context.team_b);
    assert_eq!(context.create(&admin, body).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn publishing_follows_the_configured_policy() {
    let mut body = create_body("docs");
    body["visibility"] = json!("organisation");

    let restricted = TestContext::new().await;
    let admin = restricted.team_admin(restricted.team_a).await;
    assert_eq!(
        restricted.create(&admin, body.clone()).await.0,
        StatusCode::FORBIDDEN
    );
    let server = restricted
        .server(
            "private",
            restricted.team_owned(restricted.team_a, McpServerVisibility::Team),
        )
        .await;
    assert_eq!(
        restricted
            .update(&admin, server.id, json!({ "visibility": "organisation" }))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    // A platform admin can publish it.
    let platform = restricted.platform_admin().await;
    let (status, body_after) = restricted
        .update(
            &platform,
            server.id,
            json!({ "visibility": "organisation" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body_after}");
    assert_eq!(body_after["data"]["server"]["visibility"], "organisation");
    let entry = restricted.last_audit("mcp_server.updated").await;
    assert_eq!(entry.before.as_ref().unwrap()["visibility"], "team");
    assert_eq!(entry.after.as_ref().unwrap()["visibility"], "organisation");
    assert_eq!(entry.metadata["actor_access"], "platform_admin");
    // The owning team can unpublish again.
    let (status, _) = restricted
        .update(&admin, server.id, json!({ "visibility": "team" }))
        .await;
    assert_eq!(status, StatusCode::OK);

    let open = TestContext::with_publishing(McpOrganisationPublishing::TeamAdmins).await;
    let admin = open.team_admin(open.team_a).await;
    let (status, created) = open.create(&admin, body).await;
    assert_eq!(status, StatusCode::OK, "{created}");
    assert_eq!(created["data"]["server"]["visibility"], "organisation");
}

#[tokio::test]
async fn team_admins_see_own_and_published_servers_and_filter_by_owner() {
    let context = TestContext::new().await;
    let own = context
        .server(
            "own",
            context.team_owned(context.team_a, McpServerVisibility::Team),
        )
        .await;
    context
        .server(
            "other-private",
            context.team_owned(context.team_b, McpServerVisibility::Team),
        )
        .await;
    context
        .server(
            "other-published",
            context.team_owned(context.team_b, McpServerVisibility::Organisation),
        )
        .await;
    context
        .server("platform", McpServerOwnership::default())
        .await;

    let admin = context.team_admin(context.team_a).await;
    let (status, body) = context.list(&admin, None).await;
    assert_eq!(status, StatusCode::OK);
    let items = body["data"]["items"].as_array().unwrap();
    let mut seen: Vec<(&str, bool)> = items
        .iter()
        .map(|item| {
            (
                item["server_key"].as_str().unwrap(),
                item["can_manage"].as_bool().unwrap(),
            )
        })
        .collect();
    seen.sort();
    assert_eq!(
        seen,
        vec![
            ("other-published", false),
            ("own", true),
            ("platform", false)
        ]
    );

    // `owner=platform` keeps only platform-owned servers, unlike no filter.
    let (_, body) = context.list(&admin, Some("platform")).await;
    assert_eq!(body["data"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(body["data"]["items"][0]["server_key"], "platform");
    let platform_view = context.platform_admin().await;
    let (_, body) = context.list(&platform_view, Some("platform")).await;
    assert_eq!(body["data"]["items"].as_array().unwrap().len(), 1);
    let (status, _) = context.list(&admin, Some("not-a-team")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (_, body) = context
        .list(&admin, Some(&context.team_a.to_string()))
        .await;
    assert_eq!(body["data"]["items"][0]["id"], own.id.to_string());

    // A platform admin sees every server and its owner.
    let platform = context.platform_admin().await;
    let (_, body) = context.list(&platform, None).await;
    assert_eq!(body["data"]["items"].as_array().unwrap().len(), 4);
    assert!(
        body["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["can_manage"] == true)
    );
}

#[tokio::test]
async fn team_admin_cannot_change_another_teams_or_platform_servers() {
    let context = TestContext::new().await;
    let private = context
        .server(
            "other-private",
            context.team_owned(context.team_b, McpServerVisibility::Team),
        )
        .await;
    let published = context
        .server(
            "other-published",
            context.team_owned(context.team_b, McpServerVisibility::Organisation),
        )
        .await;
    let platform = context
        .server("platform", McpServerOwnership::default())
        .await;
    let admin = context.team_admin(context.team_a).await;

    // A server they cannot see does not exist for them.
    assert_eq!(
        context.update(&admin, private.id, json!({})).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        context.disable(&admin, private.id).await,
        StatusCode::NOT_FOUND
    );
    for server in [&published, &platform] {
        assert_eq!(
            context.update(&admin, server.id, json!({})).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            context.disable(&admin, server.id).await,
            StatusCode::FORBIDDEN
        );
    }
    let (status, _) = read(
        refresh_mcp_server_discovery(
            State(context.state.clone()),
            admin.headers.clone(),
            AuditRequestMeta::default(),
            Path(published.id.to_string()),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // They can still read the tools of a published server.
    let (status, _) = read(
        list_mcp_server_tools(
            State(context.state.clone()),
            admin.headers.clone(),
            Path(published.id.to_string()),
            Query(McpToolsQuery {
                include_inactive: false,
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn team_admin_manages_their_own_server() {
    let context = TestContext::new().await;
    let own = context
        .server(
            "own",
            context.team_owned(context.team_a, McpServerVisibility::Team),
        )
        .await;
    let admin = context.team_admin(context.team_a).await;

    let (status, body) = context
        .update(&admin, own.id, json!({ "owner_contact": "#team-a-mcp" }))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["server"]["owner_contact"], "#team-a-mcp");
    let entry = context.last_audit("mcp_server.updated").await;
    assert_eq!(entry.metadata["actor_team_role"], "admin");

    assert_eq!(context.disable(&admin, own.id).await, StatusCode::OK);
    let entry = context.last_audit("mcp_server.disabled").await;
    assert_eq!(entry.metadata["actor_access"], "team_admin");
}

#[tokio::test]
async fn team_admin_grants_own_tools_only_to_their_own_team() {
    let context = TestContext::new().await;
    let own = context
        .server(
            "own",
            context.team_owned(context.team_a, McpServerVisibility::Team),
        )
        .await;
    let published = context
        .server(
            "other-published",
            context.team_owned(context.team_b, McpServerVisibility::Organisation),
        )
        .await;
    let admin = context.team_admin(context.team_a).await;

    let member_a = context.member(context.team_a).await;
    let bot_a = context.service_account(context.team_a).await;
    let key_a = context.user_api_key(member_a).await;
    for (kind, id) in [
        ("user", member_a),
        ("team", context.team_a),
        ("service_account", bot_a),
        ("api_key", key_a),
    ] {
        let (status, body) = context.grant(&admin, kind, id, "tool", own.tool_id).await;
        assert_eq!(status, StatusCode::OK, "{kind}: {body}");
    }
    let entry = context.last_audit("mcp_grant.set").await;
    assert_eq!(entry.metadata["actor_access"], "team_admin");
    assert_eq!(entry.metadata["actor_team_role"], "admin");

    let member_b = context.member(context.team_b).await;
    let bot_b = context.service_account(context.team_b).await;
    let key_b = context.user_api_key(member_b).await;
    let loner = context.actor(GlobalRole::User, None).await.user_id;
    for (kind, id) in [
        ("user", member_b),
        ("user", loner),
        ("team", context.team_b),
        ("service_account", bot_b),
        ("api_key", key_b),
    ] {
        assert_eq!(
            context.grant(&admin, kind, id, "tool", own.tool_id).await.0,
            StatusCode::FORBIDDEN,
            "{kind} outside the team"
        );
    }
    // Tools of a server they do not own, and tool sets, are out of reach.
    assert_eq!(
        context
            .grant(&admin, "user", member_a, "tool", published.tool_id)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let toolset = context
        .state
        .store
        .create_mcp_toolset(&NewMcpToolsetRecord {
            toolset_key: "shared".to_string(),
            display_name: "Shared".to_string(),
            description: None,
            created_at: OffsetDateTime::now_utc(),
        })
        .await
        .unwrap();
    assert_eq!(
        context
            .grant(&admin, "user", member_a, "toolset", toolset.toolset_id)
            .await
            .0,
        StatusCode::FORBIDDEN
    );

    // They see only grants on their own servers' tools, and revoke only those.
    let platform = context.platform_admin().await;
    assert_eq!(
        context
            .grant(&platform, "user", member_b, "tool", published.tool_id)
            .await
            .0,
        StatusCode::OK
    );
    let (_, body) = read(
        list_mcp_grants(
            State(context.state.clone()),
            admin.headers.clone(),
            Query(serde_json::from_value(json!({})).unwrap()),
        )
        .await,
    )
    .await;
    let items = body["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 4);
    assert!(
        items
            .iter()
            .all(|grant| grant["target_id"] == own.tool_id.to_string())
    );
    assert_eq!(
        context
            .revoke(&admin, "user", member_b, published.tool_id)
            .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        context.revoke(&admin, "user", member_a, own.tool_id).await,
        StatusCode::OK
    );
    let entry = context.last_audit("mcp_grant.revoked").await;
    assert_eq!(entry.metadata["actor_access"], "team_admin");
}

#[tokio::test]
async fn team_visible_tools_cannot_be_granted_outside_the_owning_team() {
    let context = TestContext::new().await;
    let private = context
        .server(
            "private",
            context.team_owned(context.team_a, McpServerVisibility::Team),
        )
        .await;
    let published = context
        .server(
            "published",
            context.team_owned(context.team_a, McpServerVisibility::Organisation),
        )
        .await;
    let platform = context.platform_admin().await;
    let member_b = context.member(context.team_b).await;

    assert_eq!(
        context
            .grant(&platform, "user", member_b, "tool", private.tool_id)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        context
            .grant(&platform, "user", member_b, "tool", published.tool_id)
            .await
            .0,
        StatusCode::OK
    );

    // Narrowing the published server would strand that grant.
    let admin = context.team_admin(context.team_a).await;
    let (status, body) = context
        .update(&admin, published.id, json!({ "visibility": "team" }))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    // Team-only tools stay out of tool sets.
    let toolset = context
        .state
        .store
        .create_mcp_toolset(&NewMcpToolsetRecord {
            toolset_key: "org".to_string(),
            display_name: "Org".to_string(),
            description: None,
            created_at: OffsetDateTime::now_utc(),
        })
        .await
        .unwrap();
    let replace = |tool_id: Uuid| {
        replace_mcp_toolset_tools(
            State(context.state.clone()),
            platform.headers.clone(),
            AuditRequestMeta::default(),
            Path(toolset.toolset_id),
            Json(serde_json::from_value(json!({ "tool_ids": [tool_id] })).unwrap()),
        )
    };
    assert_eq!(
        read(replace(private.tool_id).await).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        read(replace(published.tool_id).await).await.0,
        StatusCode::OK
    );
    let entry = context.last_audit("mcp_toolset.tools_replaced").await;
    assert_eq!(entry.metadata["actor_access"], "platform_admin");
}

#[tokio::test]
async fn only_platform_admins_transfer_ownership() {
    let context = TestContext::new().await;
    let own = context
        .server(
            "own",
            context.team_owned(context.team_a, McpServerVisibility::Team),
        )
        .await;
    let admin = context.team_admin(context.team_a).await;
    assert_eq!(
        context
            .transfer(&admin, own.id, json!({ "owner_team_id": null }))
            .await
            .0,
        StatusCode::FORBIDDEN
    );

    // A grant inside team A blocks moving the team-only server to team B.
    let member_a = context.member(context.team_a).await;
    assert_eq!(
        context
            .grant(&admin, "user", member_a, "tool", own.tool_id)
            .await
            .0,
        StatusCode::OK
    );
    let platform = context.platform_admin().await;
    assert_eq!(
        context
            .transfer(
                &platform,
                own.id,
                json!({ "owner_team_id": context.team_b })
            )
            .await
            .0,
        StatusCode::CONFLICT
    );

    // Taking it over for the platform publishes it, which strands nothing.
    let (status, body) = context
        .transfer(
            &platform,
            own.id,
            json!({ "owner_team_id": null, "owner_contact": "platform@example.test" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let server = &body["data"]["server"];
    assert_eq!(server["owner_team_id"], Value::Null);
    assert_eq!(server["visibility"], "organisation");
    assert_eq!(server["owner_contact"], "platform@example.test");
    let entry = context.last_audit("mcp_server.ownership_transferred").await;
    assert_eq!(
        entry.before.as_ref().unwrap()["owner_team_id"],
        context.team_a.to_string()
    );
    assert_eq!(entry.after.as_ref().unwrap()["owner_team_id"], Value::Null);
    assert_eq!(entry.metadata["actor_access"], "platform_admin");

    // The former owners can no longer change it.
    assert_eq!(
        context.update(&admin, own.id, json!({})).await.0,
        StatusCode::FORBIDDEN
    );
    // A platform-owned server cannot be limited to a team.
    assert_eq!(
        context
            .update(&platform, own.id, json!({ "visibility": "team" }))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
}

const CREDENTIAL_KEY_ENV: &str = "OCEANS_MCP_CREDENTIAL_ENCRYPTION_KEY";

/// Sets a test credential encryption key and restores the previous value.
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

#[tokio::test]
#[serial_test::serial]
async fn team_admin_sets_shared_credentials_for_their_own_servers_only() {
    let _key = CredentialKeyGuard::new();
    let context = TestContext::new().await;
    let own = context
        .server(
            "own",
            context.team_owned(context.team_a, McpServerVisibility::Team),
        )
        .await;
    let published = context
        .server(
            "other-published",
            context.team_owned(context.team_b, McpServerVisibility::Organisation),
        )
        .await;
    let admin = context.team_admin(context.team_a).await;
    let bot_a = context.service_account(context.team_a).await;
    let bot_b = context.service_account(context.team_b).await;
    let binding = |server_id: Uuid, scope: Value| {
        let mut body = json!({
            "server_id": server_id,
            "material_kind": "bearer_token",
            "secret": "team-shared-token",
        });
        body.as_object_mut()
            .unwrap()
            .extend(scope.as_object().unwrap().clone());
        body
    };

    assert_eq!(
        context
            .bind_credential(
                &admin,
                binding(
                    own.id,
                    json!({ "owner_scope_kind": "team", "owner_team_id": context.team_a })
                )
            )
            .await,
        StatusCode::OK
    );
    let entry = context.last_audit("mcp_credential_binding.set").await;
    assert_eq!(entry.metadata["actor_access"], "team_admin");
    // A process secret reference could name another team's credential.
    let mut by_reference = binding(
        own.id,
        json!({ "owner_scope_kind": "team", "owner_team_id": context.team_a }),
    );
    let fields = by_reference.as_object_mut().unwrap();
    fields.remove("secret");
    fields.insert(
        "secret_ref".to_string(),
        json!("env/OCEANS_MCP_CREDENTIAL_PLATFORM_TOKEN"),
    );
    assert_eq!(
        context.bind_credential(&admin, by_reference).await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        context
            .bind_credential(
                &admin,
                binding(
                    own.id,
                    json!({
                        "owner_scope_kind": "service_account",
                        "owner_team_id": context.team_a,
                        "owner_service_account_id": bot_a,
                    })
                )
            )
            .await,
        StatusCode::OK
    );
    for scope in [
        json!({ "owner_scope_kind": "team", "owner_team_id": context.team_b }),
        json!({
            "owner_scope_kind": "service_account",
            "owner_team_id": context.team_b,
            "owner_service_account_id": bot_b,
        }),
        // A team A id does not make team B's service account reachable.
        json!({
            "owner_scope_kind": "service_account",
            "owner_team_id": context.team_a,
            "owner_service_account_id": bot_b,
        }),
        json!({ "owner_scope_kind": "user", "owner_user_id": admin.user_id }),
    ] {
        assert_eq!(
            context
                .bind_credential(&admin, binding(own.id, scope.clone()))
                .await,
            StatusCode::FORBIDDEN,
            "{scope}"
        );
    }
    assert_eq!(
        context
            .bind_credential(
                &admin,
                binding(
                    published.id,
                    json!({ "owner_scope_kind": "team", "owner_team_id": context.team_a })
                )
            )
            .await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn team_admins_cannot_manage_tool_sets() {
    let context = TestContext::new().await;
    let admin = context.team_admin(context.team_a).await;
    let (status, _) = read(
        list_mcp_toolsets(
            State(context.state.clone()),
            admin.headers.clone(),
            Query(serde_json::from_value(json!({})).unwrap()),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = read(
        create_mcp_toolset(
            State(context.state.clone()),
            admin.headers.clone(),
            AuditRequestMeta::default(),
            Json(
                serde_json::from_value(json!({ "toolset_key": "x", "display_name": "X" })).unwrap(),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_transfer_must_name_the_new_owner() {
    let context = TestContext::new().await;
    let own = context
        .server(
            "own",
            context.team_owned(context.team_a, McpServerVisibility::Team),
        )
        .await;
    let platform = context.platform_admin().await;
    // Omitting owner_team_id must not move the server to the platform.
    let (status, body) = context
        .transfer(
            &platform,
            own.id,
            json!({ "owner_contact": "new@example.test" }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let server = context
        .state
        .store
        .get_external_mcp_server(own.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(server.ownership.owner_team_id, Some(context.team_a));
}

#[tokio::test]
async fn servers_a_caller_cannot_manage_hide_their_url_secrets_and_auth_config() {
    let context = TestContext::new().await;
    let now = OffsetDateTime::now_utc();
    let mut auth_config = Map::new();
    auth_config.insert("header_name".to_string(), json!("X-Api-Key"));
    auth_config.insert(
        "secret_ref".to_string(),
        json!("env/OCEANS_MCP_DISCOVERY_SHARED"),
    );
    context
        .state
        .store
        .create_external_mcp_server(&NewExternalMcpServerRecord {
            server_key: "published".to_string(),
            display_name: "Published".to_string(),
            description: None,
            transport: ExternalMcpTransport::StreamableHttp,
            server_url: "https://mcp.example.test/mcp?token=query-secret".to_string(),
            auth_mode: ExternalMcpAuthMode::GatewayStaticHeader,
            auth_config,
            timeout_ms: 5_000,
            ownership: context.team_owned(context.team_b, McpServerVisibility::Organisation),
            created_at: now,
        })
        .await
        .unwrap();

    let admin = context.team_admin(context.team_a).await;
    let (_, body) = context.list(&admin, None).await;
    let view = &body["data"]["items"][0];
    assert_eq!(view["can_manage"], false);
    // Only the origin: an opaque path segment could itself be a credential.
    assert_eq!(view["server_url"], "https://mcp.example.test");
    assert_eq!(view["auth_config"], json!({}));
    assert!(!body.to_string().contains("query-secret"));

    // The owning team still sees the full settings.
    let owner = context.team_admin(context.team_b).await;
    let (_, body) = context.list(&owner, None).await;
    let view = &body["data"]["items"][0];
    assert_eq!(view["can_manage"], true);
    assert_eq!(
        view["server_url"],
        "https://mcp.example.test/mcp?token=query-secret"
    );
    assert_eq!(view["auth_config"]["header_name"], "X-Api-Key");
}

#[tokio::test]
async fn team_admins_cannot_point_the_gateway_at_internal_addresses() {
    let context = TestContext::new().await;
    let admin = context.team_admin(context.team_a).await;
    for (index, url) in [
        "https://10.0.0.5/mcp",
        "http://127.0.0.1:9/mcp",
        "https://localhost/mcp",
        "https://api.localhost/mcp",
        "https://[::1]/mcp",
        "https://[::ffff:192.168.1.1]/mcp",
        "https://[::127.0.0.1]/mcp",
        "https://[64:ff9b::a00:5]/mcp",
        "https://169.254.169.254/latest",
        "https://100.100.100.200/mcp",
        "https://[fd00::1]/mcp",
    ]
    .into_iter()
    .enumerate()
    {
        let mut body = create_body(&format!("internal-{index}"));
        body["server_url"] = json!(url);
        let (status, response) = context.create(&admin, body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{url}: {response}");
    }
    let (status, _) = read(
        discover_mcp_server_oauth(
            State(context.state.clone()),
            admin.headers.clone(),
            Json(serde_json::from_value(json!({ "server_url": "https://127.0.0.1/mcp" })).unwrap()),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A public URL is fine, and moving it to an internal one is not.
    let own = context
        .server(
            "own",
            context.team_owned(context.team_a, McpServerVisibility::Team),
        )
        .await;
    let (status, _) = context
        .update(
            &admin,
            own.id,
            json!({ "server_url": "https://192.168.1.10/mcp" }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Platform admins register internal servers, including for a team.
    let platform = context.platform_admin().await;
    let mut body = create_body("internal-docs");
    body["server_url"] = json!("https://10.0.0.5/mcp");
    body["owner_team_id"] = json!(context.team_a);
    let (status, created) = context.create(&platform, body).await;
    assert_eq!(status, StatusCode::OK, "{created}");
    // The owning team can still edit it while the URL stays the same.
    let internal = created["data"]["server"]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let (status, _) = context
        .update(
            &admin,
            internal,
            json!({ "server_url": "https://10.0.0.5/mcp", "owner_contact": "#docs" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn team_admins_cannot_send_gateway_secrets_or_call_internal_endpoints() {
    let context = TestContext::new().await;
    let admin = context.team_admin(context.team_a).await;
    let secret_ref = json!({ "secret_ref": "env/OCEANS_MCP_DISCOVERY_PLATFORM_TOKEN" });

    // Naming a gateway secret on a team server would send it to that server.
    let mut body = create_body("bearer");
    body["auth_mode"] = json!("gateway_bearer_token");
    body["auth_config"] = secret_ref.clone();
    assert_eq!(context.create(&admin, body).await.0, StatusCode::FORBIDDEN);

    // A URL in the auth config is one the gateway calls, so it must be public.
    let mut body = create_body("oauth");
    body["auth_config"] = json!({ "token_endpoint": "https://10.0.0.7/token" });
    let (status, response) = context.create(&admin, body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        response.to_string().contains("public address"),
        "{response}"
    );

    // A platform admin can set one up for the team, and the team keeps it only
    // while the secret and the destination stay as they are.
    let platform = context.platform_admin().await;
    let mut body = create_body("shared-bearer");
    body["auth_mode"] = json!("gateway_bearer_token");
    body["auth_config"] = secret_ref.clone();
    body["owner_team_id"] = json!(context.team_a);
    let (status, created) = context.create(&platform, body).await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let id: Uuid = created["data"]["server"]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let same = json!({
        "server_url": "https://shared-bearer.example.test/mcp",
        "auth_mode": "gateway_bearer_token",
        "auth_config": secret_ref,
        "owner_contact": "#team-a",
    });
    let (status, response) = context.update(&admin, id, same.clone()).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    let mut moved = same.clone();
    moved["server_url"] = json!("https://elsewhere.example.test/mcp");
    assert_eq!(
        context.update(&admin, id, moved).await.0,
        StatusCode::FORBIDDEN
    );
    let mut swapped = same;
    swapped["auth_config"] = json!({ "secret_ref": "env/OCEANS_MCP_DISCOVERY_OTHER_TOKEN" });
    assert_eq!(
        context.update(&admin, id, swapped).await.0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn inherited_internal_endpoints_are_rechecked_when_the_server_moves() {
    let context = TestContext::new().await;
    let admin = context.team_admin(context.team_a).await;
    // A platform admin set the team's server up with an internal token endpoint.
    let auth_config = json!({
        "resource": "https://files.example.test/mcp",
        "issuer": "https://files.example.test",
        "authorization_endpoint": "https://files.example.test/authorize",
        "token_endpoint": "https://10.0.0.7/token",
        "client_registration": "manual",
        "client_id": "files",
    });
    let server = context
        .state
        .store
        .create_external_mcp_server(&NewExternalMcpServerRecord {
            server_key: "files".to_string(),
            display_name: "Files".to_string(),
            description: None,
            transport: ExternalMcpTransport::StreamableHttp,
            server_url: "https://files.example.test/mcp".to_string(),
            auth_mode: ExternalMcpAuthMode::OauthObo,
            auth_config: serde_json::from_value(auth_config.clone()).unwrap(),
            timeout_ms: 5_000,
            ownership: context.team_owned(context.team_a, McpServerVisibility::Team),
            created_at: OffsetDateTime::now_utc(),
        })
        .await
        .unwrap();
    let kept = json!({
        "server_url": "https://files.example.test/mcp",
        "auth_mode": "oauth_obo",
        "auth_config": auth_config,
    });
    // Kept as is, the inherited endpoint passes the address check.
    let (_, response) = context
        .update(&admin, server.mcp_server_id, kept.clone())
        .await;
    assert!(
        !response.to_string().contains("public address"),
        "{response}"
    );
    // Pointing the server elsewhere rechecks every URL it would call.
    let mut moved = kept;
    moved["server_url"] = json!("https://attacker.example.test/mcp");
    let (status, response) = context.update(&admin, server.mcp_server_id, moved).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        response.to_string().contains("public address"),
        "{response}"
    );
}
