use anyhow::{Context, Result};
use reqwest::{Client, Response, StatusCode};

use super::types::*;

/// Percent-encode a value for use in a URL path segment.
fn encode_path(s: &str) -> String {
    let mut encoded = String::with_capacity(s.len());
    for byte in s.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(*byte as char);
            }
            _ => {
                use std::fmt::Write;
                let _ = write!(encoded, "%{byte:02X}");
            }
        }
    }
    encoded
}

/// HTTP client for the Aletheia gateway REST API.
#[derive(Clone)]
pub struct ApiClient {
    client: Client,
    base_url: String,
    token: Option<String>,
}

impl std::fmt::Debug for ApiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiClient")
            .field("base_url", &self.base_url)
            .field("token", &self.token.as_ref().map(|_| "[REDACTED]"))
            .finish_non_exhaustive()
    }
}

impl ApiClient {
    pub fn new(base_url: &str, token: Option<String>) -> Result<Self> {
        let client = Client::builder()
            .cookie_store(true)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("failed to create HTTP client")?;

        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            token,
        })
    }

    #[expect(dead_code, reason = "reserved for future login flow")]
    pub fn set_token(&mut self, token: String) {
        self.token = Some(token);
    }

    #[expect(dead_code, reason = "reserved for future diagnostics / display")]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let mut req = self.client.request(method, self.url(path));
        if let Some(ref token) = self.token {
            req = req.bearer_auth(token);
        }
        req
    }

    // --- Health ---

    #[tracing::instrument(skip(self))]
    pub async fn health(&self) -> Result<bool> {
        let resp = self
            .client
            .get(self.url("/api/health"))
            .send()
            .await
            .context("health check failed")?;
        Ok(resp.status().is_success())
    }

    // --- Auth ---

    #[tracing::instrument(skip(self))]
    pub async fn auth_mode(&self) -> Result<AuthMode> {
        let resp = self
            .request(reqwest::Method::GET, "/api/auth/mode")
            .send()
            .await
            .context("auth mode check failed")?;
        Ok(resp.json().await?)
    }

    #[expect(dead_code, reason = "reserved for future interactive login flow")]
    #[tracing::instrument(skip(self, password))]
    pub async fn login(&self, username: &str, password: &str) -> Result<LoginResponse> {
        let resp = self
            .client
            .post(self.url("/api/auth/login"))
            .json(&serde_json::json!({ "username": username, "password": password }))
            .send()
            .await
            .context("login failed")?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            anyhow::bail!("invalid credentials");
        }

        resp.error_for_status_ref()
            .context("login request failed")?;
        Ok(resp.json().await?)
    }

    // --- Agents ---

    #[tracing::instrument(skip(self))]
    pub async fn agents(&self) -> Result<Vec<Agent>> {
        let resp = self
            .request(reqwest::Method::GET, "/api/v1/nous")
            .send()
            .await
            .context("failed to load agents")?;
        Self::check_auth(&resp)?;
        resp.error_for_status_ref()
            .context("agents request failed")?;
        let wrapper: AgentsResponse = resp.json().await?;
        Ok(wrapper.nous)
    }

    // --- Sessions ---

    #[tracing::instrument(skip(self))]
    pub async fn sessions(&self, nous_id: &str) -> Result<Vec<Session>> {
        let encoded = encode_path(nous_id);
        let resp = self
            .request(
                reqwest::Method::GET,
                &format!("/api/v1/sessions?nousId={encoded}"),
            )
            .send()
            .await
            .context("failed to load sessions")?;
        Self::check_auth(&resp)?;
        resp.error_for_status_ref()
            .context("sessions request failed")?;
        let wrapper: SessionsResponse = resp.json().await?;
        Ok(wrapper.sessions)
    }

    #[tracing::instrument(skip(self))]
    pub async fn history(&self, session_id: &str) -> Result<Vec<HistoryMessage>> {
        let encoded = encode_path(session_id);
        let resp = self
            .request(
                reqwest::Method::GET,
                &format!("/api/v1/sessions/{encoded}/history"),
            )
            .send()
            .await
            .context("failed to load history")?;
        Self::check_auth(&resp)?;
        resp.error_for_status_ref()
            .context("history request failed")?;
        let wrapper: HistoryResponse = resp.json().await?;
        Ok(wrapper.messages)
    }

    #[tracing::instrument(skip(self))]
    pub async fn create_session(&self, nous_id: &str, session_key: &str) -> Result<Session> {
        let resp = self
            .request(reqwest::Method::POST, "/api/v1/sessions")
            .json(&serde_json::json!({
                "nousId": nous_id,
                "sessionKey": session_key,
            }))
            .send()
            .await
            .context("failed to create session")?;
        Self::check_auth(&resp)?;
        resp.error_for_status_ref()
            .context("create session request failed")?;
        Ok(resp.json().await?)
    }

    #[tracing::instrument(skip(self))]
    pub async fn archive_session(&self, session_id: &str) -> Result<()> {
        let encoded = encode_path(session_id);
        self.request(
            reqwest::Method::POST,
            &format!("/api/v1/sessions/{encoded}/archive"),
        )
        .send()
        .await
        .context("failed to archive session")?
        .error_for_status_ref()
        .context("archive request failed")?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn unarchive_session(&self, session_id: &str) -> Result<()> {
        let encoded = encode_path(session_id);
        self.request(
            reqwest::Method::POST,
            &format!("/api/v1/sessions/{encoded}/unarchive"),
        )
        .send()
        .await
        .context("failed to unarchive session")?
        .error_for_status_ref()
        .context("unarchive request failed")?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn rename_session(&self, session_id: &str, name: &str) -> Result<()> {
        let encoded = encode_path(session_id);
        self.request(
            reqwest::Method::PUT,
            &format!("/api/v1/sessions/{encoded}/name"),
        )
        .json(&serde_json::json!({ "name": name }))
        .send()
        .await
        .context("failed to rename session")?
        .error_for_status_ref()
        .context("rename request failed")?;
        Ok(())
    }

    // --- Turns ---

    #[expect(dead_code, reason = "reserved for turn abort keybinding")]
    #[tracing::instrument(skip(self))]
    pub async fn abort_turn(&self, turn_id: &str) -> Result<()> {
        let encoded = encode_path(turn_id);
        self.request(
            reqwest::Method::POST,
            &format!("/api/v1/turns/{encoded}/abort"),
        )
        .send()
        .await
        .context("failed to abort turn")?
        .error_for_status_ref()
        .context("abort request failed")?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn approve_tool(&self, turn_id: &str, tool_id: &str) -> Result<()> {
        let t = encode_path(turn_id);
        let d = encode_path(tool_id);
        self.request(
            reqwest::Method::POST,
            &format!("/api/v1/turns/{t}/tools/{d}/approve"),
        )
        .send()
        .await
        .context("failed to approve tool")?
        .error_for_status_ref()
        .context("approve request failed")?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn deny_tool(&self, turn_id: &str, tool_id: &str) -> Result<()> {
        let t = encode_path(turn_id);
        let d = encode_path(tool_id);
        self.request(
            reqwest::Method::POST,
            &format!("/api/v1/turns/{t}/tools/{d}/deny"),
        )
        .send()
        .await
        .context("failed to deny tool")?
        .error_for_status_ref()
        .context("deny request failed")?;
        Ok(())
    }

    // --- Plans ---

    #[tracing::instrument(skip(self))]
    pub async fn approve_plan(&self, plan_id: &str) -> Result<()> {
        let encoded = encode_path(plan_id);
        self.request(
            reqwest::Method::POST,
            &format!("/api/v1/plans/{encoded}/approve"),
        )
        .send()
        .await
        .context("failed to approve plan")?
        .error_for_status_ref()
        .context("plan approve failed")?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn cancel_plan(&self, plan_id: &str) -> Result<()> {
        let encoded = encode_path(plan_id);
        self.request(
            reqwest::Method::POST,
            &format!("/api/v1/plans/{encoded}/cancel"),
        )
        .send()
        .await
        .context("failed to cancel plan")?
        .error_for_status_ref()
        .context("plan cancel failed")?;
        Ok(())
    }

    // --- Costs ---

    #[tracing::instrument(skip(self))]
    pub async fn today_cost_cents(&self) -> Result<u32> {
        let resp = self
            .request(reqwest::Method::GET, "/api/v1/costs/daily")
            .send()
            .await
            .context("failed to load costs")?;
        resp.error_for_status_ref()
            .context("costs request failed")?;
        let daily: DailyResponse = resp.json().await?;
        // Get today's entry (last in list)
        let today_cost = daily.daily.last().map(|d| d.cost).unwrap_or(0.0);
        // Convert dollars to cents
        Ok((today_cost * 100.0) as u32)
    }

    // --- Distillation ---

    #[tracing::instrument(skip(self))]
    pub async fn compact(&self, session_id: &str) -> Result<()> {
        let encoded = encode_path(session_id);
        self.request(
            reqwest::Method::POST,
            &format!("/api/v1/sessions/{encoded}/distill"),
        )
        .send()
        .await
        .context("failed to trigger distillation")?
        .error_for_status_ref()
        .context("distillation request failed")?;
        Ok(())
    }

    // --- Memory recall ---

    #[tracing::instrument(skip(self))]
    pub async fn recall(&self, nous_id: &str, query: &str) -> Result<String> {
        let encoded = encode_path(nous_id);
        let resp = self
            .request(
                reqwest::Method::GET,
                &format!("/api/v1/nous/{encoded}/recall"),
            )
            .query(&[("q", query)])
            .send()
            .await
            .context("failed to recall memory")?;
        resp.error_for_status_ref()
            .context("recall request failed")?;
        Ok(resp.text().await?)
    }

    // --- Config ---

    #[tracing::instrument(skip(self))]
    pub async fn config(&self) -> Result<serde_json::Value> {
        let resp = self
            .request(reqwest::Method::GET, "/api/v1/config")
            .send()
            .await
            .context("failed to load config")?;
        Self::check_auth(&resp)?;
        resp.error_for_status_ref()
            .context("config request failed")?;
        Ok(resp.json().await?)
    }

    #[tracing::instrument(skip(self, data))]
    pub async fn update_config_section(
        &self,
        section: &str,
        data: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let encoded = encode_path(section);
        let resp = self
            .request(reqwest::Method::PUT, &format!("/api/v1/config/{encoded}"))
            .json(data)
            .send()
            .await
            .context("failed to update config")?;
        Self::check_auth(&resp)?;
        resp.error_for_status_ref()
            .context("config update failed")?;
        Ok(resp.json().await?)
    }

    // --- Message queue (mid-turn) ---

    #[tracing::instrument(skip(self, text))]
    pub async fn queue_message(&self, session_id: &str, text: &str) -> Result<()> {
        let encoded = encode_path(session_id);
        self.request(
            reqwest::Method::POST,
            &format!("/api/v1/sessions/{encoded}/queue"),
        )
        .json(&serde_json::json!({ "text": text }))
        .send()
        .await
        .context("failed to queue message")?
        .error_for_status_ref()
        .context("queue request failed")?;
        Ok(())
    }

    fn check_auth(resp: &Response) -> Result<()> {
        if resp.status() == StatusCode::UNAUTHORIZED || resp.status() == StatusCode::FORBIDDEN {
            return Err(AuthError.into());
        }
        Ok(())
    }

    #[expect(
        dead_code,
        reason = "reserved for auth-aware error display in update handlers"
    )]
    pub fn is_auth_error(err: &anyhow::Error) -> bool {
        err.downcast_ref::<AuthError>().is_some()
    }

    #[expect(dead_code, reason = "reserved for future SSE connection management")]
    /// Get the raw reqwest client for SSE/streaming (they manage their own connections)
    pub fn raw_client(&self) -> &Client {
        &self.client
    }
}

#[derive(Debug)]
struct AuthError;

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "authentication failed: token expired or invalid")
    }
}

impl std::error::Error for AuthError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_path_clean_string() {
        assert_eq!(encode_path("hello-world"), "hello-world");
        assert_eq!(encode_path("abc123"), "abc123");
    }

    #[test]
    fn encode_path_special_chars() {
        assert_eq!(encode_path("a/b"), "a%2Fb");
        assert_eq!(encode_path("hello world"), "hello%20world");
        assert_eq!(encode_path("id=1&x=2"), "id%3D1%26x%3D2");
    }
}
