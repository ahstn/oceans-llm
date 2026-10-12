use crate::tests::{create_postgres_test_database, drop_postgres_test_database};
use crate::{
    GatewayStore, LibsqlStore, PostgresStore, StoreConnectionOptions, run_migrations,
    run_migrations_with_options,
};
use futures_util::future::join_all;
use gateway_core::{
    AuthMode, BudgetCadence, BudgetIncreaseRecord, BudgetRecord, BudgetRepository, BudgetScope,
    BudgetSettings, GlobalRole, Money4, StoreError, UserStatus,
};
use serial_test::serial;
use tempfile::tempdir;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

#[tokio::test]
#[serial]
async fn libsql_budget_increases_supersede_and_expire() {
    let tmp = tempdir().expect("tempdir");
    let db_path = tmp.path().join("gateway.db");
    run_migrations(&db_path).await.expect("migrations");
    let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
        .await
        .expect("store");
    exercise_budget_increases(&store).await;
}

#[tokio::test]
#[serial]
async fn postgres_budget_increases_supersede_and_expire() {
    let Some(test_db) = create_postgres_test_database().await else {
        eprintln!("skipping postgres budget increase test: TEST_POSTGRES_URL is not set");
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
    exercise_budget_increases(&store).await;
    drop(store);
    drop_postgres_test_database(&test_db).await;
}

#[tokio::test]
#[serial]
async fn postgres_concurrent_budget_increases_admit_one_create() {
    let Some(test_db) = create_postgres_test_database().await else {
        eprintln!("skipping postgres budget increase test: TEST_POSTGRES_URL is not set");
        return;
    };
    run_migrations_with_options(&StoreConnectionOptions::Postgres {
        url: test_db.database_url.clone(),
        max_connections: 8,
    })
    .await
    .expect("migrations");
    let store = PostgresStore::connect(&test_db.database_url, 8)
        .await
        .expect("store");
    let (user_id, budget, at) = create_user_budget(&store).await;

    // Every request checked eligibility with no increase active. Creates are serialized
    // per budget, so the first wins and the rest see a changed active increase.
    let increases = (1..=8)
        .map(|n| BudgetIncreaseRecord {
            budget_increase_id: Uuid::new_v4(),
            budget_id: budget.budget_id,
            scope_key: budget.scope_key.clone(),
            amount_usd: Money4::from_scaled(n * 10_000),
            starts_at: at,
            expires_at: at + Duration::days(7),
            created_by_user_id: Some(user_id),
            created_at: at,
        })
        .collect::<Vec<_>>();
    let results = join_all(
        increases
            .iter()
            .map(|increase| store.create_budget_increase(increase, None)),
    )
    .await;
    let created = results.iter().filter(|result| result.is_ok()).count();
    assert_eq!(created, 1);
    assert!(
        results
            .iter()
            .all(|result| matches!(result, Ok(()) | Err(StoreError::Conflict(_))))
    );

    let active: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM budget_increases WHERE budget_id = $1 AND superseded_at IS NULL",
    )
    .bind(budget.budget_id.to_string())
    .fetch_one(store.pool())
    .await
    .expect("count");
    assert_eq!(active, 1);

    drop(store);
    drop_postgres_test_database(&test_db).await;
}

async fn create_user_budget<S: GatewayStore + ?Sized>(
    store: &S,
) -> (Uuid, BudgetRecord, OffsetDateTime) {
    let user = store
        .create_identity_user(
            "User",
            "user@example.com",
            "user@example.com",
            GlobalRole::User,
            AuthMode::Password,
            UserStatus::Active,
        )
        .await
        .expect("user");
    let at = OffsetDateTime::from_unix_timestamp(1_773_486_600).expect("timestamp");
    let budget = store
        .upsert_active_budget(
            &BudgetScope::User {
                user_id: user.user_id,
            },
            &BudgetSettings {
                cadence: BudgetCadence::Monthly,
                amount_usd: Money4::from_scaled(1_500_000),
                hard_limit: true,
                timezone: "UTC".to_string(),
            },
            at,
        )
        .await
        .expect("budget");
    (user.user_id, budget, at)
}

async fn exercise_budget_increases<S: GatewayStore + ?Sized>(store: &S) {
    let (user_id, budget, at) = create_user_budget(store).await;
    let increase = |amount: i64, starts_at: OffsetDateTime, days: i64| BudgetIncreaseRecord {
        budget_increase_id: Uuid::new_v4(),
        budget_id: budget.budget_id,
        scope_key: budget.scope_key.clone(),
        amount_usd: Money4::from_scaled(amount),
        starts_at,
        expires_at: starts_at + Duration::days(days),
        created_by_user_id: Some(user_id),
        created_at: starts_at,
    };

    assert!(
        store
            .get_active_budget_increase(budget.budget_id, at)
            .await
            .expect("no increase")
            .is_none()
    );

    let first = increase(750_000, at, 7);
    store
        .create_budget_increase(&first, None)
        .await
        .expect("first");
    let active = store
        .get_active_budget_increase(budget.budget_id, at + Duration::hours(1))
        .await
        .expect("active")
        .expect("first is active");
    assert_eq!(active, first);

    // A later increase replaces the earlier one rather than stacking on it.
    let second = increase(1_500_000, at + Duration::days(1), 3);
    store
        .create_budget_increase(&second, Some(first.budget_increase_id))
        .await
        .expect("second");
    let listed = store
        .list_active_budget_increases(at + Duration::days(2))
        .await
        .expect("list");
    assert_eq!(listed, vec![second.clone()]);
    // Supersession is not retroactive: usage that occurred before the second increase
    // started is still evaluated against the first.
    assert_eq!(
        store
            .list_active_budget_increases(at + Duration::hours(1))
            .await
            .expect("list before second"),
        vec![first.clone()]
    );
    assert_eq!(
        store
            .get_active_budget_increase(budget.budget_id, second.starts_at)
            .await
            .expect("active at second start"),
        Some(second.clone())
    );

    // A caller that checked eligibility against a stale active increase is rejected.
    let stale = increase(250_000, at + Duration::days(2), 3);
    assert!(matches!(
        store.create_budget_increase(&stale, None).await,
        Err(StoreError::Conflict(_))
    ));
    assert_eq!(
        store
            .list_active_budget_increases(at + Duration::days(2))
            .await
            .expect("list after stale"),
        vec![second.clone()]
    );

    // Expiry is exclusive.
    assert!(
        store
            .get_active_budget_increase(budget.budget_id, second.expires_at)
            .await
            .expect("expired")
            .is_none()
    );

    let invalid = BudgetIncreaseRecord {
        amount_usd: Money4::ZERO,
        ..increase(0, at + Duration::days(10), 1)
    };
    assert!(store.create_budget_increase(&invalid, None).await.is_err());
}
