//! Admin API for rate-limit policies.
//!
//! Platform admins manage database policies here. Config policies from
//! `rate_limits.policies` are listed read-only. Every change is recorded in the
//! admin audit log.

use std::collections::HashMap;

use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use gateway_core::{
    AdminApiKeyRepository, AdminIdentityRepository, GatewayError, IdentityRepository,
    McpRegistryRepository, ModelRepository, RateLimitMetric, RateLimitOperation, RateLimitPeriod,
    RateLimitPolicyRecord, RateLimitScopeKind,
};
use gateway_service::{AuditAction, AuditEvent, ConfiguredRateLimitPolicy, admin_audit::actions};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use super::{
    admin_auth::require_platform_admin,
    admin_contract::{Envelope, envelope, format_timestamp},
    audit::AuditRequestMeta,
    error::AppError,
    state::AppState,
};

macro_rules! contract_enum {
    ($(#[$meta:meta])* $view:ident => $domain:ident { $($variant:ident),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
        #[serde(rename_all = "snake_case")]
        pub enum $view {
            $($variant),+
        }

        impl From<$domain> for $view {
            fn from(value: $domain) -> Self {
                match value {
                    $($domain::$variant => Self::$variant),+
                }
            }
        }

        impl From<$view> for $domain {
            fn from(value: $view) -> Self {
                match value {
                    $($view::$variant => Self::$variant),+
                }
            }
        }
    };
}

contract_enum!(
    /// `inference` covers chat, responses, messages, embeddings and decisions.
    RateLimitOperationView => RateLimitOperation { Inference, CountTokens, McpToolCall }
);
contract_enum!(
    /// Who shares a counter. Without `scope_id`, each principal of the kind
    /// gets its own counter.
    RateLimitScopeKindView => RateLimitScopeKind { Global, ApiKey, User, Team }
);
contract_enum!(
    /// `tokens` is only valid for `inference`.
    RateLimitMetricView => RateLimitMetric { Requests, Tokens }
);
contract_enum!(
    /// `minute` and `hour` slide; `day` and `month` are UTC calendar quotas.
    RateLimitPeriodView => RateLimitPeriod { Minute, Hour, Day, Month }
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RateLimitPolicySourceView {
    /// From `rate_limits.policies` in `gateway.yaml`; read-only here.
    Config,
    Admin,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RateLimitPolicyView {
    /// `None` for config policies.
    pub id: Option<String>,
    pub name: String,
    pub source: RateLimitPolicySourceView,
    pub operation: RateLimitOperationView,
    pub scope_kind: RateLimitScopeKindView,
    pub scope_id: Option<String>,
    /// The targeted user's email, team name or API key name.
    pub scope_label: Option<String>,
    pub model_key: Option<String>,
    pub mcp_server_id: Option<String>,
    pub mcp_server_key: Option<String>,
    pub metric: RateLimitMetricView,
    pub period: RateLimitPeriodView,
    pub limit: i64,
    pub enabled: bool,
    pub updated_at: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RateLimitPoliciesView {
    pub policies: Vec<RateLimitPolicyView>,
}

/// The whole policy; `PATCH` replaces every field.
#[derive(Debug, Deserialize, ToSchema)]
pub struct RateLimitPolicyRequest {
    pub name: String,
    pub operation: RateLimitOperationView,
    pub scope_kind: RateLimitScopeKindView,
    #[serde(default)]
    pub scope_id: Option<String>,
    #[serde(default)]
    pub model_key: Option<String>,
    #[serde(default)]
    pub mcp_server_id: Option<String>,
    pub metric: RateLimitMetricView,
    pub period: RateLimitPeriodView,
    pub limit: i64,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

const fn default_enabled() -> bool {
    true
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/rate-limits",
    responses((status = 200, body = Envelope<RateLimitPoliciesView>)),
    security(("session_cookie" = []))
)]
pub async fn list_rate_limit_policies(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Envelope<RateLimitPoliciesView>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let limiter = state.service.rate_limiter();
    let mut policies: Vec<_> = limiter
        .config_policies()
        .iter()
        .map(config_policy_view)
        .collect();
    let records = limiter.list_admin_policies().await?;
    // One query for every MCP server key, and one per principal kind that a
    // policy targets, instead of a lookup per row.
    let server_keys: HashMap<Uuid, String> = state
        .store
        .list_external_mcp_servers(true)
        .await?
        .into_iter()
        .map(|server| (server.mcp_server_id, server.server_key))
        .collect();
    let labels = scope_labels(&state, &records).await?;
    policies.extend(records.iter().map(|policy| {
        let label = policy
            .scope_id
            .and_then(|id| labels.get(&(policy.scope_kind, id)).cloned());
        let server_key = policy
            .mcp_server_id
            .and_then(|id| server_keys.get(&id).cloned());
        admin_policy_view_from(policy, label, server_key)
    }));
    Ok(Json(envelope(RateLimitPoliciesView { policies })))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/rate-limits",
    request_body = RateLimitPolicyRequest,
    responses((status = 200, body = Envelope<RateLimitPolicyView>)),
    security(("session_cookie" = []))
)]
pub async fn create_rate_limit_policy(
    State(state): State<AppState>,
    headers: HeaderMap,
    meta: AuditRequestMeta,
    Json(request): Json<RateLimitPolicyRequest>,
) -> Result<Json<Envelope<RateLimitPolicyView>>, AppError> {
    let actor = require_platform_admin(&state, &headers).await?;
    let now = OffsetDateTime::now_utc();
    let policy = policy_from_request(&state, Uuid::new_v4(), request, now, now).await?;
    let created = state
        .service
        .rate_limiter()
        .create_admin_policy(policy)
        .await?;
    state
        .audit()
        .record(
            &meta.user_context(&actor),
            policy_event(actions::RATE_LIMIT_POLICY_CREATED, &created)
                .after(policy_summary(&created)),
        )
        .await;
    Ok(Json(envelope(admin_policy_view(&state, &created).await?)))
}

#[utoipa::path(
    patch,
    path = "/api/v1/admin/rate-limits/{policy_id}",
    params(("policy_id" = String, Path, description = "Rate-limit policy id")),
    request_body = RateLimitPolicyRequest,
    responses((status = 200, body = Envelope<RateLimitPolicyView>)),
    security(("session_cookie" = []))
)]
pub async fn update_rate_limit_policy(
    State(state): State<AppState>,
    headers: HeaderMap,
    meta: AuditRequestMeta,
    Path(policy_id): Path<String>,
    Json(request): Json<RateLimitPolicyRequest>,
) -> Result<Json<Envelope<RateLimitPolicyView>>, AppError> {
    let actor = require_platform_admin(&state, &headers).await?;
    let policy_id = parse_id(&policy_id, "policy_id")?;
    let limiter = state.service.rate_limiter();
    let before = limiter.get_admin_policy(policy_id).await?;
    let policy = policy_from_request(
        &state,
        policy_id,
        request,
        before.created_at,
        OffsetDateTime::now_utc(),
    )
    .await?;
    let update = limiter.update_admin_policy(policy).await?;
    let updated = update.current;
    // The store compares the settings under the write lock and writes nothing when
    // they match, so saving a policy unchanged records nothing.
    if let Some(previous) = update.previous {
        state
            .audit()
            .record(
                &meta.user_context(&actor),
                policy_event(actions::RATE_LIMIT_POLICY_UPDATED, &updated)
                    .before(policy_summary(&previous))
                    .after(policy_summary(&updated)),
            )
            .await;
    }
    Ok(Json(envelope(admin_policy_view(&state, &updated).await?)))
}

#[utoipa::path(
    delete,
    path = "/api/v1/admin/rate-limits/{policy_id}",
    params(("policy_id" = String, Path, description = "Rate-limit policy id")),
    responses((status = 200, body = Envelope<RateLimitPolicyView>)),
    security(("session_cookie" = []))
)]
pub async fn delete_rate_limit_policy(
    State(state): State<AppState>,
    headers: HeaderMap,
    meta: AuditRequestMeta,
    Path(policy_id): Path<String>,
) -> Result<Json<Envelope<RateLimitPolicyView>>, AppError> {
    let actor = require_platform_admin(&state, &headers).await?;
    let policy_id = parse_id(&policy_id, "policy_id")?;
    // The delete returns the policy it removed, so the entry and the response
    // show what was actually deleted, even if a save landed just before.
    let deleted = state
        .service
        .rate_limiter()
        .delete_admin_policy(policy_id)
        .await?;
    state
        .audit()
        .record(
            &meta.user_context(&actor),
            policy_event(actions::RATE_LIMIT_POLICY_DELETED, &deleted)
                .before(policy_summary(&deleted)),
        )
        .await;
    Ok(Json(envelope(admin_policy_view(&state, &deleted).await?)))
}

fn policy_event(action: AuditAction, policy: &RateLimitPolicyRecord) -> AuditEvent {
    AuditEvent::new(action)
        .target(policy.rate_limit_policy_id)
        .label(policy.name.clone())
}

/// Audit summary of a policy: its settings, without timestamps or the display
/// labels the view resolves.
fn policy_summary(policy: &RateLimitPolicyRecord) -> Value {
    let mut summary =
        serde_json::to_value(admin_policy_view_from(policy, None, None)).unwrap_or(Value::Null);
    if let Some(object) = summary.as_object_mut() {
        for field in ["updated_at", "source", "scope_label", "mcp_server_key"] {
            object.remove(field);
        }
    }
    summary
}

async fn policy_from_request(
    state: &AppState,
    policy_id: Uuid,
    request: RateLimitPolicyRequest,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
) -> Result<RateLimitPolicyRecord, AppError> {
    let scope_kind = RateLimitScopeKind::from(request.scope_kind);
    let scope_id = non_empty(request.scope_id)
        .map(|id| parse_id(&id, "scope_id"))
        .transpose()?;
    if let Some(scope_id) = scope_id {
        let exists = match scope_kind {
            RateLimitScopeKind::Global => true,
            RateLimitScopeKind::ApiKey => state.store.get_api_key_by_id(scope_id).await?.is_some(),
            RateLimitScopeKind::User => state.store.get_user_by_id(scope_id).await?.is_some(),
            RateLimitScopeKind::Team => state.store.get_team_by_id(scope_id).await?.is_some(),
        };
        if !exists {
            return Err(invalid(format!(
                "scope_id does not name an existing {}",
                scope_kind.label()
            )));
        }
    }
    let model_key = non_empty(request.model_key);
    if let Some(model_key) = &model_key
        && state.store.get_model_by_key(model_key).await?.is_none()
    {
        return Err(invalid(format!("unknown model `{model_key}`")));
    }
    let mcp_server_id = non_empty(request.mcp_server_id)
        .map(|id| parse_id(&id, "mcp_server_id"))
        .transpose()?;
    if let Some(server_id) = mcp_server_id
        && state
            .store
            .get_external_mcp_server(server_id)
            .await?
            .is_none()
    {
        return Err(invalid("mcp_server_id does not name an MCP server"));
    }
    Ok(RateLimitPolicyRecord {
        rate_limit_policy_id: policy_id,
        name: request.name.trim().to_string(),
        operation: request.operation.into(),
        scope_kind,
        scope_id,
        model_key,
        mcp_server_id,
        metric: request.metric.into(),
        period: request.period.into(),
        limit: request.limit,
        enabled: request.enabled,
        created_at,
        updated_at,
    })
}

fn config_policy_view(policy: &ConfiguredRateLimitPolicy) -> RateLimitPolicyView {
    RateLimitPolicyView {
        id: None,
        name: policy.name.clone(),
        source: RateLimitPolicySourceView::Config,
        operation: policy.operation.into(),
        scope_kind: policy.scope_kind.into(),
        scope_id: None,
        scope_label: None,
        model_key: policy.model_key.clone(),
        mcp_server_id: None,
        mcp_server_key: policy.mcp_server_key.clone(),
        metric: policy.metric.into(),
        period: policy.period.into(),
        limit: policy.limit,
        enabled: true,
        updated_at: None,
    }
}

/// The targeted user's email, team name or API key name.
/// Labels for every principal the policies target, keyed by scope kind and
/// id. A principal that no longer exists has no entry.
async fn scope_labels(
    state: &AppState,
    records: &[RateLimitPolicyRecord],
) -> Result<HashMap<(RateLimitScopeKind, Uuid), String>, AppError> {
    let targets_kind = |kind| {
        records
            .iter()
            .any(|policy| policy.scope_kind == kind && policy.scope_id.is_some())
    };
    let mut labels = HashMap::new();
    if targets_kind(RateLimitScopeKind::ApiKey) {
        labels.extend(
            state
                .store
                .list_api_keys()
                .await?
                .into_iter()
                .map(|key| ((RateLimitScopeKind::ApiKey, key.id), key.name)),
        );
    }
    if targets_kind(RateLimitScopeKind::User) {
        labels.extend(
            state
                .store
                .list_identity_users()
                .await?
                .into_iter()
                .map(|identity| {
                    (
                        (RateLimitScopeKind::User, identity.user.user_id),
                        identity.user.email,
                    )
                }),
        );
    }
    if targets_kind(RateLimitScopeKind::Team) {
        labels.extend(
            state
                .store
                .list_teams()
                .await?
                .into_iter()
                .map(|team| ((RateLimitScopeKind::Team, team.team_id), team.team_name)),
        );
    }
    Ok(labels)
}

async fn scope_label(
    state: &AppState,
    scope_kind: RateLimitScopeKind,
    scope_id: Uuid,
) -> Result<Option<String>, AppError> {
    Ok(match scope_kind {
        RateLimitScopeKind::ApiKey => state
            .store
            .get_api_key_by_id(scope_id)
            .await?
            .map(|key| key.name),
        RateLimitScopeKind::User => state
            .store
            .get_user_by_id(scope_id)
            .await?
            .map(|user| user.email),
        RateLimitScopeKind::Team => state
            .store
            .get_team_by_id(scope_id)
            .await?
            .map(|team| team.team_name),
        RateLimitScopeKind::Global => None,
    })
}

/// The view for one policy, for the create, update and delete responses.
async fn admin_policy_view(
    state: &AppState,
    policy: &RateLimitPolicyRecord,
) -> Result<RateLimitPolicyView, AppError> {
    let label = match policy.scope_id {
        Some(id) => scope_label(state, policy.scope_kind, id).await?,
        None => None,
    };
    let server_key = match policy.mcp_server_id {
        Some(id) => state
            .store
            .get_external_mcp_server(id)
            .await?
            .map(|server| server.server_key),
        None => None,
    };
    Ok(admin_policy_view_from(policy, label, server_key))
}

fn admin_policy_view_from(
    policy: &RateLimitPolicyRecord,
    scope_label: Option<String>,
    mcp_server_key: Option<String>,
) -> RateLimitPolicyView {
    RateLimitPolicyView {
        id: Some(policy.rate_limit_policy_id.to_string()),
        name: policy.name.clone(),
        source: RateLimitPolicySourceView::Admin,
        operation: policy.operation.into(),
        scope_kind: policy.scope_kind.into(),
        scope_id: policy.scope_id.map(|id| id.to_string()),
        scope_label,
        model_key: policy.model_key.clone(),
        mcp_server_id: policy.mcp_server_id.map(|id| id.to_string()),
        mcp_server_key,
        metric: policy.metric.into(),
        period: policy.period.into(),
        limit: policy.limit,
        enabled: policy.enabled,
        updated_at: Some(format_timestamp(policy.updated_at)),
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_id(value: &str, field: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(value.trim()).map_err(|_| invalid(format!("{field} must be a UUID")))
}

fn invalid(message: impl Into<String>) -> AppError {
    AppError(GatewayError::InvalidRequest(message.into()))
}

#[cfg(test)]
mod handler_tests;
