//! API key scopes and MCP server lists on both backends (#446).

use crate::tests::{create_postgres_test_database, drop_postgres_test_database};
use crate::{
    GatewayStore, LibsqlStore, PostgresStore, StoreConnectionOptions, run_migrations,
    run_migrations_with_options,
};
use gateway_core::{
    ApiKeyAccess, ApiKeyModelGrantMode, ApiKeyOwnerKind, AuthMode, GlobalRole, NewApiKeyRecord,
    StoreError, UserStatus,
};
use serial_test::serial;
use tempfile::tempdir;
use time::OffsetDateTime;

#[tokio::test]
#[serial]
async fn libsql_api_key_access_round_trips_and_replaces() {
    let tmp = tempdir().expect("tempdir");
    let db_path = tmp.path().join("gateway.db");
    run_migrations(&db_path).await.expect("migrations");
    let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
        .await
        .expect("store");
    exercise_api_key_access(&store).await;
}

#[tokio::test]
#[serial]
async fn postgres_api_key_access_round_trips_and_replaces() {
    let Some(test_db) = create_postgres_test_database().await else {
        eprintln!("skipping postgres API key scope test: TEST_POSTGRES_URL is not set");
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
    exercise_api_key_access(&store).await;
    drop(store);
    drop_postgres_test_database(&test_db).await;
}

fn access(scopes: Option<&[&str]>, servers: Option<&[&str]>) -> ApiKeyAccess {
    let owned = |values: &[&str]| values.iter().map(|value| (*value).to_string()).collect();
    ApiKeyAccess::new(scopes.map(owned), servers.map(owned)).expect("valid access")
}

async fn exercise_api_key_access<S: GatewayStore>(store: &S) {
    let user = store
        .create_identity_user(
            "Scoped owner",
            "scoped@example.com",
            "scoped@example.com",
            GlobalRole::User,
            AuthMode::Password,
            UserStatus::Active,
        )
        .await
        .expect("user");
    let new_key = |public_id: &str, access: ApiKeyAccess| NewApiKeyRecord {
        name: public_id.to_string(),
        public_id: public_id.to_string(),
        secret_hash: "hash".to_string(),
        model_grant_mode: ApiKeyModelGrantMode::All,
        owner_kind: ApiKeyOwnerKind::User,
        owner_user_id: Some(user.user_id),
        owner_team_id: None,
        owner_service_account_id: None,
        access,
        created_at: OffsetDateTime::now_utc(),
    };

    // A key without scopes stores NULLs and reads back as full access.
    let unscoped = store
        .create_api_key(&new_key("unscoped", ApiKeyAccess::default()))
        .await
        .expect("unscoped key");
    assert!(unscoped.access.is_full_access());
    assert_eq!(unscoped.access.mcp_server_keys, None);

    // Create returns what it wrote; lookups read the same access.
    let proxy = access(Some(&["mcp:proxy"]), Some(&["notes", "wiki"]));
    let scoped = store
        .create_api_key(&new_key("scoped", proxy.clone()))
        .await
        .expect("scoped key");
    assert_eq!(scoped.access, proxy);
    let by_public_id = store
        .get_api_key_by_public_id("scoped")
        .await
        .expect("lookup")
        .expect("key");
    assert_eq!(by_public_id.access, proxy);

    // A replacement returns the access it replaced; an unchanged one writes nothing.
    let inference = access(Some(&["inference"]), None);
    let replaced = store
        .replace_api_key_access(scoped.id, &inference)
        .await
        .expect("replace");
    assert_eq!(replaced, Some(proxy));
    assert_eq!(
        store
            .replace_api_key_access(scoped.id, &inference)
            .await
            .expect("resubmit"),
        None
    );
    let restored = store
        .replace_api_key_access(scoped.id, &ApiKeyAccess::default())
        .await
        .expect("restore full access");
    assert_eq!(restored, Some(inference));
    let listed = store.list_api_keys().await.expect("list");
    let scoped_now = listed
        .iter()
        .find(|key| key.id == scoped.id)
        .expect("listed");
    assert!(scoped_now.access.is_full_access());

    // Revoked keys can't be rescoped.
    assert!(
        store
            .revoke_api_key(scoped.id, OffsetDateTime::now_utc())
            .await
            .expect("revoke")
    );
    let error = store
        .replace_api_key_access(scoped.id, &access(Some(&["mcp:proxy"]), None))
        .await
        .expect_err("revoked key");
    assert!(matches!(error, StoreError::Conflict(_)), "{error:?}");
}
