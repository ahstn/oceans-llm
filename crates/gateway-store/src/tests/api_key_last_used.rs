use super::*;
use crate::AnyStore;

#[tokio::test]
#[serial]
async fn libsql_api_key_last_used_is_throttled() {
    let tmp = tempdir().expect("temporary test database directory");
    let path = tmp.path().join("gateway.db");
    run_migrations(&path).await.expect("libSQL migrations");
    let store = LibsqlStore::new_local(path.to_str().expect("UTF-8 database path"))
        .await
        .expect("libSQL store");
    exercise_api_key_last_used(&AnyStore::Libsql(store)).await;
}

#[tokio::test]
#[serial]
async fn postgres_api_key_last_used_is_throttled() {
    let Some(test_db) = create_postgres_test_database().await else {
        eprintln!("skipping postgres last-used test because TEST_POSTGRES_URL is not set");
        return;
    };
    let options = StoreConnectionOptions::Postgres {
        url: test_db.database_url.clone(),
        max_connections: 4,
    };
    run_migrations_with_options(&options)
        .await
        .expect("Postgres migrations");
    let store = AnyStore::Postgres(
        PostgresStore::connect(&test_db.database_url, 4)
            .await
            .expect("Postgres store"),
    );
    exercise_api_key_last_used(&store).await;
    drop(store);
    drop_postgres_test_database(&test_db).await;
}

async fn exercise_api_key_last_used(store: &AnyStore) {
    use gateway_core::{AdminApiKeyRepository, NewApiKeyRecord};

    let user = store
        .create_identity_user(
            "last-used@example.com",
            "last-used@example.com",
            "Last Used",
            GlobalRole::User,
            AuthMode::Password,
            UserStatus::Active,
        )
        .await
        .expect("key owner");
    let key = store
        .create_api_key(&NewApiKeyRecord {
            name: "Last Used".to_string(),
            public_id: "lastused".to_string(),
            secret_hash: "hash".to_string(),
            model_grant_mode: gateway_core::ApiKeyModelGrantMode::All,
            owner_kind: ApiKeyOwnerKind::User,
            owner_user_id: Some(user.user_id),
            owner_team_id: None,
            owner_service_account_id: None,
            access: Default::default(),
            created_at: OffsetDateTime::now_utc(),
        })
        .await
        .expect("create test key");
    assert!(key.last_used_at.is_none());

    let before = OffsetDateTime::now_utc().unix_timestamp();
    store
        .touch_api_key_last_used(key.id)
        .await
        .expect("first use");
    let first = store
        .get_api_key_by_public_id(&key.public_id)
        .await
        .expect("load first use")
        .expect("test key exists")
        .last_used_at
        .expect("first use was recorded");
    assert!(first.unix_timestamp() >= before);

    // A future value is also recent. Unlike two calls in the same second, this
    // detects an unconditional UPDATE without a wall-clock sleep.
    let recent = OffsetDateTime::now_utc().unix_timestamp() + 30;
    set_last_used(store, key.id, recent).await;
    store
        .touch_api_key_last_used(key.id)
        .await
        .expect("recent use");
    let after_recent = store
        .get_api_key_by_public_id(&key.public_id)
        .await
        .expect("load recent use")
        .expect("test key exists");
    assert_eq!(
        after_recent.last_used_at.map(|at| at.unix_timestamp()),
        Some(recent)
    );

    let stale = OffsetDateTime::now_utc().unix_timestamp()
        - gateway_core::traits::API_KEY_LAST_USED_INTERVAL_SECONDS;
    set_last_used(store, key.id, stale).await;
    let mut requests = tokio::task::JoinSet::new();
    for _ in 0..16 {
        let store = store.clone();
        let id = key.id;
        requests.spawn(async move { store.touch_api_key_last_used(id).await });
    }
    while let Some(result) = requests.join_next().await {
        result
            .expect("concurrent request task")
            .expect("concurrent touch");
    }
    let after_stale = store
        .get_api_key_by_public_id(&key.public_id)
        .await
        .expect("load stale use")
        .expect("test key exists")
        .last_used_at
        .expect("stale use was updated");
    assert!(after_stale.unix_timestamp() > stale);
}

async fn set_last_used(store: &AnyStore, id: Uuid, timestamp: i64) {
    match store {
        AnyStore::Libsql(store) => {
            store
                .connection()
                .execute(
                    "UPDATE api_keys SET last_used_at = ?1 WHERE id = ?2",
                    libsql::params![timestamp, id.to_string()],
                )
                .await
                .expect("set libSQL last-used fixture");
        }
        AnyStore::Postgres(store) => {
            sqlx::query("UPDATE api_keys SET last_used_at = $1 WHERE id = $2")
                .bind(timestamp)
                .bind(id.to_string())
                .execute(store.pool())
                .await
                .expect("set Postgres last-used fixture");
        }
    }
}
