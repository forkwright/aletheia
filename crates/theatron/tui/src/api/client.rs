use reqwest::{Client, Response, StatusCode, header};
use snafu::prelude::*;

use aletheia_koina::secret::SecretString;

use super::error::{ApiError, AuthSnafu, HttpSnafu, Result, ServerSnafu};
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

/// Build the shared reqwest client used by all API paths (REST, streaming, SSE).
///
/// Default headers set here apply to every request made with this client:
/// - Authorization: Bearer <token> (if a token is configured)
/// - x-requested-with: aletheia (CSRF mitigation: server rejects absent header)
/// - Content-Type: application/json
/// - Accept: application/json (SSE callers override this per-request to text/event-stream)
///
/// WHY: A single client shares the connection pool and TLS session cache across all
/// request types. Building one per call (as the previous streaming/SSE code did) creates
/// a new pool per turn and leaks connections until they time out.
pub(crate) fn build_http_client(token: Option<&str>) -> Result<Client> {
    let mut headers = header::HeaderMap::new();

    if let Some(t) = token {
        let auth_value = header::HeaderValue::from_str(&format!("Bearer {t}"))
            .map_err(|_invalid| ApiError::InvalidToken)?;
        headers.insert(header::AUTHORIZATION, auth_value);
    }

    headers.insert(
        "x-requested-with",
        header::HeaderValue::from_static("aletheia"),
    );
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/json"),
    );
    headers.insert(
        header::ACCEPT,
        header::HeaderValue::from_static("application/json"),
    );

    Client::builder()
        .cookie_store(true)
        .timeout(std::time::Duration::from_secs(30))
        .default_headers(headers)
        .build()
        .context(HttpSnafu {
            operation: "build HTTP client",
        })
}

/// HTTP client for the Aletheia gateway REST API.
#[derive(Clone)]
pub struct ApiClient {
    client: Client,
    base_url: String,
    token: Option<SecretString>,
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
    /// Create a new API client for the given gateway URL.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::InvalidToken`] if `token` contains characters invalid in HTTP headers.
    /// Returns [`ApiError::Http`] if the HTTP client cannot be constructed.
    pub fn new(base_url: &str, token: Option<String>) -> Result<Self> {
        let client = build_http_client(token.as_deref())?;

        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            token: token.map(SecretString::from),
        })
    }

    #[expect(dead_code, reason = "reserved for future login flow")]
    pub fn set_token(&mut self, token: String) {
        self.token = Some(SecretString::from(token));
    }

    #[expect(dead_code, reason = "reserved for future diagnostics / display")]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn token(&self) -> Option<&str> {
        self.token.as_ref().map(SecretString::expose_secret)
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        // NOTE: no per-request header injection: token is fixed at construction
        self.client.request(method, self.url(path))
    }

    #[tracing::instrument(skip(self))]
    pub async fn health(&self) -> Result<bool> {
        // WHY: check reachability, not health status. A 503 (unhealthy)
        // means the server IS running but has degraded checks: still usable.
        let resp = self.client.get(self.url("/api/health")).send().await;
        Ok(resp.is_ok())
    }

    #[tracing::instrument(skip(self))]
    pub async fn auth_mode(&self) -> Result<AuthMode> {
        let resp = self
            .request(reqwest::Method::GET, "/api/auth/mode")
            .send()
            .await
            .context(HttpSnafu {
                operation: "auth mode check",
            })?;
        resp.json().await.context(HttpSnafu {
            operation: "auth mode response",
        })
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
            .context(HttpSnafu { operation: "login" })?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            return AuthSnafu.fail();
        }

        let resp = Self::check_status(resp, "login request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "login response",
        })
    }

    #[tracing::instrument(skip(self))]
    pub async fn agents(&self) -> Result<Vec<Agent>> {
        let resp = self
            .request(reqwest::Method::GET, "/api/v1/nous")
            .send()
            .await
            .context(HttpSnafu {
                operation: "load agents",
            })?;
        Self::check_auth(&resp)?;
        let resp = Self::check_status(resp, "agents request").await?;
        let wrapper: AgentsResponse = resp.json().await.context(HttpSnafu {
            operation: "agents response",
        })?;
        Ok(wrapper.nous)
    }

    /// Fetch all sessions for an agent.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::Http`] if the request fails or the response cannot be decoded.
    /// Returns [`ApiError::Auth`] if the server rejects the authentication token.
    /// Returns [`ApiError::Server`] if the server returns a non-success status.
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
            .context(HttpSnafu {
                operation: "load sessions",
            })?;
        Self::check_auth(&resp)?;
        let resp = Self::check_status(resp, "sessions request").await?;
        let wrapper: SessionsResponse = resp.json().await.context(HttpSnafu {
            operation: "sessions response",
        })?;
        Ok(wrapper.sessions)
    }

    /// Fetch message history for a session.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::Http`] if the request fails or the response cannot be decoded.
    /// Returns [`ApiError::Auth`] if the server rejects the authentication token.
    /// Returns [`ApiError::Server`] if the server returns a non-success status.
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
            .context(HttpSnafu {
                operation: "load history",
            })?;
        Self::check_auth(&resp)?;
        let resp = Self::check_status(resp, "history request").await?;
        let wrapper: HistoryResponse = resp.json().await.context(HttpSnafu {
            operation: "history response",
        })?;
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
            .context(HttpSnafu {
                operation: "create session",
            })?;
        Self::check_auth(&resp)?;
        let resp = Self::check_status(resp, "create session request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "create session response",
        })
    }

    #[tracing::instrument(skip(self))]
    pub async fn archive_session(&self, session_id: &str) -> Result<()> {
        let encoded = encode_path(session_id);
        let resp = self
            .request(
                reqwest::Method::POST,
                &format!("/api/v1/sessions/{encoded}/archive"),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "archive session",
            })?;
        Self::check_status(resp, "archive request").await?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn unarchive_session(&self, session_id: &str) -> Result<()> {
        let encoded = encode_path(session_id);
        let resp = self
            .request(
                reqwest::Method::POST,
                &format!("/api/v1/sessions/{encoded}/unarchive"),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "unarchive session",
            })?;
        Self::check_status(resp, "unarchive request").await?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn rename_session(&self, session_id: &str, name: &str) -> Result<()> {
        let encoded = encode_path(session_id);
        let resp = self
            .request(
                reqwest::Method::PUT,
                &format!("/api/v1/sessions/{encoded}/name"),
            )
            .json(&serde_json::json!({ "name": name }))
            .send()
            .await
            .context(HttpSnafu {
                operation: "rename session",
            })?;
        Self::check_status(resp, "rename request").await?;
        Ok(())
    }

    #[expect(dead_code, reason = "reserved for turn abort keybinding")]
    #[tracing::instrument(skip(self))]
    pub async fn abort_turn(&self, turn_id: &str) -> Result<()> {
        let encoded = encode_path(turn_id);
        let resp = self
            .request(
                reqwest::Method::POST,
                &format!("/api/v1/turns/{encoded}/abort"),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "abort turn",
            })?;
        Self::check_status(resp, "abort request").await?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn approve_tool(&self, turn_id: &str, tool_id: &str) -> Result<()> {
        let t = encode_path(turn_id);
        let d = encode_path(tool_id);
        let resp = self
            .request(
                reqwest::Method::POST,
                &format!("/api/v1/turns/{t}/tools/{d}/approve"),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "approve tool",
            })?;
        Self::check_status(resp, "approve request").await?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn deny_tool(&self, turn_id: &str, tool_id: &str) -> Result<()> {
        let t = encode_path(turn_id);
        let d = encode_path(tool_id);
        let resp = self
            .request(
                reqwest::Method::POST,
                &format!("/api/v1/turns/{t}/tools/{d}/deny"),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "deny tool",
            })?;
        Self::check_status(resp, "deny request").await?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn approve_plan(&self, plan_id: &str) -> Result<()> {
        let encoded = encode_path(plan_id);
        let resp = self
            .request(
                reqwest::Method::POST,
                &format!("/api/v1/plans/{encoded}/approve"),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "approve plan",
            })?;
        Self::check_status(resp, "plan approve request").await?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn cancel_plan(&self, plan_id: &str) -> Result<()> {
        let encoded = encode_path(plan_id);
        let resp = self
            .request(
                reqwest::Method::POST,
                &format!("/api/v1/plans/{encoded}/cancel"),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "cancel plan",
            })?;
        Self::check_status(resp, "plan cancel request").await?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn today_cost_cents(&self) -> Result<u32> {
        let resp = self
            .request(reqwest::Method::GET, "/api/v1/costs/daily")
            .send()
            .await
            .context(HttpSnafu {
                operation: "load costs",
            })?;
        let resp = Self::check_status(resp, "costs request").await?;
        let daily: DailyResponse = resp.json().await.context(HttpSnafu {
            operation: "costs response",
        })?;
        let today_cost = daily.daily.last().map(|d| d.cost).unwrap_or(0.0);
        Ok((today_cost * 100.0) as u32)
    }

    #[tracing::instrument(skip(self))]
    pub async fn compact(&self, session_id: &str) -> Result<()> {
        let encoded = encode_path(session_id);
        let resp = self
            .request(
                reqwest::Method::POST,
                &format!("/api/v1/sessions/{encoded}/distill"),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "trigger distillation",
            })?;
        Self::check_status(resp, "distillation request").await?;
        Ok(())
    }

    /// Fetch registered tools for an agent.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::Http`] if the request fails or the response cannot be decoded.
    /// Returns [`ApiError::Auth`] if the server rejects the authentication token.
    /// Returns [`ApiError::Server`] if the server returns a non-success status.
    #[tracing::instrument(skip(self))]
    pub async fn tools(&self, nous_id: &str) -> Result<Vec<NousTool>> {
        let encoded = encode_path(nous_id);
        let resp = self
            .request(
                reqwest::Method::GET,
                &format!("/api/v1/nous/{encoded}/tools"),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "load tools",
            })?;
        Self::check_auth(&resp)?;
        let resp = Self::check_status(resp, "tools request").await?;
        let wrapper: NousToolsResponse = resp.json().await.context(HttpSnafu {
            operation: "tools response",
        })?;
        Ok(wrapper.tools)
    }

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
            .context(HttpSnafu {
                operation: "recall memory",
            })?;
        let resp = Self::check_status(resp, "recall request").await?;
        resp.text().await.context(HttpSnafu {
            operation: "recall response",
        })
    }

    /// Fetch the server configuration.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::Http`] if the request fails or the response cannot be decoded.
    /// Returns [`ApiError::Auth`] if the server rejects the authentication token.
    /// Returns [`ApiError::Server`] if the server returns a non-success status.
    #[tracing::instrument(skip(self))]
    pub async fn config(&self) -> Result<serde_json::Value> {
        let resp = self
            .request(reqwest::Method::GET, "/api/v1/config")
            .send()
            .await
            .context(HttpSnafu {
                operation: "load config",
            })?;
        Self::check_auth(&resp)?;
        let resp = Self::check_status(resp, "config request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "config response",
        })
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
            .context(HttpSnafu {
                operation: "update config",
            })?;
        Self::check_auth(&resp)?;
        let resp = Self::check_status(resp, "config update request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "config update response",
        })
    }

    #[tracing::instrument(skip(self))]
    pub async fn knowledge_facts(
        &self,
        sort: &str,
        order: &str,
        limit: u32,
    ) -> Result<serde_json::Value> {
        let resp = self
            .request(
                reqwest::Method::GET,
                &format!("/api/v1/knowledge/facts?sort={sort}&order={order}&limit={limit}"),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "load facts",
            })?;
        let resp = Self::check_status(resp, "facts request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "facts response",
        })
    }

    #[tracing::instrument(skip(self))]
    pub async fn knowledge_fact_detail(&self, fact_id: &str) -> Result<serde_json::Value> {
        let encoded = encode_path(fact_id);
        let resp = self
            .request(
                reqwest::Method::GET,
                &format!("/api/v1/knowledge/facts/{encoded}"),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "load fact detail",
            })?;
        let resp = Self::check_status(resp, "fact detail request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "fact detail response",
        })
    }

    #[tracing::instrument(skip(self))]
    pub async fn knowledge_forget(&self, fact_id: &str) -> Result<()> {
        let encoded = encode_path(fact_id);
        let resp = self
            .request(
                reqwest::Method::POST,
                &format!("/api/v1/knowledge/facts/{encoded}/forget"),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "forget fact",
            })?;
        Self::check_status(resp, "forget request").await?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn knowledge_restore(&self, fact_id: &str) -> Result<()> {
        let encoded = encode_path(fact_id);
        let resp = self
            .request(
                reqwest::Method::POST,
                &format!("/api/v1/knowledge/facts/{encoded}/restore"),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "restore fact",
            })?;
        Self::check_status(resp, "restore request").await?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn knowledge_update_confidence(&self, fact_id: &str, confidence: f64) -> Result<()> {
        let encoded = encode_path(fact_id);
        let resp = self
            .request(
                reqwest::Method::PUT,
                &format!("/api/v1/knowledge/facts/{encoded}/confidence"),
            )
            .json(&serde_json::json!({ "confidence": confidence }))
            .send()
            .await
            .context(HttpSnafu {
                operation: "update confidence",
            })?;
        Self::check_status(resp, "confidence request").await?;
        Ok(())
    }

    #[tracing::instrument(skip(self, text))]
    pub async fn queue_message(&self, session_id: &str, text: &str) -> Result<()> {
        let encoded = encode_path(session_id);
        let resp = self
            .request(
                reqwest::Method::POST,
                &format!("/api/v1/sessions/{encoded}/queue"),
            )
            .json(&serde_json::json!({ "text": text }))
            .send()
            .await
            .context(HttpSnafu {
                operation: "queue message",
            })?;
        Self::check_status(resp, "queue request").await?;
        Ok(())
    }

    fn check_auth(resp: &Response) -> Result<()> {
        if resp.status() == StatusCode::UNAUTHORIZED || resp.status() == StatusCode::FORBIDDEN {
            return AuthSnafu.fail();
        }
        Ok(())
    }

    /// Consumes a response, returning it unchanged if 2xx, or a `Server` error with a
    /// human-readable message extracted from the body. Falls back to "{status} {reason}".
    async fn check_status(resp: Response, operation: &'static str) -> Result<Response> {
        if resp.status().is_success() {
            return Ok(resp);
        }
        let status = resp.status();
        let reason = status.canonical_reason().unwrap_or("Unknown");
        let body = resp.text().await.unwrap_or_default();
        let message = if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
            json.get("message")
                .or_else(|| json.get("error"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("{} {}", status.as_u16(), reason))
        } else {
            format!("{} {}", status.as_u16(), reason)
        };
        ServerSnafu { operation, message }.fail()
    }

    /// The shared HTTP client, pre-configured with auth and default headers.
    pub fn raw_client(&self) -> &Client {
        &self.client
    }
}

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
