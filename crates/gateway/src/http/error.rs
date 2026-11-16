use axum::{
    Json,
    response::{IntoResponse, Response},
};
use gateway_core::{AuthError, GatewayError, OpenAiErrorEnvelope, StoreError};

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
        if let GatewayError::Provider(gateway_core::ProviderError::UpstreamHttp {
            request_id: Some(request_id),
            ..
        }) = &self.0
            && let Ok(value) = axum::http::HeaderValue::from_str(request_id)
        {
            response.headers_mut().insert("x-request-id", value);
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_core::ProviderError;

    #[test]
    fn upstream_request_id_is_preserved_without_changing_error_envelope() {
        let error = AppError(GatewayError::Provider(ProviderError::UpstreamHttp {
            status: 403,
            body: r#"{"detail":"region not permitted"}"#.into(),
            request_id: Some("req-fake".into()),
        }));
        let response = error.into_response();
        assert_eq!(response.status(), axum::http::StatusCode::FORBIDDEN);
        assert_eq!(response.headers()["x-request-id"], "req-fake");
        assert_eq!(response.headers()["content-type"], "application/json");
    }
}
