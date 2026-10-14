use axum::{
    Json,
    http::{HeaderValue, header::RETRY_AFTER},
    response::{IntoResponse, Response},
};
use gateway_core::{AuthError, GatewayError, OpenAiErrorEnvelope, ProviderError, StoreError};

pub struct AppError(pub GatewayError);

impl From<GatewayError> for AppError {
    fn from(value: GatewayError) -> Self {
        Self(value)
    }
}

impl From<StoreError> for AppError {
    fn from(value: StoreError) -> Self {
        Self(value.into())
    }
}

impl From<AuthError> for AppError {
    fn from(value: AuthError) -> Self {
        Self(value.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        tracing::Span::current().record("gateway.error.type", self.0.error_type());
        let status = axum::http::StatusCode::from_u16(self.0.http_status_code())
            .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);

        let mut response = (
            status,
            Json(OpenAiErrorEnvelope::from_gateway_error(&self.0)),
        )
            .into_response();
        if let Some(retry_after) = upstream_retry_after(&self.0) {
            response.headers_mut().insert(RETRY_AFTER, retry_after);
        }
        response
    }
}

/// The upstream `Retry-After` value to pass through on a `429` or `503` provider error.
pub(crate) fn upstream_retry_after(error: &GatewayError) -> Option<HeaderValue> {
    let GatewayError::Provider(error) = error else {
        return None;
    };
    let error = match error {
        ProviderError::PartialUsage { source, .. } => source.as_ref(),
        error => error,
    };
    match error {
        ProviderError::UpstreamHttp {
            status: 429 | 503,
            retry_after: Some(retry_after),
            ..
        } => HeaderValue::from_str(retry_after).ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upstream(status: u16, retry_after: Option<&str>) -> ProviderError {
        ProviderError::UpstreamHttp {
            status,
            body: String::new(),
            retry_after: retry_after.map(str::to_string),
        }
    }

    #[test]
    fn retry_after_comes_only_from_upstream_429_and_503() {
        let header = |error: ProviderError| upstream_retry_after(&GatewayError::Provider(error));
        assert_eq!(header(upstream(429, Some("5"))).unwrap(), "5");
        assert_eq!(header(upstream(503, Some("5"))).unwrap(), "5");
        assert_eq!(
            header(ProviderError::PartialUsage {
                source: Box::new(upstream(429, Some("5"))),
                provider_usage: None,
            })
            .unwrap(),
            "5"
        );
        assert!(header(upstream(429, None)).is_none());
        assert!(header(upstream(500, Some("5"))).is_none());
        assert!(header(ProviderError::Timeout).is_none());
        assert!(upstream_retry_after(&GatewayError::InvalidRequest("bad".to_string())).is_none());
    }
}
