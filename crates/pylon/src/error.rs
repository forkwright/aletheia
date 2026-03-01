//! API error types with Axum response integration.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use snafu::Snafu;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum ApiError {
    #[snafu(display("session not found: {id}"))]
    SessionNotFound { id: String },

    #[snafu(display("nous not found: {id}"))]
    NousNotFound { id: String },

    #[snafu(display("bad request: {message}"))]
    BadRequest { message: String },

    #[snafu(display("internal error: {message}"))]
    Internal { message: String },

    #[snafu(display("unauthorized"))]
    Unauthorized,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            Self::SessionNotFound { .. } => (StatusCode::NOT_FOUND, "session_not_found"),
            Self::NousNotFound { .. } => (StatusCode::NOT_FOUND, "nous_not_found"),
            Self::BadRequest { .. } => (StatusCode::BAD_REQUEST, "bad_request"),
            Self::Internal { .. } => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
        };

        let body = serde_json::json!({
            "error": {
                "code": code,
                "message": self.to_string(),
            }
        });

        (status, Json(body)).into_response()
    }
}

impl From<aletheia_mneme::error::Error> for ApiError {
    fn from(err: aletheia_mneme::error::Error) -> Self {
        Self::Internal {
            message: err.to_string(),
        }
    }
}

impl From<aletheia_hermeneus::error::Error> for ApiError {
    fn from(err: aletheia_hermeneus::error::Error) -> Self {
        Self::Internal {
            message: err.to_string(),
        }
    }
}

impl From<tokio::task::JoinError> for ApiError {
    fn from(err: tokio::task::JoinError) -> Self {
        Self::Internal {
            message: format!("task join failed: {err}"),
        }
    }
}
