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

        (
            status,
            Json(OpenAiErrorEnvelope::from_gateway_error(&self.0)),
        )
            .into_response()
    }
}
