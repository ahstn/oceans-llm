//! Admin API for rate-limit policies.
//!
//! Platform admins manage database policies here. Config policies from
//! `rate_limits.policies` are listed read-only. Every change goes through
//! [`record_rate_limit_policy_change`], the hook for the admin audit log (#99).

use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use gateway_core::{
    AdminApiKeyRepository, GatewayError, IdentityRepository, McpRegistryRepository,
    ModelRepository, RateLimitMetric, RateLimitOperation, RateLimitPeriod, RateLimitPolicyRecord,
    RateLimitScopeKind, UserRecord,
};
use gateway_service::ConfiguredRateLimitPolicy;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use super::{
    admin_auth::require_platform_admin,
    admin_contract::{Envelope, envelope, format_timestamp},
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
    for policy in limiter.list_admin_policies().await? {
        policies.push(admin_policy_view(&state, &policy).await?);
    }
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
    record_rate_limit_policy_change(&actor, "create", None, Some(&created));
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
    let updated = limiter.update_admin_policy(policy).await?;
    record_rate_limit_policy_change(&actor, "update", Some(&before), Some(&updated));
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
    Path(policy_id): Path<String>,
) -> Result<Json<Envelope<RateLimitPolicyView>>, AppError> {
    let actor = require_platform_admin(&state, &headers).await?;
    let policy_id = parse_id(&policy_id, "policy_id")?;
    let limiter = state.service.rate_limiter();
    let before = limiter.get_admin_policy(policy_id).await?;
    let view = admin_policy_view(&state, &before).await?;
    limiter.delete_admin_policy(policy_id).await?;
    record_rate_limit_policy_change(&actor, "delete", Some(&before), None);
    Ok(Json(envelope(view)))
}

/// The admin audit hook for rate-limit policy changes. It emits a structured
/// event today; the admin audit log (#99) should persist the same fields.
fn record_rate_limit_policy_change(
    actor: &UserRecord,
    action: &'static str,
    before: Option<&RateLimitPolicyRecord>,
    after: Option<&RateLimitPolicyRecord>,
) {
    let policy_id = after
        .or(before)
        .map(|policy| policy.rate_limit_policy_id.to_string());
    tracing::info!(
        target: "admin_audit",
        resource = "rate_limit_policy",
        action,
        actor_user_id = %actor.user_id,
        policy_id = policy_id.as_deref(),
        before = before.map(|policy| format!("{policy:?}")).as_deref(),
        after = after.map(|policy| format!("{policy:?}")).as_deref(),
        "admin changed a rate limit policy"
    );
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

async fn admin_policy_view(
    state: &AppState,
    policy: &RateLimitPolicyRecord,
) -> Result<RateLimitPolicyView, AppError> {
    let scope_label = match (policy.scope_kind, policy.scope_id) {
        (RateLimitScopeKind::ApiKey, Some(id)) => {
            state.store.get_api_key_by_id(id).await?.map(|key| key.name)
        }
        (RateLimitScopeKind::User, Some(id)) => {
            state.store.get_user_by_id(id).await?.map(|user| user.email)
        }
        (RateLimitScopeKind::Team, Some(id)) => state
            .store
            .get_team_by_id(id)
            .await?
            .map(|team| team.team_name),
        _ => None,
    };
    let mcp_server_key = match policy.mcp_server_id {
        Some(id) => state
            .store
            .get_external_mcp_server(id)
            .await?
            .map(|server| server.server_key),
        None => None,
    };
    Ok(RateLimitPolicyView {
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
    })
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
