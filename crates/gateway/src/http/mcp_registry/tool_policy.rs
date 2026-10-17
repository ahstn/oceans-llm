//! Admin endpoints for MCP tool policy: admin tags, annotation overrides and
//! tag rules. See `docs/adr/2026-10-17-mcp-tag-grants-and-tool-versioning.md`.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};
use gateway_core::{
    McpAccessRepository, McpTagRuleEffect, McpToolTagRuleKey, McpToolTagRuleRecord, UserRecord,
};
use gateway_service::{AuditEvent, McpToolPolicyService, admin_audit::actions};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::views::{McpToolAnnotationsView, McpToolView, map_tool, parse_grant_subject_kind};
use crate::http::{
    admin_auth::require_platform_admin,
    admin_contract::{Envelope, OpenAiErrorEnvelopeView, envelope, format_timestamp},
    audit::AuditRequestMeta,
    error::AppError,
    state::AppState,
};

/// The one permission check for changing MCP tool policy and tag rules.
/// `mcp_server_id` is the server the change applies to, or `None` for a rule
/// across every server. Platform admins only, like grants, for now; delegated
/// server owners (#445) widen this check here.
async fn require_mcp_policy_admin(
    state: &AppState,
    headers: &HeaderMap,
    _mcp_server_id: Option<Uuid>,
) -> Result<UserRecord, AppError> {
    require_platform_admin(state, headers).await
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpToolPayload {
    tool: McpToolView,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetMcpToolTagsRequest {
    /// Admin tags. They are lowercased, deduplicated and sorted.
    tags: Vec<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetMcpToolAnnotationOverridesRequest {
    /// Hints to override. A missing or null hint keeps the upstream's value.
    annotation_overrides: McpToolAnnotationsView,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpTagRuleView {
    id: String,
    subject_kind: String,
    subject_id: String,
    /// The server the rule applies to; null for every server (exclude only).
    server_id: Option<String>,
    tag: String,
    /// `include` grants matching tools; `exclude` removes them and wins.
    effect: String,
    is_active: bool,
    created_at: String,
    updated_at: String,
    revoked_at: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpTagRulePayload {
    rule: McpTagRuleView,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpTagRulesPayload {
    items: Vec<McpTagRuleView>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct McpTagRulesQuery {
    subject_kind: Option<String>,
    subject_id: Option<Uuid>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateMcpTagRuleRequest {
    subject_kind: String,
    subject_id: Uuid,
    /// Required for `include`. Omit for an `exclude` rule on every server.
    #[serde(default)]
    server_id: Option<Uuid>,
    tag: String,
    effect: String,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct McpTagRulePreviewQuery {
    server_id: Option<Uuid>,
    tag: String,
    effect: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpTagRulePreviewPayload {
    /// Active tools the rule would select, at most 200. Only this rule is
    /// applied.
    tools: Vec<McpToolView>,
    /// More tools match than the preview returns.
    truncated: bool,
}

fn parse_effect(value: &str) -> Result<McpTagRuleEffect, AppError> {
    McpTagRuleEffect::from_db(value).ok_or_else(|| {
        AppError(gateway_core::GatewayError::InvalidRequest(format!(
            "invalid MCP tag rule effect `{value}`; expected include or exclude"
        )))
    })
}

fn map_tag_rule(rule: McpToolTagRuleRecord) -> McpTagRuleView {
    McpTagRuleView {
        id: rule.rule_id.to_string(),
        subject_kind: rule.subject_kind.as_str().to_string(),
        subject_id: rule.subject_id.to_string(),
        server_id: rule.mcp_server_id.map(|id| id.to_string()),
        tag: rule.tag,
        effect: rule.effect.as_str().to_string(),
        is_active: rule.is_active,
        created_at: format_timestamp(rule.created_at),
        updated_at: format_timestamp(rule.updated_at),
        revoked_at: rule.revoked_at.map(format_timestamp),
    }
}

/// The audited shape of a rule: what it selects, not its timestamps.
fn tag_rule_summary(rule: &McpToolTagRuleRecord) -> serde_json::Value {
    serde_json::json!({
        "subject_kind": rule.subject_kind.as_str(),
        "subject_id": rule.subject_id,
        "server_id": rule.mcp_server_id,
        "tag": rule.tag,
        "effect": rule.effect.as_str(),
    })
}

fn tool_event(
    action: gateway_service::AuditAction,
    tool: &gateway_core::ExternalMcpToolRecord,
) -> AuditEvent {
    AuditEvent::new(action)
        .target(tool.mcp_tool_id)
        .label(tool.upstream_name.clone())
        .meta("server_id", tool.mcp_server_id)
}

#[utoipa::path(
    put,
    path = "/api/v1/admin/mcp/servers/{server_id}/tools/{tool_id}/tags",
    request_body = SetMcpToolTagsRequest,
    params(
        ("server_id" = Uuid, Path, description = "External MCP server identifier"),
        ("tool_id" = Uuid, Path, description = "MCP tool identifier")
    ),
    responses(
        (status = 200, body = Envelope<McpToolPayload>),
        (status = 400, body = OpenAiErrorEnvelopeView),
        (status = 404, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
pub async fn set_mcp_tool_tags(
    State(state): State<AppState>,
    headers: HeaderMap,
    meta: AuditRequestMeta,
    Path((server_id, tool_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<SetMcpToolTagsRequest>,
) -> Result<Json<Envelope<McpToolPayload>>, AppError> {
    let actor = require_mcp_policy_admin(&state, &headers, Some(server_id)).await?;
    let update = McpToolPolicyService::new(state.store.clone())
        .set_tags(server_id, tool_id, &request.tags)
        .await?;
    // The store writes nothing when the tags already match, so a resave records nothing.
    if let Some(previous) = &update.previous {
        state
            .audit()
            .record(
                &meta.user_context(&actor),
                tool_event(actions::MCP_TOOL_TAGS_UPDATED, &update.current)
                    .before(serde_json::json!({ "tags": previous.policy.tags }))
                    .after(serde_json::json!({ "tags": update.current.policy.tags })),
            )
            .await;
    }
    Ok(Json(envelope(McpToolPayload {
        tool: map_tool(update.current),
    })))
}

#[utoipa::path(
    put,
    path = "/api/v1/admin/mcp/servers/{server_id}/tools/{tool_id}/annotation-overrides",
    request_body = SetMcpToolAnnotationOverridesRequest,
    params(
        ("server_id" = Uuid, Path, description = "External MCP server identifier"),
        ("tool_id" = Uuid, Path, description = "MCP tool identifier")
    ),
    responses(
        (status = 200, body = Envelope<McpToolPayload>),
        (status = 400, body = OpenAiErrorEnvelopeView),
        (status = 404, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
/// Replaces the tool's annotation overrides. Overrides never change the
/// schema hash or version, and the upstream values stay visible.
pub async fn set_mcp_tool_annotation_overrides(
    State(state): State<AppState>,
    headers: HeaderMap,
    meta: AuditRequestMeta,
    Path((server_id, tool_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<SetMcpToolAnnotationOverridesRequest>,
) -> Result<Json<Envelope<McpToolPayload>>, AppError> {
    let actor = require_mcp_policy_admin(&state, &headers, Some(server_id)).await?;
    let overrides = request.annotation_overrides.to_overrides();
    let update = McpToolPolicyService::new(state.store.clone())
        .set_annotation_overrides(server_id, tool_id, &overrides)
        .await?;
    if let Some(previous) = &update.previous {
        state
            .audit()
            .record(
                &meta.user_context(&actor),
                tool_event(actions::MCP_TOOL_ANNOTATIONS_OVERRIDDEN, &update.current)
                    .before(McpToolAnnotationsView::overrides(
                        &previous.policy.annotation_overrides,
                    ))
                    .after(McpToolAnnotationsView::overrides(
                        &update.current.policy.annotation_overrides,
                    )),
            )
            .await;
    }
    Ok(Json(envelope(McpToolPayload {
        tool: map_tool(update.current),
    })))
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/mcp/tag-rules",
    params(McpTagRulesQuery),
    responses(
        (status = 200, body = Envelope<McpTagRulesPayload>),
        (status = 400, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
pub async fn list_mcp_tag_rules(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<McpTagRulesQuery>,
) -> Result<Json<Envelope<McpTagRulesPayload>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let subject_kind = query
        .subject_kind
        .as_deref()
        .map(parse_grant_subject_kind)
        .transpose()?;
    let items = state
        .store
        .list_mcp_tool_tag_rules(subject_kind, query.subject_id)
        .await?
        .into_iter()
        .map(map_tag_rule)
        .collect();
    Ok(Json(envelope(McpTagRulesPayload { items })))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/mcp/tag-rules",
    request_body = CreateMcpTagRuleRequest,
    responses(
        (status = 200, body = Envelope<McpTagRulePayload>),
        (status = 400, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
pub async fn create_mcp_tag_rule(
    State(state): State<AppState>,
    headers: HeaderMap,
    meta: AuditRequestMeta,
    Json(request): Json<CreateMcpTagRuleRequest>,
) -> Result<Json<Envelope<McpTagRulePayload>>, AppError> {
    let actor = require_mcp_policy_admin(&state, &headers, request.server_id).await?;
    let key = McpToolTagRuleKey {
        subject_kind: parse_grant_subject_kind(&request.subject_kind)?,
        subject_id: request.subject_id,
        mcp_server_id: request.server_id,
        tag: request.tag,
        effect: parse_effect(&request.effect)?,
    };
    let (rule, created) = McpToolPolicyService::new(state.store.clone())
        .create_rule(key)
        .await?;
    // Repeating an active rule changes no access, so only a new rule is recorded.
    if created {
        state
            .audit()
            .record(
                &meta.user_context(&actor),
                AuditEvent::new(actions::MCP_TAG_RULE_SET)
                    .target(rule.rule_id)
                    .label(rule.tag.clone())
                    .after(tag_rule_summary(&rule)),
            )
            .await;
    }
    Ok(Json(envelope(McpTagRulePayload {
        rule: map_tag_rule(rule),
    })))
}

#[utoipa::path(
    delete,
    path = "/api/v1/admin/mcp/tag-rules/{rule_id}",
    params(("rule_id" = Uuid, Path, description = "MCP tag rule identifier")),
    responses(
        (status = 200, body = Envelope<McpTagRulesPayload>),
        (status = 400, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
pub async fn revoke_mcp_tag_rule(
    State(state): State<AppState>,
    headers: HeaderMap,
    meta: AuditRequestMeta,
    Path(rule_id): Path<Uuid>,
) -> Result<Json<Envelope<McpTagRulesPayload>>, AppError> {
    // The rule's server decides who may revoke it, so look it up first. The
    // revoke itself matches only an active rule, so a race records nothing twice.
    let server_id = state
        .store
        .get_mcp_tool_tag_rule(rule_id)
        .await?
        .and_then(|rule| rule.mcp_server_id);
    let actor = require_mcp_policy_admin(&state, &headers, server_id).await?;
    let revoked = McpToolPolicyService::new(state.store.clone())
        .revoke_rule(rule_id)
        .await?;
    if let Some(rule) = revoked {
        state
            .audit()
            .record(
                &meta.user_context(&actor),
                AuditEvent::new(actions::MCP_TAG_RULE_REVOKED)
                    .target(rule.rule_id)
                    .label(rule.tag.clone())
                    .before(tag_rule_summary(&rule)),
            )
            .await;
    }
    Ok(Json(envelope(McpTagRulesPayload { items: Vec::new() })))
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/mcp/tag-rules/preview",
    params(McpTagRulePreviewQuery),
    responses(
        (status = 200, body = Envelope<McpTagRulePreviewPayload>),
        (status = 400, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
/// Lists the active tools a rule would select, before it is saved.
pub async fn preview_mcp_tag_rule(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<McpTagRulePreviewQuery>,
) -> Result<Json<Envelope<McpTagRulePreviewPayload>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let effect = parse_effect(&query.effect)?;
    let preview = McpToolPolicyService::new(state.store.clone())
        .preview_rule(query.server_id, &query.tag, effect)
        .await?;
    Ok(Json(envelope(McpTagRulePreviewPayload {
        tools: preview
            .matches
            .into_iter()
            .map(|item| map_tool(item.tool))
            .collect(),
        truncated: preview.truncated,
    })))
}
