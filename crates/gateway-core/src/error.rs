use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("authorization header is missing")]
    MissingAuthorizationHeader,
    #[error("authorization header is invalid")]
    InvalidAuthorizationHeader,
    #[error("bearer token is missing")]
    MissingBearerToken,
    #[error("gateway api key format is invalid")]
    InvalidApiKeyFormat,
    #[error("api key was not found")]
    ApiKeyNotFound,
    #[error("api key is revoked")]
    ApiKeyRevoked,
    #[error("api key secret hash did not match")]
    ApiKeySecretMismatch,
    #[error("api key is not authorized for model `{0}`")]
    ModelNotGranted(String),
    #[error("api key owner metadata is invalid")]
    ApiKeyOwnerInvalid,
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
        "budget exceeded for user `{user_id}`: projected {projected_cost_usd:.6} exceeds limit {limit_usd:.6}"
    )]
    BudgetExceeded {
        user_id: String,
        projected_cost_usd: f64,
        limit_usd: f64,
    },
    #[error("identity constraint violation: {0}")]
    IdentityConstraint(String),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("feature not implemented: {0}")]
    NotImplemented(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl GatewayError {
    #[must_use]
    pub fn http_status_code(&self) -> u16 {
        match self {
            Self::Auth(AuthError::MissingAuthorizationHeader)
            | Self::Auth(AuthError::InvalidAuthorizationHeader)
            | Self::Auth(AuthError::MissingBearerToken)
            | Self::Auth(AuthError::ApiKeyNotFound)
            | Self::Auth(AuthError::ApiKeyRevoked)
            | Self::Auth(AuthError::ApiKeySecretMismatch)
            | Self::Auth(AuthError::InvalidApiKeyFormat) => 401,
            Self::Auth(AuthError::ApiKeyOwnerInvalid) => 500,
            Self::Auth(AuthError::ModelNotGranted(_)) => 403,
            Self::BudgetExceeded { .. } => 429,
            Self::IdentityConstraint(_) => 400,
            Self::InvalidRequest(_) => 400,
            Self::Route(RouteError::ModelNotFound(_)) => 404,
            Self::NotImplemented(_) | Self::Provider(ProviderError::NotImplemented(_)) => 501,
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
            Self::Store(_) => "store_error",
            Self::Provider(_) => "upstream_error",
            Self::NotImplemented(_) => "not_implemented_error",
            Self::Internal(_) => "internal_error",
        }
    }

    #[must_use]
    pub fn error_code(&self) -> &'static str {
        match self {
            Self::Auth(AuthError::MissingAuthorizationHeader) => "missing_authorization_header",
            Self::Auth(AuthError::InvalidAuthorizationHeader) => "invalid_authorization_header",
            Self::Auth(AuthError::MissingBearerToken) => "missing_bearer_token",
            Self::Auth(AuthError::InvalidApiKeyFormat) => "invalid_api_key_format",
            Self::Auth(AuthError::ApiKeyNotFound) => "api_key_not_found",
            Self::Auth(AuthError::ApiKeyRevoked) => "api_key_revoked",
            Self::Auth(AuthError::ApiKeySecretMismatch) => "api_key_secret_mismatch",
            Self::Auth(AuthError::ModelNotGranted(_)) => "model_not_granted",
            Self::Auth(AuthError::ApiKeyOwnerInvalid) => "api_key_owner_invalid",
            Self::Auth(AuthError::HashVerification(_)) => "api_key_hash_verification_failed",
            Self::BudgetExceeded { .. } => "budget_exceeded",
            Self::IdentityConstraint(_) => "identity_constraint_violation",
            Self::Store(_) => "store_error",
            Self::Route(RouteError::ModelNotFound(_)) => "model_not_found",
            Self::Route(RouteError::NoRoutesAvailable(_)) => "no_routes_available",
            Self::Route(RouteError::Policy(_)) => "routing_policy_error",
            Self::Provider(ProviderError::Timeout) => "upstream_timeout",
            Self::Provider(ProviderError::Transport(_)) => "upstream_transport",
            Self::Provider(ProviderError::UpstreamHttp { .. }) => "upstream_http_error",
            Self::Provider(ProviderError::NotImplemented(_)) => "provider_not_implemented",
            Self::InvalidRequest(_) => "invalid_request",
            Self::NotImplemented(_) => "not_implemented",
            Self::Internal(_) => "internal_error",
        }
    }
}
