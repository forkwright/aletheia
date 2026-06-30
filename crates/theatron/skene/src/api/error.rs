//! API-layer error types for skene HTTP clients.
//!
//! Re-exports keryx's transport `ApiError` and `Result` so existing
//! `skene::api::error` paths keep working. Adds typed wire shapes for
//! the canonical pylon error envelope so callers can extract machine-
//! readable codes, correlation IDs, and structured details without
//! re-parsing raw response bodies.

use serde::Deserialize;
use serde_json::Value;

pub use keryx::error::{
    ApiError, AuthSnafu, HttpSnafu, InvalidTokenSnafu, RateLimitedSnafu, Result, ServerSnafu,
};

/// Structured body from a pylon non-2xx response.
///
/// Mirrors `pylon::error_dto::ErrorBody`. Carry this alongside a
/// transport `ApiError` to surface machine-readable diagnostics —
/// correlation IDs, validation details, retry hints — to UIs without
/// re-parsing raw bodies.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ErrorResponse {
    /// Machine-readable error code (e.g. `"session_not_found"`).
    pub code: String,
    /// Human-readable error message.
    pub message: String,
    /// Per-request correlation ID for tracing errors across logs and
    /// client reports.
    pub request_id: Option<String>,
    /// Optional structured details (e.g. retry timing, validation
    /// errors from pylon's `FieldError` list).
    pub details: Option<Value>,
}

/// Backward-compatible alias for the structured pylon error body.
pub type ServerErrorDetail = ErrorResponse;

/// Structured pylon error body annotated with its HTTP status.
#[derive(Debug, Clone, PartialEq)]
pub struct ErrorEnvelope {
    /// HTTP status code from the response.
    pub status: u16,
    /// Structured error body from the response.
    pub error: ErrorResponse,
}

impl ErrorResponse {
    /// Format this error body for operator-facing UI surfaces.
    #[must_use]
    pub fn display_message(&self) -> String {
        format_error_fields_for_display(
            &self.message,
            None,
            Some(&self.code),
            self.request_id.as_deref(),
            self.details.as_ref(),
        )
    }
}

impl ErrorEnvelope {
    /// Format this status-bearing envelope for operator-facing UI surfaces.
    #[must_use]
    pub fn display_message(&self) -> String {
        format_error_fields_for_display(
            &self.error.message,
            Some(self.status),
            Some(&self.error.code),
            self.error.request_id.as_deref(),
            self.error.details.as_ref(),
        )
    }
}

#[derive(Deserialize)]
struct PylonErrorEnvelope {
    error: ErrorResponse,
}

/// Parse the canonical pylon error envelope `{error:{code,message,...}}`
/// from a response body string.
///
/// Returns `None` when the body is not valid JSON or does not contain
/// the canonical nested `error` object. Callers should fall back to
/// the HTTP status and reason phrase when `None` is returned.
#[must_use]
pub fn parse_pylon_error_body(body: &str) -> Option<ErrorResponse> {
    serde_json::from_str::<PylonErrorEnvelope>(body)
        .ok()
        .map(|e| e.error)
}

/// Parse a pylon error envelope and attach the HTTP status code.
#[must_use]
pub fn parse_pylon_error_envelope(status: u16, body: &str) -> Option<ErrorEnvelope> {
    parse_pylon_error_body(body).map(|error| ErrorEnvelope { status, error })
}

/// Format an HTTP error body for direct display in first-party UIs.
///
/// Canonical pylon envelopes render with status, code, request ID, and
/// structured details. Legacy flat `{message: ...}` and `{error: "..."}`
/// bodies are still accepted for older servers.
#[must_use]
pub fn format_http_error_body(status: u16, reason: &str, body: &str) -> String {
    parse_pylon_error_envelope(status, body).map_or_else(
        || legacy_flat_error_message(body).unwrap_or_else(|| format!("{status} {reason}")),
        |envelope| envelope.display_message(),
    )
}

/// Format known error fields for direct display in first-party UIs.
///
/// Missing optional fields are omitted. When only `message` is available,
/// the message is returned unchanged for compatibility with legacy stream
/// events.
#[must_use]
pub fn format_error_fields_for_display(
    message: &str,
    status: Option<u16>,
    code: Option<&str>,
    request_id: Option<&str>,
    details: Option<&Value>,
) -> String {
    let mut parts = Vec::new();
    if let Some(status) = status {
        parts.push(format!("status {status}"));
    }
    if let Some(code) = code.filter(|value| !value.is_empty()) {
        parts.push(format!("code {code}"));
    }
    if let Some(request_id) = request_id.filter(|value| !value.is_empty()) {
        parts.push(format!("request_id {request_id}"));
    }
    if let Some(details) = details {
        parts.push(format!("details {details}"));
    }

    if parts.is_empty() {
        message.to_string()
    } else {
        format!("{message} ({})", parts.join("; "))
    }
}

fn legacy_flat_error_message(body: &str) -> Option<String> {
    serde_json::from_str::<Value>(body).ok().and_then(|json| {
        json.get("message")
            .or_else(|| json.get("error"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    })
}

/// Extract a `Retry-After` delta-seconds value from response headers.
///
/// Returns `Some(secs)` when the header is present and contains a
/// valid unsigned integer (RFC 9110 § 10.2.3 delta-seconds form).
/// Returns `None` when the header is absent or contains an HTTP-date
/// value that this parser does not handle.
#[must_use]
pub(crate) fn parse_retry_after_secs(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
}

#[cfg(test)]
mod tests {
    #![expect(clippy::expect_used, reason = "test assertions")]

    use super::*;

    #[test]
    fn parse_canonical_pylon_envelope_extracts_detail() {
        let body = r#"{"error":{"code":"session_not_found","message":"session does not exist","request_id":"req-abc-123"}}"#;
        let detail = parse_pylon_error_body(body).expect("canonical envelope should parse");
        assert_eq!(detail.code, "session_not_found");
        assert_eq!(detail.message, "session does not exist");
        assert_eq!(detail.request_id.as_deref(), Some("req-abc-123"));
        assert!(detail.details.is_none());
    }

    #[test]
    fn parse_pylon_envelope_with_validation_details() {
        let body = r#"{"error":{"code":"validation_error","message":"invalid input","request_id":"req-xyz","details":{"fields":[{"field":"email","code":"format","message":"not an email"}]}}}"#;
        let detail = parse_pylon_error_body(body).expect("envelope with details should parse");
        assert_eq!(detail.code, "validation_error");
        assert_eq!(detail.request_id.as_deref(), Some("req-xyz"));
        assert!(detail.details.is_some());
    }

    #[test]
    fn parse_auth_failure_without_request_id() {
        let body = r#"{"error":{"code":"auth_failed","message":"invalid token"}}"#;
        let detail = parse_pylon_error_body(body).expect("auth failure envelope should parse");
        assert_eq!(detail.code, "auth_failed");
        assert_eq!(detail.message, "invalid token");
        assert!(detail.request_id.is_none());
    }

    #[test]
    fn parse_not_found_body() {
        let body = r#"{"error":{"code":"not_found","message":"resource not found","request_id":"req-404"}}"#;
        let detail = parse_pylon_error_body(body).expect("404 body should parse");
        assert_eq!(detail.code, "not_found");
        assert_eq!(detail.request_id.as_deref(), Some("req-404"));
    }

    #[test]
    fn parse_pylon_error_envelope_preserves_status() {
        let body = r#"{"error":{"code":"not_found","message":"resource not found","request_id":"req-404"}}"#;
        let envelope =
            parse_pylon_error_envelope(404, body).expect("canonical envelope should parse");
        assert_eq!(envelope.status, 404);
        assert_eq!(envelope.error.code, "not_found");
        assert_eq!(envelope.error.request_id.as_deref(), Some("req-404"));
    }

    #[test]
    fn format_http_error_body_preserves_pylon_envelope_details() {
        let body = r#"{"error":{"code":"validation_error","message":"invalid input","request_id":"req-xyz","details":{"errors":[{"field":"message","code":"required","message":"message is required"}]}}}"#;
        let message = format_http_error_body(422, "Unprocessable Entity", body);

        assert!(message.contains("invalid input"));
        assert!(message.contains("status 422"));
        assert!(message.contains("code validation_error"));
        assert!(message.contains("request_id req-xyz"));
        assert!(message.contains(r#""field":"message""#));
        assert!(message.contains(r#""code":"required""#));
    }

    #[test]
    fn format_error_fields_without_optional_fields_returns_message() {
        assert_eq!(
            format_error_fields_for_display("provider unavailable", None, None, None, None),
            "provider unavailable"
        );
    }

    #[test]
    fn format_error_fields_preserves_stream_request_id() {
        assert_eq!(
            format_error_fields_for_display(
                "provider unavailable",
                None,
                None,
                Some("req-stream"),
                None,
            ),
            "provider unavailable (request_id req-stream)"
        );
    }

    #[test]
    fn flat_message_field_returns_none() {
        // Legacy flat response format — not the canonical pylon envelope.
        // Callers must fall back to HTTP status + reason phrase.
        let body = r#"{"message":"something went wrong"}"#;
        assert!(parse_pylon_error_body(body).is_none());
    }

    #[test]
    fn flat_error_string_returns_none() {
        let body = r#"{"error":"bad request"}"#;
        assert!(parse_pylon_error_body(body).is_none());
    }

    #[test]
    fn format_http_error_body_accepts_legacy_flat_message() {
        let body = r#"{"message":"something went wrong"}"#;
        assert_eq!(
            format_http_error_body(400, "Bad Request", body),
            "something went wrong"
        );
    }

    #[test]
    fn format_http_error_body_accepts_legacy_error_string() {
        let body = r#"{"error":"bad request"}"#;
        assert_eq!(
            format_http_error_body(400, "Bad Request", body),
            "bad request"
        );
    }

    #[test]
    fn malformed_json_returns_none() {
        assert!(parse_pylon_error_body("{broken").is_none());
    }

    #[test]
    fn empty_body_returns_none() {
        assert!(parse_pylon_error_body("").is_none());
    }

    #[test]
    fn plain_text_body_returns_none() {
        assert!(parse_pylon_error_body("Internal Server Error").is_none());
    }

    #[test]
    fn retry_after_delta_seconds_parses() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::RETRY_AFTER,
            reqwest::header::HeaderValue::from_static("120"),
        );
        assert_eq!(parse_retry_after_secs(&headers), Some(120));
    }

    #[test]
    fn retry_after_zero_parses() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::RETRY_AFTER,
            reqwest::header::HeaderValue::from_static("0"),
        );
        assert_eq!(parse_retry_after_secs(&headers), Some(0));
    }

    #[test]
    fn retry_after_absent_returns_none() {
        let headers = reqwest::header::HeaderMap::new();
        assert!(parse_retry_after_secs(&headers).is_none());
    }

    #[test]
    fn retry_after_http_date_returns_none() {
        // HTTP-date form is not supported — callers treat as no hint.
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::RETRY_AFTER,
            reqwest::header::HeaderValue::from_static("Mon, 07 Apr 2025 12:00:00 GMT"),
        );
        assert!(parse_retry_after_secs(&headers).is_none());
    }

    #[test]
    fn retry_after_negative_returns_none() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::RETRY_AFTER,
            reqwest::header::HeaderValue::from_static("-1"),
        );
        assert!(parse_retry_after_secs(&headers).is_none());
    }
}
