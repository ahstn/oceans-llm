use crate::tests::{create_postgres_test_database, drop_postgres_test_database};
use crate::{
    GatewayStore, LibsqlStore, PostgresStore, StoreConnectionOptions, run_migrations,
    run_migrations_with_options,
};
use gateway_core::{
    AuthMode, GatewayOauthAuthorizationDecision, GatewayOauthAuthorizationRequestRecord,
    GatewayOauthAuthorizationStatus, GatewayOauthClientAuthMethod, GatewayOauthClientRecord,
    GatewayOauthGrantRecord, GatewayOauthRepository, GatewayOauthTokenKind,
    GatewayOauthTokenRecord, GlobalRole, StoreError, UserStatus,
};
use serial_test::serial;
use tempfile::tempdir;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

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
    S: GatewayStore + GatewayOauthRepository,
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
    for (hash, kind) in [
        ("access-hash", GatewayOauthTokenKind::Access),
        ("refresh-hash", GatewayOauthTokenKind::Refresh),
    ] {
        store
            .create_gateway_oauth_token(&GatewayOauthTokenRecord {
                token_hash: hash.to_string(),
                grant_id,
                kind,
                scope: "mcp:aggregate".to_string(),
                expires_at: now + Duration::hours(1),
                created_at: now,
                consumed_at: None,
            })
            .await
            .expect("token");
    }

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

    // Signing out ends the grant through its session.
    store
        .revoke_user_session(session_id, now + Duration::minutes(5))
        .await
        .expect("logout");
    let context = store
        .get_gateway_oauth_token_context("access-hash")
        .await
        .unwrap()
        .unwrap();
    assert!(!context.grant_is_live(now + Duration::minutes(6)));

    assert_eq!(
        store
            .revoke_gateway_oauth_grants_for_user(user.user_id, "user_deactivated", now)
            .await
            .unwrap(),
        1
    );
    assert!(
        !store
            .revoke_gateway_oauth_grant(grant_id, "client_revoked", now)
            .await
            .unwrap()
    );
    let context = store
        .get_gateway_oauth_token_context("access-hash")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(context.grant.revoked_at, Some(now));
    assert_eq!(
        context.grant.revoked_reason.as_deref(),
        Some("user_deactivated")
    );
}
