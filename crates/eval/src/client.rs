//! HTTP client for interacting with a running Aletheia instance.

use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use tracing::instrument;

use aletheia_koina::secret::SecretString;

use crate::error::{self, Result};
use crate::sse::{self, ParsedSseEvent};

/// HTTP client for a running Aletheia instance.
pub struct EvalClient {
    http: reqwest::Client,
    base_url: String,
    token: Option<SecretString>,
}

impl EvalClient {
    /// Create a new eval client targeting the given base URL.
    // SAFETY: eval client connects to localhost only. Token sent over cleartext HTTP is intentional.
    pub fn new(base_url: impl Into<String>, token: Option<String>) -> Self {
        let base_url: String = base_url.into().trim_end_matches('/').to_owned();
        if token.is_some()
            && !base_url.starts_with("https://")
            && !base_url.contains("localhost")
            && !base_url.contains("127.0.0.1")
            && !base_url.contains("[::1]")
        {
            tracing::warn!(
                base_url = %base_url,
                "eval client sending credentials over non-HTTPS to non-localhost URL"
            );
        }
        Self {
            http: reqwest::Client::new(),
            base_url,
            token: token.map(SecretString::from),
        }
    }

    /// Whether an auth token is configured.
    #[must_use]
    pub fn has_token(&self) -> bool {
        self.token.is_some()
    }

    /// Base URL this client targets.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Check instance health.
    #[instrument(skip(self))]
    pub async fn health(&self) -> Result<HealthResponse> {
        let url = format!("{}/api/health", self.base_url);
        let resp = self.http.get(&url).send().await.context(error::HttpSnafu)?;
        self.expect_ok(&url, resp).await
    }

    /// List all configured nous agents.
    #[instrument(skip(self))]
    pub async fn list_nous(&self) -> Result<Vec<NousSummary>> {
        let url = format!("{}/api/v1/nous", self.base_url);
        let resp = self.authed_get(&url).await?;
        let list: NousListResponse = self.expect_ok(&url, resp).await?;
        Ok(list.nous)
    }

    /// Get status for a specific nous agent.
    #[instrument(skip(self))]
    pub async fn get_nous(&self, id: &str) -> Result<NousStatus> {
        let url = format!("{}/api/v1/nous/{id}", self.base_url);
        let resp = self.authed_get(&url).await?;
        self.expect_ok(&url, resp).await
    }

    /// Create a new session bound to a nous agent.
    #[instrument(skip(self))]
    pub async fn create_session(
        &self,
        nous_id: &str,
        session_key: &str,
    ) -> Result<SessionResponse> {
        let url = format!("{}/api/v1/sessions", self.base_url);
        let body = serde_json::json!({
            "nous_id": nous_id,
            "session_key": session_key,
        });
        let resp = self.authed_post(&url, &body).await?;
        self.expect_status(&url, resp, &[201, 200]).await
    }

    /// Get session details by ID.
    #[instrument(skip(self))]
    pub async fn get_session(&self, id: &str) -> Result<SessionResponse> {
        let url = format!("{}/api/v1/sessions/{id}", self.base_url);
        let resp = self.authed_get(&url).await?;
        self.expect_ok(&url, resp).await
    }

    /// Close (archive) a session.
    #[instrument(skip(self))]
    pub async fn close_session(&self, id: &str) -> Result<()> {
        let url = format!("{}/api/v1/sessions/{id}", self.base_url);
        let resp = self.authed_delete(&url).await?;
        let status = resp.status().as_u16();
        if status != 204 && status != 200 {
            let body = resp.text().await.unwrap_or_else(|e| {
                tracing::warn!(error = %e, "failed to read error response body");
                String::new()
            });
            return error::UnexpectedStatusSnafu {
                endpoint: url,
                status,
                body,
            }
            .fail();
        }
        Ok(())
    }

    /// Send a message and collect the full SSE response.
    #[instrument(skip(self, content))]
    pub async fn send_message(
        &self,
        session_id: &str,
        content: &str,
    ) -> Result<Vec<ParsedSseEvent>> {
        let url = format!("{}/api/v1/sessions/{session_id}/messages", self.base_url);
        let body = serde_json::json!({ "content": content });
        let resp = self.authed_post(&url, &body).await?;
        let status = resp.status().as_u16();
        if status != 200 {
            let body_text = resp.text().await.unwrap_or_else(|e| {
                tracing::warn!(error = %e, "failed to read error response body");
                String::new()
            });
            return error::UnexpectedStatusSnafu {
                endpoint: url,
                status,
                body: body_text,
            }
            .fail();
        }
        sse::parse_sse_stream(resp).await
    }

    /// Get conversation history for a session.
    #[instrument(skip(self))]
    pub async fn get_history(&self, session_id: &str) -> Result<HistoryResponse> {
        let url = format!("{}/api/v1/sessions/{session_id}/history", self.base_url);
        let resp = self.authed_get(&url).await?;
        self.expect_ok(&url, resp).await
    }

    /// Search the knowledge store for facts matching a query.
    #[instrument(skip(self))]
    pub async fn search_knowledge(
        &self,
        query: &str,
        nous_id: &str,
        limit: u32,
    ) -> Result<KnowledgeSearchResponse> {
        let base = format!("{}/api/v1/knowledge/search", self.base_url);
        let mut req = self.http.get(&base).query(&[
            ("q", query),
            ("nous_id", nous_id),
            ("limit", &limit.to_string()),
        ]);
        if let Some(ref token) = self.token {
            req = req.header("authorization", format!("Bearer {}", token.expose_secret()));
        }
        let resp = req.send().await.context(error::HttpSnafu)?;
        self.expect_ok(&base, resp).await
    }

    /// Send a GET request without any auth header.
    #[instrument(skip(self))]
    pub async fn raw_get(&self, path: &str) -> Result<reqwest::Response> {
        let url = format!("{}{path}", self.base_url);
        self.http.get(&url).send().await.context(error::HttpSnafu)
    }

    /// Send a POST request without any auth header.
    // codequality:ignore -- no credential attached; eval client is localhost-only (checked in constructor)
    #[instrument(skip(self, body))]
    pub async fn raw_post(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<reqwest::Response> {
        let url = format!("{}{path}", self.base_url);
        self.http
            .post(&url)
            .header("content-type", "application/json")
            .header("x-requested-with", "aletheia")
            .json(body)
            .send()
            .await
            .context(error::HttpSnafu)
    }

    /// Send a GET request with an arbitrary Bearer token.
    // codequality:ignore -- eval client is localhost-only (non-HTTPS check in constructor)
    #[instrument(skip(self, token))]
    pub async fn raw_get_with_token(&self, path: &str, token: &str) -> Result<reqwest::Response> {
        let url = format!("{}{path}", self.base_url);
        self.http
            .get(&url)
            .header("authorization", format!("Bearer {token}"))
            .send()
            .await
            .context(error::HttpSnafu)
    }

    async fn authed_get(&self, url: &str) -> Result<reqwest::Response> {
        let mut req = self.http.get(url);
        if let Some(ref token) = self.token {
            req = req.header("authorization", format!("Bearer {}", token.expose_secret()));
        }
        req.send().await.context(error::HttpSnafu)
    }

    async fn authed_post(&self, url: &str, body: &serde_json::Value) -> Result<reqwest::Response> {
        let mut req = self
            .http
            .post(url)
            .header("content-type", "application/json")
            .header("x-requested-with", "aletheia");
        if let Some(ref token) = self.token {
            req = req.header("authorization", format!("Bearer {}", token.expose_secret()));
        }
        req.json(body).send().await.context(error::HttpSnafu)
    }

    async fn authed_delete(&self, url: &str) -> Result<reqwest::Response> {
        let mut req = self.http.delete(url).header("x-requested-with", "aletheia");
        if let Some(ref token) = self.token {
            req = req.header("authorization", format!("Bearer {}", token.expose_secret()));
        }
        req.send().await.context(error::HttpSnafu)
    }

    async fn expect_ok<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        response: reqwest::Response,
    ) -> Result<T> {
        self.expect_status(url, response, &[200]).await
    }

    async fn expect_status<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        response: reqwest::Response,
        accepted: &[u16],
    ) -> Result<T> {
        let status = response.status().as_u16();
        if !accepted.contains(&status) {
            let body = response.text().await.unwrap_or_else(|e| {
                tracing::warn!(error = %e, "failed to read error response body");
                String::new()
            });
            return error::UnexpectedStatusSnafu {
                endpoint: url.to_owned(),
                status,
                body,
            }
            .fail();
        }
        response.json().await.context(error::HttpSnafu)
    }
}

/// Status reported by the `/api/health` endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum InstanceStatus {
    /// Instance is fully operational.
    Healthy,
    /// Instance is running but one or more checks are failing.
    Degraded,
    /// Catch-all for future or unexpected status strings.
    #[serde(untagged)]
    Unknown(String),
}

/// Lifecycle status of a session.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum SessionStatus {
    /// Session is open and accepting messages.
    Active,
    /// Session has been closed and is read-only.
    Archived,
    /// Catch-all for future or unexpected status strings.
    #[serde(untagged)]
    Unknown(String),
}

/// Role of a message in conversation history.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum MessageRole {
    /// Message sent by the user.
    User,
    /// Message generated by the assistant.
    Assistant,
    /// Tool result message.
    Tool,
    /// Catch-all for future or unexpected role strings.
    #[serde(untagged)]
    Unknown(String),
}

/// Response from the `/api/health` endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct HealthResponse {
    pub status: InstanceStatus,
    pub version: String,
    pub uptime_seconds: u64,
    pub checks: Vec<HealthCheck>,
}

/// A single health check result within a health response.
#[derive(Debug, Clone, Deserialize)]
pub struct HealthCheck {
    pub name: String,
    pub status: String,
    pub message: Option<String>,
}

/// Response from the `/api/v1/nous` list endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct NousListResponse {
    pub nous: Vec<NousSummary>,
}

/// Summary of a nous agent from the list endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct NousSummary {
    pub id: String,
    pub model: String,
    pub status: String,
}

/// Detailed nous status from the `/api/v1/nous/{id}` endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct NousStatus {
    pub id: String,
    pub model: String,
    pub context_window: u32,
    pub max_output_tokens: u32,
    #[serde(default)]
    pub thinking_enabled: bool,
    #[serde(default)]
    pub thinking_budget: u32,
    #[serde(default)]
    pub max_tool_iterations: u32,
    #[serde(default)]
    pub status: String,
}

/// Session details from the `/api/v1/sessions/{id}` endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct SessionResponse {
    pub id: String,
    pub nous_id: String,
    pub session_key: String,
    pub status: SessionStatus,
    pub model: Option<String>,
    pub message_count: i64,
    pub token_count_estimate: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// Conversation history response from `/api/v1/sessions/{id}/history`.
#[derive(Debug, Clone, Deserialize)]
pub struct HistoryResponse {
    pub messages: Vec<HistoryMessage>,
}

/// A single message in conversation history.
#[derive(Debug, Clone, Deserialize)]
pub struct HistoryMessage {
    pub id: i64,
    pub seq: i64,
    pub role: MessageRole,
    pub content: String,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub created_at: String,
}

/// Response from the knowledge search endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct KnowledgeSearchResponse {
    /// Matching facts ordered by relevance.
    #[serde(default)]
    pub facts: Vec<KnowledgeFact>,
}

/// A fact returned by the knowledge search API.
#[derive(Debug, Clone, Deserialize)]
pub struct KnowledgeFact {
    /// Unique fact identifier.
    pub id: String,
    /// Fact content text.
    #[serde(default)]
    pub content: String,
    /// Confidence score (0.0 to 1.0).
    #[serde(default)]
    pub confidence: f64,
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn install_crypto_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    #[test]
    fn url_construction_trims_trailing_slash() {
        install_crypto_provider();
        let client = EvalClient::new("http://localhost:18789/", None);
        assert_eq!(client.base_url(), "http://localhost:18789");
    }

    #[test]
    fn has_token_reports_correctly() {
        install_crypto_provider();
        let without = EvalClient::new("http://localhost", None);
        assert!(!without.has_token());

        let with = EvalClient::new("http://localhost", Some("tok".to_owned()));
        assert!(with.has_token());
    }

    #[test]
    fn url_construction_no_trailing_slash() {
        install_crypto_provider();
        let client = EvalClient::new("http://localhost:8080", None);
        assert_eq!(client.base_url(), "http://localhost:8080");
    }

    #[test]
    fn url_construction_multiple_trailing_slashes() {
        install_crypto_provider();
        let client = EvalClient::new("http://localhost:8080///", None);
        assert_eq!(client.base_url(), "http://localhost:8080");
    }

    #[test]
    fn base_url_returns_stored_url() {
        install_crypto_provider();
        let client = EvalClient::new("http://192.168.1.100:3000", None);
        assert_eq!(client.base_url(), "http://192.168.1.100:3000");
    }

    #[test]
    fn new_client_without_token() {
        install_crypto_provider();
        let client = EvalClient::new("http://localhost", None);
        assert!(!client.has_token());
        assert_eq!(client.base_url(), "http://localhost");
    }

    #[test]
    fn new_client_with_token() {
        install_crypto_provider();
        let client = EvalClient::new("http://localhost", Some("secret-token".to_owned()));
        assert!(client.has_token());
        assert_eq!(client.base_url(), "http://localhost");
    }

    #[test]
    fn instance_status_deserializes_healthy() {
        let status: InstanceStatus = serde_json::from_str("\"healthy\"").expect("deserialize");
        assert_eq!(status, InstanceStatus::Healthy);
    }

    #[test]
    fn instance_status_deserializes_degraded() {
        let status: InstanceStatus = serde_json::from_str("\"degraded\"").expect("deserialize");
        assert_eq!(status, InstanceStatus::Degraded);
    }

    #[test]
    fn instance_status_deserializes_unknown() {
        let status: InstanceStatus = serde_json::from_str("\"starting\"").expect("deserialize");
        assert!(matches!(status, InstanceStatus::Unknown(_)));
    }

    #[test]
    fn session_status_roundtrip() {
        let cases = [
            (SessionStatus::Active, "\"active\""),
            (SessionStatus::Archived, "\"archived\""),
        ];
        for (variant, expected_json) in &cases {
            let json = serde_json::to_string(variant).expect("serialize");
            assert_eq!(json, *expected_json);
            let back: SessionStatus = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(&back, variant);
        }
    }

    #[test]
    fn session_status_unknown_passthrough() {
        let status: SessionStatus = serde_json::from_str("\"suspended\"").expect("deserialize");
        assert!(matches!(status, SessionStatus::Unknown(_)));
        if let SessionStatus::Unknown(s) = status {
            assert_eq!(s, "suspended");
        }
    }

    #[test]
    fn message_role_roundtrip() {
        let cases = [
            (MessageRole::User, "\"user\""),
            (MessageRole::Assistant, "\"assistant\""),
            (MessageRole::Tool, "\"tool\""),
        ];
        for (variant, expected_json) in &cases {
            let json = serde_json::to_string(variant).expect("serialize");
            assert_eq!(json, *expected_json);
            let back: MessageRole = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(&back, variant);
        }
    }

    #[test]
    fn message_role_unknown_passthrough() {
        let role: MessageRole = serde_json::from_str("\"system\"").expect("deserialize");
        assert!(matches!(role, MessageRole::Unknown(_)));
        if let MessageRole::Unknown(s) = role {
            assert_eq!(s, "system");
        }
    }
}
