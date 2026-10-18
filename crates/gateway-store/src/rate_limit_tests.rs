use crate::tests::{create_postgres_test_database, drop_postgres_test_database};
use crate::{
    LibsqlStore, PostgresStore, StoreConnectionOptions, run_migrations, run_migrations_with_options,
};
use futures_util::future::join_all;
use gateway_core::{
    RateLimitCounterCharge, RateLimitCounterIncrement, RateLimitMetric, RateLimitOperation,
    RateLimitPeriod, RateLimitPolicyRecord, RateLimitRepository, RateLimitScopeKind,
    RateLimitWindow, StoreError,
};
use serial_test::serial;
use tempfile::tempdir;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

#[tokio::test]
#[serial]
async fn libsql_rate_limit_policies_and_counters() {
    let tmp = tempdir().expect("tempdir");
    let db_path = tmp.path().join("gateway.db");
    run_migrations(&db_path).await.expect("migrations");
    let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
        .await
        .expect("store");
    exercise_policies(&store).await;
    exercise_counters(&store).await;
}

#[tokio::test]
#[serial]
async fn postgres_rate_limit_policies_and_counters() {
    let Some(test_db) = create_postgres_test_database().await else {
        eprintln!("skipping postgres rate limit test: TEST_POSTGRES_URL is not set");
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
    exercise_policies(&store).await;
    exercise_counters(&store).await;
    drop(store);
    drop_postgres_test_database(&test_db).await;
}

/// Two stores with separate pools stand in for two gateway replicas sharing
/// one database. Between them they must admit exactly `limit` requests.
#[tokio::test]
#[serial]
async fn postgres_rate_limit_counters_hold_across_replicas() {
    let Some(test_db) = create_postgres_test_database().await else {
        eprintln!("skipping postgres rate limit test: TEST_POSTGRES_URL is not set");
        return;
    };
    run_migrations_with_options(&StoreConnectionOptions::Postgres {
        url: test_db.database_url.clone(),
        max_connections: 8,
    })
    .await
    .expect("migrations");
    let replica_a = PostgresStore::connect(&test_db.database_url, 8)
        .await
        .expect("replica a");
    let replica_b = PostgresStore::connect(&test_db.database_url, 8)
        .await
        .expect("replica b");
    let now = OffsetDateTime::now_utc();
    // Two scopes per request, listed in opposite orders on the two replicas,
    // so the test also covers lock ordering.
    let key = charge("key:shared", RateLimitPeriod::Day, now, 25);
    let global = charge("global:shared", RateLimitPeriod::Day, now, 1_000);
    let forward = vec![key.clone(), global.clone()];
    let reverse = vec![global, key];

    let results = join_all((0..60).map(|n| {
        let (store, charges) = if n % 2 == 0 {
            (&replica_a, &forward)
        } else {
            (&replica_b, &reverse)
        };
        store.consume_rate_limits(charges)
    }))
    .await;
    let admitted = results
        .iter()
        .map(|result| result.as_ref().expect("consume"))
        .filter(|result| result.admitted)
        .count();
    assert_eq!(admitted, 25);

    let after = replica_a
        .consume_rate_limits(&forward)
        .await
        .expect("consume");
    assert!(!after.admitted);
    // Rejected requests charged neither scope.
    assert_eq!(after.observations[0].current, 26);
    assert_eq!(after.observations[1].current, 26);

    drop((replica_a, replica_b));
    drop_postgres_test_database(&test_db).await;
}

#[tokio::test]
#[serial]
async fn postgres_checks_wait_for_an_uncommitted_insert_into_the_previous_window() {
    let Some(test_db) = create_postgres_test_database().await else {
        eprintln!("skipping postgres rate limit test: TEST_POSTGRES_URL is not set");
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
    let raw = sqlx::PgPool::connect(&test_db.database_url)
        .await
        .expect("raw pool");
    // Early in a window, so most of the previous window still counts.
    let now = OffsetDateTime::now_utc();
    let now = now - Duration::seconds(i64::from(now.second())) + Duration::seconds(1);
    let current = charge("key:boundary", RateLimitPeriod::Minute, now, 5);
    let previous_start = current.window.previous_start.expect("sliding");

    // An old-window request is still inserting the previous window's row.
    let mut old_window = raw.begin().await.expect("begin");
    sqlx::query(
        "INSERT INTO rate_limit_counters (counter_key, window_start, amount, expires_at)
         VALUES ($1, $2, 10, $3)",
    )
    .bind(&current.counter_key)
    .bind(previous_start)
    .bind(current.window.end)
    .execute(&mut *old_window)
    .await
    .expect("insert");

    let check = tokio::spawn({
        let charges = vec![current.clone()];
        async move { store.consume_rate_limits(&charges).await }
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(
        !check.is_finished(),
        "the check did not wait for the insert"
    );
    old_window.commit().await.expect("commit");

    let result = check.await.expect("join").expect("consume");
    assert_eq!(result.observations[0].previous, 10);
    assert!(!result.admitted);

    raw.close().await;
    drop_postgres_test_database(&test_db).await;
}

fn charge(
    key: &str,
    period: RateLimitPeriod,
    now: OffsetDateTime,
    limit: i64,
) -> RateLimitCounterCharge {
    RateLimitCounterCharge {
        counter_key: key.to_string(),
        window: RateLimitWindow::at(period, now),
        limit,
        cost: 1,
    }
}

fn policy(name: &str) -> RateLimitPolicyRecord {
    let now = OffsetDateTime::from_unix_timestamp(OffsetDateTime::now_utc().unix_timestamp())
        .expect("now");
    RateLimitPolicyRecord {
        rate_limit_policy_id: Uuid::new_v4(),
        name: name.to_string(),
        operation: RateLimitOperation::Inference,
        scope_kind: RateLimitScopeKind::User,
        scope_id: Some(Uuid::new_v4()),
        model_key: Some("fast".to_string()),
        mcp_server_id: None,
        metric: RateLimitMetric::Tokens,
        period: RateLimitPeriod::Minute,
        limit: 10_000,
        enabled: true,
        created_at: now,
        updated_at: now,
    }
}

async fn exercise_policies<S: RateLimitRepository>(store: &S) {
    let first = policy("team tokens");
    store
        .insert_rate_limit_policy(&first)
        .await
        .expect("insert");
    let duplicate = policy("team tokens");
    assert!(matches!(
        store.insert_rate_limit_policy(&duplicate).await,
        Err(StoreError::Conflict(_))
    ));
    // The table rejects shapes the API would also refuse.
    let global_with_id = RateLimitPolicyRecord {
        scope_kind: RateLimitScopeKind::Global,
        ..policy("bad global")
    };
    assert!(
        store
            .insert_rate_limit_policy(&global_with_id)
            .await
            .is_err()
    );

    assert_eq!(
        store
            .get_rate_limit_policy(first.rate_limit_policy_id)
            .await
            .expect("get"),
        Some(first.clone())
    );

    let updated = RateLimitPolicyRecord {
        limit: 500,
        enabled: false,
        metric: RateLimitMetric::Requests,
        updated_at: first.updated_at + Duration::seconds(5),
        ..first.clone()
    };
    let saved = store
        .update_rate_limit_policy(&updated)
        .await
        .expect("update")
        .expect("policy exists");
    assert_eq!(saved.current, updated);
    assert_eq!(saved.previous, Some(first.clone()));
    // Saving the same settings again writes nothing, even with a newer timestamp.
    let repeated = store
        .update_rate_limit_policy(&RateLimitPolicyRecord {
            updated_at: updated.updated_at + Duration::seconds(5),
            ..updated.clone()
        })
        .await
        .expect("repeat update")
        .expect("policy exists");
    assert!(!repeated.changed());
    assert_eq!(repeated.current, updated);
    assert_eq!(
        store.list_rate_limit_policies().await.expect("list"),
        vec![updated.clone()]
    );
    // `updated` is paused, so the request-path listing leaves it out.
    assert!(
        store
            .list_enabled_rate_limit_policies()
            .await
            .expect("list enabled")
            .is_empty()
    );

    assert_eq!(
        store
            .delete_rate_limit_policy(first.rate_limit_policy_id)
            .await
            .expect("delete")
            .map(|policy| policy.rate_limit_policy_id),
        Some(first.rate_limit_policy_id)
    );
    assert!(
        store
            .delete_rate_limit_policy(first.rate_limit_policy_id)
            .await
            .expect("delete again")
            .is_none()
    );
    assert!(
        store
            .update_rate_limit_policy(&updated)
            .await
            .expect("update missing")
            .is_none()
    );
}

async fn exercise_counters<S: RateLimitRepository>(store: &S) {
    let minute = OffsetDateTime::from_unix_timestamp(1_800_000_000).expect("time");

    // Limit hit: three admitted, the fourth rejected without being charged.
    let request = charge("user:a", RateLimitPeriod::Minute, minute, 3);
    for expected in 1..=3 {
        let result = store
            .consume_rate_limits(std::slice::from_ref(&request))
            .await
            .expect("consume");
        assert!(result.admitted);
        assert_eq!(result.observations[0].current, expected);
    }
    let rejected = store
        .consume_rate_limits(std::slice::from_ref(&request))
        .await
        .expect("consume");
    assert!(!rejected.admitted);
    assert_eq!(rejected.observations[0].current, 4);

    // A rejection on one scope rolls back the charge on every other scope.
    let other = charge("key:b", RateLimitPeriod::Minute, minute, 100);
    let both = store
        .consume_rate_limits(&[other.clone(), request.clone()])
        .await
        .expect("consume");
    assert!(!both.admitted);
    assert!(both.observations[0].admitted);
    assert!(!both.observations[1].admitted);
    let other_alone = store
        .consume_rate_limits(std::slice::from_ref(&other))
        .await
        .expect("consume");
    assert_eq!(other_alone.observations[0].current, 1);

    // Reset: 45s into the next window, a quarter of the previous 3 still
    // counts (floor 0.75), so three more fit.
    let later = minute + Duration::seconds(105);
    let next = charge("user:a", RateLimitPeriod::Minute, later, 3);
    let admitted = store
        .consume_rate_limits(std::slice::from_ref(&next))
        .await
        .expect("consume");
    assert!(admitted.admitted);
    assert_eq!(admitted.observations[0].previous, 3);
    assert_eq!(admitted.observations[0].current, 1);

    // Tokens: a check-only charge rejects once usage reaches the limit.
    let tokens = RateLimitCounterCharge {
        cost: 0,
        ..charge("tokens:a", RateLimitPeriod::Hour, minute, 1_000)
    };
    let increment = RateLimitCounterIncrement {
        counter_key: tokens.counter_key.clone(),
        window: tokens.window,
        amount: 999,
    };
    store
        .add_rate_limit_usage(std::slice::from_ref(&increment))
        .await
        .expect("add");
    assert!(
        store
            .consume_rate_limits(std::slice::from_ref(&tokens))
            .await
            .expect("check")
            .admitted
    );
    store
        .add_rate_limit_usage(&[RateLimitCounterIncrement {
            amount: 1,
            ..increment
        }])
        .await
        .expect("add");
    let exhausted = store
        .consume_rate_limits(std::slice::from_ref(&tokens))
        .await
        .expect("check");
    assert!(!exhausted.admitted);
    assert_eq!(exhausted.observations[0].current, 1_000);

    // Purge removes rows whose window, and the window that reads them, ended.
    // Postgres also holds empty previous-window rows that request checks lock,
    // so it purges more than the four written here.
    let purged = store
        .purge_expired_rate_limit_counters(minute.unix_timestamp() + 10_000, 100)
        .await
        .expect("purge");
    assert!(purged >= 4, "purged {purged}");
    assert_eq!(
        store
            .purge_expired_rate_limit_counters(minute.unix_timestamp() + 10_000, 100)
            .await
            .expect("purge"),
        0
    );
    let fresh = store
        .consume_rate_limits(std::slice::from_ref(&request))
        .await
        .expect("consume");
    assert_eq!(fresh.observations[0].current, 1);
    // A row reused by a longer window keeps the later expiry, so a purge
    // cannot drop a counter that is still in use.
    let midnight = OffsetDateTime::from_unix_timestamp(1_800_057_600).expect("midnight");
    let short = RateLimitCounterIncrement {
        counter_key: "reused".to_string(),
        window: RateLimitWindow::at(RateLimitPeriod::Minute, midnight),
        amount: 1,
    };
    let long = RateLimitCounterIncrement {
        window: RateLimitWindow::at(RateLimitPeriod::Day, midnight),
        ..short.clone()
    };
    assert_eq!(short.window.start, long.window.start);
    store
        .add_rate_limit_usage(&[short.clone(), long.clone()])
        .await
        .expect("add");
    store
        .purge_expired_rate_limit_counters(short.window.expires_at() + 1, 100)
        .await
        .expect("purge");
    let reused = store
        .consume_rate_limits(&[RateLimitCounterCharge {
            counter_key: long.counter_key,
            window: long.window,
            limit: 100,
            cost: 0,
        }])
        .await
        .expect("check");
    assert_eq!(reused.observations[0].current, 2);
}
