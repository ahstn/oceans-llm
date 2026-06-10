use thiserror::Error;

use crate::domain::Money4;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("authenticated session is required")]
    SessionRequired,
    #[error("credentials are invalid")]
    InvalidCredentials,
    #[error("authorization header is missing")]
    MissingAuthorizationHeader,
    #[error("authorization header is invalid")]
    InvalidAuthorizationHeader,
    #[error("bearer token is missing")]
    MissingBearerToken,
    #[error("gateway api key format is invalid")]
    InvalidApiKeyFormat,
    #[error("authorization and x-oceans-api-key headers do not match")]
    ConflictingApiKeyHeaders,
    #[error("api key was not found")]
    ApiKeyNotFound,
    #[error("api key is revoked")]
    ApiKeyRevoked,
    #[error("api key secret hash did not match")]
    ApiKeySecretMismatch,
    #[error("api key is not authorized for model `{0}`")]
    ModelNotGranted(String),
    #[error("authenticated subject lacks required privileges")]
    InsufficientPrivileges,
    #[error("api key owner metadata is invalid")]
    ApiKeyOwnerInvalid,
    #[error("service-account api key requires an active service-account budget")]
    ServiceAccountBudgetRequired,
    #[error("api key hash verification failed: {0}")]
    HashVerification(String),
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("store query failed: {0}")]
    Query(String),
    #[error("store serialization failed: {0}")]
    Serialization(String),
    #[error("store not found: {0}")]
    NotFound(String),
    #[error("store conflict: {0}")]
    Conflict(String),
    #[error("store unavailable: {0}")]
    Unavailable(String),
    #[error("store unexpected error: {0}")]
    Unexpected(String),
}

#[derive(Debug, Error)]
pub enum RouteError {
    #[error("requested model `{0}` was not found")]
    ModelNotFound(String),
    #[error("no routes are available for model `{0}`")]
    NoRoutesAvailable(String),
    #[error("routing policy could not evaluate routes: {0}")]
    Policy(String),
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("invalid request for provider: {0}")]
    InvalidRequest(String),
    #[error("upstream provider timed out")]
    Timeout,
    #[error("upstream provider transport failure: {0}")]
    Transport(String),
    #[error("upstream provider returned {status}: {body}")]
    UpstreamHttp { status: u16, body: String },
    #[error("provider is not implemented: {0}")]
    NotImplemented(String),
}

impl ProviderError {
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Timeout
                | Self::Transport(_)
                | Self::UpstreamHttp {
                    status: 408 | 429 | 500..=599,
                    ..
                }
        )
    }
}

#[derive(Debug, Error)]
pub enum GatewayError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Route(#[from] RouteError),
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error(
        "budget exceeded for ownership scope `{ownership_scope}`: projected {projected_cost_usd} exceeds limit {limit_usd}"
    )]
    BudgetExceeded {
        ownership_scope: String,
        projected_cost_usd: Money4,
        limit_usd: Money4,
    },
    #[error("identity constraint violation: {0}")]
    IdentityConstraint(String),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("request body exceeds {limit_bytes} bytes")]
    PayloadTooLarge { limit_bytes: usize },
    #[error("feature not implemented: {0}")]
    NotImplemented(String),
    #[error("MCP upstream auth requires user-scoped credentials for server `{server_key}`")]
    McpUpstreamAuthRequired { server_key: String },
    #[error("MCP credential is required for server `{server_key}`")]
    McpCredentialRequired { server_key: String },
    #[error("MCP credential for server `{server_key}` is expired")]
    McpCredentialExpired { server_key: String },
    #[error("internal error: {0}")]
    Internal(String),
}

impl GatewayError {
    #[must_use]
    pub fn http_status_code(&self) -> u16 {
        match self {
            Self::Auth(AuthError::SessionRequired)
            | Self::Auth(AuthError::InvalidCredentials)
            | Self::Auth(AuthError::MissingAuthorizationHeader)
            | Self::Auth(AuthError::InvalidAuthorizationHeader)
            | Self::Auth(AuthError::MissingBearerToken)
            | Self::Auth(AuthError::ApiKeyNotFound)
            | Self::Auth(AuthError::ApiKeyRevoked)
            | Self::Auth(AuthError::ApiKeySecretMismatch)
            | Self::Auth(AuthError::InvalidApiKeyFormat)
            | Self::Auth(AuthError::ConflictingApiKeyHeaders) => 401,
            Self::Auth(AuthError::ApiKeyOwnerInvalid) => 500,
            Self::Auth(AuthError::ModelNotGranted(_))
            | Self::Auth(AuthError::ServiceAccountBudgetRequired)
            | Self::Auth(AuthError::InsufficientPrivileges) => 403,
            Self::BudgetExceeded { .. } => 429,
            Self::IdentityConstraint(_) => 400,
            Self::InvalidRequest(_) => 400,
            Self::PayloadTooLarge { .. } => 413,
            Self::Store(StoreError::NotFound(_)) => 404,
            Self::Store(StoreError::Conflict(_)) => 409,
            Self::Route(RouteError::ModelNotFound(_)) => 404,
            Self::NotImplemented(_) | Self::Provider(ProviderError::NotImplemented(_)) => 501,
            Self::McpUpstreamAuthRequired { .. }
            | Self::McpCredentialRequired { .. }
            | Self::McpCredentialExpired { .. } => 403,
            Self::Provider(ProviderError::InvalidRequest(_)) => 400,
            Self::Provider(ProviderError::UpstreamHttp { status, .. }) => *status,
            Self::Provider(ProviderError::Timeout) => 504,
            Self::Provider(ProviderError::Transport(_)) => 502,
            Self::Route(RouteError::NoRoutesAvailable(_)) => 503,
            Self::Store(StoreError::Unavailable(_)) => 503,
            Self::Auth(AuthError::HashVerification(_))
            | Self::Store(_)
            | Self::Route(RouteError::Policy(_))
            | Self::Internal(_) => 500,
        }
    }

    #[must_use]
    pub fn error_type(&self) -> &'static str {
        match self {
            Self::Auth(_) => "authentication_error",
            Self::BudgetExceeded { .. } => "budget_error",
            Self::IdentityConstraint(_) => "identity_error",
            Self::InvalidRequest(_) => "invalid_request_error",
            Self::Route(RouteError::ModelNotFound(_)) => "not_found_error",
            Self::Route(_) => "routing_error",
            Self::Store(StoreError::NotFound(_)) => "not_found_error",
            Self::Store(StoreError::Conflict(_)) => "conflict_error",
            Self::Store(_) => "store_error",
            Self::Provider(ProviderError::InvalidRequest(_)) => "invalid_request_error",
            Self::PayloadTooLarge { .. } => "invalid_request_error",
            Self::Provider(_) => "upstream_error",
            Self::NotImplemented(_) => "not_implemented_error",
            Self::McpUpstreamAuthRequired { .. }
            | Self::McpCredentialRequired { .. }
            | Self::McpCredentialExpired { .. } => "authentication_error",
            Self::Internal(_) => "internal_error",
        }
    }

    #[must_use]
    pub fn error_code(&self) -> &'static str {
        match self {
            Self::Auth(AuthError::SessionRequired) => "session_required",
            Self::Auth(AuthError::InvalidCredentials) => "invalid_credentials",
            Self::Auth(AuthError::MissingAuthorizationHeader) => "missing_authorization_header",
            Self::Auth(AuthError::InvalidAuthorizationHeader) => "invalid_authorization_header",
            Self::Auth(AuthError::MissingBearerToken) => "missing_bearer_token",
            Self::Auth(AuthError::InvalidApiKeyFormat) => "invalid_api_key_format",
            Self::Auth(AuthError::ConflictingApiKeyHeaders) => "conflicting_api_key_headers",
            Self::Auth(AuthError::ApiKeyNotFound) => "api_key_not_found",
            Self::Auth(AuthError::ApiKeyRevoked) => "api_key_revoked",
            Self::Auth(AuthError::ApiKeySecretMismatch) => "api_key_secret_mismatch",
            Self::Auth(AuthError::ModelNotGranted(_)) => "model_not_granted",
            Self::Auth(AuthError::InsufficientPrivileges) => "insufficient_privileges",
            Self::Auth(AuthError::ApiKeyOwnerInvalid) => "api_key_owner_invalid",
            Self::Auth(AuthError::ServiceAccountBudgetRequired) => {
                "service_account_budget_required"
            }
            Self::Auth(AuthError::HashVerification(_)) => "api_key_hash_verification_failed",
            Self::BudgetExceeded { .. } => "budget_exceeded",
            Self::IdentityConstraint(_) => "identity_constraint_violation",
            Self::Store(StoreError::NotFound(_)) => "not_found",
            Self::Store(StoreError::Conflict(_)) => "conflict",
            Self::Store(_) => "store_error",
            Self::Route(RouteError::ModelNotFound(_)) => "model_not_found",
            Self::Route(RouteError::NoRoutesAvailable(_)) => "no_routes_available",
            Self::Route(RouteError::Policy(_)) => "routing_policy_error",
            Self::Provider(ProviderError::Timeout) => "upstream_timeout",
            Self::Provider(ProviderError::Transport(_)) => "upstream_transport",
            Self::Provider(ProviderError::UpstreamHttp { .. }) => "upstream_http_error",
            Self::Provider(ProviderError::NotImplemented(_)) => "provider_not_implemented",
            Self::Provider(ProviderError::InvalidRequest(_)) => "invalid_request",
            Self::InvalidRequest(_) => "invalid_request",
            Self::PayloadTooLarge { .. } => "request_body_too_large",
            Self::NotImplemented(_) => "not_implemented",
            Self::McpUpstreamAuthRequired { .. } => "mcp_upstream_auth_required",
            Self::McpCredentialRequired { .. } => "credential_required",
            Self::McpCredentialExpired { .. } => "credential_expired",
            Self::Internal(_) => "internal_error",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AuthError, GatewayError};

    #[test]
    fn service_account_budget_required_is_auth_forbidden() {
        let error = GatewayError::Auth(AuthError::ServiceAccountBudgetRequired);

        assert_eq!(error.http_status_code(), 403);
        assert_eq!(error.error_type(), "authentication_error");
        assert_eq!(error.error_code(), "service_account_budget_required");
    }
}
