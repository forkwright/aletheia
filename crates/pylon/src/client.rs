// kanon:ignore RUST/file-too-long — cohesive first-party gateway client; splitting would fragment route/auth/error handling
//! Typed HTTP client for first-party consumers of the Aletheia gateway.
//!
//! This client centralises the concerns that previously leaked into every
//! CLI command that talked to a running server: URL resolution, bearer-token
//! handling, CSRF/header defaults, route builders, typed response envelopes,
//! and structured error rendering. It is owned by `pylon` so that the gateway
//! contract and the consumer that speaks it live in the same crate.

use std::time::Duration;

use reqwest::{Client, Response, StatusCode, header};
use snafu::prelude::*;

use koina::http::BEARER_PREFIX;
use koina::secret::SecretString;

use crate::handlers::health::{HealthResponse, LivenessResponse};
use crate::handlers::sessions::types::ListSessionsResponse;
pub use crate::handlers::sessions::types::{
    HistoryResponse, ReplayMessage, ReplaySession, ReplayToolAuditRecord, ReplayTurnAttempt,
    ReplayUsageRecord, SessionReplayResponse, SessionResponse,
};

// WHY: 30 seconds matches the skene/desktop client connect timeout and is long
// enough for local-loopback discovery but short enough to surface a dead server.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

// WHY: 120 seconds covers large ingest batches and replay exports without
// letting a hung request run forever.
const REQUEST_TIMEOUT: Duration = Duration::from_mins(2);

const REQUEST_ID_HEADER_NAME: &str = "x-request-id";

/// Error returned by [`GatewayClient`] operations.
#[derive(Debug, Snafu)]
// kanon:ignore RUST/no-debug-derive-on-public-types — fields are gateway error codes and HTTP metadata, not secrets
#[non_exhaustive]
pub enum Error {
    /// The HTTP client could not be constructed (e.g. invalid TLS config).
    #[snafu(display("failed to build HTTP client: {source}"))]
    BuildClient {
        /// Underlying reqwest error.
        source: reqwest::Error,
    },

    /// The supplied bearer token contains characters that are illegal in an
    /// HTTP header value.
    #[snafu(display("invalid bearer token value"))]
    InvalidToken,

    /// The request never reached the server or failed at the transport layer.
    #[snafu(display("{operation} failed: {source}"))]
    Request {
        /// What the client was trying to do when the failure happened.
        operation: String,
        /// Underlying reqwest error.
        source: reqwest::Error,
    },

    /// The server rejected the authentication token (401 or 403).
    #[snafu(display("auth failed ({code}): {message}"))]
    Auth {
        /// Machine-readable error code from the gateway envelope.
        code: String,
        /// Human-readable error message from the gateway envelope.
        message: String,
        /// Per-request correlation ID, when the gateway returned one.
        request_id: Option<String>,
    },

    /// The server returned a non-success status other than an auth failure.
    #[snafu(display("server returned HTTP {status} ({code}): {message}"))]
    Server {
        /// HTTP status code.
        status: u16,
        /// Machine-readable error code from the gateway envelope.
        code: String,
        /// Human-readable error message from the gateway envelope.
        message: String,
        /// Per-request correlation ID, when the gateway returned one.
        request_id: Option<String>,
    },

    /// The response body could not be decoded as the expected type.
    #[snafu(display("failed to decode response: {source}"))]
    Decode {
        /// Underlying reqwest/serde error.
        source: reqwest::Error,
    },
}

/// Wire shape of a gateway error response body.
#[derive(Debug, serde::Deserialize)]
struct PylonErrorBody {
    code: String,
    message: String,
    #[serde(default)]
    request_id: Option<String>,
}

/// Wire shape of the outer gateway error envelope.
#[derive(Debug, serde::Deserialize)]
struct PylonErrorEnvelope {
    error: PylonErrorBody,
}

/// Canonical route builders for gateway endpoints.
///
/// Every path segment containing an identifier is encoded exactly once with
/// [`keryx::url::encode_path_segment`]. Callers must pass raw identifiers;
/// pre-encoding would produce double-encoded values such as `%252F`.
pub mod routes {
    use koina::http::{API_HEALTH, API_V1};

    /// Public liveness endpoint.
    #[must_use]
    pub fn health() -> &'static str {
        API_HEALTH
    }

    /// Operator-only detailed health endpoint.
    #[must_use]
    pub fn health_details() -> String {
        format!("{API_V1}/system/health")
    }

    /// Single-file knowledge ingestion endpoint.
    #[must_use]
    pub fn ingest() -> String {
        format!("{API_V1}/knowledge/ingest")
    }

    /// Directory/batch knowledge ingestion endpoint.
    #[must_use]
    pub fn ingest_batch() -> String {
        format!("{API_V1}/knowledge/ingest/batch")
    }

    /// List or create sessions.
    #[must_use]
    pub fn sessions() -> String {
        format!("{API_V1}/sessions")
    }

    /// Get, close, or purge a single session.
    #[must_use]
    pub fn session(id: &str) -> String {
        let encoded = keryx::url::encode_path_segment(id);
        format!("{API_V1}/sessions/{encoded}")
    }

    /// Conversation history for a session.
    #[must_use]
    pub fn session_history(id: &str) -> String {
        let encoded = keryx::url::encode_path_segment(id);
        format!("{API_V1}/sessions/{encoded}/history")
    }

    /// Replay-faithful export for a session (including archived sessions).
    #[must_use]
    pub fn session_replay(id: &str) -> String {
        let encoded = keryx::url::encode_path_segment(id);
        format!("{API_V1}/sessions/{encoded}/replay")
    }
}

fn default_headers(token: Option<&str>) -> Result<header::HeaderMap, Error> {
    let mut headers = header::HeaderMap::new();

    if let Some(t) = token {
        let value = header::HeaderValue::from_str(&format!("{BEARER_PREFIX}{t}"))
            .map_err(|_source| InvalidTokenSnafu.build())?;
        headers.insert(header::AUTHORIZATION, value);
    }

    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/json"),
    );
    headers.insert(
        header::ACCEPT,
        header::HeaderValue::from_static("application/json"),
    );
    let request_id = koina::ulid::Ulid::new().to_string();
    let request_id_value =
        header::HeaderValue::from_str(&request_id).map_err(|_source| InvalidTokenSnafu.build())?;
    headers.insert(REQUEST_ID_HEADER_NAME, request_id_value);

    Ok(headers)
}

/// Typed HTTP client for the Aletheia gateway.
#[derive(Clone)]
pub struct GatewayClient {
    client: Client,
    base_url: String,
    token: Option<SecretString>,
}

impl std::fmt::Debug for GatewayClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GatewayClient")
            .field("base_url", &self.base_url)
            .field("token", &self.token.as_ref().map(|_| "[REDACTED]"))
            .finish_non_exhaustive()
    }
}

impl GatewayClient {
    /// Create a client for the given gateway base URL.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidToken`] if `token` contains characters that are
    /// illegal in an HTTP header. Returns [`Error::BuildClient`] if reqwest
    /// cannot construct the underlying client.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    pub fn new(base_url: &str, token: Option<String>) -> Result<Self, Error> {
        let headers = default_headers(token.as_deref())?;
        let client = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .default_headers(headers)
            .build()
            .context(BuildClientSnafu)?;

        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_owned(),
            token: token.map(SecretString::from),
        })
    }

    /// The base URL this client connects to.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The current bearer token, if one was supplied at construction.
    #[must_use]
    pub fn token(&self) -> Option<&str> {
        self.token.as_ref().map(SecretString::expose_secret)
    }

    fn url(&self, path: &str) -> String {
        // WHY: path builders always emit an absolute path starting with '/'.
        format!("{}{}", self.base_url, path)
    }

    /// Check whether the server is reachable.
    ///
    /// A `503 Service Unavailable` response still means the server is running;
    /// only transport failures return `false`.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    pub async fn health(&self) -> Result<bool, Error> {
        let resp = self
            .client
            .get(self.url(routes::health()))
            .send()
            .await
            .context(RequestSnafu {
                operation: "health check",
            })?;
        Ok(resp.status().is_success() || resp.status() == StatusCode::SERVICE_UNAVAILABLE)
    }

    /// Fetch the public liveness response.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    pub async fn liveness(&self) -> Result<LivenessResponse, Error> {
        let resp = self
            .client
            .get(self.url(routes::health()))
            .send()
            .await
            .context(RequestSnafu {
                operation: "liveness check",
            })?;
        let resp = Self::check_status(resp, "liveness check").await?;
        resp.json().await.context(DecodeSnafu)
    }

    /// Fetch the operator-only detailed health report.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    pub async fn health_details(&self) -> Result<HealthResponse, Error> {
        let resp = self
            .client
            .get(self.url(&routes::health_details()))
            .send()
            .await
            .context(RequestSnafu {
                operation: "detailed health check",
            })?;
        let resp = Self::check_status(resp, "detailed health check").await?;
        resp.json().await.context(DecodeSnafu)
    }

    /// List sessions, optionally filtered to a single agent.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    pub async fn list_sessions(
        &self,
        nous_id: Option<&str>,
    ) -> Result<ListSessionsResponse, Error> {
        let mut url = self.url(&routes::sessions());
        if let Some(id) = nous_id {
            let encoded = keryx::url::encode_path_segment(id);
            url = format!("{url}?nous_id={encoded}");
        }
        let resp = self.client.get(url).send().await.context(RequestSnafu {
            operation: "list sessions",
        })?;
        let resp = Self::check_status(resp, "list sessions").await?;
        resp.json().await.context(DecodeSnafu)
    }

    /// Fetch conversation history for a session.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    pub async fn history(&self, session_id: &str) -> Result<HistoryResponse, Error> {
        let resp = self
            .client
            .get(self.url(&routes::session_history(session_id)))
            .send()
            .await
            .context(RequestSnafu {
                operation: "session history",
            })?;
        let resp = Self::check_status(resp, "session history").await?;
        resp.json().await.context(DecodeSnafu)
    }

    /// Fetch metadata for a single session.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    pub async fn session(&self, session_id: &str) -> Result<SessionResponse, Error> {
        let resp = self
            .client
            .get(self.url(&routes::session(session_id)))
            .send()
            .await
            .context(RequestSnafu {
                operation: "session details",
            })?;
        let resp = Self::check_status(resp, "session details").await?;
        resp.json().await.context(DecodeSnafu)
    }

    /// Fetch the replay-faithful export for a session.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    pub async fn session_replay(&self, session_id: &str) -> Result<SessionReplayResponse, Error> {
        let resp = self
            .client
            .get(self.url(&routes::session_replay(session_id)))
            .send()
            .await
            .context(RequestSnafu {
                operation: "session replay export",
            })?;
        let resp = Self::check_status(resp, "session replay export").await?;
        resp.json().await.context(DecodeSnafu)
    }

    async fn check_status(resp: Response, operation: &str) -> Result<Response, Error> {
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }

        let body_text = resp
            .text()
            .await
            .unwrap_or_else(|_| "<unable to read response body>".to_owned());
        let (code, message, request_id) = parse_error_body(&body_text, status, operation);

        match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(Error::Auth {
                code,
                message,
                request_id,
            }),
            _ => Err(Error::Server {
                status: status.as_u16(),
                code,
                message,
                request_id,
            }),
        }
    }
}

fn parse_error_body(
    text: &str,
    status: StatusCode,
    operation: &str,
) -> (String, String, Option<String>) {
    if let Ok(envelope) = serde_json::from_str::<PylonErrorEnvelope>(text) {
        (
            envelope.error.code,
            envelope.error.message,
            envelope.error.request_id,
        )
    } else {
        (
            status
                .canonical_reason()
                .map_or_else(|| "error".to_owned(), str::to_lowercase)
                .replace(' ', "_"),
            format!("{operation} failed: {text}"),
            None,
        )
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn base_url_is_normalized() {
        let client = GatewayClient::new("http://127.0.0.1:18789/", None).expect("valid client");
        assert_eq!(client.base_url(), "http://127.0.0.1:18789");
    }

    #[test]
    fn route_session_id_is_encoded() {
        let path = routes::session("has/slash");
        assert!(path.contains("%2F"), "slash must be encoded: {path}");
        assert!(path.starts_with("/api/v1/sessions/"));
    }

    #[test]
    fn route_session_history_id_is_encoded() {
        let path = routes::session_history("a?b#c");
        assert!(path.contains("%3F"), "? must be encoded: {path}");
        assert!(path.contains("%23"), "# must be encoded: {path}");
        assert!(path.ends_with("/history"));
    }

    #[test]
    fn route_session_replay_id_is_encoded() {
        let path = routes::session_replay("a/b c?d#e");
        assert_eq!(path, "/api/v1/sessions/a%2Fb%20c%3Fd%23e/replay");
    }

    #[test]
    fn invalid_token_rejected_at_build_time() {
        let err = GatewayClient::new("http://127.0.0.1:18789", Some("\n".to_owned()))
            .expect_err("newline in token must fail");
        assert!(matches!(err, Error::InvalidToken), "got {err:?}");
    }

    #[test]
    fn parse_error_body_prefers_json_envelope() {
        let text =
            r#"{"error":{"code":"session_not_found","message":"missing","request_id":"req-1"}}"#;
        let (code, message, request_id) = parse_error_body(text, StatusCode::NOT_FOUND, "test");
        assert_eq!(code, "session_not_found");
        assert_eq!(message, "missing");
        assert_eq!(request_id, Some("req-1".to_string()));
    }

    #[test]
    fn parse_error_body_falls_back_to_status_text() {
        let (code, message, request_id) =
            parse_error_body("plain text", StatusCode::BAD_GATEWAY, "test-op");
        assert_eq!(code, "bad_gateway");
        assert!(message.contains("test-op"));
        assert!(message.contains("plain text"));
        assert_eq!(request_id, None);
    }
}
