use std::{
    sync::Arc,
    time::{Duration as StdDuration, Instant},
};

use gateway_core::{
    ResponseRouteOrigin, RouteFailureRecord, RouteSelectionMode, RouteSelectionRequest,
    RoutingCandidate, RoutingRepository, StoreError,
};
use serial_test::serial;
use tempfile::tempdir;
use time::{Duration, OffsetDateTime};
use tokio::{sync::Barrier, task::JoinSet};
use uuid::Uuid;

use super::{create_postgres_test_database, drop_postgres_test_database};
use crate::{
    LibsqlStore, PostgresStore, StoreConnectionOptions, run_migrations, run_migrations_with_options,
};

mod postgres_locks;

fn selection_request(mode: RouteSelectionMode) -> RouteSelectionRequest {
    RouteSelectionRequest {
        model_id: Uuid::new_v4(),
        affinity_key: Some("caller:conversation".to_string()),
        candidates: [10, 10, 20]
            .into_iter()
            .enumerate()
            .map(|(index, priority)| RoutingCandidate {
                route_id: Uuid::from_u128(index as u128 + 1),
                fingerprint: Uuid::new_v4().to_string(),
                cooldown_key: None,
                priority,
            })
            .collect(),
        mode,
        now: OffsetDateTime::from_unix_timestamp(1_800_000_000).expect("timestamp"),
        idle_timeout_seconds: 3_600,
    }
}

fn cooldown_request(mode: RouteSelectionMode) -> RouteSelectionRequest {
    let mut request = selection_request(mode);
    for candidate in &mut request.candidates {
        candidate.cooldown_key = Some(Uuid::new_v4().to_string());
    }
    request
}

async fn exercise_idle_expiry<S: RoutingRepository>(store: &S) {
    let mut request = selection_request(RouteSelectionMode::First);
    let start = request.now;
    let first = store
        .select_route(&request)
        .await
        .expect("initial route")
        .expect("eligible route");
    assert_eq!(first.route_id, request.candidates[0].route_id);
    assert!(!first.reused);
    let original = first.binding.expect("initial binding");

    request.candidates.swap(0, 1);
    request.now = start + Duration::seconds(3_599);
    let reused = store
        .select_route(&request)
        .await
        .expect("live binding")
        .expect("eligible route");
    assert!(reused.reused);
    assert_eq!(reused.route_id, original.route_id);
    assert_eq!(reused.binding.as_ref(), Some(&original));

    // Merely selecting the binding does not extend its idle deadline.
    request.now = start + Duration::seconds(3_600);
    let expired = store
        .select_route(&request)
        .await
        .expect("expired binding")
        .expect("eligible route");
    assert!(!expired.reused);
    assert_eq!(expired.route_id, request.candidates[0].route_id);
    let replacement = expired.binding.expect("replacement binding");
    assert_ne!(replacement.token, original.token);

    store
        .refresh_route_binding(&replacement, start + Duration::seconds(3_700))
        .await
        .expect("successful request refresh");
    request.now = start + Duration::seconds(7_299);
    let refreshed = store
        .select_route(&request)
        .await
        .expect("refreshed binding")
        .expect("eligible route");
    assert!(refreshed.reused);
    assert_eq!(refreshed.binding.as_ref(), Some(&replacement));
    request.now = start + Duration::seconds(7_300);
    assert!(
        !store
            .select_route(&request)
            .await
            .expect("refreshed binding expiry")
            .expect("eligible route")
            .reused
    );
}

async fn exercise_refresh_guards<S: RoutingRepository>(store: &S) {
    let mut request = selection_request(RouteSelectionMode::First);
    let start = request.now;
    let initial = store
        .select_route(&request)
        .await
        .expect("initial route")
        .expect("eligible route")
        .binding
        .expect("initial binding");
    store
        .refresh_route_binding(&initial, start + Duration::seconds(3_500))
        .await
        .expect("later refresh");
    store
        .refresh_route_binding(&initial, start + Duration::seconds(2_000))
        .await
        .expect("out-of-order refresh");
    request.now = start + Duration::seconds(7_000);
    let retained = store
        .select_route(&request)
        .await
        .expect("monotonic deadline")
        .expect("eligible route");
    assert!(retained.reused);
    assert_eq!(retained.binding.as_ref(), Some(&initial));

    request.now = start + Duration::seconds(7_100);
    let replacement = store
        .select_route(&request)
        .await
        .expect("replace expired binding")
        .expect("eligible route")
        .binding
        .expect("replacement binding");
    assert_eq!(replacement.route_id, initial.route_id);
    assert_ne!(replacement.token, initial.token);
    store
        .refresh_route_binding(&initial, start + Duration::seconds(10_000))
        .await
        .expect("stale refresh is ignored");

    let mut wrong_route = replacement.clone();
    wrong_route.route_id = request.candidates[1].route_id;
    store
        .refresh_route_binding(&wrong_route, start + Duration::seconds(10_000))
        .await
        .expect("wrong route refresh is ignored");
    request.now = start + Duration::seconds(10_700);
    let expired = store
        .select_route(&request)
        .await
        .expect("guarded deadline")
        .expect("eligible route");
    assert!(!expired.reused);
    assert_ne!(
        expired.binding.expect("new binding").token,
        replacement.token
    );
}

async fn exercise_candidate_changes<S: RoutingRepository>(store: &S) {
    let mut request = selection_request(RouteSelectionMode::First);
    let initial = store
        .select_route(&request)
        .await
        .expect("initial route")
        .expect("eligible route");
    let original = initial.binding.expect("initial binding");

    // A priority change affects new placements, but keeps a valid live binding.
    request.candidates[0].priority = 30;
    let retained = store
        .select_route(&request)
        .await
        .expect("priority update")
        .expect("eligible route");
    assert!(retained.reused);
    assert_eq!(retained.binding.as_ref(), Some(&original));
    let mut other_session = request.clone();
    other_session.affinity_key = Some("caller:another-conversation".to_string());
    let preferred = store
        .select_route(&other_session)
        .await
        .expect("new placement after priority update")
        .expect("eligible route");
    assert_eq!(preferred.route_id, request.candidates[1].route_id);

    request.candidates[0].priority = 10;
    request.candidates[0].fingerprint = "changed-account-or-endpoint".to_string();
    let changed = store
        .select_route(&request)
        .await
        .expect("changed fingerprint")
        .expect("eligible route");
    assert!(!changed.reused);
    assert_eq!(changed.route_id, original.route_id);
    let changed_receipt = changed.binding.expect("changed binding");
    assert_ne!(changed_receipt.token, original.token);

    request.candidates.remove(0);
    let removed = store
        .select_route(&request)
        .await
        .expect("removed route")
        .expect("eligible route");
    assert!(!removed.reused);
    assert_eq!(removed.route_id, request.candidates[0].route_id);
    assert_ne!(
        removed.binding.expect("remaining route binding").token,
        changed_receipt.token
    );

    // The same caller/conversation key is independent for each Oceans model.
    request.model_id = Uuid::new_v4();
    let other_model = store
        .select_route(&request)
        .await
        .expect("other model")
        .expect("eligible route");
    assert!(!other_model.reused);
    assert_eq!(
        other_model.binding.expect("other model binding").model_id,
        request.model_id
    );
}

async fn exercise_shared_round_robin<S: RoutingRepository>(first: &S, second: &S) {
    let mut request = selection_request(RouteSelectionMode::RoundRobin);
    request.affinity_key = None;
    for (index, store) in [first, second, first, second].into_iter().enumerate() {
        let selected = store
            .select_route(&request)
            .await
            .expect("shared cursor")
            .expect("eligible route");
        assert_eq!(selected.route_id, request.candidates[index % 2].route_id);
        assert!(selected.binding.is_none());
        assert!(!selected.reused);
    }

    request.affinity_key = Some("caller:first-sticky-session".to_string());
    let bound = first
        .select_route(&request)
        .await
        .expect("sticky round robin")
        .expect("eligible route");
    assert_eq!(bound.route_id, request.candidates[0].route_id);
    let reused = second
        .select_route(&request)
        .await
        .expect("shared binding")
        .expect("eligible route");
    assert!(reused.reused);
    assert_eq!(reused.binding, bound.binding);
    request.affinity_key = Some("caller:second-sticky-session".to_string());
    let next = second
        .select_route(&request)
        .await
        .expect("next placement")
        .expect("eligible route");
    assert_eq!(next.route_id, request.candidates[1].route_id);

    // A smaller eligible pool uses its own cursor and excludes the fallback tier.
    request.candidates.remove(0);
    request.affinity_key = None;
    for store in [first, second] {
        assert_eq!(
            store
                .select_route(&request)
                .await
                .expect("smaller pool")
                .expect("eligible route")
                .route_id,
            request.candidates[0].route_id
        );
    }
}

async fn exercise_round_robin_eligible_pools<S: RoutingRepository>(first: &S, second: &S) {
    let mut full_pool = selection_request(RouteSelectionMode::RoundRobin);
    full_pool.affinity_key = None;
    for candidate in &mut full_pool.candidates {
        candidate.priority = 10;
    }
    let mut subset = full_pool.clone();
    subset.candidates.remove(0);

    for index in 0..6 {
        let (full_store, subset_store) = if index % 2 == 0 {
            (first, second)
        } else {
            (second, first)
        };
        let full_selection = full_store
            .select_route(&full_pool)
            .await
            .expect("full pool round robin")
            .expect("eligible route");
        assert_eq!(full_selection.route_id, Uuid::from_u128(index % 3 + 1));
        let subset_selection = subset_store
            .select_route(&subset)
            .await
            .expect("subset round robin")
            .expect("eligible route");
        assert_eq!(subset_selection.route_id, Uuid::from_u128(index % 2 + 2));

        // Neither candidate order nor another eligible set may alter this pool's cycle.
        full_pool.candidates.rotate_left(1);
        subset.candidates.reverse();
    }
}

async fn exercise_concurrent_binding<S: RoutingRepository + 'static>(
    first: Arc<S>,
    second: Arc<S>,
) {
    let mut request = selection_request(RouteSelectionMode::RoundRobin);
    let barrier = Arc::new(Barrier::new(13));
    let mut tasks = JoinSet::new();
    for index in 0..12 {
        let store = Arc::clone(if index % 2 == 0 { &first } else { &second });
        let request = request.clone();
        let barrier = Arc::clone(&barrier);
        tasks.spawn(async move {
            barrier.wait().await;
            store
                .select_route(&request)
                .await
                .expect("concurrent selection")
                .expect("eligible route")
        });
    }
    barrier.wait().await;
    let mut receipt = None;
    let mut placements = 0;
    while let Some(result) = tasks.join_next().await {
        let selected = result.expect("selection task");
        assert_eq!(selected.route_id, request.candidates[0].route_id);
        placements += usize::from(!selected.reused);
        let binding = selected.binding.expect("shared session binding");
        if let Some(expected) = &receipt {
            assert_eq!(&binding, expected);
        } else {
            receipt = Some(binding);
        }
    }
    assert_eq!(placements, 1, "only one first turn may advance the cursor");
    request.affinity_key = Some("caller:next-conversation".to_string());
    assert_eq!(
        second
            .select_route(&request)
            .await
            .expect("next session")
            .expect("eligible route")
            .route_id,
        request.candidates[1].route_id
    );
}

async fn exercise_shared_cooldowns<S: RoutingRepository>(first: &S, second: &S) {
    let mut request = cooldown_request(RouteSelectionMode::First);
    first
        .select_route(&request)
        .await
        .expect("initial binding")
        .expect("route A");
    let cooldown_until = request.now + Duration::seconds(60);
    first
        .record_route_failure(&RouteFailureRecord {
            cooldown_key: request.candidates[0].cooldown_key.clone(),
            cooldown_until,
            binding: None,
        })
        .await
        .expect("shared route failure");

    // Cooldown applies even to a live binding not invalidated by this failure.
    for affinity_key in [
        request.affinity_key.clone(),
        Some("new-session".to_string()),
        None,
    ] {
        request.affinity_key = affinity_key;
        let selected = second
            .select_route(&request)
            .await
            .expect("exclude cooled route")
            .expect("route B");
        assert_eq!(selected.route_id, request.candidates[1].route_id);
        assert!(!selected.reused);
    }
    request.mode = RouteSelectionMode::RoundRobin;
    assert_eq!(
        second
            .select_route(&request)
            .await
            .expect("round robin excludes cooldown")
            .map(|selection| selection.route_id),
        Some(request.candidates[1].route_id)
    );
    for (index, expected) in [(1, Some(request.candidates[2].route_id)), (2, None)] {
        first
            .record_route_failure(&RouteFailureRecord {
                cooldown_key: request.candidates[index].cooldown_key.clone(),
                cooldown_until,
                binding: None,
            })
            .await
            .expect("cool remaining route");
        for mode in [RouteSelectionMode::First, RouteSelectionMode::RoundRobin] {
            request.mode = mode;
            assert_eq!(
                second
                    .select_route(&request)
                    .await
                    .expect("fallback or exhausted pool")
                    .map(|selection| selection.route_id),
                expected
            );
        }
    }
}

async fn exercise_cooldown_expiry_and_scope<S: RoutingRepository>(first: &S, second: &S) {
    let mut request = cooldown_request(RouteSelectionMode::First);
    request.affinity_key = None;
    let deadline = request.now + Duration::seconds(60);
    let failure = RouteFailureRecord {
        cooldown_key: request.candidates[0].cooldown_key.clone(),
        cooldown_until: deadline,
        binding: None,
    };
    first
        .record_route_failure(&failure)
        .await
        .expect("initial cooldown");
    second
        .record_route_failure(&RouteFailureRecord {
            cooldown_until: deadline - Duration::seconds(30),
            ..failure.clone()
        })
        .await
        .expect("older failure cannot shorten cooldown");
    request.now = deadline - Duration::seconds(1);
    for (cooldown_key, expected) in [
        (failure.cooldown_key.clone(), request.candidates[1].route_id),
        (
            Some(Uuid::new_v4().to_string()),
            request.candidates[0].route_id,
        ),
        (None, request.candidates[0].route_id),
    ] {
        let mut scoped = request.clone();
        scoped.candidates[0].cooldown_key = cooldown_key;
        assert_eq!(
            second
                .select_route(&scoped)
                .await
                .expect("cooldown scope")
                .map(|selection| selection.route_id),
            Some(expected)
        );
    }
    request.now = deadline;
    assert_eq!(
        second
            .select_route(&request)
            .await
            .expect("cooldown expiry boundary")
            .map(|selection| selection.route_id),
        Some(request.candidates[0].route_id)
    );
}

async fn exercise_failure_receipt_guards<S: RoutingRepository>(first: &S, second: &S) {
    let mut request = cooldown_request(RouteSelectionMode::First);
    request.idle_timeout_seconds = 60;
    let start = request.now;
    let original = first
        .select_route(&request)
        .await
        .expect("initial selection")
        .expect("route A")
        .binding
        .expect("initial receipt");
    let mut wrong_route = original.clone();
    wrong_route.route_id = request.candidates[1].route_id;
    let mut wrong_token = original.clone();
    wrong_token.token = Uuid::new_v4();
    for receipt in [wrong_route, wrong_token] {
        second
            .record_route_failure(&RouteFailureRecord {
                cooldown_key: None,
                cooldown_until: start + Duration::seconds(120),
                binding: Some(receipt),
            })
            .await
            .expect("mismatched failure receipt");
        assert_eq!(
            first
                .select_route(&request)
                .await
                .expect("guarded binding")
                .expect("route A")
                .binding,
            Some(original.clone())
        );
    }
    second
        .record_route_failure(&RouteFailureRecord {
            cooldown_key: None,
            cooldown_until: start + Duration::seconds(120),
            binding: Some(original.clone()),
        })
        .await
        .expect("matching receipt invalidates without cooldown");
    let renewed = first
        .select_route(&request)
        .await
        .expect("rebind still-eligible route")
        .expect("route A");
    assert!(!renewed.reused);
    let renewed = renewed.binding.expect("new route A receipt");
    assert_eq!(renewed.route_id, original.route_id);
    assert_ne!(renewed.token, original.token);
    let original = renewed;
    let failure = RouteFailureRecord {
        cooldown_key: request.candidates[0].cooldown_key.clone(),
        cooldown_until: start + Duration::seconds(120),
        binding: Some(original.clone()),
    };
    first
        .record_route_failure(&failure)
        .await
        .expect("invalidate failed binding");
    request.now = start + Duration::seconds(1);
    let replacement = second
        .select_route(&request)
        .await
        .expect("replacement selection")
        .expect("route B")
        .binding
        .expect("replacement receipt");
    assert_eq!(replacement.route_id, request.candidates[1].route_id);
    assert_ne!(replacement.token, original.token);
    second
        .refresh_route_binding(&replacement, start + Duration::seconds(10))
        .await
        .expect("replacement success");
    first
        .refresh_route_binding(&original, start + Duration::seconds(1_000))
        .await
        .expect("late original success");
    first
        .record_route_failure(&failure)
        .await
        .expect("late original failure");
    request.now = start + Duration::seconds(69);
    assert_eq!(
        second
            .select_route(&request)
            .await
            .expect("replacement survives stale completion")
            .expect("route B")
            .binding,
        Some(replacement.clone())
    );
    request.now = start + Duration::seconds(70);
    let expired = second
        .select_route(&request)
        .await
        .expect("replacement expiry")
        .expect("route B");
    assert!(!expired.reused);
    assert_ne!(
        expired.binding.expect("new receipt").token,
        replacement.token
    );
}

async fn exercise_concurrent_failover<S: RoutingRepository + 'static>(
    first: Arc<S>,
    second: Arc<S>,
) {
    let mut request = cooldown_request(RouteSelectionMode::RoundRobin);
    for candidate in &mut request.candidates {
        candidate.priority = 10;
    }
    let original = first
        .select_route(&request)
        .await
        .expect("initial selection")
        .expect("route A");
    let failure = RouteFailureRecord {
        cooldown_key: request.candidates[0].cooldown_key.clone(),
        cooldown_until: request.now + Duration::seconds(60),
        binding: original.binding,
    };
    let barrier = Arc::new(Barrier::new(13));
    let mut tasks = JoinSet::new();
    for index in 0..12 {
        let store = Arc::clone(if index % 2 == 0 { &first } else { &second });
        let request = request.clone();
        let failure = failure.clone();
        let barrier = Arc::clone(&barrier);
        tasks.spawn(async move {
            barrier.wait().await;
            store
                .record_route_failure(&failure)
                .await
                .expect("duplicate failure");
            store
                .select_route(&request)
                .await
                .expect("concurrent failover")
                .expect("route B")
        });
    }
    barrier.wait().await;
    let mut receipt = None;
    let mut placements = 0;
    while let Some(result) = tasks.join_next().await {
        let selected = result.expect("failover task");
        assert_eq!(selected.route_id, request.candidates[1].route_id);
        placements += usize::from(!selected.reused);
        let binding = selected.binding.expect("replacement receipt");
        if let Some(expected) = &receipt {
            assert_eq!(&binding, expected);
        } else {
            receipt = Some(binding);
        }
    }
    assert_eq!(
        placements, 1,
        "duplicate failures must create one replacement"
    );
    request.affinity_key = Some("caller:after-failover".to_string());
    assert_eq!(
        second
            .select_route(&request)
            .await
            .expect("next placement")
            .map(|selection| selection.route_id),
        Some(request.candidates[2].route_id),
        "only one replacement may advance the remaining pool's cursor"
    );
}

async fn exercise_response_origins<S: RoutingRepository>(first: &S, second: &S) {
    let request = selection_request(RouteSelectionMode::First);
    let now = request.now;
    let origin = ResponseRouteOrigin {
        model_id: request.model_id,
        route_id: request.candidates[0].route_id,
        fingerprint: request.candidates[0].fingerprint.clone(),
    };
    first
        .record_response_route_origin("caller-a", "response-hash", &origin, now)
        .await
        .expect("record origin");
    assert_eq!(
        second
            .get_response_route_origin("caller-a", "response-hash", now)
            .await
            .expect("shared origin"),
        Some(origin.clone())
    );
    assert_eq!(
        second
            .get_response_route_origin("caller-b", "response-hash", now)
            .await
            .expect("caller isolation"),
        None
    );

    let other_origin = ResponseRouteOrigin {
        route_id: request.candidates[1].route_id,
        fingerprint: request.candidates[1].fingerprint.clone(),
        ..origin.clone()
    };
    second
        .record_response_route_origin("caller-b", "response-hash", &other_origin, now)
        .await
        .expect("other caller may own the same response ID");
    for conflicting in [
        other_origin.clone(),
        ResponseRouteOrigin {
            fingerprint: "different-credentials".to_string(),
            ..origin.clone()
        },
        ResponseRouteOrigin {
            model_id: Uuid::new_v4(),
            ..origin.clone()
        },
    ] {
        let conflict = second
            .record_response_route_origin("caller-a", "response-hash", &conflicting, now)
            .await;
        assert!(matches!(conflict, Err(StoreError::Conflict(_))));
    }
    assert_eq!(
        first
            .get_response_route_origin("caller-a", "response-hash", now)
            .await
            .expect("immutable origin"),
        Some(origin.clone())
    );
    assert_eq!(
        first
            .get_response_route_origin("caller-b", "response-hash", now)
            .await
            .expect("independent caller origin"),
        Some(other_origin)
    );
    exercise_origin_retention(first, second, &origin, now).await;
}

async fn exercise_origin_retention<S: RoutingRepository>(
    first: &S,
    second: &S,
    origin: &ResponseRouteOrigin,
    now: OffsetDateTime,
) {
    first
        .record_response_route_origin("caller-a", "expiring-hash", origin, now)
        .await
        .expect("record expiring origin");
    let deadline = now + Duration::days(30);
    assert_eq!(
        second
            .get_response_route_origin("caller-a", "expiring-hash", deadline - Duration::seconds(1))
            .await
            .expect("origin before expiry"),
        Some(origin.clone())
    );
    assert_eq!(
        second
            .get_response_route_origin("caller-a", "expiring-hash", deadline)
            .await
            .expect("origin expiry boundary"),
        None
    );
    second
        .record_response_route_origin("caller-a", "response-hash", origin, now + Duration::days(2))
        .await
        .expect("identical origin extends retention");
    first
        .record_response_route_origin("caller-a", "response-hash", origin, now + Duration::days(1))
        .await
        .expect("out-of-order identical write");
    assert_eq!(
        second
            .get_response_route_origin(
                "caller-a",
                "response-hash",
                now + Duration::days(32) - Duration::seconds(1)
            )
            .await
            .expect("monotonic origin retention"),
        Some(origin.clone())
    );
    assert_eq!(
        second
            .get_response_route_origin("caller-a", "response-hash", now + Duration::days(32))
            .await
            .expect("renewed origin expires"),
        None
    );
}

async fn exercise_routing<S: RoutingRepository + 'static>(first: Arc<S>, second: Arc<S>) {
    exercise_idle_expiry(first.as_ref()).await;
    exercise_refresh_guards(first.as_ref()).await;
    exercise_candidate_changes(first.as_ref()).await;
    exercise_shared_round_robin(first.as_ref(), second.as_ref()).await;
    exercise_round_robin_eligible_pools(first.as_ref(), second.as_ref()).await;
    exercise_shared_cooldowns(first.as_ref(), second.as_ref()).await;
    exercise_cooldown_expiry_and_scope(first.as_ref(), second.as_ref()).await;
    exercise_failure_receipt_guards(first.as_ref(), second.as_ref()).await;
    exercise_response_origins(first.as_ref(), second.as_ref()).await;
    exercise_concurrent_failover(Arc::clone(&first), Arc::clone(&second)).await;
    exercise_concurrent_binding(first, second).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn libsql_routing_bindings_cursors_and_response_origins() {
    let tmp = tempdir().expect("tempdir");
    let db_path = tmp.path().join("gateway.db");
    run_migrations(&db_path).await.expect("migrations");
    let path = db_path.to_str().expect("db path");
    let first = LibsqlStore::new_local(path).await.expect("first store");
    let second = LibsqlStore::new_local(path)
        .await
        .expect("independent store");
    exercise_routing(Arc::new(first), Arc::new(second)).await;
}

#[tokio::test(flavor = "current_thread")]
#[serial]
async fn libsql_routing_writer_contention_does_not_block_runtime() {
    let tmp = tempdir().expect("tempdir");
    let db_path = tmp.path().join("gateway.db");
    run_migrations(&db_path).await.expect("migrations");
    let path = db_path.to_str().expect("db path");
    let first = LibsqlStore::new_local(path).await.expect("first store");
    let second = LibsqlStore::new_local(path)
        .await
        .expect("independent store");
    let writer = first
        .connection()
        .transaction_with_behavior(libsql::TransactionBehavior::Immediate)
        .await
        .expect("hold SQLite writer lock");
    let request = selection_request(RouteSelectionMode::RoundRobin);
    let started = Instant::now();
    let selection = tokio::spawn(async move { second.select_route(&request).await });
    tokio::time::sleep(StdDuration::from_millis(100)).await;
    let timer_elapsed = started.elapsed();
    let pending_during_lock = !selection.is_finished();
    writer.commit().await.expect("release SQLite writer lock");

    assert!(
        timer_elapsed < StdDuration::from_secs(1),
        "SQLite contention blocked the runtime timer for {timer_elapsed:?}"
    );
    assert!(
        pending_during_lock,
        "selection must wait for the writer lock"
    );
    let selected = selection
        .await
        .expect("selection task")
        .expect("selection succeeds after writer release")
        .expect("eligible route");
    assert_eq!(selected.route_id, Uuid::from_u128(1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn postgres_routing_bindings_cursors_and_response_origins() {
    let Some(test_db) = create_postgres_test_database().await else {
        eprintln!("skipping postgres routing test: TEST_POSTGRES_URL is not set");
        return;
    };
    run_migrations_with_options(&StoreConnectionOptions::Postgres {
        url: test_db.database_url.clone(),
        max_connections: 4,
    })
    .await
    .expect("migrations");
    let first = PostgresStore::connect(&test_db.database_url, 4)
        .await
        .expect("first store");
    let second = PostgresStore::connect(&test_db.database_url, 4)
        .await
        .expect("independent store");
    exercise_routing(Arc::new(first), Arc::new(second)).await;
    drop_postgres_test_database(&test_db).await;
}
