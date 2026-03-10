use reqwest::{Client, Response, StatusCode};
use snafu::prelude::*;

use super::types::*;
use crate::error::{AuthSnafu, HttpSnafu, Result};

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
            .context(HttpSnafu {
                operation: "build HTTP client",
            })?;

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

    #[tracing::instrument(skip(self))]
    pub async fn health(&self) -> Result<bool> {
        let resp = self
            .client
            .get(self.url("/api/health"))
            .send()
            .await
            .context(HttpSnafu {
                operation: "health check",
            })?;
        Ok(resp.status().is_success())
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

        resp.error_for_status_ref().context(HttpSnafu {
            operation: "login request",
        })?;
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
        resp.error_for_status_ref().context(HttpSnafu {
            operation: "agents request",
        })?;
        let wrapper: AgentsResponse = resp.json().await.context(HttpSnafu {
            operation: "agents response",
        })?;
        Ok(wrapper.nous)
    }

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
        resp.error_for_status_ref().context(HttpSnafu {
            operation: "sessions request",
        })?;
        let wrapper: SessionsResponse = resp.json().await.context(HttpSnafu {
            operation: "sessions response",
        })?;
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
            .context(HttpSnafu {
                operation: "load history",
            })?;
        Self::check_auth(&resp)?;
        resp.error_for_status_ref().context(HttpSnafu {
            operation: "history request",
        })?;
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
        resp.error_for_status_ref().context(HttpSnafu {
            operation: "create session request",
        })?;
        resp.json().await.context(HttpSnafu {
            operation: "create session response",
        })
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
        .context(HttpSnafu {
            operation: "archive session",
        })?
        .error_for_status_ref()
        .context(HttpSnafu {
            operation: "archive request",
        })?;
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
        .context(HttpSnafu {
            operation: "unarchive session",
        })?
        .error_for_status_ref()
        .context(HttpSnafu {
            operation: "unarchive request",
        })?;
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
        .context(HttpSnafu {
            operation: "rename session",
        })?
        .error_for_status_ref()
        .context(HttpSnafu {
            operation: "rename request",
        })?;
        Ok(())
    }

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
        .context(HttpSnafu {
            operation: "abort turn",
        })?
        .error_for_status_ref()
        .context(HttpSnafu {
            operation: "abort request",
        })?;
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
        .context(HttpSnafu {
            operation: "approve tool",
        })?
        .error_for_status_ref()
        .context(HttpSnafu {
            operation: "approve request",
        })?;
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
        .context(HttpSnafu {
            operation: "deny tool",
        })?
        .error_for_status_ref()
        .context(HttpSnafu {
            operation: "deny request",
        })?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn approve_plan(&self, plan_id: &str) -> Result<()> {
        let encoded = encode_path(plan_id);
        self.request(
            reqwest::Method::POST,
            &format!("/api/v1/plans/{encoded}/approve"),
        )
        .send()
        .await
        .context(HttpSnafu {
            operation: "approve plan",
        })?
        .error_for_status_ref()
        .context(HttpSnafu {
            operation: "plan approve request",
        })?;
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
        .context(HttpSnafu {
            operation: "cancel plan",
        })?
        .error_for_status_ref()
        .context(HttpSnafu {
            operation: "plan cancel request",
        })?;
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
        resp.error_for_status_ref().context(HttpSnafu {
            operation: "costs request",
        })?;
        let daily: DailyResponse = resp.json().await.context(HttpSnafu {
            operation: "costs response",
        })?;
        // Get today's entry (last in list)
        let today_cost = daily.daily.last().map(|d| d.cost).unwrap_or(0.0);
        // Convert dollars to cents
        Ok((today_cost * 100.0) as u32)
    }

    #[tracing::instrument(skip(self))]
    pub async fn compact(&self, session_id: &str) -> Result<()> {
        let encoded = encode_path(session_id);
        self.request(
            reqwest::Method::POST,
            &format!("/api/v1/sessions/{encoded}/distill"),
        )
        .send()
        .await
        .context(HttpSnafu {
            operation: "trigger distillation",
        })?
        .error_for_status_ref()
        .context(HttpSnafu {
            operation: "distillation request",
        })?;
        Ok(())
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
        resp.error_for_status_ref().context(HttpSnafu {
            operation: "recall request",
        })?;
        resp.text().await.context(HttpSnafu {
            operation: "recall response",
        })
    }

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
        resp.error_for_status_ref().context(HttpSnafu {
            operation: "config request",
        })?;
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
        resp.error_for_status_ref().context(HttpSnafu {
            operation: "config update request",
        })?;
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
        resp.error_for_status_ref().context(HttpSnafu {
            operation: "facts request",
        })?;
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
        resp.error_for_status_ref().context(HttpSnafu {
            operation: "fact detail request",
        })?;
        resp.json().await.context(HttpSnafu {
            operation: "fact detail response",
        })
    }

    #[tracing::instrument(skip(self))]
    pub async fn knowledge_forget(&self, fact_id: &str) -> Result<()> {
        let encoded = encode_path(fact_id);
        self.request(
            reqwest::Method::POST,
            &format!("/api/v1/knowledge/facts/{encoded}/forget"),
        )
        .send()
        .await
        .context(HttpSnafu {
            operation: "forget fact",
        })?
        .error_for_status_ref()
        .context(HttpSnafu {
            operation: "forget request",
        })?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn knowledge_restore(&self, fact_id: &str) -> Result<()> {
        let encoded = encode_path(fact_id);
        self.request(
            reqwest::Method::POST,
            &format!("/api/v1/knowledge/facts/{encoded}/restore"),
        )
        .send()
        .await
        .context(HttpSnafu {
            operation: "restore fact",
        })?
        .error_for_status_ref()
        .context(HttpSnafu {
            operation: "restore request",
        })?;
        Ok(())
    }

    #[expect(
        dead_code,
        reason = "will be wired up when confidence editing calls the API"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn knowledge_update_confidence(&self, fact_id: &str, confidence: f64) -> Result<()> {
        let encoded = encode_path(fact_id);
        self.request(
            reqwest::Method::POST,
            &format!("/api/v1/knowledge/facts/{encoded}/confidence"),
        )
        .json(&serde_json::json!({ "confidence": confidence }))
        .send()
        .await
        .context(HttpSnafu {
            operation: "update confidence",
        })?
        .error_for_status_ref()
        .context(HttpSnafu {
            operation: "confidence request",
        })?;
        Ok(())
    }

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
        .context(HttpSnafu {
            operation: "queue message",
        })?
        .error_for_status_ref()
        .context(HttpSnafu {
            operation: "queue request",
        })?;
        Ok(())
    }

    fn check_auth(resp: &Response) -> Result<()> {
        if resp.status() == StatusCode::UNAUTHORIZED || resp.status() == StatusCode::FORBIDDEN {
            return AuthSnafu.fail();
        }
        Ok(())
    }

    #[expect(dead_code, reason = "reserved for future SSE connection management")]
    /// Get the raw reqwest client for SSE/streaming (they manage their own connections)
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
