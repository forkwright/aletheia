use anyhow::{Context, Result};
use reqwest::{Client, Response, StatusCode};

use super::types::*;

/// HTTP client for the Aletheia gateway REST API.
#[derive(Debug, Clone)]
pub struct ApiClient {
    client: Client,
    base_url: String,
    token: Option<String>,
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

    pub fn set_token(&mut self, token: String) {
        self.token = Some(token);
    }

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

    pub async fn health(&self) -> Result<bool> {
        let resp = self
            .client
            .get(self.url("/health"))
            .send()
            .await
            .context("health check failed")?;
        Ok(resp.status().is_success())
    }

    // --- Auth ---

    pub async fn auth_mode(&self) -> Result<AuthMode> {
        let resp = self
            .request(reqwest::Method::GET, "/api/auth/mode")
            .send()
            .await
            .context("auth mode check failed")?;
        Ok(resp.json().await?)
    }

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

    pub async fn agents(&self) -> Result<Vec<Agent>> {
        let resp = self
            .request(reqwest::Method::GET, "/api/agents")
            .send()
            .await
            .context("failed to load agents")?;
        Self::check_auth(&resp)?;
        resp.error_for_status_ref()
            .context("agents request failed")?;
        let wrapper: AgentsResponse = resp.json().await?;
        Ok(wrapper.agents)
    }

    // --- Sessions ---

    pub async fn sessions(&self, nous_id: &str) -> Result<Vec<Session>> {
        let resp = self
            .request(
                reqwest::Method::GET,
                &format!("/api/sessions?nousId={nous_id}"),
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

    pub async fn history(&self, session_id: &str) -> Result<Vec<HistoryMessage>> {
        let resp = self
            .request(
                reqwest::Method::GET,
                &format!("/api/sessions/{session_id}/history"),
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

    pub async fn create_session(&self, nous_id: &str, session_key: &str) -> Result<Session> {
        let resp = self
            .request(reqwest::Method::POST, "/api/sessions")
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

    pub async fn archive_session(&self, session_id: &str) -> Result<()> {
        self.request(
            reqwest::Method::POST,
            &format!("/api/sessions/{session_id}/archive"),
        )
        .send()
        .await
        .context("failed to archive session")?
        .error_for_status_ref()
        .context("archive request failed")?;
        Ok(())
    }

    // --- Turns ---

    pub async fn abort_turn(&self, turn_id: &str) -> Result<()> {
        self.request(
            reqwest::Method::POST,
            &format!("/api/turns/{turn_id}/abort"),
        )
        .send()
        .await
        .context("failed to abort turn")?
        .error_for_status_ref()
        .context("abort request failed")?;
        Ok(())
    }

    pub async fn approve_tool(&self, turn_id: &str, tool_id: &str) -> Result<()> {
        self.request(
            reqwest::Method::POST,
            &format!("/api/turns/{turn_id}/tools/{tool_id}/approve"),
        )
        .send()
        .await
        .context("failed to approve tool")?
        .error_for_status_ref()
        .context("approve request failed")?;
        Ok(())
    }

    pub async fn deny_tool(&self, turn_id: &str, tool_id: &str) -> Result<()> {
        self.request(
            reqwest::Method::POST,
            &format!("/api/turns/{turn_id}/tools/{tool_id}/deny"),
        )
        .send()
        .await
        .context("failed to deny tool")?
        .error_for_status_ref()
        .context("deny request failed")?;
        Ok(())
    }

    // --- Plans ---

    pub async fn approve_plan(&self, plan_id: &str) -> Result<()> {
        self.request(
            reqwest::Method::POST,
            &format!("/api/plans/{plan_id}/approve"),
        )
        .send()
        .await
        .context("failed to approve plan")?
        .error_for_status_ref()
        .context("plan approve failed")?;
        Ok(())
    }

    pub async fn cancel_plan(&self, plan_id: &str) -> Result<()> {
        self.request(
            reqwest::Method::POST,
            &format!("/api/plans/{plan_id}/cancel"),
        )
        .send()
        .await
        .context("failed to cancel plan")?
        .error_for_status_ref()
        .context("plan cancel failed")?;
        Ok(())
    }

    // --- Costs ---

    pub async fn today_cost_cents(&self) -> Result<u32> {
        let resp = self
            .request(reqwest::Method::GET, "/api/costs/daily")
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

    pub async fn compact(&self, session_id: &str) -> Result<()> {
        self.request(
            reqwest::Method::POST,
            &format!("/api/sessions/{session_id}/distill"),
        )
        .send()
        .await
        .context("failed to trigger distillation")?
        .error_for_status_ref()
        .context("distillation request failed")?;
        Ok(())
    }

    // --- Memory recall ---

    pub async fn recall(&self, nous_id: &str, query: &str) -> Result<String> {
        let resp = self
            .request(
                reqwest::Method::GET,
                &format!("/api/nous/{nous_id}/recall"),
            )
            .query(&[("q", query)])
            .send()
            .await
            .context("failed to recall memory")?;
        resp.error_for_status_ref()
            .context("recall request failed")?;
        Ok(resp.text().await?)
    }

    // --- Message queue (mid-turn) ---

    pub async fn queue_message(&self, session_id: &str, text: &str) -> Result<()> {
        self.request(
            reqwest::Method::POST,
            &format!("/api/sessions/{session_id}/queue"),
        )
        .json(&serde_json::json!({ "text": text }))
        .send()
        .await
        .context("failed to queue message")?
        .error_for_status_ref()
        .context("queue request failed")?;
        Ok(())
    }

    /// Check a response for 401 and return a typed error.
    fn check_auth(resp: &Response) -> Result<()> {
        if resp.status() == StatusCode::UNAUTHORIZED {
            anyhow::bail!("UNAUTHORIZED: token expired or invalid");
        }
        Ok(())
    }

    /// Returns true if the error message indicates an auth failure (401).
    pub fn is_auth_error(err: &anyhow::Error) -> bool {
        format!("{err}").contains("UNAUTHORIZED")
    }

    /// Get the raw reqwest client for SSE/streaming (they manage their own connections)
    pub fn raw_client(&self) -> &Client {
        &self.client
    }
}
