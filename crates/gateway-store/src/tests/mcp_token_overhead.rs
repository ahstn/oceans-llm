use gateway_core::{
    ExternalMcpAuthMode, ExternalMcpDiscoveryRunRecord, ExternalMcpDiscoveryStatus,
    ExternalMcpTransport, McpRegistryRepository, McpTokenEstimateConfidence,
    McpTokenEstimateSource, McpTokenOverheadRepository, McpToolTokenEstimateRecord,
    NewExternalMcpServerRecord, UpsertExternalMcpToolRecord,
};
use serde_json::{Map, json};
use serial_test::serial;
use tempfile::tempdir;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use super::{create_postgres_test_database, drop_postgres_test_database};
use crate::{
    LibsqlStore, PostgresStore, StoreConnectionOptions, run_migrations, run_migrations_with_options,
};

/// Estimates for several tools come back from one lookup, without expired
/// ones or keys that were never stored.
async fn exercise_batched_estimates<S>(store: &S)
where
    S: McpRegistryRepository + McpTokenOverheadRepository + Sync,
{
    let now = OffsetDateTime::now_utc();
    let server = store
        .create_external_mcp_server(&NewExternalMcpServerRecord {
            server_key: "files".to_string(),
            display_name: "Files".to_string(),
            description: None,
            transport: ExternalMcpTransport::StreamableHttp,
            server_url: "https://files.example.test/mcp".to_string(),
            auth_mode: ExternalMcpAuthMode::None,
            auth_config: Map::new(),
            timeout_ms: 2_000,
            created_at: now,
            approve_tool_changes: false,
            ownership: Default::default(),
        })
        .await
        .expect("server");
    let upserts = ["read", "write", "list"]
        .iter()
        .map(|name| UpsertExternalMcpToolRecord {
            mcp_server_id: server.mcp_server_id,
            upstream_name: (*name).to_string(),
            display_name: (*name).to_string(),
            description: None,
            input_schema: json!({"type": "object"}),
            metadata: Default::default(),
            schema_hash: format!("hash-{name}"),
        })
        .collect::<Vec<_>>();
    let tools = store
        .record_external_mcp_discovery_success(
            &ExternalMcpDiscoveryRunRecord {
                discovery_run_id: Uuid::new_v4(),
                mcp_server_id: server.mcp_server_id,
                status: ExternalMcpDiscoveryStatus::Success,
                started_at: now,
                finished_at: now,
                discovered_tool_count: 3,
                active_tool_count: 3,
                schema_set_hash: None,
                error_summary: None,
                details: Map::new(),
                expected_server: None,
            },
            &upserts,
        )
        .await
        .expect("discovery");
    for tool in &tools {
        let expires_at = if tool.upstream_name == "write" {
            now - Duration::seconds(1)
        } else {
            now + Duration::days(1)
        };
        store
            .upsert_mcp_tool_token_estimate(&McpToolTokenEstimateRecord {
                cache_key: format!("key-{}", tool.upstream_name),
                provider_family: "openai".to_string(),
                model_or_encoding: "o200k".to_string(),
                mcp_server_id: server.mcp_server_id,
                mcp_tool_id: tool.mcp_tool_id,
                tool_name: tool.upstream_name.clone(),
                schema_hash: tool.schema_hash.clone(),
                description_hash: "d".to_string(),
                protocol_version: "2025-11-25".to_string(),
                serializer_version: "v".to_string(),
                estimated_tokens: tool.upstream_name.len() as i64,
                estimator_source: McpTokenEstimateSource::ConservativeFallback,
                confidence: McpTokenEstimateConfidence::Low,
                created_at: now,
                updated_at: now,
                expires_at,
            })
            .await
            .expect("estimate");
    }
    let keys = ["key-read", "key-write", "key-list", "key-missing"]
        .map(str::to_string)
        .to_vec();
    let mut found = store
        .get_mcp_tool_token_estimates(&keys, now)
        .await
        .expect("estimates")
        .into_iter()
        .map(|estimate| (estimate.cache_key, estimate.estimated_tokens))
        .collect::<Vec<_>>();
    found.sort();
    assert_eq!(
        found,
        vec![("key-list".to_string(), 4), ("key-read".to_string(), 4)]
    );
    assert!(
        store
            .get_mcp_tool_token_estimates(&[], now)
            .await
            .expect("no keys")
            .is_empty()
    );
}

#[tokio::test]
async fn libsql_mcp_tool_token_estimates_are_read_in_one_query() {
    let tmp = tempdir().expect("tempdir");
    let db_path = tmp.path().join("gateway.db");
    run_migrations(&db_path).await.expect("migrations");
    let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
        .await
        .expect("store");
    exercise_batched_estimates(&store).await;
}

#[tokio::test]
#[serial]
async fn postgres_mcp_tool_token_estimates_are_read_in_one_query() {
    let Some(test_db) = create_postgres_test_database().await else {
        eprintln!("skipping postgres MCP token estimate test: TEST_POSTGRES_URL is not set");
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
    exercise_batched_estimates(&store).await;
    drop(store);
    drop_postgres_test_database(&test_db).await;
}
