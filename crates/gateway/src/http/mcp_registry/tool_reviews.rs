//! Admin endpoints for MCP tool versions and approval of changed tools. See
//! `docs/adr/2026-10-17-mcp-tag-grants-and-tool-versioning.md`.

use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use gateway_core::{ExternalMcpToolRecord, McpToolReviewDecision, McpToolVersionRecord};
use gateway_service::{AuditAction, McpToolPolicyService, admin_audit::actions};
use serde::Serialize;
use serde_json::{Map, Value};
use utoipa::ToSchema;
use uuid::Uuid;

use super::tool_policy::{McpToolPayload, require_mcp_policy_admin, tool_event};
use super::views::map_tool;
use crate::http::{
    admin_contract::{Envelope, OpenAiErrorEnvelopeView, envelope, format_timestamp},
    audit::AuditRequestMeta,
    error::AppError,
    state::AppState,
};

/// One definition the upstream served for a tool.
#[derive(Debug, Serialize, ToSchema)]
pub struct McpToolVersionView {
    schema_version: i64,
    schema_hash: String,
    /// `approved`, `pending`, `rejected` or `superseded`.
    status: String,
    title: Option<String>,
    description: Option<String>,
    #[schema(additional_properties = true)]
    input_schema: Value,
    #[schema(additional_properties = true)]
    output_schema: Option<Value>,
    /// Annotations as the upstream declared them, without admin overrides.
    #[schema(additional_properties = true)]
    annotations: Option<Map<String, Value>>,
    discovered_at: String,
    reviewed_at: Option<String>,
    /// Empty for a version approved automatically by discovery.
    reviewed_by_user_id: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpToolVersionsPayload {
    /// Newest first.
    items: Vec<McpToolVersionView>,
}

fn map_version(version: McpToolVersionRecord) -> McpToolVersionView {
    McpToolVersionView {
        schema_version: version.schema_version,
        schema_hash: version.definition.schema_hash,
        status: version.status.as_str().to_string(),
        title: version.definition.title,
        description: version.definition.description,
        input_schema: version.definition.input_schema,
        output_schema: version.definition.output_schema,
        annotations: version.definition.annotations,
        discovered_at: format_timestamp(version.discovered_at),
        reviewed_at: version.reviewed_at.map(format_timestamp),
        reviewed_by_user_id: version.reviewed_by_user_id.map(|id| id.to_string()),
    }
}

/// The audited approval state of a tool.
fn review_summary(tool: &ExternalMcpToolRecord) -> Value {
    serde_json::json!({
        "approval_status": tool.review.approval_status.as_str(),
        "schema_version": tool.schema_version,
        "pending_schema_version": tool.review.pending_schema_version,
    })
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/mcp/servers/{server_id}/tools/{tool_id}/versions",
    params(
        ("server_id" = Uuid, Path, description = "External MCP server identifier"),
        ("tool_id" = Uuid, Path, description = "MCP tool identifier")
    ),
    responses(
        (status = 200, body = Envelope<McpToolVersionsPayload>),
        (status = 404, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
pub async fn list_mcp_tool_versions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((server_id, tool_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Envelope<McpToolVersionsPayload>>, AppError> {
    require_mcp_policy_admin(&state, &headers, Some(server_id)).await?;
    let items = McpToolPolicyService::new(state.store.clone())
        .list_versions(server_id, tool_id)
        .await?
        .into_iter()
        .map(map_version)
        .collect();
    Ok(Json(envelope(McpToolVersionsPayload { items })))
}

async fn review(
    state: AppState,
    headers: HeaderMap,
    meta: AuditRequestMeta,
    (server_id, tool_id, schema_version): (Uuid, Uuid, i64),
    decision: McpToolReviewDecision,
    action: AuditAction,
) -> Result<Json<Envelope<McpToolPayload>>, AppError> {
    let actor = require_mcp_policy_admin(&state, &headers, Some(server_id)).await?;
    // The store applies a review only to a pending version, so a second
    // review of the same version is a conflict and records nothing.
    let update = McpToolPolicyService::new(state.store.clone())
        .review_version(
            server_id,
            tool_id,
            schema_version,
            decision,
            Some(actor.user_id),
        )
        .await?;
    if let Some(previous) = &update.previous {
        state
            .audit()
            .record(
                &meta.user_context(&actor),
                tool_event(action, &update.current)
                    .meta("reviewed_schema_version", schema_version)
                    .before(review_summary(previous))
                    .after(review_summary(&update.current)),
            )
            .await;
    }
    Ok(Json(envelope(McpToolPayload {
        tool: map_tool(update.current),
    })))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/mcp/servers/{server_id}/tools/{tool_id}/versions/{schema_version}/approve",
    params(
        ("server_id" = Uuid, Path, description = "External MCP server identifier"),
        ("tool_id" = Uuid, Path, description = "MCP tool identifier"),
        ("schema_version" = i64, Path, description = "Pending version to approve")
    ),
    responses(
        (status = 200, body = Envelope<McpToolPayload>),
        (status = 404, body = OpenAiErrorEnvelopeView),
        (status = 409, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
/// Approves a pending version, which becomes the served definition.
pub async fn approve_mcp_tool_version(
    State(state): State<AppState>,
    headers: HeaderMap,
    meta: AuditRequestMeta,
    Path(path): Path<(Uuid, Uuid, i64)>,
) -> Result<Json<Envelope<McpToolPayload>>, AppError> {
    review(
        state,
        headers,
        meta,
        path,
        McpToolReviewDecision::Approve,
        actions::MCP_TOOL_VERSION_APPROVED,
    )
    .await
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/mcp/servers/{server_id}/tools/{tool_id}/versions/{schema_version}/reject",
    params(
        ("server_id" = Uuid, Path, description = "External MCP server identifier"),
        ("tool_id" = Uuid, Path, description = "MCP tool identifier"),
        ("schema_version" = i64, Path, description = "Pending version to reject")
    ),
    responses(
        (status = 200, body = Envelope<McpToolPayload>),
        (status = 404, body = OpenAiErrorEnvelopeView),
        (status = 409, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
/// Rejects a pending version. A text-only change keeps serving the previous
/// version; a new tool or a contract change stays withheld.
pub async fn reject_mcp_tool_version(
    State(state): State<AppState>,
    headers: HeaderMap,
    meta: AuditRequestMeta,
    Path(path): Path<(Uuid, Uuid, i64)>,
) -> Result<Json<Envelope<McpToolPayload>>, AppError> {
    review(
        state,
        headers,
        meta,
        path,
        McpToolReviewDecision::Reject,
        actions::MCP_TOOL_VERSION_REJECTED,
    )
    .await
}
