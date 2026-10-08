use super::*;

async fn exercise_postgres_shared_cooldown_locks(
    store: Arc<PostgresStore>,
    lock_pool: &sqlx::PgPool,
) {
    let mut request = cooldown_request(RouteSelectionMode::First);
    request.affinity_key = None;
    let key = request.candidates[0]
        .cooldown_key
        .clone()
        .expect("cooldown key");
    let mut blocker = lock_pool.begin().await.expect("blocker transaction");
    sqlx::query(
        "SELECT pg_advisory_xact_lock_shared(
            hashtextextended('oceans:model_route_cooldown:' || $1, 0)
        )",
    )
    .bind(&key)
    .execute(&mut *blocker)
    .await
    .expect("hold shared cooldown lock");
    let selected =
        tokio::time::timeout(StdDuration::from_secs(2), store.select_route(&request)).await;

    let failure = RouteFailureRecord {
        cooldown_key: Some(key),
        cooldown_until: request.now + Duration::seconds(60),
        binding: None,
    };
    let writer_store = Arc::clone(&store);
    let writer = tokio::spawn(async move { writer_store.record_route_failure(&failure).await });
    let writer_waited = tokio::time::timeout(StdDuration::from_secs(2), async {
        loop {
            let waiting = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS (
                    SELECT 1 FROM pg_locks AS waiting
                    WHERE waiting.locktype = 'advisory'
                        AND waiting.mode = 'ExclusiveLock' AND NOT waiting.granted
                        AND (waiting.database, waiting.classid, waiting.objid, waiting.objsubid) IN (
                            SELECT database, classid, objid, objsubid FROM pg_locks
                            WHERE pid = pg_backend_pid() AND locktype = 'advisory'
                                AND mode = 'ShareLock' AND granted
                        )
                )",
            )
            .fetch_one(&mut *blocker)
            .await
            .expect("observe waiting cooldown writer");
            if waiting {
                break;
            }
            tokio::time::sleep(StdDuration::from_millis(10)).await;
        }
    })
    .await;
    let pending_before_release = !writer.is_finished();
    blocker
        .commit()
        .await
        .expect("release shared cooldown lock");
    let recorded = tokio::time::timeout(StdDuration::from_secs(2), writer).await;

    assert_eq!(
        selected
            .expect("healthy selection must share the cooldown lock")
            .expect("healthy selection")
            .expect("route A")
            .route_id,
        request.candidates[0].route_id
    );
    writer_waited.expect("failure writer must wait on the held shared lock");
    assert!(
        pending_before_release,
        "failure cannot finish while a reader holds the lock"
    );
    recorded
        .expect("failure completes after reader releases lock")
        .expect("failure task")
        .expect("record cooldown");
    assert_eq!(
        tokio::time::timeout(StdDuration::from_secs(2), store.select_route(&request))
            .await
            .expect("selection after failure must finish")
            .expect("selection after failure")
            .expect("route B")
            .route_id,
        request.candidates[1].route_id
    );
}

async fn exercise_postgres_cursor_bypass(store: &PostgresStore, lock_pool: &sqlx::PgPool) {
    let mut request = cooldown_request(RouteSelectionMode::RoundRobin);
    let original = store
        .select_route(&request)
        .await
        .expect("initial round robin")
        .expect("route A");
    let pool_key = crate::routing::routing_pool_key(&request).expect("routing pool");
    let mut blocker = lock_pool.begin().await.expect("cursor blocker transaction");
    sqlx::query("SELECT next_index FROM model_routing_cursors WHERE pool_key = $1 FOR UPDATE")
        .bind(pool_key.to_string())
        .fetch_one(&mut *blocker)
        .await
        .expect("hold round-robin cursor row");
    let reused =
        tokio::time::timeout(StdDuration::from_secs(2), store.select_route(&request)).await;
    request.mode = RouteSelectionMode::First;
    request.affinity_key = Some("caller:first-mode-session".to_string());
    let first = tokio::time::timeout(StdDuration::from_secs(2), store.select_route(&request)).await;
    blocker.commit().await.expect("release cursor row");

    let reused = reused
        .expect("live affinity must not wait for cursor")
        .expect("reuse binding")
        .expect("route A");
    assert!(reused.reused);
    assert_eq!(reused.binding, original.binding);
    let first = first
        .expect("First placement must not wait for cursor")
        .expect("First placement")
        .expect("route A");
    assert!(!first.reused);
    assert_eq!(first.route_id, request.candidates[0].route_id);

    let fresh = cooldown_request(RouteSelectionMode::First);
    tokio::time::timeout(StdDuration::from_secs(2), store.select_route(&fresh))
        .await
        .expect("fresh First placement must finish")
        .expect("fresh First placement")
        .expect("route A");
    let rows = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM model_routing_cursors WHERE pool_key = $1",
    )
    .bind(
        crate::routing::routing_pool_key(&fresh)
            .expect("fresh pool")
            .to_string(),
    )
    .fetch_one(lock_pool)
    .await
    .expect("count First pool cursors");
    assert_eq!(rows, 0, "First placement must not create a cursor");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn postgres_routing_readers_share_cooldown_locks_and_bypass_unused_cursors() {
    let Some(test_db) = create_postgres_test_database().await else {
        eprintln!("skipping postgres routing lock test: TEST_POSTGRES_URL is not set");
        return;
    };
    run_migrations_with_options(&StoreConnectionOptions::Postgres {
        url: test_db.database_url.clone(),
        max_connections: 4,
    })
    .await
    .expect("migrations");
    let store = Arc::new(
        PostgresStore::connect(&test_db.database_url, 4)
            .await
            .expect("store"),
    );
    let lock_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&test_db.database_url)
        .await
        .expect("independent lock connection");
    exercise_postgres_shared_cooldown_locks(Arc::clone(&store), &lock_pool).await;
    exercise_postgres_cursor_bypass(store.as_ref(), &lock_pool).await;
    lock_pool.close().await;
    drop(store);
    drop_postgres_test_database(&test_db).await;
}
