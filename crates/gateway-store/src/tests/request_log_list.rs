use gateway_core::{
    AuthMode, GlobalRole, RequestLogQuery, RequestLogRecord, RequestTags, RequestToolCardinality,
    SeedApiKey, SeedModel, SeedProvider, UsagePricingStatus, UserStatus,
};
use serde_json::{Map, json};
use serial_test::serial;
use tempfile::tempdir;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use super::{
    build_usage_ledger_record, create_postgres_test_database, drop_postgres_test_database,
    seed_api_key_service_accounts, seed_api_key_teams,
};
use crate::{
    GatewayStore, LibsqlStore, PostgresStore, StoreConnectionOptions, run_migrations,
    run_migrations_with_options,
};

struct SeededLogs {
    ada_log: RequestLogRecord,
    grace_log: RequestLogRecord,
    anonymous_log: RequestLogRecord,
}

async fn seed_request_logs<S: GatewayStore + Sync>(store: &S) -> SeededLogs {
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
        routing: None,
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
    let ada = store
        .create_identity_user(
            "Ada Lovelace",
            "ada@example.com",
            "ada@example.com",
            GlobalRole::User,
            AuthMode::Password,
            UserStatus::Active,
        )
        .await
        .expect("create ada");
    let grace = store
        .create_identity_user(
            "Grace Hopper",
            "grace@analytical.dev",
            "grace@analytical.dev",
            GlobalRole::User,
            AuthMode::Password,
            UserStatus::Active,
        )
        .await
        .expect("create grace");

    let now = OffsetDateTime::now_utc();
    let ada_log = RequestLogRecord {
        request_log_id: Uuid::new_v4(),
        request_id: "req-ada".to_string(),
        api_key_id: api_key.id,
        user_id: Some(ada.user_id),
        team_id: None,
        service_account_id: None,
        model_key: "fast".to_string(),
        resolved_model_key: "gpt-4o-mini".to_string(),
        provider_key: "openai-prod".to_string(),
        status_code: Some(200),
        latency_ms: Some(42),
        prompt_tokens: Some(100),
        completion_tokens: Some(50),
        total_tokens: Some(150),
        error_code: None,
        has_payload: false,
        request_payload_truncated: false,
        response_payload_truncated: false,
        request_tags: RequestTags::default(),
        tool_cardinality: RequestToolCardinality {
            referenced_mcp_server_count: Some(1),
            exposed_tool_count: Some(3),
            invoked_tool_count: Some(1),
            filtered_tool_count: Some(0),
            request_tool_count: Some(7),
        },
        user_agent_raw: None,
        agent_harness_key: "unknown".to_string(),
        agent_harness_label: "Unknown".to_string(),
        metadata: Map::new(),
        occurred_at: now,
    };
    let grace_log = RequestLogRecord {
        request_log_id: Uuid::new_v4(),
        request_id: "req-grace".to_string(),
        user_id: Some(grace.user_id),
        model_key: "Reasoning-Pro".to_string(),
        resolved_model_key: "reasoning-pro-2026".to_string(),
        tool_cardinality: RequestToolCardinality::default(),
        occurred_at: now - Duration::seconds(1),
        ..ada_log.clone()
    };
    let anonymous_log = RequestLogRecord {
        request_log_id: Uuid::new_v4(),
        request_id: "req-anonymous".to_string(),
        user_id: None,
        resolved_model_key: "fast".to_string(),
        tool_cardinality: RequestToolCardinality::default(),
        occurred_at: now - Duration::seconds(2),
        ..ada_log.clone()
    };
    for log in [&ada_log, &grace_log, &anonymous_log] {
        store
            .insert_request_log(log, None)
            .await
            .expect("insert request log");
    }

    SeededLogs {
        ada_log,
        grace_log,
        anonymous_log,
    }
}

async fn search_request_ids<S: GatewayStore + Sync>(
    store: &S,
    query: RequestLogQuery,
) -> (u64, Vec<String>) {
    let page = store
        .list_request_logs(&RequestLogQuery {
            page: 1,
            page_size: 50,
            ..query
        })
        .await
        .expect("list request logs");
    let ids = page.items.into_iter().map(|log| log.request_id).collect();
    (page.total, ids)
}

async fn exercise_request_log_list_search<S: GatewayStore + Sync>(store: &S) {
    let logs = seed_request_logs(store).await;
    let search = |q: &str| RequestLogQuery {
        q: Some(q.to_string()),
        ..RequestLogQuery::default()
    };

    // Resolved model key, case-insensitive.
    assert_eq!(
        search_request_ids(store, search("GPT-4O")).await,
        (1, vec!["req-ada".to_string()])
    );
    // Requested model key matches both logs routed through `fast`.
    assert_eq!(
        search_request_ids(store, search("fast")).await,
        (2, vec!["req-ada".to_string(), "req-anonymous".to_string()])
    );
    // User name and email.
    assert_eq!(
        search_request_ids(store, search("lovelace")).await,
        (1, vec!["req-ada".to_string()])
    );
    assert_eq!(
        search_request_ids(store, search("ANALYTICAL.dev")).await,
        (1, vec!["req-grace".to_string()])
    );
    // Search combines with the structured filters.
    assert_eq!(
        search_request_ids(
            store,
            RequestLogQuery {
                user_id: logs.ada_log.user_id,
                ..search("fast")
            }
        )
        .await,
        (1, vec!["req-ada".to_string()])
    );
    assert_eq!(
        search_request_ids(store, search("no-such-thing")).await,
        (0, Vec::new())
    );
    // Blank searches are ignored rather than matching nothing.
    assert_eq!(search_request_ids(store, search("   ")).await.0, 3);

    // `request_tool_count` round-trips through the list and detail decoders.
    let page = store
        .list_request_logs(&RequestLogQuery {
            page: 1,
            page_size: 50,
            ..RequestLogQuery::default()
        })
        .await
        .expect("list request logs");
    let listed = page
        .items
        .iter()
        .find(|log| log.request_log_id == logs.ada_log.request_log_id)
        .expect("ada log listed");
    assert_eq!(listed.tool_cardinality, logs.ada_log.tool_cardinality);
    let detail = store
        .get_request_log_detail(logs.ada_log.request_log_id)
        .await
        .expect("ada log detail");
    assert_eq!(detail.log.tool_cardinality.request_tool_count, Some(7));
    assert_eq!(detail.log.tool_cardinality.exposed_tool_count, Some(3));
    let detail = store
        .get_request_log_detail(logs.grace_log.request_log_id)
        .await
        .expect("grace log detail");
    assert_eq!(detail.log.tool_cardinality.request_tool_count, None);

    // Usage ledger rows are looked up by request id across ownership scopes.
    let ada_user_id = logs.ada_log.user_id.expect("ada user id");
    for (scope, pricing_status, cost) in [
        (
            format!("user:{ada_user_id}"),
            UsagePricingStatus::Priced,
            1_234,
        ),
        ("team:other".to_string(), UsagePricingStatus::Unpriced, 0),
    ] {
        let event = build_usage_ledger_record(
            &logs.ada_log.request_id,
            scope,
            logs.ada_log.api_key_id,
            logs.ada_log.user_id,
            None,
            None,
            None,
            "gpt-4o-mini",
            pricing_status,
            cost,
            logs.ada_log.occurred_at,
        );
        store
            .insert_usage_ledger_if_absent(&event)
            .await
            .expect("insert usage event");
    }
    let usage = store
        .get_usage_ledgers_by_request_ids(&[
            logs.ada_log.request_id.clone(),
            logs.anonymous_log.request_id.clone(),
            "req-missing".to_string(),
        ])
        .await
        .expect("usage ledgers by request ids");
    assert_eq!(usage.len(), 2);
    assert!(usage.iter().all(|record| record.request_id == "req-ada"));
    let priced = usage
        .iter()
        .find(|record| record.pricing_status == UsagePricingStatus::Priced)
        .expect("priced usage row");
    assert_eq!(priced.computed_cost_usd.as_scaled_i64(), 1_234);
    assert_eq!(priced.cache_read_tokens, Some(80));
    assert!(
        store
            .get_usage_ledgers_by_request_ids(&[])
            .await
            .expect("empty usage lookup")
            .is_empty()
    );
}

#[tokio::test]
#[serial]
async fn libsql_request_log_list_search_tool_count_and_usage_lookup() {
    let tmp = tempdir().expect("tempdir");
    let db_path = tmp.path().join("gateway.db");
    run_migrations(&db_path).await.expect("migrations");

    let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
        .await
        .expect("store");
    exercise_request_log_list_search(&store).await;
}

#[tokio::test]
#[serial]
async fn postgres_request_log_list_search_tool_count_and_usage_lookup() {
    let Some(test_db) = create_postgres_test_database().await else {
        eprintln!("skipping postgres request log list test because TEST_POSTGRES_URL is not set");
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
    exercise_request_log_list_search(&store).await;

    drop(store);
    drop_postgres_test_database(&test_db).await;
}
