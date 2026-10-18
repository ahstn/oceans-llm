use std::collections::BTreeSet;

use gateway_core::{
    ExternalMcpAuthMode, ExternalMcpDiscoveryRunRecord, ExternalMcpDiscoveryStatus,
    ExternalMcpServerRecord, ExternalMcpToolRecord, ExternalMcpTransport, McpAccessRepository,
    McpAnnotationOverrides, McpGrantSubject, McpRegistryRepository, McpTagRuleEffect,
    McpToolGrantSubjectKind, McpToolGrantTargetKind, McpToolMetadata, McpToolProjection,
    McpToolTagRuleKey, NewExternalMcpServerRecord, UpsertExternalMcpToolRecord,
    UpsertMcpToolGrantRecord,
};
use serde_json::{Map, Value, json};
use serial_test::serial;
use tempfile::tempdir;
use time::OffsetDateTime;
use uuid::Uuid;

use super::{create_postgres_test_database, drop_postgres_test_database};
use crate::{
    LibsqlStore, PostgresStore, StoreConnectionOptions, run_migrations, run_migrations_with_options,
};

async fn create_server<S>(store: &S, server_key: &str) -> ExternalMcpServerRecord
where
    S: McpRegistryRepository + Sync,
{
    store
        .create_external_mcp_server(&NewExternalMcpServerRecord {
            server_key: server_key.to_string(),
            display_name: server_key.to_string(),
            description: None,
            transport: ExternalMcpTransport::StreamableHttp,
            server_url: format!("https://{server_key}.example.test/mcp"),
            auth_mode: ExternalMcpAuthMode::None,
            auth_config: Map::new(),
            timeout_ms: 2_000,
            created_at: OffsetDateTime::now_utc(),
            approve_tool_changes: false,
        })
        .await
        .expect("server")
}

/// Records one discovery of `tools`, given as `(name, annotations)`.
async fn discover<S>(
    store: &S,
    server: &ExternalMcpServerRecord,
    tools: &[(&str, Value)],
) -> Vec<ExternalMcpToolRecord>
where
    S: McpRegistryRepository + Sync,
{
    let now = OffsetDateTime::now_utc();
    let upserts = tools
        .iter()
        .map(|(name, annotations)| UpsertExternalMcpToolRecord {
            mcp_server_id: server.mcp_server_id,
            upstream_name: (*name).to_string(),
            display_name: (*name).to_string(),
            description: None,
            input_schema: json!({"type": "object"}),
            metadata: McpToolMetadata {
                title: None,
                annotations: annotations.as_object().cloned(),
                output_schema: None,
            },
            schema_hash: format!("{}-{name}-{annotations}", server.server_key),
        })
        .collect::<Vec<_>>();
    store
        .record_external_mcp_discovery_success(
            &ExternalMcpDiscoveryRunRecord {
                expected_server_status: None,
                discovery_run_id: Uuid::new_v4(),
                mcp_server_id: server.mcp_server_id,
                status: ExternalMcpDiscoveryStatus::Success,
                started_at: now,
                finished_at: now,
                discovered_tool_count: upserts.len() as i64,
                active_tool_count: upserts.len() as i64,
                schema_set_hash: None,
                error_summary: None,
                details: Map::new(),
            },
            &upserts,
        )
        .await
        .expect("discovery")
}

fn tool<'a>(tools: &'a [ExternalMcpToolRecord], name: &str) -> &'a ExternalMcpToolRecord {
    tools
        .iter()
        .find(|tool| tool.upstream_name == name)
        .expect("tool")
}

/// The tools the subjects may use on the server. It also checks that the
/// single-tool lookup `tools/call` uses agrees for every tool on the server.
async fn allowed_names<S>(store: &S, subjects: &[McpGrantSubject], server_id: Uuid) -> Vec<String>
where
    S: McpAccessRepository + McpRegistryRepository + Sync,
{
    let mut names = store
        .resolve_mcp_access_for_subjects(subjects, Some(server_id), McpToolProjection::Access)
        .await
        .expect("access")
        .allowed_tools
        .into_iter()
        .map(|tool| tool.upstream_name)
        .collect::<Vec<_>>();
    names.sort();
    for tool in store
        .list_external_mcp_tools(server_id, true)
        .await
        .expect("server tools")
    {
        let single = store
            .resolve_mcp_tool_for_subjects(subjects, server_id, &tool.upstream_name)
            .await
            .expect("single tool");
        assert_eq!(
            single.is_some(),
            names.contains(&tool.upstream_name),
            "single-tool lookup disagrees for {}",
            tool.upstream_name
        );
    }
    names
}

async fn catalog_names<S>(store: &S, subjects: &[McpGrantSubject], server_key: &str) -> Vec<String>
where
    S: McpAccessRepository + Sync,
{
    let mut names = store
        .resolve_mcp_catalog_access_for_subjects(subjects, Some(server_key))
        .await
        .expect("catalog")
        .allowed_tools
        .into_iter()
        .map(|record| record.tool.upstream_name)
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn rule(
    subject: &McpGrantSubject,
    server: Option<&ExternalMcpServerRecord>,
    tag: &str,
    effect: McpTagRuleEffect,
) -> McpToolTagRuleKey {
    McpToolTagRuleKey {
        subject_kind: subject.subject_kind,
        subject_id: subject.subject_id,
        mcp_server_id: server.map(|server| server.mcp_server_id),
        tag: tag.to_string(),
        effect,
    }
}

async fn exercise_tag_rules<S>(store: &S)
where
    S: McpAccessRepository + McpRegistryRepository + Sync,
{
    let now = OffsetDateTime::now_utc();
    let files = create_server(store, "files").await;
    let notes = create_server(store, "notes").await;
    let read_only = json!({"readOnlyHint": true});
    let tools = discover(
        store,
        &files,
        &[
            ("read", read_only.clone()),
            ("write", json!({"destructiveHint": false})),
            ("delete", Value::Null),
        ],
    )
    .await;
    discover(store, &notes, &[("search", read_only.clone())]).await;

    let team = McpGrantSubject {
        subject_kind: McpToolGrantSubjectKind::Team,
        subject_id: Uuid::new_v4(),
    };
    let key = McpGrantSubject {
        subject_kind: McpToolGrantSubjectKind::ApiKey,
        subject_id: Uuid::new_v4(),
    };
    let subjects = [key.clone(), team.clone()];

    // An include rule grants every read-only tool on its server, and no other.
    let (include, created) = store
        .upsert_mcp_tool_tag_rule(
            &rule(&team, Some(&files), "read_only", McpTagRuleEffect::Include),
            now,
        )
        .await
        .expect("include rule");
    assert!(created);
    let (_, created_again) = store
        .upsert_mcp_tool_tag_rule(
            &rule(&team, Some(&files), "read_only", McpTagRuleEffect::Include),
            now,
        )
        .await
        .expect("repeat include rule");
    assert!(
        !created_again,
        "an identical active rule is not created twice"
    );
    assert_eq!(
        allowed_names(store, &subjects, files.mcp_server_id).await,
        ["read"]
    );
    assert!(
        allowed_names(store, &subjects, notes.mcp_server_id)
            .await
            .is_empty()
    );
    assert_eq!(catalog_names(store, &subjects, "files").await, ["read"]);
    assert!(
        store
            .has_mcp_server_grant(&subjects, files.mcp_server_id)
            .await
            .expect("server grant")
    );

    // A tool discovered later matches without a new grant.
    let tools_later = discover(
        store,
        &files,
        &[
            ("read", read_only.clone()),
            ("write", json!({"destructiveHint": false})),
            ("delete", Value::Null),
            ("list", read_only.clone()),
        ],
    )
    .await;
    assert_eq!(
        allowed_names(store, &subjects, files.mcp_server_id).await,
        ["list", "read"]
    );

    // An exclude rule on any subject wins over an explicit grant to another.
    store
        .upsert_mcp_tool_grant(&UpsertMcpToolGrantRecord {
            subject_kind: key.subject_kind,
            subject_id: key.subject_id,
            target_kind: McpToolGrantTargetKind::Tool,
            target_id: tool(&tools, "delete").mcp_tool_id,
            updated_at: now,
        })
        .await
        .expect("explicit grant");
    assert_eq!(
        allowed_names(store, &subjects, files.mcp_server_id).await,
        ["delete", "list", "read"]
    );
    let (exclude, _) = store
        .upsert_mcp_tool_tag_rule(
            &rule(&team, None, "destructive", McpTagRuleEffect::Exclude),
            now,
        )
        .await
        .expect("exclude rule");
    assert_eq!(exclude.mcp_server_id, None);
    assert_eq!(
        allowed_names(store, &subjects, files.mcp_server_id).await,
        ["list", "read"]
    );
    assert_eq!(
        catalog_names(store, &subjects, "files").await,
        ["list", "read"]
    );

    // An admin override changes which rules match, and discovery keeps it.
    let read_id = tool(&tools_later, "read").mcp_tool_id;
    let overridden = store
        .set_mcp_tool_annotation_overrides(
            read_id,
            &McpAnnotationOverrides {
                read_only: Some(false),
                ..McpAnnotationOverrides::default()
            },
        )
        .await
        .expect("override");
    assert!(overridden.changed());
    assert_eq!(overridden.current.metadata.hint("readOnlyHint"), Some(true));
    let unchanged = store
        .set_mcp_tool_annotation_overrides(read_id, &overridden.current.policy.annotation_overrides)
        .await
        .expect("same override");
    assert!(!unchanged.changed());
    assert_eq!(
        allowed_names(store, &subjects, files.mcp_server_id).await,
        ["list"]
    );
    discover(
        store,
        &files,
        &[
            ("read", read_only.clone()),
            ("write", json!({"destructiveHint": false})),
            ("delete", Value::Null),
            ("list", read_only.clone()),
        ],
    )
    .await;
    let reloaded = store
        .get_external_mcp_tool(read_id)
        .await
        .expect("tool lookup")
        .expect("tool");
    assert_eq!(
        reloaded.policy.annotation_overrides.read_only,
        Some(false),
        "discovery never writes admin overrides"
    );
    assert_eq!(reloaded.schema_hash, tool(&tools_later, "read").schema_hash);

    // Admin tags can be selected by rules too.
    let write_id = tool(&tools, "write").mcp_tool_id;
    let tagged = store
        .set_mcp_tool_tags(write_id, &["finance".to_string()])
        .await
        .expect("tags");
    assert!(tagged.changed());
    store
        .upsert_mcp_tool_tag_rule(
            &rule(&key, Some(&files), "finance", McpTagRuleEffect::Include),
            now,
        )
        .await
        .expect("admin tag rule");
    assert_eq!(
        allowed_names(store, &subjects, files.mcp_server_id).await,
        ["list", "write"]
    );

    // An exclude rule can remove a caller's last tool, and with it the server.
    let lone = [McpGrantSubject {
        subject_kind: McpToolGrantSubjectKind::User,
        subject_id: Uuid::new_v4(),
    }];
    store
        .upsert_mcp_tool_grant(&UpsertMcpToolGrantRecord {
            subject_kind: lone[0].subject_kind,
            subject_id: lone[0].subject_id,
            target_kind: McpToolGrantTargetKind::Tool,
            target_id: tool(&tools, "delete").mcp_tool_id,
            updated_at: now,
        })
        .await
        .expect("lone grant");
    assert!(
        store
            .has_mcp_server_grant(&lone, files.mcp_server_id)
            .await
            .expect("lone server grant")
    );
    let (lone_exclude, _) = store
        .upsert_mcp_tool_tag_rule(
            &rule(
                &lone[0],
                Some(&files),
                "destructive",
                McpTagRuleEffect::Exclude,
            ),
            now,
        )
        .await
        .expect("lone exclude");
    assert!(
        !store
            .has_mcp_server_grant(&lone, files.mcp_server_id)
            .await
            .expect("excluded server grant")
    );

    // Revoking restores access, and a second revoke changes nothing.
    let revoked = store
        .revoke_mcp_tool_tag_rule(lone_exclude.rule_id, now)
        .await
        .expect("revoke")
        .expect("revoked rule");
    assert_eq!(revoked.rule_id, lone_exclude.rule_id);
    assert!(
        store
            .revoke_mcp_tool_tag_rule(lone_exclude.rule_id, now)
            .await
            .expect("second revoke")
            .is_none()
    );
    assert!(
        store
            .has_mcp_server_grant(&lone, files.mcp_server_id)
            .await
            .expect("restored server grant")
    );
    let fetched = store
        .get_mcp_tool_tag_rule(lone_exclude.rule_id)
        .await
        .expect("get rule")
        .expect("rule");
    assert!(!fetched.is_active);

    let team_rules = store
        .list_mcp_tool_tag_rules(Some(team.subject_kind), Some(team.subject_id))
        .await
        .expect("list rules");
    assert_eq!(team_rules.len(), 2);
    assert!(
        team_rules
            .iter()
            .any(|rule| rule.rule_id == include.rule_id)
    );

    assert_active_tool_pages(store).await;
}

/// Paging through active tools two at a time yields the same tools, in the
/// same order, as one large page.
async fn assert_active_tool_pages<S>(store: &S)
where
    S: McpAccessRepository + Sync,
{
    let ids = |tools: &[ExternalMcpToolRecord]| {
        tools
            .iter()
            .map(|tool| tool.mcp_tool_id)
            .collect::<Vec<_>>()
    };
    let all = store
        .list_active_mcp_tools(None, None, 1_000)
        .await
        .expect("all tools");
    assert!(all.len() > 2, "need more than one page");
    let mut paged = Vec::new();
    let mut after: Option<(Uuid, String)> = None;
    loop {
        let page = store
            .list_active_mcp_tools(
                None,
                after
                    .as_ref()
                    .map(|(server, name)| (*server, name.as_str())),
                2,
            )
            .await
            .expect("page");
        after = page
            .last()
            .map(|tool| (tool.mcp_server_id, tool.upstream_name.clone()));
        let done = page.len() < 2;
        paged.extend(page);
        if done {
            break;
        }
    }
    assert_eq!(ids(&paged), ids(&all));

    let server_id = all[0].mcp_server_id;
    let one_server = store
        .list_active_mcp_tools(Some(server_id), None, 1_000)
        .await
        .expect("server tools");
    assert!(
        one_server
            .iter()
            .all(|tool| tool.mcp_server_id == server_id)
    );
    assert_eq!(
        one_server.len(),
        all.iter()
            .filter(|tool| tool.mcp_server_id == server_id)
            .count()
    );
}

#[tokio::test]
#[serial]
async fn libsql_mcp_tag_rules_include_exclude_and_overrides() {
    let tmp = tempdir().expect("tempdir");
    let db_path = tmp.path().join("gateway.db");
    run_migrations(&db_path).await.expect("migrations");
    let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
        .await
        .expect("store");
    exercise_tag_rules(&store).await;

    // The index the writes maintain, and the migration's backfill run again
    // from scratch, both hold exactly `effective_tags` for every tool.
    let expected = expected_effective_tags(&store).await;
    assert_eq!(libsql_effective_tags(&store).await, expected);
    store
        .connection()
        .execute("DELETE FROM mcp_tool_effective_tags", ())
        .await
        .expect("clear tags");
    store
        .connection()
        .execute_batch(include_str!(
            "../../migrations/V69__mcp_tool_effective_tags.sql"
        ))
        .await
        .expect("backfill");
    assert_eq!(libsql_effective_tags(&store).await, expected);
}

async fn libsql_effective_tags(store: &LibsqlStore) -> BTreeSet<(String, String)> {
    let mut rows = store
        .connection()
        .query("SELECT mcp_tool_id, tag FROM mcp_tool_effective_tags", ())
        .await
        .expect("tags");
    let mut tags = BTreeSet::new();
    while let Some(row) = rows.next().await.expect("row") {
        tags.insert((row.get(0).expect("id"), row.get(1).expect("tag")));
    }
    tags
}

/// `(tool id, tag)` for every tool's `effective_tags`.
async fn expected_effective_tags<S>(store: &S) -> BTreeSet<(String, String)>
where
    S: McpRegistryRepository + Sync,
{
    let mut expected = BTreeSet::new();
    for server in store
        .list_external_mcp_servers(true)
        .await
        .expect("servers")
    {
        for tool in store
            .list_external_mcp_tools(server.mcp_server_id, true)
            .await
            .expect("tools")
        {
            for tag in gateway_core::mcp_tool_policy::effective_tags(&tool) {
                expected.insert((tool.mcp_tool_id.to_string(), tag));
            }
        }
    }
    expected
}

#[tokio::test]
#[serial]
async fn postgres_mcp_tag_rules_include_exclude_and_overrides() {
    let Some(test_db) = create_postgres_test_database().await else {
        eprintln!("skipping postgres MCP tag rule test: TEST_POSTGRES_URL is not set");
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
    exercise_tag_rules(&store).await;

    let expected = expected_effective_tags(&store).await;
    let read = || async {
        sqlx::query_as::<_, (String, String)>(
            "SELECT mcp_tool_id, tag FROM mcp_tool_effective_tags",
        )
        .fetch_all(store.pool())
        .await
        .expect("tags")
        .into_iter()
        .collect::<BTreeSet<_>>()
    };
    assert_eq!(read().await, expected);
    sqlx::query("DELETE FROM mcp_tool_effective_tags")
        .execute(store.pool())
        .await
        .expect("clear tags");
    sqlx::raw_sql(include_str!(
        "../../migrations/postgres/V69__mcp_tool_effective_tags.sql"
    ))
    .execute(store.pool())
    .await
    .expect("backfill");
    assert_eq!(read().await, expected);
    drop(store);
    drop_postgres_test_database(&test_db).await;
}
