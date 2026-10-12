use crate::tests::{create_postgres_test_database, drop_postgres_test_database};
use crate::{
    GatewayStore, LibsqlStore, PostgresStore, StoreConnectionOptions, run_migrations,
    run_migrations_with_options,
};
use gateway_core::{
    AuthMode, BudgetCadence, BudgetIncreaseRecord, BudgetScope, BudgetSettings, GlobalRole, Money4,
    UserStatus,
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

async fn exercise_budget_increases<S: GatewayStore + ?Sized>(store: &S) {
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
    let increase = |amount: i64, starts_at: OffsetDateTime, days: i64| BudgetIncreaseRecord {
        budget_increase_id: Uuid::new_v4(),
        budget_id: budget.budget_id,
        scope_key: budget.scope_key.clone(),
        amount_usd: Money4::from_scaled(amount),
        starts_at,
        expires_at: starts_at + Duration::days(days),
        created_by_user_id: Some(user.user_id),
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
    store.create_budget_increase(&first).await.expect("first");
    let active = store
        .get_active_budget_increase(budget.budget_id, at + Duration::hours(1))
        .await
        .expect("active")
        .expect("first is active");
    assert_eq!(active, first);

    // A later increase replaces the earlier one rather than stacking on it.
    let second = increase(1_500_000, at + Duration::days(1), 3);
    store.create_budget_increase(&second).await.expect("second");
    let listed = store
        .list_active_budget_increases(at + Duration::days(2))
        .await
        .expect("list");
    assert_eq!(listed, vec![second.clone()]);
    // The superseded increase no longer applies, even inside its original window.
    assert!(
        store
            .list_active_budget_increases(at + Duration::hours(1))
            .await
            .expect("list before second")
            .is_empty()
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
    assert!(store.create_budget_increase(&invalid).await.is_err());
}
