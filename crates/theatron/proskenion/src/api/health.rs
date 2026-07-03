use std::fmt;

use reqwest::StatusCode;
use skene::api::types::HealthResponse;

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

    fn health_body(status: &str) -> String {
        serde_json::json!({
            "status": status,
            "version": "0.13.1",
            "git_sha": "abc123",
            "uptime_seconds": 300,
            "checks": [
                {"name": "providers", "status": "pass", "message": null}
            ],
            "data_dir": "/tmp/data"
        })
        .to_string()
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
}
