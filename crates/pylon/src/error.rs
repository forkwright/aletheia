//! API error types with Axum response integration.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use snafu::Snafu;

#[path = "error_dto.rs"]
mod error_dto;
pub use error_dto::{ErrorBody, ErrorResponse, FieldError};

/// HTTP API errors, each mapped to an appropriate status code via [`IntoResponse`].
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
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

    #[snafu(display("rate limited, retry after {retry_after_secs}s"))]
    RateLimited {
        retry_after_secs: u64,
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

    /// Idempotency conflict: a request with this key is already in flight (409).
    #[snafu(display("conflict: {message}"))]
    Conflict {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Stream turn idempotency conflict with the canonical turn identifier (409).
    #[snafu(display("stream turn conflict: {message}"))]
    StreamTurnConflict {
        message: String,
        turn_id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Validation failed with field-level errors (422).
    #[snafu(display("validation failed"))]
    ValidationFailed {
        errors: Vec<FieldError>,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Feature not yet implemented (501).
    #[snafu(display("not implemented: {message}"))]
    NotImplemented {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Client-visible error already classified by a lower-layer presentation contract.
    #[snafu(display("user-facing error ({status} {code}): {message}"))]
    UserFacing {
        status: StatusCode,
        code: String,
        message: String,
        retry_after_secs: Option<u64>,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

impl ApiError {
    pub(crate) fn forbidden(message: &str) -> Self {
        ForbiddenSnafu { message }.build()
    }
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
            Self::RateLimited {
                retry_after_secs, ..
            } => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                Some(serde_json::json!({ "retry_after_secs": retry_after_secs })),
            ),
            Self::Conflict { .. } => (StatusCode::CONFLICT, "conflict", None),
            Self::StreamTurnConflict { turn_id, .. } => (
                StatusCode::CONFLICT,
                "stream_turn_conflict",
                Some(serde_json::json!({ "turn_id": turn_id })),
            ),
            Self::Forbidden { .. } => (StatusCode::FORBIDDEN, "forbidden", None),
            Self::ServiceUnavailable { .. } => {
                (StatusCode::SERVICE_UNAVAILABLE, "service_unavailable", None)
            }
            Self::ValidationFailed { errors, .. } => {
                // WHY(#3275): serialize structured field errors so consumers
                // can match on field + code without parsing English messages.
                let errors_json =
                    serde_json::to_value(errors).unwrap_or_else(|_| serde_json::json!([]));
                (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "validation_failed",
                    Some(serde_json::json!({ "errors": errors_json })),
                )
            }
            Self::NotImplemented { .. } => (StatusCode::NOT_IMPLEMENTED, "not_implemented", None),
            Self::UserFacing {
                status,
                code,
                retry_after_secs,
                ..
            } => (
                *status,
                code.as_str(),
                retry_after_secs.map(|secs| serde_json::json!({ "retry_after_secs": secs })),
            ),
        };

        // WHY: retry_after_secs must be extracted before self is moved into client_message construction below.
        let retry_after_secs = if let Self::RateLimited {
            retry_after_secs, ..
        } = &self
        {
            Some(*retry_after_secs)
        } else if let Self::UserFacing {
            retry_after_secs, ..
        } = &self
        {
            *retry_after_secs
        } else {
            None
        };

        // WHY: 5xx errors log full details internally but return a generic message so SQL paths,
        // panic text, and provider details are never exposed to clients (#827, #846, #847).
        let client_message = if let Self::UserFacing { message, .. } = &self {
            message.clone()
        } else if status.is_server_error() {
            tracing::error!(error = %self, "internal server error");
            "An internal error occurred".to_owned()
        } else {
            self.to_string()
        };

        let body = ErrorResponse {
            error: ErrorBody {
                code: code.to_owned(),
                message: client_message,
                request_id: None,
                details,
            },
        };

        let mut response = (status, Json(body)).into_response();

        // WHY: RFC 6585 requires Retry-After on 429 responses.
        if let Some(secs) = retry_after_secs {
            response.headers_mut().insert(
                axum::http::header::RETRY_AFTER,
                axum::http::HeaderValue::from_str(&secs.to_string())
                    .unwrap_or_else(|_| axum::http::HeaderValue::from_static("60")),
            );
        }

        response
    }
}

/// Convert a crate error into an [`ApiError`] with explicit match arms for
/// every known variant, plus a `#[non_exhaustive]` catch-all that logs the
/// full error (including source chain) at `error` level.
///
/// WHY: every known variant gets an explicit match arm so the HTTP status code
/// is semantically correct (transient -> 503, permanent -> 500, client fault
/// -> 4xx). The catch-all exists only because downstream enums are
/// `#[non_exhaustive]` — if a new variant is added, it triggers the catch-all
/// which logs the full error with `tracing::error!`, making the gap immediately
/// visible in monitoring without silently erasing the error type (#3283).
macro_rules! impl_from_error {
    ($error_mod:path, |$err:ident| { $($arms:tt)* }) => {
        impl From<$error_mod> for ApiError {
            fn from($err: $error_mod) -> Self {
                #[allow(clippy::enum_glob_use, reason = "scoped to From impl body for concise match arms")]
                use $error_mod::*;
                match $err {
                    $($arms)*
                    // WHY: `#[non_exhaustive]` requires a catch-all. Unlike the
                    // previous version that silently converted to `Internal` via
                    // `.to_string()`, this logs the full error at error level so
                    // new unclassified variants are immediately visible (#3283).
                    // WHY `#[allow]` not `#[expect]`: this arm is unreachable when
                    // all current variants are matched, but becomes reachable when
                    // a downstream crate adds a new variant. `#[expect]` would
                    // fire `unfulfilled_lint_expectations` in the common case.
                    #[allow(unreachable_patterns, reason = "required by #[non_exhaustive]; triggers on new unhandled variants")]
                    _ => {
                        tracing::error!(
                            error = %$err,
                            error_debug = ?$err,
                            "unclassified error variant — add an explicit match arm"
                        );
                        InternalSnafu {
                            message: $err.to_string(),
                        }
                        .build()
                    }
                }
            }
        }
    };
}

impl_from_error!(mneme::error::Error, |err| {
    SessionNotFound { id, .. } => SessionNotFoundSnafu { id }.build(),
    // WHY: archived sessions cannot receive messages without explicit unarchival.
    // 409 Conflict is correct — the resource exists but is in a state that
    // prevents the requested operation.
    SessionIsArchived { ref id, .. } => ConflictSnafu {
        message: format!(
            "session '{id}' is archived; use POST /sessions/{id}/unarchive to reactivate"
        ),
    }
    .build(),
    SessionCreate { .. } | Storage { .. } | StoredJson { .. } | Io { .. } => {
        tracing::error!(error = %err, "mneme internal error");
        InternalSnafu {
            message: err.to_string(),
        }
        .build()
    }
});

impl_from_error!(mneme::knowledge_error::Error, |err| {
    FactNotFound { id, .. } => NotFoundSnafu {
        path: format!("fact/{id}"),
    }
    .build(),
    // WHY: validation errors are the caller's fault and are safe to expose.
    EmptyContent { .. }
    | ContentTooLong { .. }
    | InvalidConfidence { .. }
    | InvalidTimestamp { .. }
    | EmptyEntityName { .. }
    | InvalidWeight { .. }
    | EmptyEmbedding { .. }
    | EmptyEmbeddingContent { .. }
    | AdmissionRejected { .. }
    | InvalidId { .. } => BadRequestSnafu {
        message: err.to_string(),
    }
    .build(),
    EngineInit { .. }
    | EngineQuery { .. }
    | SchemaVersion { .. }
    | Conversion { .. }
    | QueryRewrite { .. }
    | EnhancedSearch { .. }
    | EmbeddingDrift { .. } => {
        tracing::error!(error = %err, "mneme engine error");
        InternalSnafu {
            message: format!("engine error: {err}"),
        }
        .build()
    }
    QueryTimeout { .. } => ServiceUnavailableSnafu {
        message: format!("query timed out: {err}"),
    }
    .build(),
    Join { .. } => InternalSnafu {
        message: format!("task join failed: {err}"),
    }
    .build(),
    EmbeddingDimensionMismatch { .. } => BadRequestSnafu {
        message: err.to_string(),
    }
    .build(),
});

impl_from_error!(hermeneus::error::Error, |err| {
    RateLimited { retry_after_ms, .. } => RateLimitedSnafu {
        retry_after_secs: retry_after_ms.div_ceil(1000),
    }
    .build(),
    AuthFailed { message, .. } => ServiceUnavailableSnafu {
        message: format!("provider auth failed: {message}"),
    }
    .build(),
    // WHY: ProviderInit errors from subprocess providers (CC binary crashed,
    // disappeared, auth expired) are transient — the server is alive but the
    // LLM provider is temporarily unavailable. 503 is the correct HTTP status
    // so clients know to retry, and the health endpoint reports degraded.
    ProviderInit { message, .. } => ServiceUnavailableSnafu {
        message: format!("provider unavailable: {message}"),
    }
    .build(),
    ApiError { status: 429, .. } => RateLimitedSnafu {
        retry_after_secs: 0_u64,
    }
    .build(),
    ApiError {
        status: 503,
        message,
        ..
    } => ServiceUnavailableSnafu { message }.build(),
    // WHY: 5xx from the upstream provider is a transient condition. 503 tells
    // the client to retry.
    ApiError {
        status: 500..=599,
        message,
        ..
    } => ServiceUnavailableSnafu {
        message: format!("provider error: {message}"),
    }
    .build(),
    // WHY: 4xx from upstream is the caller's fault (bad model, too many tokens, etc.)
    ApiError { status, ref message, .. } => {
        tracing::error!(error = %err, "hermeneus API error");
        InternalSnafu {
            message: format!("provider API error ({status}): {message}"),
        }
        .build()
    }
    // WHY: Parse errors indicate the provider returned unexpected data — not retryable.
    ParseResponse { .. } => {
        tracing::error!(error = %err, "hermeneus parse response error");
        InternalSnafu {
            message: format!("provider response parse failed: {err}"),
        }
        .build()
    }
    UnsupportedModel { model, .. } => BadRequestSnafu {
        message: format!("model '{model}' is not supported by this provider"),
    }
    .build(),
    // WHY: network-level request failures (timeout, connection refused, etc.)
    // are transient — 503 so clients know to retry.
    ApiRequest { .. } => ServiceUnavailableSnafu {
        message: format!("provider request failed: {err}"),
    }
    .build(),
});

impl From<nous::error::Error> for ApiError {
    fn from(err: nous::error::Error) -> Self {
        if let Some(user_error) = nous::user_error::to_user_facing(&err) {
            return user_facing_error_response(&user_error);
        }

        map_nous_error(err)
    }
}

fn map_nous_error(err: nous::error::Error) -> ApiError {
    use nous::error::Error;
    match err {
        Error::NousNotFound { nous_id, .. } => NousNotFoundSnafu { id: nous_id }.build(),
        Error::GuardRejected { reason, .. } => ForbiddenSnafu { message: reason }.build(),
        Error::PipelineTimeout {
            stage,
            timeout_secs,
            ..
        } => ServiceUnavailableSnafu {
            message: format!("pipeline stage '{stage}' timed out after {timeout_secs}s"),
        }
        .build(),
        Error::PipelineStage { stage, message, .. } => {
            tracing::error!(stage, message, "pipeline stage error");
            InternalSnafu {
                message: format!("pipeline stage '{stage}' failed: {message}"),
            }
            .build()
        }
        Error::ServiceDegraded {
            nous_id,
            panic_count,
            ..
        } => ServiceUnavailableSnafu {
            message: format!("agent '{nous_id}' is degraded after {panic_count} panics"),
        }
        .build(),
        Error::Llm { source, .. } => ApiError::from(source),
        other @ (Error::ActorSend { .. }
        | Error::ActorRecv { .. }
        | Error::AskTimeout { .. }
        | Error::InboxFull { .. }
        | Error::RecallEmbedding { .. }
        | Error::RecallSearch { .. }
        | Error::Store { .. }
        | Error::CompetenceStore { .. }
        | Error::UncertaintyStore { .. }
        | Error::WorkingCheckpointStore { .. }
        | Error::DeliveryFailed { .. }) => map_nous_service_error(other),
        _ => map_nous_internal_error(err),
    }
}

fn map_nous_service_error(err: nous::error::Error) -> ApiError {
    use nous::error::Error;
    match err {
        Error::ActorSend { .. } | Error::ActorRecv { .. } => ServiceUnavailableSnafu {
            message: "agent actor unavailable".to_owned(),
        }
        .build(),
        Error::AskTimeout {
            nous_id,
            timeout_secs,
            ..
        } => ServiceUnavailableSnafu {
            message: format!("cross-agent ask to '{nous_id}' timed out after {timeout_secs}s"),
        }
        .build(),
        Error::InboxFull { nous_id, .. } => ServiceUnavailableSnafu {
            message: format!("agent '{nous_id}' is overloaded"),
        }
        .build(),
        Error::RecallEmbedding { .. } | Error::RecallSearch { .. } => ServiceUnavailableSnafu {
            message: "recall service unavailable".to_owned(),
        }
        .build(),
        Error::Store { .. } => ServiceUnavailableSnafu {
            message: "session store unavailable".to_owned(),
        }
        .build(),
        Error::CompetenceStore { .. }
        | Error::UncertaintyStore { .. }
        | Error::WorkingCheckpointStore { .. } => ServiceUnavailableSnafu {
            message: "store unavailable".to_owned(),
        }
        .build(),
        Error::DeliveryFailed { nous_id, .. } => ServiceUnavailableSnafu {
            message: format!("delivery to '{nous_id}' failed"),
        }
        .build(),
        other => map_nous_internal_error(other),
    }
}

fn map_nous_internal_error(err: nous::error::Error) -> ApiError {
    use nous::error::Error;
    match err {
        Error::Config { message, .. } => InternalSnafu {
            message: format!("configuration error: {message}"),
        }
        .build(),
        Error::WorkspaceValidation {
            nous_id, message, ..
        } => InternalSnafu {
            message: format!("agent '{nous_id}' workspace invalid: {message}"),
        }
        .build(),
        Error::ContextAssembly { message, .. } => InternalSnafu {
            message: format!("context assembly failed: {message}"),
        }
        .build(),
        Error::ContextAssemblyIo { file, .. } => InternalSnafu {
            message: format!("context assembly failed: file '{file}' unreadable"),
        }
        .build(),
        Error::LoopDetected {
            iterations,
            pattern,
            ..
        } => InternalSnafu {
            message: format!("loop detected after {iterations} iterations: {pattern}"),
        }
        .build(),
        Error::AskCycleDetected { chain, .. } => InternalSnafu {
            message: format!("ask cycle detected: {chain}"),
        }
        .build(),
        Error::ReplyNotFound { .. } => InternalSnafu {
            message: "reply channel not found".to_owned(),
        }
        .build(),
        Error::MutexPoisoned { .. } => InternalSnafu {
            message: "internal lock poisoned".to_owned(),
        }
        .build(),
        Error::PipelinePanic { .. } => InternalSnafu {
            message: "pipeline encountered an unexpected internal error".to_owned(),
        }
        .build(),
        Error::Distillation { .. } => {
            tracing::error!(error = %err, "distillation failed");
            InternalSnafu {
                message: format!("distillation failed: {err}"),
            }
            .build()
        }
        Error::SelfAudit { .. } | Error::RoleContract { .. } => {
            tracing::error!(error = %err, "self-monitoring error");
            InternalSnafu {
                message: format!("self-monitoring error: {err}"),
            }
            .build()
        }
        _ => {
            tracing::error!(error = %err, "unclassified nous error variant");
            InternalSnafu {
                message: err.to_string(),
            }
            .build()
        }
    }
}

fn user_facing_error_response(error: &nous::user_error::UserFacingError) -> ApiError {
    let status = match &error {
        nous::user_error::UserFacingError::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
        nous::user_error::UserFacingError::ContextOverflow { .. }
        | nous::user_error::UserFacingError::ToolExecutionFailed { .. } => StatusCode::BAD_REQUEST,
        nous::user_error::UserFacingError::SessionExpired { .. } => StatusCode::NOT_FOUND,
        nous::user_error::UserFacingError::ProviderUnavailable { .. } => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    ApiError::UserFacing {
        status,
        code: error.code().to_owned(),
        message: error.to_string(),
        retry_after_secs: error.retry_after_secs(),
        location: snafu::location!(),
    }
}

impl From<tokio::task::JoinError> for ApiError {
    fn from(err: tokio::task::JoinError) -> Self {
        InternalSnafu {
            message: format!("task join failed: {err}"),
        }
        .build()
    }
}

/// WHY: Axum's `Json` extractor returns `JsonRejection` for malformed or
/// missing request bodies. Without this impl, those rejections bypass
/// `ApiError` and produce plain-text error responses instead of the
/// `ErrorResponse` JSON envelope (#3160).
impl From<axum::extract::rejection::JsonRejection> for ApiError {
    fn from(err: axum::extract::rejection::JsonRejection) -> Self {
        use axum::extract::rejection::JsonRejection;
        match err {
            // WHY: Missing/mistyped fields return 422 (same status as Axum's default)
            // to preserve backward compatibility while wrapping in the error envelope.
            // WHY(#3275): Map Axum's JSON data errors to structured field errors.
            // The error message contains the field path from serde, which we
            // extract when possible.
            JsonRejection::JsonDataError(_) => {
                let msg = err.to_string();
                let field_error = parse_json_data_error(&msg);
                Self::ValidationFailed {
                    errors: vec![field_error],
                    location: snafu::location!(),
                }
            }
            JsonRejection::MissingJsonContentType(_) => BadRequestSnafu {
                message: "expected Content-Type: application/json",
            }
            .build(),
            JsonRejection::BytesRejection(_) => BadRequestSnafu {
                message: "failed to read request body",
            }
            .build(),
            // WHY: JsonSyntaxError and future unknown variants all map to bad_request
            // with the original error message preserved.
            _ => BadRequestSnafu {
                message: err.to_string(),
            }
            .build(),
        }
    }
}

/// Parse a serde/axum JSON data error message into a structured `FieldError`.
///
/// Axum's `JsonDataError` messages typically look like:
/// - `"missing field `content` at line 1 column 2"`
/// - `"invalid type: string ..., expected u32 at line 1 column 5: at field `limit`"`
///
/// We extract the field name when present, otherwise use `"_body"` as a
/// catch-all indicating the error applies to the request body as a whole.
fn parse_json_data_error(msg: &str) -> FieldError {
    // WHY: serde error messages include the field name in backtick-delimited
    // segments. We try two patterns: "missing field `X`" and "at field `X`".
    let field = extract_backtick_field(msg, "missing field")
        .or_else(|| extract_backtick_field(msg, "at field"))
        .or_else(|| extract_backtick_field(msg, "unknown field"))
        .unwrap_or("_body");

    let code = if msg.contains("missing field") {
        "required"
    } else if msg.contains("invalid type") {
        "format"
    } else if msg.contains("unknown field") {
        "unknown_field"
    } else {
        "invalid"
    };

    FieldError {
        field: field.to_owned(),
        code: code.to_owned(),
        message: msg.to_owned(),
    }
}

/// Extract a field name from a serde error message after a given prefix.
///
/// Looks for the pattern: prefix followed by a backtick-delimited field name.
fn extract_backtick_field<'a>(msg: &'a str, prefix: &str) -> Option<&'a str> {
    let pos = msg.find(prefix)?;
    let after = msg.get(pos + prefix.len()..)?;
    let start = after.find('`')? + 1;
    let rest = after.get(start..)?;
    let end = rest.find('`')?;
    rest.get(..end)
}

/// WHY: Axum's `Query` extractor returns `QueryRejection` for malformed query
/// strings. Without this impl, those rejections bypass `ApiError` and produce
/// plain-text error responses instead of the `ErrorResponse` JSON envelope (#3160).
impl From<axum::extract::rejection::QueryRejection> for ApiError {
    fn from(err: axum::extract::rejection::QueryRejection) -> Self {
        BadRequestSnafu {
            message: err.to_string(),
        }
        .build()
    }
}

/// WHY: Axum's `Path` extractor returns `PathRejection` for invalid path
/// parameters. Without this impl, those rejections bypass `ApiError` and
/// produce plain-text error responses instead of the `ErrorResponse` JSON
/// envelope (#3160).
impl From<axum::extract::rejection::PathRejection> for ApiError {
    fn from(err: axum::extract::rejection::PathRejection) -> Self {
        BadRequestSnafu {
            message: err.to_string(),
        }
        .build()
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: JSON key indexing on known-present keys"
)]
mod tests {
    use axum::response::IntoResponse;
    use tracing::Instrument;

    use super::*;

    #[test]
    fn rate_limited_includes_retry_after_header() {
        let err = ApiError::RateLimited {
            retry_after_secs: 5,
            location: snafu::location!(),
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let retry = response
            .headers()
            .get(axum::http::header::RETRY_AFTER)
            .expect("should have Retry-After header");
        assert_eq!(retry.to_str().unwrap(), "5");
    }

    #[test]
    fn rate_limited_zero_secs_has_retry_after() {
        let err = ApiError::RateLimited {
            retry_after_secs: 0,
            location: snafu::location!(),
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let retry = response
            .headers()
            .get(axum::http::header::RETRY_AFTER)
            .expect("should have Retry-After header");
        assert_eq!(retry.to_str().unwrap(), "0");
    }

    #[test]
    fn non_rate_limited_no_retry_after() {
        let err = ApiError::Internal {
            message: "test".to_owned(),
            location: snafu::location!(),
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(
            response
                .headers()
                .get(axum::http::header::RETRY_AFTER)
                .is_none(),
            "non-429 should not have Retry-After"
        );
    }

    #[test]
    fn session_not_found_returns_404() {
        let err = ApiError::SessionNotFound {
            id: "ses-123".to_owned(),
            location: snafu::location!(),
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn pipeline_timeout_maps_to_service_unavailable() {
        let err = nous::error::Error::PipelineTimeout {
            stage: "execute".to_owned(),
            timeout_secs: 300,
            location: snafu::location!(),
        };
        let api_err = ApiError::from(err);
        let response = api_err.into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn guard_rejected_maps_to_forbidden() {
        let err = nous::error::Error::GuardRejected {
            reason: "safety check".to_owned(),
            location: snafu::location!(),
        };
        let api_err = ApiError::from(err);
        let response = api_err.into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn api_error_is_send_sync() {
        const _: fn() = || {
            fn assert<T: Send + Sync>() {}
            assert::<ApiError>();
        };
    }

    /// Helper: extract the `message` field from an `ErrorResponse` JSON body.
    fn body_message(response: Response) -> String {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let body = rt
            .block_on(axum::body::to_bytes(response.into_body(), 64 * 1024))
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        json["error"]["message"].as_str().unwrap().to_owned()
    }

    #[test]
    fn internal_error_returns_generic_message() {
        let err = ApiError::Internal {
            message: "SELECT * FROM users; file: /etc/passwd".to_owned(),
            location: snafu::location!(),
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let msg = body_message(response);
        assert_eq!(msg, "An internal error occurred");
        assert!(!msg.contains("SELECT"));
        assert!(!msg.contains("/etc/passwd"));
    }

    #[test]
    fn service_unavailable_returns_generic_message() {
        let err = ApiError::ServiceUnavailable {
            message: "provider auth failed: Anthropic API key is invalid".to_owned(),
            location: snafu::location!(),
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let msg = body_message(response);
        assert_eq!(msg, "An internal error occurred");
        assert!(!msg.contains("Anthropic"));
        assert!(!msg.contains("invalid"));
    }

    #[tokio::test]
    async fn join_error_returns_generic_message() {
        let join_err = tokio::spawn(
            async { panic!("database connection string leaked") }
                .instrument(tracing::info_span!("test_panic_task")),
        )
        .await
        .unwrap_err();
        let api_err = ApiError::from(join_err);
        let response = api_err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let msg = json["error"]["message"].as_str().unwrap();
        assert_eq!(msg, "An internal error occurred");
        assert!(!msg.contains("database connection string"));
    }

    #[test]
    fn auth_failed_does_not_leak_provider_details() {
        let hermeneus_err = hermeneus::error::Error::AuthFailed {
            message: "Anthropic returned 401: x-api-key header is invalid".to_owned(),
            location: snafu::location!(),
        };
        let api_err = ApiError::from(hermeneus_err);
        let response = api_err.into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let msg = body_message(response);
        assert_eq!(msg, "An internal error occurred");
        assert!(!msg.contains("Anthropic"));
        assert!(!msg.contains("x-api-key"));
    }

    #[test]
    fn bad_request_message_is_preserved() {
        let err = ApiError::BadRequest {
            message: "content must not be empty".to_owned(),
            location: snafu::location!(),
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let msg = body_message(response);
        assert!(msg.contains("content must not be empty"));
    }

    #[test]
    fn mneme_session_not_found_maps_to_404() {
        let mneme_err = mneme::error::Error::SessionNotFound {
            id: "ses-01abc".to_owned(),
            location: snafu::location!(),
        };
        let api_err = ApiError::from(mneme_err);
        let response = api_err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn mneme_session_is_archived_maps_to_409() {
        let mneme_err = mneme::error::Error::SessionIsArchived {
            id: "ses-arc-01".to_owned(),
            location: snafu::location!(),
        };
        let api_err = ApiError::from(mneme_err);
        let response = api_err.into_response();
        assert_eq!(
            response.status(),
            StatusCode::CONFLICT,
            "archived session access must return 409 Conflict"
        );
        let msg = body_message(response);
        assert!(
            msg.contains("archived"),
            "conflict message must mention archived state: {msg}"
        );
        assert!(
            msg.contains("unarchive"),
            "conflict message must hint at unarchive endpoint: {msg}"
        );
    }

    #[test]
    fn mneme_fact_not_found_maps_to_404() {
        let mneme_err = mneme::knowledge_error::Error::FactNotFound {
            id: "fact-01abc".to_owned(),
            location: snafu::location!(),
        };
        let api_err = ApiError::from(mneme_err);
        let response = api_err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn mneme_empty_content_maps_to_400() {
        let mneme_err = mneme::knowledge_error::Error::EmptyContent {
            location: snafu::location!(),
        };
        let api_err = ApiError::from(mneme_err);
        let response = api_err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn provider_init_maps_to_503() {
        // WHY: ProviderInit errors from CC subprocess (binary crashed,
        // disappeared) are transient — 503 tells clients the server is
        // alive but the LLM provider is temporarily unavailable.
        let hermeneus_err = hermeneus::error::Error::ProviderInit {
            message: "failed to spawn claude CLI at /usr/bin/claude: No such file or directory"
                .to_owned(),
            location: snafu::location!(),
        };
        let api_err = ApiError::from(hermeneus_err);
        let response = api_err.into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn provider_init_via_nous_llm_maps_to_503() {
        // WHY: When a ProviderInit error propagates through nous::Error::Llm,
        // it must still map to 503, not 500.
        let hermeneus_err = hermeneus::error::Error::ProviderInit {
            message: "failed to spawn claude CLI".to_owned(),
            location: snafu::location!(),
        };
        let nous_err = nous::error::Error::Llm {
            source: hermeneus_err,
            location: snafu::location!(),
        };
        let api_err = ApiError::from(nous_err);
        let response = api_err.into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn pipeline_stage_provider_unavailable_maps_to_503() {
        // WHY: When resolve_provider_checked returns "provider is currently
        // unavailable" (circuit breaker open), the response must be 503.
        let nous_err = nous::error::Error::PipelineStage {
            stage: "execute".to_owned(),
            message: "provider 'cc' is currently unavailable".to_owned(),
            location: snafu::location!(),
        };
        let api_err = ApiError::from(nous_err);
        let response = api_err.into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn service_degraded_maps_to_503() {
        // WHY: When the nous actor is in degraded mode after panics,
        // subsequent turn requests should get 503, not 500.
        let nous_err = nous::error::Error::ServiceDegraded {
            nous_id: "syn".to_owned(),
            panic_count: 5,
            location: snafu::location!(),
        };
        let api_err = ApiError::from(nous_err);
        let response = api_err.into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn mneme_invalid_confidence_maps_to_400() {
        let mneme_err = mneme::knowledge_error::Error::InvalidConfidence {
            value: 1.5,
            location: snafu::location!(),
        };
        let api_err = ApiError::from(mneme_err);
        let response = api_err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn json_syntax_error_maps_to_400_with_envelope() {
        // Simulate a JSON syntax error rejection
        let api_err = ApiError::BadRequest {
            message: "expected value at line 1 column 1".to_owned(),
            location: snafu::location!(),
        };
        let response = api_err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let msg = body_message(response);
        assert!(msg.contains("expected value"));
    }

    #[test]
    fn validation_failed_returns_422_with_structured_errors() {
        let api_err = ApiError::ValidationFailed {
            errors: vec![FieldError {
                field: "content".to_owned(),
                code: "required".to_owned(),
                message: "must not be empty".to_owned(),
            }],
            location: snafu::location!(),
        };
        let response = api_err.into_response();
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation failures should return 422"
        );
        let rt = tokio::runtime::Runtime::new().unwrap();
        let body = rt
            .block_on(axum::body::to_bytes(response.into_body(), 64 * 1024))
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["code"], "validation_failed");
        let errors = json["error"]["details"]["errors"].as_array().unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0]["field"], "content");
        assert_eq!(errors[0]["code"], "required");
        assert_eq!(errors[0]["message"], "must not be empty");
    }
}
