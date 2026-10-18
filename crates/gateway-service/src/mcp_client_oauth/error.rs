use gateway_core::GatewayError;

/// An OAuth error response (RFC 6749 section 5.2, RFC 7591 section 3.2.2,
/// RFC 8707 section 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OauthError {
    pub error: &'static str,
    pub description: String,
    /// Set when a request limit refused the request: the response is `429`
    /// with this `Retry-After`.
    pub retry_after_seconds: Option<u64>,
}

impl OauthError {
    fn new(error: &'static str, description: impl Into<String>) -> Self {
        Self {
            error,
            description: description.into(),
            retry_after_seconds: None,
        }
    }

    /// A request over a per-source or gateway-wide limit.
    pub fn rate_limited(description: impl Into<String>, retry_after_seconds: u64) -> Self {
        Self {
            retry_after_seconds: Some(retry_after_seconds.max(1)),
            ..Self::temporarily_unavailable(description)
        }
    }

    pub fn invalid_request(description: impl Into<String>) -> Self {
        Self::new("invalid_request", description)
    }

    pub fn invalid_client(description: impl Into<String>) -> Self {
        Self::new("invalid_client", description)
    }

    pub fn invalid_grant(description: impl Into<String>) -> Self {
        Self::new("invalid_grant", description)
    }

    pub fn unauthorized_client(description: impl Into<String>) -> Self {
        Self::new("unauthorized_client", description)
    }

    pub fn unsupported_grant_type(description: impl Into<String>) -> Self {
        Self::new("unsupported_grant_type", description)
    }

    pub fn unsupported_response_type(description: impl Into<String>) -> Self {
        Self::new("unsupported_response_type", description)
    }

    pub fn invalid_scope(description: impl Into<String>) -> Self {
        Self::new("invalid_scope", description)
    }

    pub fn invalid_target(description: impl Into<String>) -> Self {
        Self::new("invalid_target", description)
    }

    pub fn access_denied(description: impl Into<String>) -> Self {
        Self::new("access_denied", description)
    }

    pub fn invalid_redirect_uri(description: impl Into<String>) -> Self {
        Self::new("invalid_redirect_uri", description)
    }

    pub fn invalid_client_metadata(description: impl Into<String>) -> Self {
        Self::new("invalid_client_metadata", description)
    }

    pub fn temporarily_unavailable(description: impl Into<String>) -> Self {
        Self::new("temporarily_unavailable", description)
    }

    pub fn server_error(description: impl Into<String>) -> Self {
        Self::new("server_error", description)
    }

    /// HTTP status for a token-endpoint response.
    #[must_use]
    pub fn status_code(&self) -> u16 {
        if self.retry_after_seconds.is_some() {
            return 429;
        }
        match self.error {
            "invalid_client" => 401,
            "server_error" => 500,
            "temporarily_unavailable" => 503,
            _ => 400,
        }
    }
}

impl From<GatewayError> for OauthError {
    fn from(error: GatewayError) -> Self {
        tracing::warn!(error = %error, "MCP client OAuth request failed");
        Self::server_error("the authorization server could not complete the request")
    }
}

impl From<gateway_core::StoreError> for OauthError {
    fn from(error: gateway_core::StoreError) -> Self {
        GatewayError::from(error).into()
    }
}
