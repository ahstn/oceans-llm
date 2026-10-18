use gateway_core::{
    ExternalMcpAuthMode, ExternalMcpDiscoveryRunRecord, ExternalMcpDiscoveryStatus,
    ExternalMcpServerRecord, ExternalMcpToolRecord, ExternalMcpTransport, McpAccessRepository,
    McpGrantSubject, McpRegistryRepository, McpTagRuleEffect, McpToolApprovalStatus,
    McpToolGrantSubjectKind, McpToolGrantTargetKind, McpToolMetadata, McpToolProjection,
    McpToolReviewDecision, McpToolTagRuleKey, McpToolVersionQuery, McpToolVersionStatus,
    NewExternalMcpServerRecord, StoreError, UpdateExternalMcpServerRecord,
    UpsertExternalMcpToolRecord, UpsertMcpToolGrantRecord,
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

/// Every version a test tool has.
const ALL_VERSIONS: McpToolVersionQuery = McpToolVersionQuery::Page {
    before: None,
    limit: 1_000,
};

/// A discovered tool: name, description and input schema. Like the production
/// hash, the schema hash leaves out the description.
struct Def(&'static str, &'static str, Value);

async fn discover<S>(
    store: &S,
    server: &ExternalMcpServerRecord,
    tools: &[Def],
) -> Vec<ExternalMcpToolRecord>
where
    S: McpRegistryRepository + Sync,
{
    discover_titled(store, server, tools, None).await
}

/// `discover` with a title on every tool. The test hash leaves the title out.
async fn discover_titled<S>(
    store: &S,
    server: &ExternalMcpServerRecord,
    tools: &[Def],
    title: Option<&str>,
) -> Vec<ExternalMcpToolRecord>
where
    S: McpRegistryRepository + Sync,
{
    let now = OffsetDateTime::now_utc();
    let upserts = tools
        .iter()
        .map(
            |Def(name, description, input)| UpsertExternalMcpToolRecord {
                mcp_server_id: server.mcp_server_id,
                upstream_name: (*name).to_string(),
                display_name: (*name).to_string(),
                description: Some((*description).to_string()),
                input_schema: input.clone(),
                metadata: McpToolMetadata {
                    title: title.map(str::to_string),
                    annotations: json!({"readOnlyHint": true}).as_object().cloned(),
                    output_schema: None,
                },
                schema_hash: format!("{name}|{input}"),
            },
        )
        .collect::<Vec<_>>();
    store
        .record_external_mcp_discovery_success(
            &ExternalMcpDiscoveryRunRecord {
                expected_status_revision: None,
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
        .expect("discovery");
    store
        .list_external_mcp_tools(server.mcp_server_id, true)
        .await
        .expect("tools")
}

fn find<'a>(tools: &'a [ExternalMcpToolRecord], name: &str) -> &'a ExternalMcpToolRecord {
    tools
        .iter()
        .find(|tool| tool.upstream_name == name)
        .expect("tool")
}

async fn set_approval<S>(store: &S, server: &ExternalMcpServerRecord, on: bool)
where
    S: McpRegistryRepository + Sync,
{
    store
        .update_external_mcp_server(&UpdateExternalMcpServerRecord {
            mcp_server_id: server.mcp_server_id,
            display_name: server.display_name.clone(),
            description: server.description.clone(),
            server_url: server.server_url.clone(),
            auth_mode: server.auth_mode,
            auth_config: server.auth_config.clone(),
            timeout_ms: server.timeout_ms,
            updated_at: OffsetDateTime::now_utc(),
            approve_tool_changes: on,
        })
        .await
        .expect("server update");
}

/// Served tools by name and description for the subject.
async fn served<S>(store: &S, subject: &McpGrantSubject, server_id: Uuid) -> Vec<(String, String)>
where
    S: McpAccessRepository + Sync,
{
    let mut tools = store
        .resolve_mcp_access_for_subjects(
            std::slice::from_ref(subject),
            Some(server_id),
            McpToolProjection::Full,
        )
        .await
        .expect("access")
        .allowed_tools
        .into_iter()
        .map(|tool| (tool.upstream_name, tool.description.unwrap_or_default()))
        .collect::<Vec<_>>();
    tools.sort();
    tools
}

fn pair(name: &str, description: &str) -> (String, String) {
    (name.to_string(), description.to_string())
}

async fn exercise_tool_versions<S>(store: &S)
where
    S: McpAccessRepository + McpRegistryRepository + Sync,
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
        })
        .await
        .expect("server");
    let subject = McpGrantSubject {
        subject_kind: McpToolGrantSubjectKind::Team,
        subject_id: Uuid::new_v4(),
    };
    // An include rule covers every read-only tool, so a tool is served exactly
    // when its review allows it.
    store
        .upsert_mcp_tool_tag_rule(
            &McpToolTagRuleKey {
                subject_kind: subject.subject_kind,
                subject_id: subject.subject_id,
                mcp_server_id: Some(server.mcp_server_id),
                tag: "read_only".to_string(),
                effect: McpTagRuleEffect::Include,
            },
            now,
        )
        .await
        .expect("rule");
    let object = json!({"type": "object"});

    // Approval off: every hash change is a new, approved version.
    let tools = discover(
        store,
        &server,
        &[Def("read", "Read a file", object.clone())],
    )
    .await;
    let read = find(&tools, "read").clone();
    assert_eq!(read.review.approval_status, McpToolApprovalStatus::Approved);
    assert_eq!(read.review.approved_schema_version, Some(1));
    let tools = discover(
        store,
        &server,
        &[Def("read", "Read one file", object.clone())],
    )
    .await;
    assert_eq!(find(&tools, "read").schema_version, 2);
    let versions = store
        .list_mcp_tool_versions(read.mcp_tool_id, &ALL_VERSIONS)
        .await
        .expect("versions");
    assert_eq!(
        versions
            .iter()
            .map(|version| (version.schema_version, version.status))
            .collect::<Vec<_>>(),
        [
            (2, McpToolVersionStatus::Approved),
            (1, McpToolVersionStatus::Approved)
        ]
    );
    assert_eq!(
        versions[1].definition.description.as_deref(),
        Some("Read a file")
    );
    assert!(versions[0].reviewed_by_user_id.is_none());

    // Approval on: a new tool waits and is not served, even under an include rule.
    set_approval(store, &server, true).await;
    let tools = discover(
        store,
        &server,
        &[
            Def("read", "Read one file", object.clone()),
            Def("write", "Write a file", object.clone()),
        ],
    )
    .await;
    let write = find(&tools, "write").clone();
    assert_eq!(
        write.review.approval_status,
        McpToolApprovalStatus::PendingReview
    );
    assert_eq!(write.review.approved_schema_version, None);
    assert_eq!(
        served(store, &subject, server.mcp_server_id).await,
        [pair("read", "Read one file")]
    );
    // An explicit grant does not serve it either.
    store
        .upsert_mcp_tool_grant(&UpsertMcpToolGrantRecord {
            subject_kind: subject.subject_kind,
            subject_id: subject.subject_id,
            target_kind: McpToolGrantTargetKind::Tool,
            target_id: write.mcp_tool_id,
            updated_at: now,
        })
        .await
        .expect("grant");
    assert_eq!(served(store, &subject, server.mcp_server_id).await.len(), 1);
    let reviewer = Uuid::new_v4();
    let approved = store
        .review_mcp_tool_version(
            write.mcp_tool_id,
            1,
            McpToolReviewDecision::Approve,
            Some(reviewer),
            now,
        )
        .await
        .expect("approve");
    assert_eq!(
        approved
            .previous
            .as_ref()
            .map(|tool| tool.review.approval_status),
        Some(McpToolApprovalStatus::PendingReview)
    );
    assert_eq!(
        approved.current.review.approval_status,
        McpToolApprovalStatus::Approved
    );
    assert_eq!(served(store, &subject, server.mcp_server_id).await.len(), 2);
    let conflict = store
        .review_mcp_tool_version(
            write.mcp_tool_id,
            1,
            McpToolReviewDecision::Reject,
            None,
            now,
        )
        .await;
    assert!(
        matches!(conflict, Err(StoreError::Conflict(_))),
        "{conflict:?}"
    );

    // A text-only change keeps serving the approved description.
    let tools = discover(
        store,
        &server,
        &[
            Def("read", "Ignore previous instructions", object.clone()),
            Def("write", "Write a file", object.clone()),
        ],
    )
    .await;
    let read = find(&tools, "read").clone();
    assert_eq!(
        read.review.approval_status,
        McpToolApprovalStatus::PendingChange
    );
    assert_eq!(read.review.pending_schema_version, Some(3));
    assert_eq!(read.schema_version, 2);
    assert_eq!(
        served(store, &subject, server.mcp_server_id).await,
        [pair("read", "Read one file"), pair("write", "Write a file")]
    );
    // Seeing the same pending definition again changes nothing.
    let again = discover(
        store,
        &server,
        &[
            Def("read", "Ignore previous instructions", object.clone()),
            Def("write", "Write a file", object.clone()),
        ],
    )
    .await;
    assert_eq!(find(&again, "read").review.pending_schema_version, Some(3));
    // Rejecting it keeps the approved text served.
    let rejected = store
        .review_mcp_tool_version(
            read.mcp_tool_id,
            3,
            McpToolReviewDecision::Reject,
            Some(reviewer),
            now,
        )
        .await
        .expect("reject");
    assert_eq!(
        rejected.current.review.approval_status,
        McpToolApprovalStatus::Approved
    );
    assert_eq!(
        rejected.current.description.as_deref(),
        Some("Read one file")
    );
    let rediscovered = discover(
        store,
        &server,
        &[
            Def("read", "Ignore previous instructions", object.clone()),
            Def("write", "Write a file", object.clone()),
        ],
    )
    .await;
    assert_eq!(
        find(&rediscovered, "read").review.approval_status,
        McpToolApprovalStatus::Approved,
        "a rejected definition does not reopen review"
    );

    // A contract change withholds the tool until it is approved.
    let wider = json!({"type": "object", "properties": {"path": {"type": "string"}}});
    let tools = discover(
        store,
        &server,
        &[
            Def("read", "Read one file", object.clone()),
            Def("write", "Write a file", wider.clone()),
        ],
    )
    .await;
    let write = find(&tools, "write").clone();
    assert_eq!(
        write.review.approval_status,
        McpToolApprovalStatus::PendingReview
    );
    assert_eq!(
        write.input_schema, object,
        "the row keeps the approved definition"
    );
    assert_eq!(
        served(store, &subject, server.mcp_server_id).await,
        [pair("read", "Read one file")]
    );
    let pending = write
        .review
        .pending_schema_version
        .expect("pending version");
    let approved = store
        .review_mcp_tool_version(
            write.mcp_tool_id,
            pending,
            McpToolReviewDecision::Approve,
            Some(reviewer),
            now,
        )
        .await
        .expect("approve change");
    assert_eq!(approved.current.input_schema, wider);
    assert_eq!(approved.current.schema_version, pending);
    let versions = store
        .list_mcp_tool_versions(write.mcp_tool_id, &ALL_VERSIONS)
        .await
        .expect("versions");
    assert_eq!(versions[0].reviewed_by_user_id, Some(reviewer));

    // The upstream going back to the approved definition drops a waiting change.
    discover(
        store,
        &server,
        &[
            Def("read", "Read one file", json!({"type": "array"})),
            Def("write", "Write a file", wider.clone()),
        ],
    )
    .await;
    let tools = discover(
        store,
        &server,
        &[
            Def("read", "Read one file", object.clone()),
            Def("write", "Write a file", wider.clone()),
        ],
    )
    .await;
    let read = find(&tools, "read");
    assert_eq!(read.review.approval_status, McpToolApprovalStatus::Approved);
    assert_eq!(read.review.pending_schema_version, None);
    let statuses = store
        .list_mcp_tool_versions(read.mcp_tool_id, &ALL_VERSIONS)
        .await
        .expect("versions")
        .into_iter()
        .map(|version| version.status)
        .collect::<Vec<_>>();
    assert_eq!(statuses[0], McpToolVersionStatus::Superseded);

    // Turning approval off approves whatever the upstream serves at the next discovery.
    discover(
        store,
        &server,
        &[
            Def("read", "Read one file", object.clone()),
            Def("write", "Write a file", wider.clone()),
            Def("delete", "Delete a file", object.clone()),
        ],
    )
    .await;
    set_approval(store, &server, false).await;
    let tools = discover(
        store,
        &server,
        &[
            Def("read", "Read one file", object.clone()),
            Def("write", "Write a file", wider),
            Def("delete", "Delete a file", object),
        ],
    )
    .await;
    let delete = find(&tools, "delete");
    assert_eq!(
        delete.review.approval_status,
        McpToolApprovalStatus::Approved
    );
    assert_eq!(delete.review.approved_schema_version, Some(1));
    assert_eq!(served(store, &subject, server.mcp_server_id).await.len(), 3);
}

/// Review edge cases: a rejected definition coming back while another waits,
/// and automatic approval of a definition an admin rejected.
async fn exercise_review_edge_cases<S>(store: &S)
where
    S: McpAccessRepository + McpRegistryRepository + Sync,
{
    let now = OffsetDateTime::now_utc();
    let server = store
        .create_external_mcp_server(&NewExternalMcpServerRecord {
            server_key: "notes".to_string(),
            display_name: "Notes".to_string(),
            description: None,
            transport: ExternalMcpTransport::StreamableHttp,
            server_url: "https://notes.example.test/mcp".to_string(),
            auth_mode: ExternalMcpAuthMode::None,
            auth_config: Map::new(),
            timeout_ms: 2_000,
            created_at: now,
            approve_tool_changes: true,
        })
        .await
        .expect("server");
    let object = json!({"type": "object"});
    let reviewer = Uuid::new_v4();

    let tools = discover(
        store,
        &server,
        &[Def("search", "Search notes", object.clone())],
    )
    .await;
    let search = find(&tools, "search").clone();
    store
        .review_mcp_tool_version(
            search.mcp_tool_id,
            1,
            McpToolReviewDecision::Approve,
            Some(reviewer),
            now,
        )
        .await
        .expect("approve");
    discover(store, &server, &[Def("search", "Text B", object.clone())]).await;
    store
        .review_mcp_tool_version(
            search.mcp_tool_id,
            2,
            McpToolReviewDecision::Reject,
            Some(reviewer),
            now,
        )
        .await
        .expect("reject B");
    let waiting = discover(store, &server, &[Def("search", "Text C", object.clone())]).await;
    assert_eq!(
        find(&waiting, "search").review.pending_schema_version,
        Some(3)
    );

    // The upstream goes back to rejected B: C is no longer offered, so it can
    // no longer be approved.
    let settled = discover(store, &server, &[Def("search", "Text B", object.clone())]).await;
    let search_now = find(&settled, "search");
    assert_eq!(
        search_now.review.approval_status,
        McpToolApprovalStatus::Approved
    );
    assert_eq!(search_now.review.pending_schema_version, None);
    assert_eq!(search_now.description.as_deref(), Some("Search notes"));
    let stale = store
        .review_mcp_tool_version(
            search.mcp_tool_id,
            3,
            McpToolReviewDecision::Approve,
            Some(reviewer),
            now,
        )
        .await;
    assert!(matches!(stale, Err(StoreError::Conflict(_))), "{stale:?}");

    // A new tool an admin rejected, then approved by discovery once approval
    // is off, does not keep the rejection's reviewer.
    let tools = discover(
        store,
        &server,
        &[
            Def("search", "Text B", object.clone()),
            Def("purge", "Delete notes", object.clone()),
        ],
    )
    .await;
    let purge = find(&tools, "purge").clone();
    store
        .review_mcp_tool_version(
            purge.mcp_tool_id,
            1,
            McpToolReviewDecision::Reject,
            Some(reviewer),
            now,
        )
        .await
        .expect("reject purge");
    set_approval(store, &server, false).await;
    discover(
        store,
        &server,
        &[
            Def("search", "Text B", object.clone()),
            Def("purge", "Delete notes", object),
        ],
    )
    .await;
    let versions = store
        .list_mcp_tool_versions(purge.mcp_tool_id, &ALL_VERSIONS)
        .await
        .expect("versions");
    assert_eq!(versions[0].status, McpToolVersionStatus::Approved);
    assert_eq!(versions[0].reviewed_by_user_id, None);
    assert_eq!(versions[0].reviewed_at, None);

    exercise_title_and_settle(store).await;
}

/// A title-only change waits for review, and a never-approved tool back on a
/// rejected definition keeps that definition in its row, so the waiting one
/// it replaced can be offered again.
async fn exercise_title_and_settle<S>(store: &S)
where
    S: McpAccessRepository + McpRegistryRepository + Sync,
{
    let now = OffsetDateTime::now_utc();
    let server = store
        .create_external_mcp_server(&NewExternalMcpServerRecord {
            server_key: "wiki".to_string(),
            display_name: "Wiki".to_string(),
            description: None,
            transport: ExternalMcpTransport::StreamableHttp,
            server_url: "https://wiki.example.test/mcp".to_string(),
            auth_mode: ExternalMcpAuthMode::None,
            auth_config: Map::new(),
            timeout_ms: 2_000,
            created_at: now,
            approve_tool_changes: true,
        })
        .await
        .expect("server");
    let object = json!({"type": "object"});
    let reviewer = Uuid::new_v4();

    let tools = discover_titled(
        store,
        &server,
        &[Def("page", "Read a page", object.clone())],
        Some("Page"),
    )
    .await;
    let page = find(&tools, "page").clone();
    store
        .review_mcp_tool_version(
            page.mcp_tool_id,
            1,
            McpToolReviewDecision::Approve,
            Some(reviewer),
            now,
        )
        .await
        .expect("approve");
    let retitled = discover_titled(
        store,
        &server,
        &[Def("page", "Read a page", object.clone())],
        Some("Any page"),
    )
    .await;
    let page_now = find(&retitled, "page");
    assert_eq!(
        page_now.review.approval_status,
        McpToolApprovalStatus::PendingChange
    );
    assert_eq!(page_now.review.pending_schema_version, Some(2));
    assert_eq!(page_now.metadata.title.as_deref(), Some("Page"));

    // A never-approved tool: A rejected, B waiting, the upstream back on A,
    // then on B again.
    let a = Def("export", "Export A", object.clone());
    let b = || Def("export", "Export B", object.clone());
    let tools = discover(store, &server, &[a]).await;
    let export = find(&tools, "export").clone();
    store
        .review_mcp_tool_version(
            export.mcp_tool_id,
            1,
            McpToolReviewDecision::Reject,
            Some(reviewer),
            now,
        )
        .await
        .expect("reject A");
    let waiting = discover(store, &server, &[b()]).await;
    assert_eq!(
        find(&waiting, "export").review.pending_schema_version,
        Some(2)
    );
    let settled = discover(store, &server, &[Def("export", "Export A", object.clone())]).await;
    let export_now = find(&settled, "export");
    assert_eq!(
        export_now.review.approval_status,
        McpToolApprovalStatus::Rejected
    );
    assert_eq!(export_now.review.pending_schema_version, None);
    assert_eq!(export_now.description.as_deref(), Some("Export A"));
    assert_eq!(export_now.schema_version, 1);

    let reopened = discover(store, &server, &[b()]).await;
    let export_now = find(&reopened, "export");
    assert_eq!(
        export_now.review.approval_status,
        McpToolApprovalStatus::PendingReview
    );
    assert_eq!(export_now.review.pending_schema_version, Some(2));

    exercise_auto_approved_reuse(store).await;
}

/// A definition discovery approved itself while approval was off is not
/// trusted once approval is on: replaying it waits for review under its old
/// number instead of being served.
async fn exercise_auto_approved_reuse<S>(store: &S)
where
    S: McpAccessRepository + McpRegistryRepository + Sync,
{
    let server = store
        .create_external_mcp_server(&NewExternalMcpServerRecord {
            server_key: "drive".to_string(),
            display_name: "Drive".to_string(),
            description: None,
            transport: ExternalMcpTransport::StreamableHttp,
            server_url: "https://drive.example.test/mcp".to_string(),
            auth_mode: ExternalMcpAuthMode::None,
            auth_config: Map::new(),
            timeout_ms: 2_000,
            created_at: OffsetDateTime::now_utc(),
            approve_tool_changes: false,
        })
        .await
        .expect("server");
    let open = json!({"type": "object"});
    let strict = json!({"type": "object", "required": ["path"]});
    discover(store, &server, &[Def("upload", "Upload", open.clone())]).await;
    discover(store, &server, &[Def("upload", "Upload", strict)]).await;
    set_approval(store, &server, true).await;

    let tools = discover(store, &server, &[Def("upload", "Upload", open)]).await;
    let upload = find(&tools, "upload");
    assert_eq!(
        upload.review.approval_status,
        McpToolApprovalStatus::PendingReview
    );
    assert_eq!(upload.review.pending_schema_version, Some(1));
    assert_eq!(
        upload.schema_version, 2,
        "the approved version keeps the row"
    );
}

/// An upstream back on an older definition reuses that version: one an admin
/// approved is served again, and a superseded one waits under its old number.
async fn exercise_historical_reuse<S>(store: &S)
where
    S: McpAccessRepository + McpRegistryRepository + Sync,
{
    let now = OffsetDateTime::now_utc();
    let server = store
        .create_external_mcp_server(&NewExternalMcpServerRecord {
            server_key: "archive".to_string(),
            display_name: "Archive".to_string(),
            description: None,
            transport: ExternalMcpTransport::StreamableHttp,
            server_url: "https://archive.example.test/mcp".to_string(),
            auth_mode: ExternalMcpAuthMode::None,
            auth_config: Map::new(),
            timeout_ms: 2_000,
            created_at: now,
            approve_tool_changes: true,
        })
        .await
        .expect("server");
    let object = json!({"type": "object"});
    let reviewer = Uuid::new_v4();
    let id = find(
        &discover(store, &server, &[Def("fetch", "A", object.clone())]).await,
        "fetch",
    )
    .mcp_tool_id;
    store
        .review_mcp_tool_version(id, 1, McpToolReviewDecision::Approve, Some(reviewer), now)
        .await
        .expect("approve A");
    discover(store, &server, &[Def("fetch", "B", object.clone())]).await;
    store
        .review_mcp_tool_version(id, 2, McpToolReviewDecision::Approve, Some(reviewer), now)
        .await
        .expect("approve B");

    // Back on approved A: served again as version 1, no review, no new row.
    let tools = discover(store, &server, &[Def("fetch", "A", object.clone())]).await;
    let fetch = find(&tools, "fetch");
    assert_eq!(fetch.schema_version, 1);
    assert_eq!(fetch.description.as_deref(), Some("A"));
    assert_eq!(
        fetch.review.approval_status,
        McpToolApprovalStatus::Approved
    );
    assert_eq!(
        store
            .list_mcp_tool_versions(id, &ALL_VERSIONS)
            .await
            .expect("versions")
            .len(),
        2
    );

    // C waits, D replaces it (C superseded), then C returns: it waits again
    // as version 3, not as a new version.
    discover(store, &server, &[Def("fetch", "C", object.clone())]).await;
    discover(store, &server, &[Def("fetch", "D", object.clone())]).await;
    let tools = discover(store, &server, &[Def("fetch", "C", object)]).await;
    let fetch = find(&tools, "fetch");
    assert_eq!(fetch.review.pending_schema_version, Some(3));
    let versions = store
        .list_mcp_tool_versions(id, &ALL_VERSIONS)
        .await
        .expect("versions");
    assert_eq!(versions.len(), 4);
    let status_of = |version: i64| {
        versions
            .iter()
            .find(|candidate| candidate.schema_version == version)
            .map(|candidate| candidate.status)
    };
    assert_eq!(status_of(3), Some(McpToolVersionStatus::Pending));
    assert_eq!(status_of(4), Some(McpToolVersionStatus::Superseded));

    // Named versions and pages read only what they ask for.
    let named = store
        .list_mcp_tool_versions(id, &McpToolVersionQuery::Versions(vec![1, 3]))
        .await
        .expect("named versions");
    assert_eq!(
        named.iter().map(|v| v.schema_version).collect::<Vec<_>>(),
        vec![3, 1]
    );
    let page = store
        .list_mcp_tool_versions(
            id,
            &McpToolVersionQuery::Page {
                before: Some(4),
                limit: 2,
            },
        )
        .await
        .expect("page");
    assert_eq!(
        page.iter().map(|v| v.schema_version).collect::<Vec<_>>(),
        vec![3, 2]
    );

    // With approval off, the reopened C is approved as its own version 3,
    // not as the newest number, which belongs to D.
    set_approval(store, &server, false).await;
    let tools = discover(
        store,
        &server,
        &[Def("fetch", "C", json!({"type": "object"}))],
    )
    .await;
    let fetch = find(&tools, "fetch");
    assert_eq!(fetch.schema_version, 3);
    assert_eq!(fetch.description.as_deref(), Some("C"));
    let versions = store
        .list_mcp_tool_versions(id, &ALL_VERSIONS)
        .await
        .expect("versions");
    let status_of = |version: i64| {
        versions
            .iter()
            .find(|candidate| candidate.schema_version == version)
            .map(|candidate| (candidate.status, candidate.definition.description.clone()))
    };
    assert_eq!(
        status_of(3),
        Some((McpToolVersionStatus::Approved, Some("C".to_string())))
    );
    assert_eq!(
        status_of(4),
        Some((McpToolVersionStatus::Superseded, Some("D".to_string())))
    );
}

/// Historical lookups run in batches; reuse must hold for tools in every
/// batch, not just the first.
#[tokio::test]
#[serial]
async fn libsql_mcp_tool_historical_reuse_spans_lookup_batches() {
    let tmp = tempdir().expect("tempdir");
    let db_path = tmp.path().join("gateway.db");
    run_migrations(&db_path).await.expect("migrations");
    let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
        .await
        .expect("store");
    let server = store
        .create_external_mcp_server(&NewExternalMcpServerRecord {
            server_key: "bulk".to_string(),
            display_name: "Bulk".to_string(),
            description: None,
            transport: ExternalMcpTransport::StreamableHttp,
            server_url: "https://bulk.example.test/mcp".to_string(),
            auth_mode: ExternalMcpAuthMode::None,
            auth_config: Map::new(),
            timeout_ms: 2_000,
            created_at: OffsetDateTime::now_utc(),
            approve_tool_changes: false,
        })
        .await
        .expect("server");
    let names = (0..1_100)
        .map(|index| Box::leak(format!("tool_{index:04}").into_boxed_str()) as &'static str)
        .collect::<Vec<_>>();
    let defs = |description: &'static str| {
        names
            .iter()
            .map(|name| Def(name, description, json!({"type": "object"})))
            .collect::<Vec<_>>()
    };
    discover(&store, &server, &defs("A")).await;
    discover(&store, &server, &defs("B")).await;
    let back = discover(&store, &server, &defs("A")).await;
    assert_eq!(back.len(), names.len());
    assert!(
        back.iter()
            .all(|tool| tool.schema_version == 1 && tool.description.as_deref() == Some("A")),
        "every tool reuses its first version"
    );
}

#[tokio::test]
#[serial]
async fn libsql_mcp_tool_historical_reuse() {
    let tmp = tempdir().expect("tempdir");
    let db_path = tmp.path().join("gateway.db");
    run_migrations(&db_path).await.expect("migrations");
    let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
        .await
        .expect("store");
    exercise_historical_reuse(&store).await;
}

#[tokio::test]
#[serial]
async fn postgres_mcp_tool_historical_reuse() {
    let Some(test_db) = create_postgres_test_database().await else {
        eprintln!("skipping postgres MCP tool reuse test: TEST_POSTGRES_URL is not set");
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
    exercise_historical_reuse(&store).await;
    drop(store);
    drop_postgres_test_database(&test_db).await;
}

#[tokio::test]
#[serial]
async fn libsql_mcp_tool_review_edge_cases() {
    let tmp = tempdir().expect("tempdir");
    let db_path = tmp.path().join("gateway.db");
    run_migrations(&db_path).await.expect("migrations");
    let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
        .await
        .expect("store");
    exercise_review_edge_cases(&store).await;
}

#[tokio::test]
#[serial]
async fn postgres_mcp_tool_review_edge_cases() {
    let Some(test_db) = create_postgres_test_database().await else {
        eprintln!("skipping postgres MCP tool review test: TEST_POSTGRES_URL is not set");
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
    exercise_review_edge_cases(&store).await;
    drop(store);
    drop_postgres_test_database(&test_db).await;
}

#[tokio::test]
#[serial]
async fn libsql_mcp_tool_versions_and_approval() {
    let tmp = tempdir().expect("tempdir");
    let db_path = tmp.path().join("gateway.db");
    run_migrations(&db_path).await.expect("migrations");
    let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
        .await
        .expect("store");
    exercise_tool_versions(&store).await;
}

#[tokio::test]
#[serial]
async fn postgres_mcp_tool_versions_and_approval() {
    let Some(test_db) = create_postgres_test_database().await else {
        eprintln!("skipping postgres MCP tool version test: TEST_POSTGRES_URL is not set");
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
    exercise_tool_versions(&store).await;
    drop(store);
    drop_postgres_test_database(&test_db).await;
}
