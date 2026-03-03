use axum::{
    Json,
    response::{IntoResponse, Response},
};
use gateway_core::{GatewayError, OpenAiErrorEnvelope};

pub struct AppError(pub GatewayError);

impl From<GatewayError> for AppError {
    fn from(value: GatewayError) -> Self {
        Self(value)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = axum::http::StatusCode::from_u16(self.0.http_status_code())
            .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);

        (
            status,
            Json(OpenAiErrorEnvelope::from_gateway_error(&self.0)),
        )
            .into_response()
    }
}
