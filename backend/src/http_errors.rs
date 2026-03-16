use axum::{http::StatusCode, Json};

use crate::ErrorResponse;

pub fn bad_request(message: &str) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse {
            error: message.to_string(),
        }),
    )
}

pub fn payload_too_large(message: &str) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::PAYLOAD_TOO_LARGE,
        Json(ErrorResponse {
            error: message.to_string(),
        }),
    )
}

pub fn internal_error<E: std::fmt::Debug>(error: E) -> (StatusCode, Json<ErrorResponse>) {
    tracing::error!(error = ?error, "Internal server error");
    let message = if cfg!(debug_assertions) {
        format!("Internal Server Error: {:?}", error)
    } else {
        "Internal Server Error".to_string()
    };
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse { error: message }),
    )
}
