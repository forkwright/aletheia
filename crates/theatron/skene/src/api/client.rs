// kanon:ignore RUST/file-too-long — cohesive HTTP client; extracting now would fragment request/response handling
//! HTTP client for the Aletheia gateway REST API.
use std::time::Duration;

use reqwest::{Client, Response, StatusCode, header};
use snafu::prelude::*;

use koina::secret::SecretString;

use super::error::{
    ApiError, HttpSnafu, RateLimitedSnafu, Result, ServerSnafu, format_http_error_body,
    parse_pylon_error_body, parse_retry_after_secs,
};
use super::types::{
    Agent, AgentsResponse, HealthResponse, HistoryMessage, HistoryResponse, ListSessionsRequest,
    NousTool, NousToolsResponse, PaginatedSessionsResponse, Session, SessionsResponse,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const REST_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const CLIENT_CONTRACT_PATH: &str = "/api/v1/client/contract";
const REQUEST_ID_HEADER_NAME: &str = "x-request-id";

/// CSRF header material required by a running pylon instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsrfHeader {
    /// Header name to send on mutating requests.
    pub name: String,
    /// Header value to send on mutating requests.
    pub value: String,
}

/// First-party client contract returned by pylon.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientContract {
    /// CSRF settings for first-party clients.
    pub csrf: ClientCsrfContract,
}

/// CSRF portion of the first-party client contract.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientCsrfContract {
    /// Whether mutating requests must include the CSRF header.
    pub enabled: bool,
    /// Header name to send when CSRF is enabled.
    pub header_name: String,
    /// Header value to send when CSRF is enabled.
    #[serde(default)]
    pub header_value: Option<String>,
}

impl ClientCsrfContract {
    /// Convert this contract into request header material.
    #[must_use]
    pub fn required_header(&self) -> Option<CsrfHeader> {
        if !self.enabled {
            return None;
        }
        let value = self.header_value.as_ref()?.trim();
        if self.header_name.trim().is_empty() || value.is_empty() {
            return None;
        }
        Some(CsrfHeader {
            name: self.header_name.clone(),
            value: value.to_owned(),
        })
    }
}

fn default_headers(token: Option<&str>, csrf: Option<&CsrfHeader>) -> Result<header::HeaderMap> {
    let mut headers = header::HeaderMap::new();

    if let Some(t) = token {
        let auth_value = header::HeaderValue::from_str(&format!("Bearer {t}"))
            .map_err(|_invalid| ApiError::InvalidToken)?;
        headers.insert(header::AUTHORIZATION, auth_value);
    }

    if let Some(csrf) = csrf {
        let name = header::HeaderName::from_bytes(csrf.name.as_bytes())
            .map_err(|_invalid| ApiError::InvalidToken)?;
        let value = header::HeaderValue::from_str(&csrf.value)
            .map_err(|_invalid| ApiError::InvalidToken)?;
        headers.insert(name, value);
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
        header::HeaderValue::from_str(&request_id).map_err(|_invalid| ApiError::InvalidToken)?;
    headers.insert(REQUEST_ID_HEADER_NAME, request_id_value);

    Ok(headers)
}

/// Build the reqwest client used for short REST API calls.
pub fn build_http_client(token: Option<&str>, csrf: Option<&CsrfHeader>) -> Result<Client> {
    Client::builder()
        .cookie_store(true)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REST_REQUEST_TIMEOUT)
        .default_headers(default_headers(token, csrf)?)
        .build()
        .context(HttpSnafu {
            operation: "build REST HTTP client",
        })
}

/// Build the reqwest client used for long-lived SSE/streaming connections.
pub fn build_streaming_client(token: Option<&str>, csrf: Option<&CsrfHeader>) -> Result<Client> {
    // kanon:ignore RUST/missing-http-timeout — SSE connections are long-lived; a request-level timeout would terminate the stream prematurely; connect_timeout guards against connection hang
    Client::builder()
        .cookie_store(true)
        .connect_timeout(CONNECT_TIMEOUT)
        .default_headers(default_headers(token, csrf)?)
        .build()
        .context(HttpSnafu {
            operation: "build streaming HTTP client",
        })
}

/// HTTP client for the Aletheia gateway REST API.
#[derive(Clone)]
pub struct ApiClient {
    client: Client,
    streaming_client: Client,
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
    /// Returns [`ApiError::Http`] if either HTTP client cannot be constructed.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    pub fn new(base_url: &str, token: Option<String>) -> Result<Self> {
        // kanon:ignore RUST/pub-visibility
        Self::new_with_csrf(base_url, token, None)
    }

    /// Create a new API client with discovered CSRF header material.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::InvalidToken`] if `token` or the CSRF material contains
    /// characters invalid in HTTP headers. Returns [`ApiError::Http`] if either
    /// HTTP client cannot be constructed.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    pub fn new_with_csrf(
        base_url: &str,
        token: Option<String>,
        csrf: Option<&CsrfHeader>,
    ) -> Result<Self> {
        // kanon:ignore RUST/pub-visibility
        let client = build_http_client(token.as_deref(), csrf)?;
        let streaming_client = build_streaming_client(token.as_deref(), csrf)?;

        Ok(Self {
            client,
            streaming_client,
            base_url: base_url.trim_end_matches('/').to_string(),
            token: token.map(SecretString::from),
        })
    }

    /// Replace the authentication token.
    #[expect(dead_code, reason = "API client methods for TUI/desktop integration")]
    pub(crate) fn set_token(&mut self, token: SecretString) {
        // kanon:ignore RUST/pub-visibility
        self.token = Some(token);
    }

    /// The base URL this client connects to.
    #[must_use]
    #[expect(dead_code, reason = "API client methods for TUI/desktop integration")]
    pub(crate) fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The current authentication token, if set.
    #[must_use]
    pub fn token(&self) -> Option<&str> {
        // kanon:ignore RUST/pub-visibility
        self.token.as_ref().map(SecretString::expose_secret)
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        // NOTE: no per-request header injection: token is fixed at construction
        self.client.request(method, self.url(path))
    }

    /// Check server reachability (not health status).
    ///
    /// A 503 (unhealthy) means the server IS running but has degraded checks.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn health(&self) -> Result<bool> {
        let resp = self.client.get(self.url("/api/health")).send().await;
        Ok(resp.is_ok())
    }

    /// Fetch the server's full health report.
    ///
    /// Returns the parsed [`HealthResponse`] for both successful (healthy/degraded)
    /// and `503 Service Unavailable` (unhealthy) responses so callers can render
    /// the real check states. Network failures and unparseable responses are
    /// returned as errors, preserving the distinction between reachability and
    /// backend health.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn health_details(&self) -> Result<HealthResponse> {
        let resp = self
            .client
            .get(self.url("/api/health"))
            .send()
            .await
            .context(HttpSnafu {
                operation: "health details",
            })?;

        // WHY: the health endpoint returns a JSON body for both OK and 503
        // responses. Accept both so callers can distinguish server reachability
        // from an unhealthy backend.
        if resp.status().is_success() || resp.status() == StatusCode::SERVICE_UNAVAILABLE {
            resp.json().await.context(HttpSnafu {
                operation: "health details response",
            })
        } else {
            let resp = Self::check_status(resp, "health details request").await?;
            resp.json().await.context(HttpSnafu {
                operation: "health details response",
            })
        }
    }

    /// Fetch all registered agents.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn agents(&self) -> Result<Vec<Agent>> {
        let resp = self
            .request(reqwest::Method::GET, "/api/v1/nous")
            .send()
            .await
            .context(HttpSnafu {
                operation: "load agents",
            })?;
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
    /// Returns [`ApiError::Server`] if the server returns a non-success status.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn sessions(&self, nous_id: &str) -> Result<Vec<Session>> {
        let resp = self
            .request(
                reqwest::Method::GET,
                &super::routes::sessions::sessions_for_agent_path(nous_id),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "load sessions",
            })?;
        let resp = Self::check_status(resp, "sessions request").await?;
        let wrapper: SessionsResponse = resp.json().await.context(HttpSnafu {
            operation: "sessions response",
        })?;
        Ok(wrapper.sessions)
    }

    /// Fetch sessions with pagination, search, and status filtering.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::Http`] if the request fails or the response cannot be decoded.
    /// Returns [`ApiError::Server`] if the server returns a non-success status.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn sessions_paginated(
        &self,
        params: &ListSessionsRequest,
    ) -> Result<PaginatedSessionsResponse> {
        let mut path = super::routes::sessions::sessions_path().to_string();
        let mut sep = '?';

        let mut push_param = |name: &str, value: &str| {
            path.push(sep);
            sep = '&';
            path.push_str(name);
            path.push('=');
            path.push_str(&super::routes::encoding::query_value(value));
        };

        if let Some(nous_id) = &params.nous_id {
            push_param("nous_id", nous_id);
        }
        if let Some(search) = &params.search {
            push_param("search", search);
        }
        if let Some(status) = &params.status {
            push_param("status", status.as_str());
        }
        if let Some(limit) = params.limit {
            push_param("limit", &limit.to_string());
        }
        if let Some(after) = &params.after {
            push_param("after", after);
        }

        let resp = self
            .request(reqwest::Method::GET, &path)
            .send()
            .await
            .context(HttpSnafu {
                operation: "load sessions paginated",
            })?;
        let resp = Self::check_status(resp, "sessions paginated request").await?;
        let wrapper: PaginatedSessionsResponse = resp.json().await.context(HttpSnafu {
            operation: "sessions paginated response",
        })?;
        Ok(wrapper)
    }

    /// Fetch message history for a session.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::Http`] if the request fails or the response cannot be decoded.
    /// Returns [`ApiError::Server`] if the server returns a non-success status.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn history(&self, session_id: &str) -> Result<Vec<HistoryMessage>> {
        let resp = self
            .request(
                reqwest::Method::GET,
                &super::routes::sessions::session_history_path(session_id),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "load history",
            })?;
        let resp = Self::check_status(resp, "history request").await?;
        let wrapper: HistoryResponse = resp.json().await.context(HttpSnafu {
            operation: "history response",
        })?;
        Ok(wrapper.messages)
    }

    /// Create a new session for an agent.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn create_session(&self, nous_id: &str, session_key: &str) -> Result<Session> {
        let resp = self
            .request(reqwest::Method::POST, "/api/v1/sessions")
            .json(&serde_json::json!({
                "nous_id": nous_id,
                "session_key": session_key,
            }))
            .send()
            .await
            .context(HttpSnafu {
                operation: "create session",
            })?;
        let resp = Self::check_status(resp, "create session request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "create session response",
        })
    }

    /// Archive a session.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn archive_session(&self, session_id: &str) -> Result<()> {
        let resp = self
            .request(
                reqwest::Method::POST,
                &super::routes::sessions::session_archive_path(session_id),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "archive session",
            })?;
        Self::check_status(resp, "archive request").await?;
        Ok(())
    }

    /// Unarchive a previously archived session.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn unarchive_session(&self, session_id: &str) -> Result<()> {
        let resp = self
            .request(
                reqwest::Method::POST,
                &super::routes::sessions::session_unarchive_path(session_id),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "unarchive session",
            })?;
        Self::check_status(resp, "unarchive request").await?;
        Ok(())
    }

    /// Rename a session.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn rename_session(&self, session_id: &str, name: &str) -> Result<()> {
        let resp = self
            .request(
                reqwest::Method::PUT,
                &super::routes::sessions::session_name_path(session_id),
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

    /// Approve a tool invocation awaiting user consent.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn approve_tool(&self, turn_id: &str, tool_id: &str) -> Result<()> {
        let t = keryx::url::encode_path_segment(turn_id);
        let d = keryx::url::encode_path_segment(tool_id);
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

    /// Deny a tool invocation awaiting user consent.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn deny_tool(&self, turn_id: &str, tool_id: &str) -> Result<()> {
        let t = keryx::url::encode_path_segment(turn_id);
        let d = keryx::url::encode_path_segment(tool_id);
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

    /// Fetch registered tools for an agent.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::Http`] if the request fails or the response cannot be decoded.
    /// Returns [`ApiError::Server`] if the server returns a non-success status.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn tools(&self, nous_id: &str) -> Result<Vec<NousTool>> {
        let resp = self
            .request(
                reqwest::Method::GET,
                &super::routes::nous::agent_tools_path(nous_id),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "load tools",
            })?;
        let resp = Self::check_status(resp, "tools request").await?;
        let wrapper: NousToolsResponse = resp.json().await.context(HttpSnafu {
            operation: "tools response",
        })?;
        Ok(wrapper.tools)
    }

    /// Fetch the server configuration.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::Http`] if the request fails or the response cannot be decoded.
    /// Returns [`ApiError::Server`] if the server returns a non-success status.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn config(&self) -> Result<serde_json::Value> {
        let resp = self
            .request(reqwest::Method::GET, "/api/v1/config")
            .send()
            .await
            .context(HttpSnafu {
                operation: "load config",
            })?;
        let resp = Self::check_status(resp, "config request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "config response",
        })
    }

    /// Update a single configuration section.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self, data))]
    pub async fn update_config_section(
        &self,
        section: &str,
        data: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let resp = self
            .request(
                reqwest::Method::PUT,
                &super::routes::config::section_path(section),
            )
            .json(data)
            .send()
            .await
            .context(HttpSnafu {
                operation: "update config",
            })?;
        let resp = Self::check_status(resp, "config update request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "config update response",
        })
    }

    /// Fetch knowledge facts with sorting and pagination.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
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

    /// Fetch detail for a single knowledge fact.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn knowledge_fact_detail(&self, fact_id: &str) -> Result<serde_json::Value> {
        let encoded = keryx::url::encode_path_segment(fact_id);
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

    /// Mark a knowledge fact as forgotten.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn knowledge_forget(&self, fact_id: &str) -> Result<()> {
        let encoded = keryx::url::encode_path_segment(fact_id);
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

    /// Restore a previously forgotten fact.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn knowledge_restore(&self, fact_id: &str) -> Result<()> {
        let encoded = keryx::url::encode_path_segment(fact_id);
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

    /// Fetch all knowledge entities.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn knowledge_entities(&self) -> Result<serde_json::Value> {
        let resp = self
            .request(reqwest::Method::GET, "/api/v1/knowledge/entities")
            .send()
            .await
            .context(HttpSnafu {
                operation: "load entities",
            })?;
        let resp = Self::check_status(resp, "entities request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "entities response",
        })
    }

    /// Fetch relationships for a specific entity.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn knowledge_entity_relationships(
        &self,
        entity_id: &str,
    ) -> Result<serde_json::Value> {
        let encoded = keryx::url::encode_path_segment(entity_id);
        let resp = self
            .request(
                reqwest::Method::GET,
                &format!("/api/v1/knowledge/entities/{encoded}/relationships"),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "load entity relationships",
            })?;
        let resp = Self::check_status(resp, "entity relationships request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "entity relationships response",
        })
    }

    /// Fetch the knowledge activity timeline.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn knowledge_timeline(&self) -> Result<serde_json::Value> {
        let resp = self
            .request(reqwest::Method::GET, "/api/v1/knowledge/timeline")
            .send()
            .await
            .context(HttpSnafu {
                operation: "load timeline",
            })?;
        let resp = Self::check_status(resp, "timeline request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "timeline response",
        })
    }

    /// Update the confidence score for a knowledge fact.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn knowledge_update_confidence(&self, fact_id: &str, confidence: f64) -> Result<()> {
        let encoded = keryx::url::encode_path_segment(fact_id);
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

    /// Consumes a response, returning it unchanged if 2xx.
    ///
    /// On non-2xx:
    /// - 429 without a canonical pylon envelope → [`ApiError::RateLimited`]
    ///   with `retry_after_secs` parsed from the `Retry-After` header
    ///   (delta-seconds form only).
    /// - 429 with a canonical pylon envelope → [`ApiError::Server`] so
    ///   request IDs and structured details survive to first-party UIs.
    /// - Other → [`ApiError::Server`] with the human-readable message
    ///   extracted from the canonical pylon envelope
    ///   `{error:{code,message,...}}`; falls back to `"{status} {reason}"`
    ///   when the envelope is absent or malformed.
    async fn check_status(resp: Response, operation: &'static str) -> Result<Response> {
        if resp.status().is_success() {
            return Ok(resp);
        }
        let status = resp.status();

        if status == StatusCode::TOO_MANY_REQUESTS {
            let retry_after_secs = parse_retry_after_secs(resp.headers());
            // kanon:ignore RUST/no-result-unwrap-or-default — empty body on text() failure is acceptable; status code is the primary error signal
            let body = resp.text().await.unwrap_or_default();
            if let Some(detail) = parse_pylon_error_body(&body) {
                return ServerSnafu {
                    operation,
                    status: status.as_u16(),
                    message: detail.display_message(),
                }
                .fail();
            }
            return RateLimitedSnafu {
                operation,
                retry_after_secs,
            }
            .fail();
        }

        let reason = status.canonical_reason().unwrap_or("Unknown");
        // kanon:ignore RUST/no-result-unwrap-or-default — empty body on text() failure is acceptable; status code is the primary error signal
        let body = resp.text().await.unwrap_or_default();
        let message = parse_pylon_error_body(&body).map_or_else(
            || format_http_error_body(status.as_u16(), reason, &body),
            |detail| detail.display_message(),
        );
        ServerSnafu {
            operation,
            status: status.as_u16(),
            message,
        }
        .fail()
    }

    /// The REST HTTP client, pre-configured with auth and default headers.
    #[must_use]
    pub fn raw_client(&self) -> &Client {
        // kanon:ignore RUST/pub-visibility
        &self.client
    }

    /// The streaming HTTP client, pre-configured with auth and default headers.
    #[must_use]
    pub fn streaming_client(&self) -> &Client {
        // kanon:ignore RUST/pub-visibility
        &self.streaming_client
    }
}

/// Discover the first-party client contract from a running pylon instance.
///
/// The endpoint is a GET so it is exempt from CSRF. Token-authenticated
/// deployments still require the bearer token before returning CSRF material.
///
/// # Errors
///
/// Returns [`ApiError`] when the request fails, auth is rejected, or the
/// response body does not match the expected contract.
#[must_use]
#[expect(
    clippy::double_must_use,
    reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
)]
pub async fn discover_client_contract(
    base_url: &str,
    token: Option<&str>,
) -> Result<ClientContract> {
    let client = build_http_client(token, None)?;
    let base = base_url.trim_end_matches('/');
    let resp = client
        .get(format!("{base}{CLIENT_CONTRACT_PATH}"))
        .send()
        .await
        .context(HttpSnafu {
            operation: "client contract",
        })?;
    let resp = ApiClient::check_status(resp, "client contract request").await?;
    resp.json().await.context(HttpSnafu {
        operation: "client contract response",
    })
}

#[cfg(test)]
mod tests {
    #![expect(clippy::expect_used, reason = "test helper failures should panic")]

    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    fn install_crypto() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    fn serve_http_error_once(
        status_line: &'static str,
        headers: &'static str,
        body: &'static str,
    ) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local test server");
        let addr = listener.local_addr().expect("read local test server addr");
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept test request");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("set read timeout");
            let mut buf = [0_u8; 2048];
            let _ = stream.read(&mut buf);
            let response = format!(
                "HTTP/1.1 {status_line}\r\ncontent-type: application/json\r\nconnection: close\r\n{headers}\r\n{body}"
            );
            stream
                .write_all(response.as_bytes())
                .expect("write HTTP error test response");
        });
        (format!("http://{addr}"), handle)
    }

    fn capture_request_once() -> (String, mpsc::Receiver<String>, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local test server");
        let addr = listener.local_addr().expect("read local test server addr");
        let (tx, rx) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept test request");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("set read timeout");
            let mut buf = [0_u8; 4096];
            let n = stream.read(&mut buf).expect("read test request");
            let request = String::from_utf8_lossy(
                buf.get(..n)
                    .expect("read byte count fits request capture buffer"),
            )
            .into_owned();
            tx.send(request).expect("send captured request");
            let body = "{}";
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .expect("write capture test response");
        });
        (format!("http://{addr}"), rx, handle)
    }

    fn captured_header<'a>(request: &'a str, name: &str) -> Option<&'a str> {
        request.lines().find_map(|line| {
            line.split_once(':')
                .and_then(|(header, value)| header.eq_ignore_ascii_case(name).then(|| value.trim()))
        })
    }

    #[test]
    fn rest_client_builds_with_timeout() {
        let client = build_http_client(None, None);
        assert!(client.is_ok(), "REST client must build");
    }

    #[test]
    fn streaming_client_builds_without_total_timeout() {
        let client = build_streaming_client(None, None);
        assert!(client.is_ok(), "streaming client must build");
    }

    #[test]
    fn invalid_token_fails_for_rest_and_streaming() {
        let invalid = "\n";
        assert!(build_http_client(Some(invalid), None).is_err());
        assert!(build_streaming_client(Some(invalid), None).is_err());
    }

    #[test]
    fn csrf_contract_maps_enabled_header() {
        let contract = ClientCsrfContract {
            enabled: true,
            header_name: "x-aletheia-csrf".to_string(),
            header_value: Some("custom-value".to_string()),
        };

        let header = contract.required_header().expect("enabled contract");

        assert_eq!(header.name, "x-aletheia-csrf");
        assert_eq!(header.value, "custom-value");
    }

    #[test]
    fn csrf_contract_disabled_requires_no_header() {
        let contract = ClientCsrfContract {
            enabled: false,
            header_name: "x-aletheia-csrf".to_string(),
            header_value: Some("custom-value".to_string()),
        };

        assert!(contract.required_header().is_none());
    }

    #[tokio::test]
    async fn client_builder_sends_configured_csrf_header() {
        install_crypto();
        let (base_url, request_rx, server) = capture_request_once();
        let csrf = CsrfHeader {
            name: "x-aletheia-csrf".to_string(),
            value: "custom-csrf-value".to_string(),
        };
        let client = build_http_client(Some("secret-token"), Some(&csrf)).expect("build client");

        let resp = client
            .post(format!("{base_url}/api/v1/sessions"))
            .body("{}")
            .send()
            .await
            .expect("send request");
        assert!(resp.status().is_success());
        let request = request_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("captured request");
        server.join().expect("test server thread should finish");

        assert_eq!(
            captured_header(&request, "authorization"),
            Some("Bearer secret-token")
        );
        assert_eq!(
            captured_header(&request, "x-aletheia-csrf"),
            Some("custom-csrf-value")
        );
        assert_eq!(
            captured_header(&request, "accept"),
            Some("application/json")
        );
        assert_eq!(
            captured_header(&request, "content-type"),
            Some("application/json")
        );
        assert!(captured_header(&request, "x-request-id").is_some());
        assert!(
            captured_header(&request, "x-requested-with").is_none(),
            "client must not hardcode the published bootstrap CSRF value"
        );
    }

    #[tokio::test]
    async fn client_builder_without_csrf_omits_bootstrap_header() {
        install_crypto();
        let (base_url, request_rx, server) = capture_request_once();
        let client = build_http_client(None, None).expect("build client");

        let resp = client
            .post(format!("{base_url}/api/v1/sessions"))
            .body("{}")
            .send()
            .await
            .expect("send request");
        assert!(resp.status().is_success());
        let request = request_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("captured request");
        server.join().expect("test server thread should finish");

        assert!(captured_header(&request, "x-request-id").is_some());
        assert!(captured_header(&request, "x-requested-with").is_none());
    }

    #[test]
    fn api_client_provides_distinct_rest_and_streaming_clients() {
        let client = match ApiClient::new("http://localhost:18789", None) {
            Ok(client) => client,
            Err(err) => panic!("ApiClient must build both clients: {err}"),
        };
        assert!(!std::ptr::eq(
            client.raw_client(),
            client.streaming_client()
        ));
    }

    #[tokio::test]
    async fn rest_http_error_preserves_pylon_envelope() {
        let body = r#"{"error":{"code":"validation_error","message":"invalid request","request_id":"req-rest","details":{"errors":[{"field":"nous_id","code":"required","message":"nous_id is required"}]}}}"#;
        let (base_url, server) = serve_http_error_once("422 Unprocessable Entity", "", body);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        let Err(err) = client.agents().await else {
            panic!("agents request should fail");
        };
        server.join().expect("test server thread should finish");

        let ApiError::Server {
            status, message, ..
        } = err
        else {
            panic!("expected Server error");
        };
        assert_eq!(status, 422);
        assert!(message.contains("invalid request"));
        assert!(message.contains("code validation_error"));
        assert!(message.contains("request_id req-rest"));
        assert!(message.contains(r#""field":"nous_id""#));
    }

    #[tokio::test]
    async fn rest_rate_limit_with_pylon_envelope_preserves_body() {
        let body = r#"{"error":{"code":"rate_limited","message":"rate limited, retry after 9s","request_id":"req-rate","details":{"retry_after_secs":9}}}"#;
        let (base_url, server) =
            serve_http_error_once("429 Too Many Requests", "retry-after: 9\r\n", body);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        let Err(err) = client.agents().await else {
            panic!("agents request should fail");
        };
        server.join().expect("test server thread should finish");

        let ApiError::Server {
            status, message, ..
        } = err
        else {
            panic!("expected Server error with pylon envelope");
        };
        assert_eq!(status, 429);
        assert!(message.contains("rate limited, retry after 9s"));
        assert!(message.contains("code rate_limited"));
        assert!(message.contains("request_id req-rate"));
        assert!(message.contains(r#""retry_after_secs":9"#));
    }

    #[tokio::test]
    async fn rest_legacy_rate_limit_keeps_retry_after_variant() {
        let (base_url, server) =
            serve_http_error_once("429 Too Many Requests", "retry-after: 7\r\n", "not json");
        let client = ApiClient::new(&base_url, None).expect("build test client");

        let Err(err) = client.agents().await else {
            panic!("agents request should fail");
        };
        server.join().expect("test server thread should finish");

        let ApiError::RateLimited {
            retry_after_secs, ..
        } = err
        else {
            panic!("expected legacy RateLimited error");
        };
        assert_eq!(retry_after_secs, Some(7));
    }
}
