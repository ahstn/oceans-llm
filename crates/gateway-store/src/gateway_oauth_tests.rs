use crate::tests::{create_postgres_test_database, drop_postgres_test_database};
use crate::{
    GatewayStore, LibsqlStore, PostgresStore, StoreConnectionOptions, run_migrations,
    run_migrations_with_options,
};
use gateway_core::{
    ApiKeyOwnerKind, AuthMode, GatewayOauthAuthorizationDecision,
    GatewayOauthAuthorizationRequestRecord, GatewayOauthAuthorizationStatus,
    GatewayOauthClientAuthMethod, GatewayOauthClientRecord, GatewayOauthGrantRecord,
    GatewayOauthPurgeCounts, GatewayOauthRepository, GatewayOauthTokenKind,
    GatewayOauthTokenRecord, GlobalRole, McpAggregateSessionRepository,
    NewMcpAggregateSessionRecord, StoreError, UserStatus,
};
use serial_test::serial;
use tempfile::tempdir;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

const AGGREGATE: &str = "https://oceans.example.com/mcp";

#[tokio::test]
#[serial]
async fn libsql_gateway_oauth_flow_records_round_trip() {
    let tmp = tempdir().expect("tempdir");
    let db_path = tmp.path().join("gateway.db");
    run_migrations(&db_path).await.expect("migrations");
    let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
        .await
        .expect("store");
    exercise_gateway_oauth(&store).await;
}

#[tokio::test]
#[serial]
async fn postgres_gateway_oauth_flow_records_round_trip() {
    let Some(test_db) = create_postgres_test_database().await else {
        eprintln!("skipping postgres gateway OAuth test: TEST_POSTGRES_URL is not set");
        return;
    };
    run_migrations_with_options(&StoreConnectionOptions::Postgres {
        url: test_db.database_url.clone(),
        max_connections: 4,
    })
    .await
    .expect("migrations");
    let store = PostgresStore::connect(&test_db.database_url, 4)
        .await
        .expect("store");
    exercise_gateway_oauth(&store).await;
    drop(store);
    drop_postgres_test_database(&test_db).await;
}

fn timestamp(seconds: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_790_000_000 + seconds).expect("timestamp")
}

async fn exercise_gateway_oauth<S>(store: &S)
where
    S: GatewayStore + GatewayOauthRepository + McpAggregateSessionRepository,
{
    let now = timestamp(0);
    let user = store
        .create_identity_user(
            "Client user",
            "client@example.com",
            "client@example.com",
            GlobalRole::User,
            AuthMode::Password,
            UserStatus::Active,
        )
        .await
        .expect("user");
    let session_id = Uuid::new_v4();
    store
        .create_user_session(
            session_id,
            user.user_id,
            "session-hash",
            now + Duration::days(30),
            now,
        )
        .await
        .expect("session");

    let session = store
        .get_gateway_oauth_session_state(session_id)
        .await
        .unwrap()
        .expect("session state");
    assert_eq!(session.user_id, user.user_id);
    assert!(session.is_live(now));
    assert!(!session.is_live(now + Duration::days(31)));

    let client = GatewayOauthClientRecord {
        client_id: "client-1".to_string(),
        registration_kind: "dynamic".to_string(),
        client_name: Some("Editor".to_string()),
        client_uri: None,
        logo_uri: None,
        redirect_uris: vec!["http://127.0.0.1/callback".to_string()],
        grant_types: vec![
            "authorization_code".to_string(),
            "refresh_token".to_string(),
        ],
        token_endpoint_auth_method: GatewayOauthClientAuthMethod::None,
        client_secret_hash: None,
        software_id: None,
        software_version: Some("1.0".to_string()),
        created_at: now,
        updated_at: now,
    };
    store
        .create_gateway_oauth_client(&client)
        .await
        .expect("client");
    assert_eq!(
        store.get_gateway_oauth_client("client-1").await.unwrap(),
        Some(client.clone())
    );
    // A public client cannot carry a secret.
    let mut secret_client = client.clone();
    secret_client.client_id = "client-2".to_string();
    secret_client.client_secret_hash = Some("hash".to_string());
    assert!(matches!(
        store.create_gateway_oauth_client(&secret_client).await,
        Err(StoreError::Conflict(_))
    ));

    let request = GatewayOauthAuthorizationRequestRecord {
        request_id: Uuid::new_v4(),
        client_id: client.client_id.clone(),
        redirect_uri: "http://127.0.0.1:4000/callback".to_string(),
        redirect_uri_supplied: true,
        state: Some("xyz".to_string()),
        code_challenge: "challenge".to_string(),
        resource: "https://oceans.example.com/mcp".to_string(),
        scope: "mcp:aggregate".to_string(),
        status: GatewayOauthAuthorizationStatus::Pending,
        user_id: None,
        user_session_id: None,
        code_hash: None,
        code_expires_at: None,
        grant_id: None,
        expires_at: now + Duration::minutes(10),
        created_at: now,
        decided_at: None,
        consumed_at: None,
    };
    store
        .create_gateway_oauth_authorization_request(&request)
        .await
        .expect("request");

    // An expired request cannot be decided.
    let approve = GatewayOauthAuthorizationDecision::Approved {
        user_id: user.user_id,
        user_session_id: session_id,
        code_hash: "code-hash".to_string(),
        code_expires_at: now + Duration::seconds(60),
    };
    assert!(
        !store
            .decide_gateway_oauth_authorization_request(
                request.request_id,
                &approve,
                now + Duration::minutes(11),
            )
            .await
            .unwrap()
    );
    assert!(
        store
            .decide_gateway_oauth_authorization_request(request.request_id, &approve, now)
            .await
            .unwrap()
    );
    // The first decision wins.
    assert!(
        !store
            .decide_gateway_oauth_authorization_request(
                request.request_id,
                &GatewayOauthAuthorizationDecision::Denied {
                    user_id: user.user_id
                },
                now,
            )
            .await
            .unwrap()
    );
    let approved = store
        .get_gateway_oauth_authorization_request_by_code_hash("code-hash")
        .await
        .unwrap()
        .expect("approved request");
    assert_eq!(approved.status, GatewayOauthAuthorizationStatus::Approved);
    assert_eq!(approved.user_session_id, Some(session_id));
    assert_eq!(approved.code_expires_at, Some(now + Duration::seconds(60)));

    let grant_id = Uuid::new_v4();
    assert!(
        store
            .consume_gateway_oauth_authorization_code(request.request_id, grant_id, now)
            .await
            .unwrap()
    );
    assert!(
        !store
            .consume_gateway_oauth_authorization_code(request.request_id, Uuid::new_v4(), now)
            .await
            .unwrap()
    );
    let consumed = store
        .get_gateway_oauth_authorization_request(request.request_id)
        .await
        .unwrap()
        .expect("consumed request");
    assert_eq!(consumed.status, GatewayOauthAuthorizationStatus::Consumed);
    assert_eq!(consumed.grant_id, Some(grant_id));

    store
        .create_gateway_oauth_grant(&GatewayOauthGrantRecord {
            grant_id,
            client_id: client.client_id.clone(),
            user_id: user.user_id,
            user_session_id: session_id,
            resource: request.resource.clone(),
            scope: request.scope.clone(),
            created_at: now,
            updated_at: now,
            revoked_at: None,
            revoked_reason: None,
        })
        .await
        .expect("grant");
    let token = |hash: &str, kind| GatewayOauthTokenRecord {
        token_hash: hash.to_string(),
        grant_id,
        kind,
        scope: "mcp:aggregate".to_string(),
        expires_at: now + Duration::hours(1),
        created_at: now,
        consumed_at: None,
    };
    store
        .create_gateway_oauth_tokens(&[
            token("access-hash", GatewayOauthTokenKind::Access),
            token("refresh-hash", GatewayOauthTokenKind::Refresh),
        ])
        .await
        .expect("tokens");
    // A pair is stored together or not at all.
    assert!(
        store
            .create_gateway_oauth_tokens(&[
                token("pair-access", GatewayOauthTokenKind::Access),
                token("refresh-hash", GatewayOauthTokenKind::Refresh),
            ])
            .await
            .is_err()
    );
    assert!(
        store
            .get_gateway_oauth_token_context("pair-access")
            .await
            .unwrap()
            .is_none()
    );

    // Aggregate sessions belong to exactly one of an API key or a grant.
    let session = |api_key_id, gateway_oauth_grant_id| NewMcpAggregateSessionRecord {
        session_id: Uuid::new_v4(),
        token_hash: Uuid::new_v4().to_string(),
        api_key_id,
        gateway_oauth_grant_id,
        owner_kind: ApiKeyOwnerKind::User,
        owner_user_id: Some(user.user_id),
        owner_team_id: None,
        owner_service_account_id: None,
        protocol_version: "2025-11-25".to_string(),
        expires_at: now + Duration::hours(12),
        created_at: now,
    };
    let created = store
        .create_mcp_aggregate_session(&session(None, Some(grant_id)))
        .await
        .expect("OAuth aggregate session");
    assert_eq!(created.gateway_oauth_grant_id, Some(grant_id));
    assert_eq!(created.api_key_id, None);
    assert!(matches!(
        store
            .create_mcp_aggregate_session(&session(None, None))
            .await,
        Err(StoreError::Conflict(_))
    ));

    let context = store
        .get_gateway_oauth_token_context("access-hash")
        .await
        .unwrap()
        .expect("token context");
    assert_eq!(context.token.kind, GatewayOauthTokenKind::Access);
    assert_eq!(context.token.scope, "mcp:aggregate");
    assert_eq!(context.grant.user_id, user.user_id);
    assert_eq!(context.session_expires_at, now + Duration::days(30));
    assert!(context.grant_is_live(now));
    assert!(
        store
            .get_gateway_oauth_token_context("missing")
            .await
            .unwrap()
            .is_none()
    );

    // An access token is never consumed as a refresh token, and rotation is single use.
    assert!(
        !store
            .consume_gateway_oauth_refresh_token("access-hash", now)
            .await
            .unwrap()
    );
    assert!(
        store
            .consume_gateway_oauth_refresh_token("refresh-hash", now)
            .await
            .unwrap()
    );
    assert!(
        !store
            .consume_gateway_oauth_refresh_token("refresh-hash", now)
            .await
            .unwrap()
    );

    // Only the live access token passes the batched stream check; refresh
    // tokens and unknown hashes never do.
    let candidates = ["access-hash", "refresh-hash", "unknown-hash"].map(String::from);
    assert_eq!(
        store
            .live_gateway_oauth_access_tokens(&candidates, now)
            .await
            .unwrap(),
        vec!["access-hash".to_string()]
    );
    assert!(
        store
            .live_gateway_oauth_access_tokens(&candidates, now + Duration::hours(2))
            .await
            .unwrap()
            .is_empty(),
        "an expired access token is not live"
    );

    // Signing out ends the grant through its session, and revokes it too.
    let signed_out = now + Duration::minutes(5);
    store
        .revoke_user_session(session_id, signed_out)
        .await
        .expect("logout");
    let context = store
        .get_gateway_oauth_token_context("access-hash")
        .await
        .unwrap()
        .unwrap();
    assert!(!context.grant_is_live(now + Duration::minutes(6)));
    assert_eq!(context.grant.revoked_at, Some(signed_out));
    assert_eq!(
        context.grant.revoked_reason.as_deref(),
        Some("session_ended")
    );
    assert!(
        store
            .live_gateway_oauth_access_tokens(&candidates, now + Duration::minutes(6))
            .await
            .unwrap()
            .is_empty(),
        "signing out ends the stream check too"
    );

    // A grant can no longer be created for the signed-out session.
    assert!(
        !store
            .create_gateway_oauth_grant(&GatewayOauthGrantRecord {
                grant_id: Uuid::new_v4(),
                ..context.grant.clone()
            })
            .await
            .unwrap()
    );

    // Nothing is left to revoke, and the first reason is kept.
    assert_eq!(
        store
            .revoke_gateway_oauth_grants_for_user(user.user_id, "user_deactivated", now)
            .await
            .unwrap(),
        0
    );
    assert!(
        !store
            .revoke_gateway_oauth_grant(grant_id, "client_revoked", now)
            .await
            .unwrap()
    );
}

#[tokio::test]
#[serial]
async fn libsql_gateway_oauth_grants_list_revoke_and_purge() {
    let tmp = tempdir().expect("tempdir");
    let db_path = tmp.path().join("gateway.db");
    run_migrations(&db_path).await.expect("migrations");
    let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
        .await
        .expect("store");
    exercise_grant_management(&store).await;
}

#[tokio::test]
#[serial]
async fn postgres_gateway_oauth_grants_list_revoke_and_purge() {
    let Some(test_db) = create_postgres_test_database().await else {
        eprintln!("skipping postgres gateway OAuth management test: TEST_POSTGRES_URL is not set");
        return;
    };
    run_migrations_with_options(&StoreConnectionOptions::Postgres {
        url: test_db.database_url.clone(),
        max_connections: 4,
    })
    .await
    .expect("migrations");
    let store = PostgresStore::connect(&test_db.database_url, 4)
        .await
        .expect("store");
    exercise_grant_management(&store).await;

    // While another replica holds the cleanup lock, this one skips its run.
    use sqlx::Connection;
    let now = timestamp(0);
    store
        .create_gateway_oauth_client(&dynamic_client("lock-old", now - Duration::days(30)))
        .await
        .unwrap();
    let mut other_replica = sqlx::PgConnection::connect(&test_db.database_url)
        .await
        .unwrap();
    sqlx::query("SELECT pg_advisory_lock(hashtext('oceans_llm_gateway_oauth_purge'))")
        .execute(&mut other_replica)
        .await
        .unwrap();
    let skipped = store
        .purge_gateway_oauth_records(now - Duration::days(7), now - Duration::days(1), now)
        .await
        .unwrap();
    assert_eq!(skipped, GatewayOauthPurgeCounts::default());
    assert!(
        store
            .get_gateway_oauth_client("lock-old")
            .await
            .unwrap()
            .is_some()
    );
    other_replica.close().await.unwrap();
    let ran = store
        .purge_gateway_oauth_records(now - Duration::days(7), now - Duration::days(1), now)
        .await
        .unwrap();
    assert_eq!(ran.clients, 1);

    drop(store);
    drop_postgres_test_database(&test_db).await;
}

fn dynamic_client(client_id: &str, created_at: OffsetDateTime) -> GatewayOauthClientRecord {
    GatewayOauthClientRecord {
        client_id: client_id.to_string(),
        registration_kind: "dynamic".to_string(),
        client_name: Some(format!("Client {client_id}")),
        client_uri: None,
        logo_uri: None,
        redirect_uris: vec!["http://127.0.0.1/callback".to_string()],
        grant_types: vec!["authorization_code".to_string()],
        token_endpoint_auth_method: GatewayOauthClientAuthMethod::None,
        client_secret_hash: None,
        software_id: None,
        software_version: None,
        created_at,
        updated_at: created_at,
    }
}

async fn exercise_grant_management<S>(store: &S)
where
    S: GatewayStore + GatewayOauthRepository,
{
    let now = timestamp(0);
    let mut users = Vec::new();
    for email in ["owner@example.com", "other@example.com"] {
        let user = store
            .create_identity_user(
                email,
                email,
                email,
                GlobalRole::User,
                AuthMode::Password,
                UserStatus::Active,
            )
            .await
            .expect("user");
        let session_id = Uuid::new_v4();
        store
            .create_user_session(
                session_id,
                user.user_id,
                &format!("hash-{email}"),
                now + Duration::days(30),
                now,
            )
            .await
            .expect("session");
        users.push((user.user_id, session_id));
    }
    let (owner, owner_session) = users[0];
    let (other, other_session) = users[1];

    let old = now - Duration::days(30);
    for client_id in ["used", "unused-old", "unused-new", "unused-active"] {
        let created_at = if client_id == "unused-new" { now } else { old };
        store
            .create_gateway_oauth_client(&dynamic_client(client_id, created_at))
            .await
            .expect("client");
    }
    let grant = |grant_id, user_id, user_session_id, created_at| GatewayOauthGrantRecord {
        grant_id,
        client_id: "used".to_string(),
        user_id,
        user_session_id,
        resource: "https://oceans.example.com/mcp".to_string(),
        scope: "mcp:aggregate".to_string(),
        created_at,
        updated_at: created_at,
        revoked_at: None,
        revoked_reason: None,
    };
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    let others = Uuid::new_v4();
    let stale = Uuid::new_v4();
    store
        .create_gateway_oauth_grant(&grant(stale, owner, owner_session, old))
        .await
        .unwrap();
    store
        .create_gateway_oauth_grant(&grant(first, owner, owner_session, old))
        .await
        .unwrap();
    store
        .create_gateway_oauth_grant(&grant(second, owner, owner_session, now))
        .await
        .unwrap();
    store
        .create_gateway_oauth_grant(&grant(others, other, other_session, now))
        .await
        .unwrap();
    // Newer live grants under another issuer, or a resource that only shares
    // a prefix, never list and never take a place in the page.
    for (index, resource) in [
        "https://old.example.com/mcp",
        "https://oceans.example.com/mcp-old",
    ]
    .into_iter()
    .enumerate()
    {
        let grant_id = Uuid::new_v4();
        let created_at = now + Duration::minutes(1);
        store
            .create_gateway_oauth_grant(&GatewayOauthGrantRecord {
                resource: resource.to_string(),
                ..grant(grant_id, owner, owner_session, created_at)
            })
            .await
            .unwrap();
        store
            .create_gateway_oauth_tokens(&[GatewayOauthTokenRecord {
                token_hash: format!("foreign-{index}"),
                grant_id,
                kind: GatewayOauthTokenKind::Access,
                scope: "mcp:aggregate".to_string(),
                expires_at: now + Duration::hours(1),
                created_at,
                consumed_at: None,
            }])
            .await
            .unwrap();
    }
    // `stale` has only an expired access token and a consumed refresh token,
    // so it can never get another token and must not list.
    let hour = Duration::hours(1);
    for (hash, grant_id, kind, created_at, expires_at, consumed_at) in [
        (
            "expired-token",
            first,
            GatewayOauthTokenKind::Access,
            old,
            old + hour,
            None,
        ),
        (
            "live-token",
            first,
            GatewayOauthTokenKind::Access,
            now,
            now + hour,
            None,
        ),
        (
            "second-live",
            second,
            GatewayOauthTokenKind::Access,
            now,
            now + 2 * hour,
            None,
        ),
        (
            "others-live",
            others,
            GatewayOauthTokenKind::Access,
            now,
            now + hour,
            None,
        ),
        // Long expired on the still-live `second`: an access token is no
        // replay marker, so cleanup deletes it.
        (
            "second-old-access",
            second,
            GatewayOauthTokenKind::Access,
            old,
            old + hour,
            None,
        ),
        // Long expired, but `second` is still live, so cleanup keeps it to
        // catch a replay.
        (
            "second-consumed-refresh",
            second,
            GatewayOauthTokenKind::Refresh,
            old,
            old + hour,
            Some(old + Duration::minutes(30)),
        ),
        (
            "stale-access",
            stale,
            GatewayOauthTokenKind::Access,
            now - 3 * hour,
            now - 2 * hour,
            None,
        ),
        (
            "stale-refresh",
            stale,
            GatewayOauthTokenKind::Refresh,
            now - 3 * hour,
            now + Duration::days(10),
            Some(now - 2 * hour),
        ),
    ] {
        store
            .create_gateway_oauth_tokens(&[GatewayOauthTokenRecord {
                token_hash: hash.to_string(),
                grant_id,
                kind,
                scope: "mcp:aggregate".to_string(),
                expires_at,
                created_at,
                consumed_at,
            }])
            .await
            .unwrap();
    }
    // `unused-active` is past the retention window but mid-authorization.
    for (expires_at, client_id) in [
        (old, "used"),
        (now + Duration::minutes(5), "unused-new"),
        (now + Duration::minutes(5), "unused-active"),
    ] {
        store
            .create_gateway_oauth_authorization_request(&GatewayOauthAuthorizationRequestRecord {
                request_id: Uuid::new_v4(),
                client_id: client_id.to_string(),
                redirect_uri: "http://127.0.0.1/callback".to_string(),
                redirect_uri_supplied: true,
                state: None,
                code_challenge: "challenge".to_string(),
                resource: "https://oceans.example.com/mcp".to_string(),
                scope: "mcp:aggregate".to_string(),
                status: GatewayOauthAuthorizationStatus::Pending,
                user_id: None,
                user_session_id: None,
                code_hash: None,
                code_expires_at: None,
                grant_id: None,
                expires_at,
                created_at: expires_at - Duration::minutes(10),
                decided_at: None,
                consumed_at: None,
            })
            .await
            .unwrap();
    }

    // The redeemed code behind `second`, kept for replay detection.
    store
        .create_gateway_oauth_authorization_request(&GatewayOauthAuthorizationRequestRecord {
            request_id: Uuid::new_v4(),
            client_id: "used".to_string(),
            redirect_uri: "http://127.0.0.1/callback".to_string(),
            redirect_uri_supplied: true,
            state: None,
            code_challenge: "challenge".to_string(),
            resource: "https://oceans.example.com/mcp".to_string(),
            scope: "mcp:aggregate".to_string(),
            status: GatewayOauthAuthorizationStatus::Consumed,
            user_id: Some(owner),
            user_session_id: Some(owner_session),
            code_hash: Some("second-code".to_string()),
            code_expires_at: Some(old + Duration::minutes(1)),
            grant_id: Some(second),
            expires_at: old + Duration::minutes(10),
            created_at: old,
            decided_at: Some(old),
            consumed_at: Some(old + Duration::minutes(1)),
        })
        .await
        .unwrap();

    // Newest first; the last token issued counts as the latest activity.
    let listed = store
        .list_live_gateway_oauth_grants_for_user(owner, Some(AGGREGATE), now, 100)
        .await
        .unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|grant| grant.grant_id)
            .collect::<Vec<_>>(),
        vec![second, first]
    );
    assert_eq!(listed[1].client_name.as_deref(), Some("Client used"));
    assert_eq!(listed[1].last_token_issued_at, now);
    assert_eq!(listed[0].last_token_issued_at, now);
    assert_eq!(listed[0].expires_at, now + 2 * hour);
    assert_eq!(listed[1].expires_at, now + hour);

    // The limit keeps the newest grants, with their own token aggregates.
    let page = store
        .list_live_gateway_oauth_grants_for_user(owner, Some(AGGREGATE), now, 1)
        .await
        .unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].grant_id, second);
    // Without an issuer filter, every live grant lists, so dormant grants can
    // still be found and revoked while OAuth is disabled.
    assert_eq!(
        store
            .list_live_gateway_oauth_grants_for_user(owner, None, now, 100)
            .await
            .unwrap()
            .len(),
        4
    );
    assert_eq!(page[0].expires_at, now + 2 * hour);

    // A user cannot revoke someone else's grant.
    assert!(
        store
            .revoke_gateway_oauth_grant_for_user(others, owner, "user_revoked", now)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .revoke_gateway_oauth_grant_for_user(first, owner, "user_revoked", now)
            .await
            .unwrap()
            .is_some_and(|grant| grant.grant_id == first
                && grant.revoked_reason.as_deref() == Some("user_revoked"))
    );
    let listed = store
        .list_live_gateway_oauth_grants_for_user(owner, Some(AGGREGATE), now, 100)
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);

    // A disabled user's grants stop listing even if revoking them never ran.
    store
        .update_user_status(other, UserStatus::Disabled, now)
        .await
        .unwrap();
    assert!(
        store
            .list_live_gateway_oauth_grants_for_user(other, Some(AGGREGATE), now, 100)
            .await
            .unwrap()
            .is_empty()
    );
    store
        .update_user_status(other, UserStatus::Active, now)
        .await
        .unwrap();
    assert_eq!(
        store
            .list_live_gateway_oauth_grants_for_user(other, Some(AGGREGATE), now, 100)
            .await
            .unwrap()
            .len(),
        1
    );

    // Revoking the approving session revokes its grants at once.
    store.revoke_user_session(other_session, now).await.unwrap();
    let ended = store
        .get_gateway_oauth_token_context("others-live")
        .await
        .unwrap()
        .unwrap()
        .grant;
    assert_eq!(ended.revoked_at, Some(now));
    assert_eq!(ended.revoked_reason.as_deref(), Some("session_ended"));
    assert!(
        store
            .list_live_gateway_oauth_grants_for_user(other, Some(AGGREGATE), now, 100)
            .await
            .unwrap()
            .is_empty()
    );

    let counts = store
        .purge_gateway_oauth_records(now - Duration::days(7), now - Duration::days(1), now)
        .await
        .unwrap();
    assert_eq!(
        counts.clients, 1,
        "only the old client with no grant and no authorization in flight"
    );
    assert_eq!(
        counts.tokens, 2,
        "the revoked grant's token and the old access token"
    );
    assert_eq!(counts.authorization_requests, 1);
    assert_eq!(counts.retired_grants, 0, "no approving session has expired");
    assert!(
        store
            .get_gateway_oauth_client("unused-old")
            .await
            .unwrap()
            .is_none()
    );
    // An authorization that loaded the client just before cleanup removed it
    // fails its foreign key; the service maps this conflict to an expired
    // registration.
    let late = GatewayOauthAuthorizationRequestRecord {
        request_id: Uuid::new_v4(),
        client_id: "unused-old".to_string(),
        redirect_uri: "http://127.0.0.1/callback".to_string(),
        redirect_uri_supplied: true,
        state: None,
        code_challenge: "challenge".to_string(),
        resource: "https://oceans.example.com/mcp".to_string(),
        scope: "mcp:aggregate".to_string(),
        status: GatewayOauthAuthorizationStatus::Pending,
        user_id: None,
        user_session_id: None,
        code_hash: None,
        code_expires_at: None,
        grant_id: None,
        expires_at: now + Duration::minutes(10),
        created_at: now,
        decided_at: None,
        consumed_at: None,
    };
    assert!(matches!(
        store
            .create_gateway_oauth_authorization_request(&late)
            .await,
        Err(StoreError::Conflict(_))
    ));
    assert!(
        store
            .get_gateway_oauth_client("unused-active")
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .get_gateway_oauth_client("unused-new")
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .get_gateway_oauth_client("used")
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .get_gateway_oauth_token_context("live-token")
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .get_gateway_oauth_token_context("second-old-access")
            .await
            .unwrap()
            .is_none()
    );
    // Replay markers of a live grant survive cleanup...
    assert!(
        store
            .get_gateway_oauth_token_context("second-consumed-refresh")
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .get_gateway_oauth_authorization_request_by_code_hash("second-code")
            .await
            .unwrap()
            .is_some()
    );
    // ...until the approving session ends and the grant can no longer be used.
    store.revoke_user_session(owner_session, now).await.unwrap();
    let counts = store
        .purge_gateway_oauth_records(now - Duration::days(7), now - Duration::days(1), now)
        .await
        .unwrap();
    assert_eq!((counts.tokens, counts.authorization_requests), (1, 1));
    assert_eq!(
        counts.retired_grants, 0,
        "revoking the session already did it"
    );

    // Grants of a session that expired retire in cleanup, dated to the
    // expiry. One run stops at its batch budget (one batch of two in tests)
    // and the next run finishes.
    let expired_session = Uuid::new_v4();
    store
        .create_user_session(
            expired_session,
            owner,
            "hash-expired",
            now - Duration::days(2),
            now - Duration::days(3),
        )
        .await
        .unwrap();
    for _ in 0..3 {
        store
            .create_gateway_oauth_grant(&grant(
                Uuid::new_v4(),
                owner,
                expired_session,
                now - Duration::days(3),
            ))
            .await
            .unwrap();
    }
    for expected in [2, 1, 0] {
        let counts = store
            .purge_gateway_oauth_records(now - Duration::days(7), now - Duration::days(1), now)
            .await
            .unwrap();
        assert_eq!(counts.retired_grants, expected);
    }
    assert!(
        store
            .list_live_gateway_oauth_grants_for_user(owner, None, now, 100)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .get_gateway_oauth_authorization_request_by_code_hash("second-code")
            .await
            .unwrap()
            .is_none()
    );
}
