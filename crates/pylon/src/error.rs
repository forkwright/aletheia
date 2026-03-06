//! API error types with Axum response integration.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use snafu::Snafu;
use utoipa::ToSchema;

/// Consistent error response envelope.
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorResponse {
    pub error: ErrorBody,
}

/// Error body returned in all error responses.
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

/// HTTP API errors, each mapped to an appropriate status code via [`IntoResponse`].
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum ApiError {
    /// Requested session does not exist (404).
    #[snafu(display("session not found: {id}"))]
    SessionNotFound {
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Requested nous agent does not exist (404).
    #[snafu(display("nous not found: {id}"))]
    NousNotFound {
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Client sent an invalid request (400).
    #[snafu(display("bad request: {message}"))]
    BadRequest {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Unrecoverable server-side failure (500).
    #[snafu(display("internal error: {message}"))]
    Internal {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Missing or invalid authentication credentials (401).
    #[snafu(display("unauthorized"))]
    Unauthorized {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("not found: {path}"))]
    NotFound {
        path: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("rate limited, retry after {retry_after_ms}ms"))]
    RateLimited {
        retry_after_ms: u64,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("forbidden: {message}"))]
    Forbidden {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("service unavailable: {message}"))]
    ServiceUnavailable {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Config validation failed (422).
    #[snafu(display("validation failed"))]
    ValidationFailed {
        errors: Vec<String>,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, details) = match &self {
            Self::SessionNotFound { .. } => (StatusCode::NOT_FOUND, "session_not_found", None),
            Self::NousNotFound { .. } => (StatusCode::NOT_FOUND, "nous_not_found", None),
            Self::BadRequest { .. } => (StatusCode::BAD_REQUEST, "bad_request", None),
            Self::Internal { .. } => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error", None),
            Self::Unauthorized { .. } => (StatusCode::UNAUTHORIZED, "unauthorized", None),
            Self::NotFound { .. } => (StatusCode::NOT_FOUND, "not_found", None),
            Self::RateLimited { retry_after_ms, .. } => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                Some(serde_json::json!({ "retry_after_ms": retry_after_ms })),
            ),
            Self::Forbidden { .. } => (StatusCode::FORBIDDEN, "forbidden", None),
            Self::ServiceUnavailable { .. } => {
                (StatusCode::SERVICE_UNAVAILABLE, "service_unavailable", None)
            }
            Self::ValidationFailed { errors, .. } => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation_failed",
                Some(serde_json::json!({ "errors": errors })),
            ),
        };

        let body = ErrorResponse {
            error: ErrorBody {
                code: code.to_owned(),
                message: self.to_string(),
                details,
            },
        };

        (status, Json(body)).into_response()
    }
}

impl From<aletheia_mneme::error::Error> for ApiError {
    #[track_caller]
    fn from(err: aletheia_mneme::error::Error) -> Self {
        Self::Internal {
            message: err.to_string(),
            location: snafu::Location::default(),
        }
    }
}

impl From<aletheia_hermeneus::error::Error> for ApiError {
    #[track_caller]
    fn from(err: aletheia_hermeneus::error::Error) -> Self {
        use aletheia_hermeneus::error::Error;
        match err {
            Error::RateLimited { retry_after_ms, .. } => Self::RateLimited {
                retry_after_ms,
                location: snafu::Location::default(),
            },
            Error::AuthFailed { message, .. } => Self::ServiceUnavailable {
                message: format!("provider auth failed: {message}"),
                location: snafu::Location::default(),
            },
            Error::ApiError { status: 429, .. } => Self::RateLimited {
                retry_after_ms: 0,
                location: snafu::Location::default(),
            },
            Error::ApiError {
                status: 503,
                message,
                ..
            } => Self::ServiceUnavailable {
                message,
                location: snafu::Location::default(),
            },
            _ => Self::Internal {
                message: err.to_string(),
                location: snafu::Location::default(),
            },
        }
    }
}

impl From<aletheia_nous::error::Error> for ApiError {
    #[track_caller]
    fn from(err: aletheia_nous::error::Error) -> Self {
        use aletheia_nous::error::Error;
        match err {
            Error::NousNotFound { nous_id, .. } => Self::NousNotFound {
                id: nous_id,
                location: snafu::Location::default(),
            },
            Error::GuardRejected { reason, .. } => Self::Forbidden {
                message: reason,
                location: snafu::Location::default(),
            },
            Error::Llm { source, .. } => Self::from(source),
            _ => Self::Internal {
                message: err.to_string(),
                location: snafu::Location::default(),
            },
        }
    }
}

impl From<tokio::task::JoinError> for ApiError {
    #[track_caller]
    fn from(err: tokio::task::JoinError) -> Self {
        Self::Internal {
            message: format!("task join failed: {err}"),
            location: snafu::Location::default(),
        }
    }
}
