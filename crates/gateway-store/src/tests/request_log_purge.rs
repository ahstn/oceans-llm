use gateway_core::{
    RequestAttemptRecord, RequestAttemptStatus, RequestLogPayloadRecord, RequestLogRecord,
    RequestTag, RequestTags, RequestToolCardinality, SeedApiKey, SeedModel, SeedProvider,
    StoreError,
};
use serde_json::{Map, json};
use serial_test::serial;
use sqlx::Row;
use tempfile::tempdir;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use super::{
    create_postgres_test_database, drop_postgres_test_database, seed_api_key_service_accounts,
    seed_api_key_teams,
};
use crate::{
    GatewayStore, LibsqlStore, PostgresStore, StoreConnectionOptions, run_migrations,
    run_migrations_with_options,
};

async fn exercise_request_log_purge<S: GatewayStore + Sync>(store: &S) {
    let providers = [SeedProvider {
        provider_key: "openai-prod".to_string(),
        provider_type: "openai_compat".to_string(),
        config: json!({}),
        secrets: None,
    }];
    let models = [SeedModel {
        model_key: "fast".to_string(),
        alias_target_model_key: None,
        max_reasoning_effort: None,
        description: None,
        tags: Vec::new(),
        rank: 10,
        routes: Vec::new(),
        allowlist: None,
    }];
    let api_keys = [SeedApiKey {
        name: "dev".to_string(),
        public_id: "dev123".to_string(),
        secret_hash: "$argon2id$v=19$m=19456,t=2,p=1$8WJ6UydAx2RbDXy+zuYbAw$EF+rEtkc71VhwwvS+TS6EiZZvW6rtrjzXX4XvIsDhbU".to_string(),
        service_account_key: "seed-workloads".to_string(),
        allowed_models: vec!["fast".to_string()],
    }];
    store
        .seed_from_inputs(
            &providers,
            &models,
            &api_keys,
            &seed_api_key_service_accounts(),
            &[],
            &[],
            &seed_api_key_teams(),
            &[],
        )
        .await
        .expect("seed");
    let api_key = store
        .get_api_key_by_public_id("dev123")
        .await
        .expect("query key")
        .expect("api key should exist");

    let now = OffsetDateTime::now_utc();
    let old_log = RequestLogRecord {
        request_log_id: Uuid::new_v4(),
        request_id: "req-old".to_string(),
        api_key_id: api_key.id,
        user_id: None,
        team_id: api_key.owner_team_id,
        service_account_id: None,
        model_key: "fast".to_string(),
        resolved_model_key: "fast".to_string(),
        provider_key: "openai-prod".to_string(),
        status_code: Some(200),
        latency_ms: Some(42),
        prompt_tokens: Some(1),
        completion_tokens: Some(2),
        total_tokens: Some(3),
        error_code: None,
        has_payload: true,
        request_payload_truncated: false,
        response_payload_truncated: false,
        request_tags: RequestTags {
            service: Some("svc".to_string()),
            component: None,
            env: None,
            bespoke: vec![RequestTag {
                key: "tenant".to_string(),
                value: "alpha".to_string(),
            }],
        },
        tool_cardinality: RequestToolCardinality::default(),
        user_agent_raw: None,
        agent_harness_key: "unknown".to_string(),
        agent_harness_label: "Unknown".to_string(),
        metadata: Map::new(),
        occurred_at: now - Duration::days(4),
    };
    let young_log = RequestLogRecord {
        request_log_id: Uuid::new_v4(),
        request_id: "req-young".to_string(),
        has_payload: false,
        request_tags: RequestTags::default(),
        occurred_at: now,
        ..old_log.clone()
    };
    let payload = RequestLogPayloadRecord {
        request_log_id: old_log.request_log_id,
        request_json: json!({"prompt": "old"}),
        response_json: json!({"ok": true}),
    };
    let attempt = RequestAttemptRecord {
        request_attempt_id: Uuid::new_v4(),
        request_log_id: old_log.request_log_id,
        request_id: old_log.request_id.clone(),
        attempt_number: 1,
        route_id: Uuid::new_v4(),
        provider_key: "openai-prod".to_string(),
        upstream_model: "gpt-4o-mini".to_string(),
        status: RequestAttemptStatus::Success,
        status_code: Some(200),
        error_code: None,
        error_detail: None,
        error_detail_truncated: false,
        retryable: false,
        terminal: true,
        produced_final_response: true,
        stream: false,
        started_at: old_log.occurred_at,
        completed_at: Some(old_log.occurred_at + Duration::milliseconds(42)),
        latency_ms: Some(42),
        metadata: Map::new(),
    };

    let attempt_id = attempt.request_attempt_id;
    store
        .insert_request_log_with_attempts(&old_log, Some(&payload), &[attempt])
        .await
        .expect("insert old request log");
    store
        .insert_request_log(&young_log, None)
        .await
        .expect("insert young request log");

    let cutoff = now - Duration::days(3);
    let dry_run = store
        .purge_request_logs_older_than(cutoff, true)
        .await
        .expect("dry run purge");
    assert_eq!(dry_run.matched_count, 1);
    assert_eq!(dry_run.deleted_count, 0);
    let retained = store
        .get_request_log_detail(old_log.request_log_id)
        .await
        .expect("dry run retains old log and children");
    let retained_payload = retained.payload.expect("dry run retains payload");
    assert_eq!(retained_payload.request_json, payload.request_json);
    assert_eq!(retained_payload.response_json, payload.response_json);
    assert_eq!(
        retained.log.request_tags.service,
        old_log.request_tags.service
    );
    assert_eq!(retained.log.request_tags.bespoke.len(), 1);
    assert_eq!(retained.log.request_tags.bespoke[0].key, "tenant");
    assert_eq!(retained.log.request_tags.bespoke[0].value, "alpha");
    assert_eq!(retained.attempts.len(), 1);
    assert_eq!(retained.attempts[0].request_attempt_id, attempt_id);

    let purge = store
        .purge_request_logs_older_than(cutoff, false)
        .await
        .expect("purge old request logs");
    assert_eq!(purge.matched_count, 1);
    assert_eq!(purge.deleted_count, 1);
    assert!(matches!(
        store.get_request_log_detail(old_log.request_log_id).await,
        Err(StoreError::NotFound(_))
    ));
    assert!(
        store
            .get_request_log_detail(young_log.request_log_id)
            .await
            .is_ok()
    );
}

#[tokio::test]
#[serial]
async fn libsql_request_log_purge_supports_dry_run_and_cascades_children() {
    let tmp = tempdir().expect("tempdir");
    let db_path = tmp.path().join("gateway.db");
    run_migrations(&db_path).await.expect("migrations");

    let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
        .await
        .expect("store");
    exercise_request_log_purge(&store).await;

    let db = libsql::Builder::new_local(db_path)
        .build()
        .await
        .expect("libsql db");
    let connection = db.connect().expect("libsql connection");
    let mut rows = connection
        .query(
            r#"
            SELECT
                (SELECT COUNT(*) FROM request_log_payloads),
                (SELECT COUNT(*) FROM request_log_tags),
                (SELECT COUNT(*) FROM request_log_attempts)
            "#,
            (),
        )
        .await
        .expect("child counts");
    let row = rows
        .next()
        .await
        .expect("child count row")
        .expect("child count row exists");
    let payload_count: i64 = row.get(0).expect("payload count");
    let tag_count: i64 = row.get(1).expect("tag count");
    let attempt_count: i64 = row.get(2).expect("attempt count");
    assert_eq!((payload_count, tag_count, attempt_count), (0, 0, 0));
}

#[tokio::test]
#[serial]
async fn postgres_request_log_purge_supports_dry_run_and_cascades_children() {
    let Some(test_db) = create_postgres_test_database().await else {
        eprintln!("skipping postgres request log purge test because TEST_POSTGRES_URL is not set");
        return;
    };

    let options = StoreConnectionOptions::Postgres {
        url: test_db.database_url.clone(),
        max_connections: 4,
    };
    run_migrations_with_options(&options)
        .await
        .expect("postgres migrations");

    let store = PostgresStore::connect(&test_db.database_url, 4)
        .await
        .expect("postgres store");
    exercise_request_log_purge(&store).await;

    let pool = sqlx::PgPool::connect(&test_db.database_url)
        .await
        .expect("postgres pool");
    let row = sqlx::query(
        r#"
        SELECT
            (SELECT COUNT(*) FROM request_log_payloads),
            (SELECT COUNT(*) FROM request_log_tags),
            (SELECT COUNT(*) FROM request_log_attempts)
        "#,
    )
    .fetch_one(&pool)
    .await
    .expect("child counts");
    let payload_count: i64 = row.try_get(0).expect("payload count");
    let tag_count: i64 = row.try_get(1).expect("tag count");
    let attempt_count: i64 = row.try_get(2).expect("attempt count");
    assert_eq!((payload_count, tag_count, attempt_count), (0, 0, 0));

    pool.close().await;
    drop_postgres_test_database(&test_db).await;
}
