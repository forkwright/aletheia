use std::fmt;

use reqwest::StatusCode;
use skene::api::types::{HealthResponse, LivenessResponse};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HealthFetchError {
    Connection(String),
    Status(StatusCode),
    Malformed(String),
}

impl fmt::Display for HealthFetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connection(message) => write!(f, "connection error: {message}"),
            Self::Status(status) => write!(f, "health endpoint returned {status}"),
            Self::Malformed(message) => write!(f, "failed to parse health response: {message}"),
        }
    }
}

/// Parses a `GET /api/v1/system/health` body. Do not use this for
/// `/api/health`: that route is unauthenticated liveness only and carries
/// none of [`HealthResponse`]'s fields beyond `status` — use
/// [`parse_liveness_body`] there instead.
pub(crate) fn parse_health_body(
    status: StatusCode,
    body: &str,
) -> Result<HealthResponse, HealthFetchError> {
    if status.is_success() || status == StatusCode::SERVICE_UNAVAILABLE {
        serde_json::from_str::<HealthResponse>(body)
            .map_err(|err| HealthFetchError::Malformed(err.to_string()))
    } else {
        Err(HealthFetchError::Status(status))
    }
}

/// Parses a `GET /api/health` liveness body.
pub(crate) fn parse_liveness_body(
    status: StatusCode,
    body: &str,
) -> Result<LivenessResponse, HealthFetchError> {
    if status.is_success() || status == StatusCode::SERVICE_UNAVAILABLE {
        serde_json::from_str::<LivenessResponse>(body)
            .map_err(|err| HealthFetchError::Malformed(err.to_string()))
    } else {
        Err(HealthFetchError::Status(status))
    }
}

pub(crate) async fn fetch_health_response(
    result: Result<reqwest::Response, reqwest::Error>,
) -> Result<HealthResponse, HealthFetchError> {
    match result {
        Ok(response) => {
            let status = response.status();
            let body = response
                .text()
                .await
                .map_err(|err| HealthFetchError::Connection(err.to_string()))?;
            parse_health_body(status, &body)
        }
        Err(err) => Err(HealthFetchError::Connection(err.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a fixture by serializing the real `HealthResponse` DTO rather
    /// than hand-authoring JSON, so a future narrowing of the DTO breaks this
    /// fixture instead of silently drifting from what the server sends.
    fn health_body(status: &str) -> String {
        serde_json::to_string(&HealthResponse {
            status: status.to_string(),
            version: "0.13.1".to_string(),
            git_sha: "abc123".to_string().into(),
            uptime_seconds: 300,
            checks: vec![skene::api::types::HealthCheck {
                name: "providers".to_string(),
                status: "pass".to_string(),
                message: None,
            }],
            data_dir: "/tmp/data".to_string(),
        })
        .expect("HealthResponse must serialize")
    }

    /// Builds a fixture from the real `LivenessResponse` DTO — the actual
    /// shape `GET /api/health` returns.
    fn liveness_body(status: &str) -> String {
        serde_json::to_string(&LivenessResponse {
            status: status.to_string(),
        })
        .expect("LivenessResponse must serialize")
    }

    #[test]
    fn parses_2xx_healthy_json() {
        let response = parse_health_body(StatusCode::OK, &health_body("healthy"))
            .expect("healthy JSON must parse");
        assert_eq!(response.status, "healthy");
        assert_eq!(response.uptime_seconds, 300);
    }

    #[test]
    fn parses_503_unhealthy_json() {
        let response =
            parse_health_body(StatusCode::SERVICE_UNAVAILABLE, &health_body("unhealthy"))
                .expect("503 health JSON must parse");
        assert_eq!(response.status, "unhealthy");
        assert_eq!(response.uptime_seconds, 300);
    }

    #[test]
    fn malformed_json_returns_parse_error() {
        let err =
            parse_health_body(StatusCode::OK, "not-json").expect_err("malformed JSON must fail");
        assert!(matches!(err, HealthFetchError::Malformed(_)));
    }

    #[test]
    fn non_503_error_status_returns_status_error() {
        let err = parse_health_body(StatusCode::INTERNAL_SERVER_ERROR, "{}")
            .expect_err("500 body must not be parsed as health");
        assert!(matches!(
            err,
            HealthFetchError::Status(StatusCode::INTERNAL_SERVER_ERROR)
        ));
    }

    #[test]
    fn parses_genuine_liveness_body() {
        let response = parse_liveness_body(StatusCode::OK, &liveness_body("healthy"))
            .expect("a genuine `/api/health` liveness body must parse");
        assert_eq!(response.status, "healthy");
    }

    #[test]
    fn genuine_liveness_body_does_not_satisfy_the_detailed_parser() {
        // WHY: this is the regression this module exists to prevent — a body
        // shaped like what `/api/health` actually sends (see #6732) must not
        // silently parse as the richer `/api/v1/system/health` contract.
        let err = parse_health_body(StatusCode::OK, &liveness_body("healthy"))
            .expect_err("a liveness-only body must not satisfy HealthResponse");
        assert!(matches!(err, HealthFetchError::Malformed(_)));
    }
}
