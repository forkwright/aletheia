//! Shared health endpoint parsing and reachability classification.

use std::fmt;

use reqwest::StatusCode;

use super::types::{HealthResponse, LivenessResponse};

/// Failure class for `GET /api/health`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthFetchError {
    /// Request failed before a response body could be parsed.
    Connection(String),
    /// Server returned a status that is not part of the health contract.
    Status(StatusCode),
    /// Response status was accepted, but the body was not a valid health report.
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

impl std::error::Error for HealthFetchError {}

/// Whether this HTTP status is expected to carry a `HealthResponse` body.
#[must_use]
pub fn accepts_health_body(status: StatusCode) -> bool {
    status.is_success() || status == StatusCode::SERVICE_UNAVAILABLE
}

/// Whether this status represents rejected health credentials.
#[must_use]
pub fn is_auth_status(status: StatusCode) -> bool {
    status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN
}

/// Parse a health response body using pylon's reachability contract.
///
/// `503 Service Unavailable` is accepted because pylon uses it for a reachable
/// backend whose aggregate health is unhealthy.
///
/// # Errors
///
/// Returns [`HealthFetchError::Status`] for out-of-contract statuses and
/// [`HealthFetchError::Malformed`] for accepted statuses with invalid JSON.
pub fn parse_health_body(
    status: StatusCode,
    body: &str,
) -> Result<HealthResponse, HealthFetchError> {
    if accepts_health_body(status) {
        serde_json::from_str::<HealthResponse>(body)
            .map_err(|err| HealthFetchError::Malformed(err.to_string()))
    } else {
        Err(HealthFetchError::Status(status))
    }
}

/// Fetch and parse a health response from a completed reqwest request.
///
/// # Errors
///
/// Returns [`HealthFetchError::Connection`] when the request or body read fails.
/// Other failures are produced by [`parse_health_body`].
pub async fn fetch_health_response(
    result: Result<reqwest::Response, reqwest::Error>,
) -> Result<HealthResponse, HealthFetchError> {
    match result {
        Ok(response) => {
            let status = response.status();
            if is_auth_status(status) {
                return Err(HealthFetchError::Status(status));
            }
            let body = response
                .text()
                .await
                .map_err(|err| HealthFetchError::Connection(err.to_string()))?;
            parse_health_body(status, &body)
        }
        Err(err) => Err(HealthFetchError::Connection(err.to_string())),
    }
}

/// Parse a liveness response body using pylon's `GET /api/health` contract.
///
/// Distinct from [`parse_health_body`]: the liveness endpoint carries only
/// `status`, so this must not be used to parse `/api/v1/system/health`.
///
/// # Errors
///
/// Returns [`HealthFetchError::Status`] for out-of-contract statuses and
/// [`HealthFetchError::Malformed`] for accepted statuses with invalid JSON.
pub fn parse_liveness_body(
    status: StatusCode,
    body: &str,
) -> Result<LivenessResponse, HealthFetchError> {
    if accepts_health_body(status) {
        serde_json::from_str::<LivenessResponse>(body)
            .map_err(|err| HealthFetchError::Malformed(err.to_string()))
    } else {
        Err(HealthFetchError::Status(status))
    }
}

/// Fetch and parse a liveness response from a completed reqwest request.
///
/// # Errors
///
/// Returns [`HealthFetchError::Connection`] when the request or body read fails.
/// Other failures are produced by [`parse_liveness_body`].
pub async fn fetch_liveness_response(
    result: Result<reqwest::Response, reqwest::Error>,
) -> Result<LivenessResponse, HealthFetchError> {
    match result {
        Ok(response) => {
            let status = response.status();
            if is_auth_status(status) {
                return Err(HealthFetchError::Status(status));
            }
            let body = response
                .text()
                .await
                .map_err(|err| HealthFetchError::Connection(err.to_string()))?;
            parse_liveness_body(status, &body)
        }
        Err(err) => Err(HealthFetchError::Connection(err.to_string())),
    }
}

/// Return check names whose status is not `pass`.
#[must_use]
pub fn failing_check_names(response: &HealthResponse) -> Vec<String> {
    response
        .checks
        .iter()
        .filter(|check| check.status != "pass")
        .map(|check| check.name.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    #![expect(clippy::expect_used, reason = "test assertions need contextual panics")]

    use super::super::types::HealthCheck;
    use super::*;

    fn install_crypto() {
        crate::install_test_crypto_provider();
    }

    /// Builds a fixture by serializing the real `HealthResponse` DTO rather
    /// than hand-authoring JSON, so a future field change to the DTO breaks
    /// this fixture instead of silently drifting from what the server sends.
    fn health_body(status: &str, check_status: &str) -> String {
        serde_json::to_string(&HealthResponse {
            status: status.to_string(),
            version: "0.13.1".to_string(),
            git_sha: "abc123".to_string().into(),
            uptime_seconds: 300,
            checks: vec![HealthCheck {
                name: "providers".to_string(),
                status: check_status.to_string(),
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
    fn parses_200_healthy_json() {
        let response = parse_health_body(StatusCode::OK, &health_body("healthy", "pass"))
            .expect("healthy JSON must parse");
        assert_eq!(response.status, "healthy");
        assert!(failing_check_names(&response).is_empty());
    }

    #[test]
    fn parses_200_degraded_json() {
        let response = parse_health_body(StatusCode::OK, &health_body("degraded", "warn"))
            .expect("degraded JSON must parse");
        assert_eq!(response.status, "degraded");
        assert_eq!(failing_check_names(&response), vec!["providers"]);
    }

    #[test]
    fn parses_503_unhealthy_json() {
        let response = parse_health_body(
            StatusCode::SERVICE_UNAVAILABLE,
            &health_body("unhealthy", "fail"),
        )
        .expect("503 health JSON must parse");
        assert_eq!(response.status, "unhealthy");
        assert_eq!(failing_check_names(&response), vec!["providers"]);
    }

    #[test]
    fn malformed_503_body_is_distinct() {
        let err = parse_health_body(StatusCode::SERVICE_UNAVAILABLE, "not-json")
            .expect_err("malformed 503 JSON must fail");
        assert!(matches!(err, HealthFetchError::Malformed(_)));
    }

    #[test]
    fn auth_status_is_identified_distinctly() {
        let err = parse_health_body(StatusCode::UNAUTHORIZED, "{}")
            .expect_err("401 must not be parsed as health");
        assert!(matches!(err, HealthFetchError::Status(status) if is_auth_status(status)));
        assert!(!is_auth_status(StatusCode::INTERNAL_SERVER_ERROR));
    }

    #[tokio::test]
    async fn network_failure_is_distinct() {
        install_crypto();
        let client = reqwest::Client::new();
        let result = client.get("http://127.0.0.1:1/api/health").send().await;
        let err = fetch_health_response(result)
            .await
            .expect_err("closed local port must fail");
        assert!(matches!(err, HealthFetchError::Connection(_)));
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
        // shaped like what `/api/health` actually sends must not silently
        // parse as the richer `/api/v1/system/health` contract.
        let err = parse_health_body(StatusCode::OK, &liveness_body("healthy"))
            .expect_err("a liveness-only body must not satisfy HealthResponse");
        assert!(matches!(err, HealthFetchError::Malformed(_)));
    }

    #[test]
    fn liveness_auth_status_is_identified_distinctly() {
        let err = parse_liveness_body(StatusCode::UNAUTHORIZED, "{}")
            .expect_err("401 must not be parsed as liveness");
        assert!(matches!(err, HealthFetchError::Status(status) if is_auth_status(status)));
    }
}
