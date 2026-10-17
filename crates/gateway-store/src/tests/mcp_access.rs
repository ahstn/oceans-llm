use gateway_core::{
    ExternalMcpAuthMode, ExternalMcpDiscoveryRunRecord, ExternalMcpDiscoveryStatus,
    ExternalMcpServerRecord, ExternalMcpToolRecord, ExternalMcpTransport, McpAccessRepository,
    McpGrantSubject, McpRegistryRepository, McpToolGrantSubjectKind, McpToolGrantTargetKind,
    McpToolMetadata, McpToolProjection, NewExternalMcpServerRecord, NewMcpToolsetRecord,
    UpsertExternalMcpToolRecord, UpsertMcpToolGrantRecord,
};
use serde_json::{Map, json};
use serial_test::serial;
use tempfile::tempdir;
use time::OffsetDateTime;
use uuid::Uuid;

use super::{create_postgres_test_database, drop_postgres_test_database};
use crate::{
    LibsqlStore, PostgresStore, StoreConnectionOptions, run_migrations, run_migrations_with_options,
};

async fn seed_server<S>(
    store: &S,
    server_key: &str,
) -> (ExternalMcpServerRecord, ExternalMcpToolRecord)
where
    S: McpRegistryRepository + Sync,
{
    let now = OffsetDateTime::now_utc();
    let server = store
        .create_external_mcp_server(&NewExternalMcpServerRecord {
            ownership: Default::default(),
            server_key: server_key.to_string(),
            display_name: server_key.to_string(),
            description: None,
            transport: ExternalMcpTransport::StreamableHttp,
            server_url: format!("https://{server_key}.example.test/mcp"),
            auth_mode: ExternalMcpAuthMode::None,
            auth_config: Map::new(),
            timeout_ms: 2_000,
            created_at: now,
        })
        .await
        .expect("server");
    let annotations = json!({"readOnlyHint": true});
    let tools = store
        .record_external_mcp_discovery_success(
            &ExternalMcpDiscoveryRunRecord {
                discovery_run_id: Uuid::new_v4(),
                mcp_server_id: server.mcp_server_id,
                status: ExternalMcpDiscoveryStatus::Success,
                started_at: now,
                finished_at: now,
                discovered_tool_count: 1,
                active_tool_count: 1,
                schema_set_hash: None,
                error_summary: None,
                details: Map::new(),
                expected_server: None,
            },
            &[UpsertExternalMcpToolRecord {
                mcp_server_id: server.mcp_server_id,
                upstream_name: "search".to_string(),
                display_name: "Search".to_string(),
                description: None,
                input_schema: json!({"type": "object"}),
                metadata: McpToolMetadata {
                    title: Some("Search notes".to_string()),
                    annotations: annotations.as_object().cloned(),
                    output_schema: Some(json!({"type": "object"})),
                },
                schema_hash: format!("{server_key}-search-v1"),
            }],
        )
        .await
        .expect("tool discovery");
    (server, tools.into_iter().next().expect("tool"))
}

async fn exercise_mcp_server_grants<S>(store: &S)
where
    S: McpAccessRepository + McpRegistryRepository + Sync,
{
    let now = OffsetDateTime::now_utc();
    let (notes, notes_search) = seed_server(store, "notes").await;
    let (drive, drive_search) = seed_server(store, "drive").await;
    let subjects = [McpGrantSubject {
        subject_kind: McpToolGrantSubjectKind::ApiKey,
        subject_id: Uuid::new_v4(),
    }];

    assert!(
        !store
            .has_mcp_server_grant(&subjects, notes.mcp_server_id)
            .await
            .expect("no grants")
    );

    let toolset = store
        .create_mcp_toolset(&NewMcpToolsetRecord {
            toolset_key: "notes-readers".to_string(),
            display_name: "Notes readers".to_string(),
            description: None,
            created_at: now,
        })
        .await
        .expect("toolset");
    store
        .replace_mcp_toolset_tools(toolset.toolset_id, &[notes_search.mcp_tool_id], now)
        .await
        .expect("toolset tools");
    store
        .upsert_mcp_tool_grant(&UpsertMcpToolGrantRecord {
            subject_kind: subjects[0].subject_kind,
            subject_id: subjects[0].subject_id,
            target_kind: McpToolGrantTargetKind::Toolset,
            target_id: toolset.toolset_id,
            updated_at: now,
            expected_server: None,
            expected_subject_team: None,
        })
        .await
        .expect("toolset grant");

    assert!(
        store
            .has_mcp_server_grant(&subjects, notes.mcp_server_id)
            .await
            .expect("toolset grant check")
    );
    assert!(
        !store
            .has_mcp_server_grant(&subjects, drive.mcp_server_id)
            .await
            .expect("other server check")
    );

    store
        .disable_mcp_toolset(toolset.toolset_id, now)
        .await
        .expect("disable toolset");
    assert!(
        !store
            .has_mcp_server_grant(&subjects, notes.mcp_server_id)
            .await
            .expect("disabled toolset check")
    );

    store
        .upsert_mcp_tool_grant(&UpsertMcpToolGrantRecord {
            subject_kind: subjects[0].subject_kind,
            subject_id: subjects[0].subject_id,
            target_kind: McpToolGrantTargetKind::Tool,
            target_id: drive_search.mcp_tool_id,
            updated_at: now,
            expected_server: None,
            expected_subject_team: None,
        })
        .await
        .expect("tool grant");
    assert!(
        store
            .has_mcp_server_grant(&subjects, drive.mcp_server_id)
            .await
            .expect("direct grant check")
    );

    let access = store
        .resolve_mcp_access_for_subjects(
            &subjects,
            Some(drive.mcp_server_id),
            McpToolProjection::Access,
        )
        .await
        .expect("access projection");
    let full = store
        .resolve_mcp_access_for_subjects(
            &subjects,
            Some(drive.mcp_server_id),
            McpToolProjection::Full,
        )
        .await
        .expect("full projection");
    assert_eq!(access.allowed_tools.len(), 1);
    assert_eq!(full.allowed_tools.len(), 1);
    assert_eq!(
        access.allowed_tools[0].mcp_tool_id,
        drive_search.mcp_tool_id
    );
    assert_eq!(
        access.allowed_tools[0].input_schema,
        json!({"type": "object"})
    );
    assert_eq!(access.allowed_tools[0].metadata, McpToolMetadata::default());
    assert_eq!(full.allowed_tools[0].metadata, drive_search.metadata);

    store
        .disable_external_mcp_server(drive.mcp_server_id, now)
        .await
        .expect("disable server");
    assert!(
        !store
            .has_mcp_server_grant(&subjects, drive.mcp_server_id)
            .await
            .expect("disabled server check")
    );
}

#[tokio::test]
#[serial]
async fn libsql_mcp_server_grants_and_access_projection() {
    let tmp = tempdir().expect("tempdir");
    let db_path = tmp.path().join("gateway.db");
    run_migrations(&db_path).await.expect("migrations");
    let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
        .await
        .expect("store");
    exercise_mcp_server_grants(&store).await;
}

#[tokio::test]
#[serial]
async fn postgres_mcp_server_grants_and_access_projection() {
    let Some(test_db) = create_postgres_test_database().await else {
        eprintln!("skipping postgres MCP access test: TEST_POSTGRES_URL is not set");
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
    exercise_mcp_server_grants(&store).await;
    drop(store);
    drop_postgres_test_database(&test_db).await;
}
