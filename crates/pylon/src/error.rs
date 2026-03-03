//! API error types with Axum response integration.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use snafu::Snafu;

/// HTTP API error that maps directly to an Axum response with a JSON error body.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum ApiError {
    #[snafu(display("session not found: {id}"))]
    SessionNotFound {
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("nous not found: {id}"))]
    NousNotFound {
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("bad request: {message}"))]
    BadRequest {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("internal error: {message}"))]
    Internal {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("unauthorized"))]
    Unauthorized {
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            Self::SessionNotFound { .. } => (StatusCode::NOT_FOUND, "session_not_found"),
            Self::NousNotFound { .. } => (StatusCode::NOT_FOUND, "nous_not_found"),
            Self::BadRequest { .. } => (StatusCode::BAD_REQUEST, "bad_request"),
            Self::Internal { .. } => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
            Self::Unauthorized { .. } => (StatusCode::UNAUTHORIZED, "unauthorized"),
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
            location: snafu::Location::new(file!(), line!(), column!()),
        }
    }
}

impl From<aletheia_hermeneus::error::Error> for ApiError {
    fn from(err: aletheia_hermeneus::error::Error) -> Self {
        Self::Internal {
            message: err.to_string(),
            location: snafu::Location::new(file!(), line!(), column!()),
        }
    }
}

impl From<aletheia_nous::error::Error> for ApiError {
    fn from(err: aletheia_nous::error::Error) -> Self {
        Self::Internal {
            message: err.to_string(),
            location: snafu::Location::new(file!(), line!(), column!()),
        }
    }
}

impl From<tokio::task::JoinError> for ApiError {
    fn from(err: tokio::task::JoinError) -> Self {
        Self::Internal {
            message: format!("task join failed: {err}"),
            location: snafu::Location::new(file!(), line!(), column!()),
        }
    }
}
