//! Request-time policy shared by the inference and MCP routes: client-IP
//! resolution and rate limits.
//!
//! The middleware resolves the client IP and gives the handler a
//! [`RequestPolicyContext`]. Handlers run the rate-limit check, because only
//! they know the principal, model and MCP server, and record the tightest
//! policy in the context. The middleware then writes the rate-limit headers on
//! whatever response the handler produced, before any streamed body is sent.
//!
//! IP allow and deny lists (#450) plug in here: a gateway-wide check in
//! [`request_policy`], and per-server checks next to the MCP rate-limit check,
//! both using [`RequestPolicyContext::client_ip`].

use std::{
    convert::Infallible,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
};

use axum::{
    extract::{ConnectInfo, FromRequestParts, Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, header::RETRY_AFTER, request::Parts},
    middleware::Next,
    response::Response,
};
use gateway_core::GatewayError;
use gateway_service::{
    RateLimitDecision, RateLimitStatus, RateLimitSubject, rate_limits::RateLimitMemberTeam,
};

use super::{client_ip::resolve_client_ip, state::AppState};

pub const X_RATELIMIT_LIMIT: HeaderName = HeaderName::from_static("x-ratelimit-limit");
pub const X_RATELIMIT_REMAINING: HeaderName = HeaderName::from_static("x-ratelimit-remaining");
pub const X_RATELIMIT_RESET: HeaderName = HeaderName::from_static("x-ratelimit-reset");

/// Per-request policy state, created by [`request_policy`].
#[derive(Debug, Clone, Default)]
pub struct RequestPolicyContext {
    pub client_ip: Option<IpAddr>,
    rate_limit: Arc<Mutex<Option<RateLimitStatus>>>,
}

impl RequestPolicyContext {
    fn record(&self, status: &RateLimitStatus) {
        *self
            .rate_limit
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = Some(status.clone());
    }

    fn take(&self) -> Option<RateLimitStatus> {
        self.rate_limit
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take()
    }
}

/// Handlers outside the middleware (in-process tests) get a detached context
/// whose headers are simply not written.
impl<S: Send + Sync> FromRequestParts<S> for RequestPolicyContext {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(parts.extensions.get::<Self>().cloned().unwrap_or_default())
    }
}

pub async fn request_policy(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(address)| address.ip());
    let context = RequestPolicyContext {
        client_ip: resolve_client_ip(peer, request.headers(), &state.trusted_proxies),
        rate_limit: Arc::default(),
    };
    request.extensions_mut().insert(context.clone());

    let mut response = next.run(request).await;
    if let Some(status) = context.take() {
        apply_rate_limit_headers(response.headers_mut(), &status);
    }
    response
}

fn apply_rate_limit_headers(headers: &mut HeaderMap, status: &RateLimitStatus) {
    headers.insert(X_RATELIMIT_LIMIT, HeaderValue::from(status.limit));
    headers.insert(X_RATELIMIT_REMAINING, HeaderValue::from(status.remaining));
    headers.insert(
        X_RATELIMIT_RESET,
        HeaderValue::from(status.reset_after_seconds),
    );
    if let Some(retry_after) = status.retry_after_seconds {
        headers.insert(RETRY_AFTER, HeaderValue::from(retry_after));
    }
}

/// Checks the matching rate-limit policies. A rejection returns the
/// [`RateLimitStatus`] so MCP callers can shape a JSON-RPC error; inference
/// callers use [`enforce_rate_limits`].
pub async fn check_rate_limits(
    state: &AppState,
    context: &RequestPolicyContext,
    subject: &RateLimitSubject<'_>,
) -> Result<Option<RateLimitStatus>, GatewayError> {
    Ok(check_rate_limits_with_team(state, context, subject)
        .await?
        .0)
}

/// [`check_rate_limits`], also returning the user's team as the check
/// resolved it, for the request's token charge.
async fn check_rate_limits_with_team(
    state: &AppState,
    context: &RequestPolicyContext,
    subject: &RateLimitSubject<'_>,
) -> Result<(Option<RateLimitStatus>, RateLimitMemberTeam), GatewayError> {
    let operation = subject.operation.as_str();
    let (decision, team) = match state.service.rate_limiter().check_with_team(subject).await {
        Ok(checked) => checked,
        Err(error) => {
            state
                .metrics
                .record_rate_limit_decision(operation, "store_error");
            return Err(error);
        }
    };
    if decision != RateLimitDecision::Unlimited {
        state
            .metrics
            .record_rate_limit_decision(operation, decision.outcome());
    }
    if let Some(status) = decision.status() {
        context.record(status);
    }
    let rejected = match decision {
        RateLimitDecision::Rejected(status) => {
            tracing::info!(
                client_ip = ?context.client_ip,
                operation,
                policy = %status.policy_name,
                "request rejected by rate limit"
            );
            Some(status)
        }
        _ => None,
    };
    Ok((rejected, team))
}

/// [`check_rate_limits`] as a `429 rate_limited` error. Returns the user's
/// team as the check resolved it, which the request's token charge must use
/// (see [`RateLimitSubject::with_member_team`]).
pub async fn enforce_rate_limits(
    state: &AppState,
    context: &RequestPolicyContext,
    subject: &RateLimitSubject<'_>,
) -> Result<RateLimitMemberTeam, GatewayError> {
    match check_rate_limits_with_team(state, context, subject).await? {
        (Some(rejected), _) => Err(rejected.to_error()),
        (None, team) => Ok(team),
    }
}
