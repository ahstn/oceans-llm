use gateway_core::{
    ExternalMcpAuthMode, ExternalMcpDiscoveryRunRecord, ExternalMcpDiscoveryStatus,
    ExternalMcpServerRecord, ExternalMcpTransport, McpRegistryRepository, McpServerOwnershipGuard,
    NewExternalMcpServerRecord, StoreError, UpsertExternalMcpToolRecord,
};
use serde_json::{Map, json};
use serial_test::serial;
use time::OffsetDateTime;
use uuid::Uuid;

use super::{create_postgres_test_database, drop_postgres_test_database};
use crate::{
    LibsqlStore, PostgresStore, StoreConnectionOptions, run_migrations, run_migrations_with_options,
};

async fn create_server<S>(store: &S) -> ExternalMcpServerRecord
where
    S: McpRegistryRepository + Sync,
{
    store
        .create_external_mcp_server(&NewExternalMcpServerRecord {
            server_key: "files".to_string(),
            display_name: "Files".to_string(),
            description: None,
            transport: ExternalMcpTransport::StreamableHttp,
            server_url: "https://files.example.test/mcp".to_string(),
            auth_mode: ExternalMcpAuthMode::None,
            auth_config: Map::new(),
            timeout_ms: 2_000,
            created_at: OffsetDateTime::now_utc(),
            approve_tool_changes: false,
            ownership: Default::default(),
        })
        .await
        .expect("server")
}

fn run(
    server: &ExternalMcpServerRecord,
    started_at: OffsetDateTime,
) -> ExternalMcpDiscoveryRunRecord {
    ExternalMcpDiscoveryRunRecord {
        discovery_run_id: Uuid::new_v4(),
        mcp_server_id: server.mcp_server_id,
        status: ExternalMcpDiscoveryStatus::Success,
        started_at,
        finished_at: OffsetDateTime::now_utc(),
        discovered_tool_count: 1,
        active_tool_count: 1,
        schema_set_hash: None,
        error_summary: None,
        details: Map::new(),
        expected_server: None,
    }
}

/// A run pinned to the server's current owner, URL and auth settings, as a
/// refresh is.
fn guarded_run(
    server: &ExternalMcpServerRecord,
    started_at: OffsetDateTime,
) -> ExternalMcpDiscoveryRunRecord {
    ExternalMcpDiscoveryRunRecord {
        expected_server: Some(McpServerOwnershipGuard::of(server).with_auth_settings(server)),
        ..run(server, started_at)
    }
}

fn tool(server: &ExternalMcpServerRecord, hash: &str) -> UpsertExternalMcpToolRecord {
    UpsertExternalMcpToolRecord {
        mcp_server_id: server.mcp_server_id,
        upstream_name: "read".to_string(),
        display_name: "read".to_string(),
        description: None,
        input_schema: json!({"type": "object"}),
        metadata: Default::default(),
        schema_hash: hash.to_string(),
    }
}

/// Replicas refreshing the same server at once used to deadlock: each run
/// insert took a key-share lock on the server row, then each waited to
/// upgrade it.
#[tokio::test]
#[serial]
async fn postgres_concurrent_discoveries_of_one_server_do_not_deadlock() {
    let Some(test_db) = create_postgres_test_database().await else {
        eprintln!("skipping postgres discovery test: TEST_POSTGRES_URL is not set");
        return;
    };
    run_migrations_with_options(&StoreConnectionOptions::Postgres {
        url: test_db.database_url.clone(),
        max_connections: 8,
    })
    .await
    .expect("migrations");
    let store = std::sync::Arc::new(
        PostgresStore::connect(&test_db.database_url, 8)
            .await
            .expect("store"),
    );
    let server = create_server(store.as_ref()).await;
    for round in 0..5 {
        let commits = (0..8).map(|index| {
            let store = store.clone();
            let server = server.clone();
            tokio::spawn(async move {
                store
                    .record_external_mcp_discovery_success(
                        &run(&server, OffsetDateTime::now_utc()),
                        &[tool(&server, &format!("hash-{round}-{index}"))],
                    )
                    .await
            })
        });
        for result in futures_util::future::join_all(commits).await {
            result.expect("task").expect("concurrent discovery commits");
        }
    }
    drop(store);
    drop_postgres_test_database(&test_db).await;
}

/// The stored `read` tool's schema hash and whether it is active.
async fn read_tool<S>(store: &S, server: &ExternalMcpServerRecord) -> (String, bool)
where
    S: McpRegistryRepository + Sync,
{
    let tools = store
        .list_external_mcp_tools(server.mcp_server_id, true)
        .await
        .expect("tools");
    (tools[0].schema_hash.clone(), tools[0].is_active)
}

/// A guarded discovery never commits over one that started later, nor for a
/// server an admin disabled while it ran.
async fn exercise_stale_commits<S>(store: &S)
where
    S: McpRegistryRepository + Sync,
{
    let server = create_server(store).await;
    // On a whole second, so the same-second case below stays in one second.
    let t0 = OffsetDateTime::from_unix_timestamp(
        (OffsetDateTime::now_utc() - time::Duration::minutes(10)).unix_timestamp(),
    )
    .expect("timestamp");
    let newer_start = t0 + time::Duration::minutes(5) + time::Duration::milliseconds(400);
    for (started_at, hash) in [(t0, "first"), (newer_start, "newer")] {
        store
            .record_external_mcp_discovery_success(
                &guarded_run(&server, started_at),
                &[tool(&server, hash)],
            )
            .await
            .expect("discovery");
    }
    // One that started before the newer one, but finished after it, is dropped.
    let older = store
        .record_external_mcp_discovery_success(
            &guarded_run(&server, t0 + time::Duration::minutes(2)),
            &[tool(&server, "older")],
        )
        .await;
    assert!(matches!(older, Err(StoreError::Conflict(_))), "{older:?}");
    // Starts are ordered to the millisecond: one that started earlier in the
    // same second is refused too.
    let same_second = store
        .record_external_mcp_discovery_success(
            &guarded_run(&server, t0 + time::Duration::minutes(5)),
            &[tool(&server, "same-second")],
        )
        .await;
    assert!(
        matches!(same_second, Err(StoreError::Conflict(_))),
        "{same_second:?}"
    );
    assert_eq!(read_tool(store, &server).await, ("newer".to_string(), true));

    // An older failure is kept in the history but leaves the newer success as
    // the server's last discovery; a newer failure replaces it.
    for (started_at, summary) in [
        (t0 + time::Duration::minutes(3), "older failure"),
        (t0 + time::Duration::minutes(6), "newer failure"),
    ] {
        store
            .record_external_mcp_discovery_failure(&ExternalMcpDiscoveryRunRecord {
                status: ExternalMcpDiscoveryStatus::Failed,
                error_summary: Some(summary.to_string()),
                ..guarded_run(&server, started_at)
            })
            .await
            .expect("failure");
        let summary_now = store
            .get_external_mcp_server(server.mcp_server_id)
            .await
            .expect("server lookup")
            .expect("server");
        let expected = match summary {
            "older failure" => (Some(ExternalMcpDiscoveryStatus::Success), None),
            _ => (
                Some(ExternalMcpDiscoveryStatus::Failed),
                Some(summary.to_string()),
            ),
        };
        assert_eq!(
            (
                summary_now.last_discovery_status,
                summary_now.last_error_summary
            ),
            expected,
            "after the {summary}"
        );
    }

    // Two refreshes that start in the same millisecond are ordered by run id:
    // once the later one committed, the earlier one is refused.
    let same_instant = t0 + time::Duration::minutes(7);
    let tied = |id: u128| ExternalMcpDiscoveryRunRecord {
        discovery_run_id: Uuid::from_u128(id),
        ..guarded_run(&server, same_instant)
    };
    store
        .record_external_mcp_discovery_success(&tied(2), &[tool(&server, "tied-later")])
        .await
        .expect("later tied run");
    let tied_earlier = store
        .record_external_mcp_discovery_success(&tied(1), &[tool(&server, "tied-earlier")])
        .await;
    assert!(
        matches!(tied_earlier, Err(StoreError::Conflict(_))),
        "{tied_earlier:?}"
    );
    assert_eq!(
        read_tool(store, &server).await,
        ("tied-later".to_string(), true)
    );

    // A failure that started later commits first; an older success still
    // commits its tools but leaves the failure as the last discovery.
    store
        .record_external_mcp_discovery_failure(&ExternalMcpDiscoveryRunRecord {
            status: ExternalMcpDiscoveryStatus::Failed,
            error_summary: Some("later failure".to_string()),
            ..guarded_run(&server, t0 + time::Duration::seconds(450))
        })
        .await
        .expect("later failure");
    store
        .record_external_mcp_discovery_success(
            &guarded_run(&server, t0 + time::Duration::seconds(435)),
            &[tool(&server, "older-success")],
        )
        .await
        .expect("older success");
    assert_eq!(
        read_tool(store, &server).await,
        ("older-success".to_string(), true)
    );
    let summary = store
        .get_external_mcp_server(server.mcp_server_id)
        .await
        .expect("server lookup")
        .expect("server");
    assert_eq!(
        (summary.last_discovery_status, summary.last_error_summary),
        (
            Some(ExternalMcpDiscoveryStatus::Failed),
            Some("later failure".to_string())
        )
    );

    // An admin disables the server while a discovery is in flight.
    store
        .disable_external_mcp_server(server.mcp_server_id, OffsetDateTime::now_utc())
        .await
        .expect("disable server");
    let after_disable = read_tool(store, &server).await;
    let stale = store
        .record_external_mcp_discovery_success(
            &guarded_run(&server, t0 + time::Duration::minutes(8)),
            &[tool(&server, "after-disable")],
        )
        .await;
    assert!(matches!(stale, Err(StoreError::Conflict(_))), "{stale:?}");
    // Nothing was written, and no tool was reactivated.
    assert_eq!(read_tool(store, &server).await, after_disable);
}

#[tokio::test]
async fn libsql_stale_discoveries_are_discarded() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("gateway.db");
    run_migrations(&db_path).await.expect("migrations");
    let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
        .await
        .expect("store");
    exercise_stale_commits(&store).await;
}

#[tokio::test]
#[serial]
async fn postgres_stale_discoveries_are_discarded() {
    let Some(test_db) = create_postgres_test_database().await else {
        eprintln!("skipping postgres discovery test: TEST_POSTGRES_URL is not set");
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
    exercise_stale_commits(&store).await;
    drop(store);
    drop_postgres_test_database(&test_db).await;
}
