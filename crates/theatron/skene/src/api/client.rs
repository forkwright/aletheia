// kanon:ignore RUST/file-too-long — cohesive HTTP client; extracting now would fragment request/response handling
//! HTTP client for the Aletheia gateway REST API.
use std::time::Duration;

use reqwest::{Client, Response, StatusCode, header};
use snafu::prelude::*;

use koina::secret::SecretString;

use super::error::{
    ApiError, AuthSnafu, HttpSnafu, RateLimitedSnafu, Result, ServerSnafu, parse_pylon_error_body,
    parse_retry_after_secs,
};
use super::request_policy::{RequestPolicy, RequestPolicyError};
use super::types::{
    Agent, AgentsResponse, HealthResponse, HistoryMessage, HistoryResponse, ListSessionsRequest,
    NousTool, NousToolsResponse, PaginatedSessionsResponse, RequestPolicyResponse, Session,
    SessionsResponse,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const REST_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

fn request_policy_error(err: &RequestPolicyError) -> ApiError {
    ApiError::Server {
        operation: "build API client",
        status: 0,
        message: err.to_string(),
    }
}

fn default_headers(
    token: Option<&str>,
    request_policy: &RequestPolicy,
) -> Result<header::HeaderMap> {
    let mut headers = header::HeaderMap::new();

    if let Some(t) = token {
        let auth_value = header::HeaderValue::from_str(&format!("Bearer {t}"))
            .map_err(|_invalid| ApiError::InvalidToken)?;
        headers.insert(header::AUTHORIZATION, auth_value);
    }

    request_policy
        .insert_headers(&mut headers)
        .map_err(|err| request_policy_error(&err))?;
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/json"),
    );
    headers.insert(
        header::ACCEPT,
        header::HeaderValue::from_static("application/json"),
    );

    Ok(headers)
}

/// Build the reqwest client used for short REST API calls.
pub(crate) fn build_http_client(token: Option<&str>) -> Result<Client> {
    build_http_client_with_policy(token, &RequestPolicy::default())
}

fn build_http_client_with_policy(
    token: Option<&str>,
    request_policy: &RequestPolicy,
) -> Result<Client> {
    Client::builder()
        .cookie_store(true)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REST_REQUEST_TIMEOUT)
        .default_headers(default_headers(token, request_policy)?)
        .build()
        .context(HttpSnafu {
            operation: "build REST HTTP client",
        })
}

/// Build the reqwest client used for long-lived SSE/streaming connections.
pub(crate) fn build_streaming_client(token: Option<&str>) -> Result<Client> {
    build_streaming_client_with_policy(token, &RequestPolicy::default())
}

fn build_streaming_client_with_policy(
    token: Option<&str>,
    request_policy: &RequestPolicy,
) -> Result<Client> {
    // kanon:ignore RUST/missing-http-timeout — SSE connections are long-lived; a request-level timeout would terminate the stream prematurely; connect_timeout guards against connection hang
    Client::builder()
        .cookie_store(true)
        .connect_timeout(CONNECT_TIMEOUT)
        .default_headers(default_headers(token, request_policy)?)
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
    request_policy: RequestPolicy,
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
        let client = build_http_client(token.as_deref())?;
        let streaming_client = build_streaming_client(token.as_deref())?;

        Ok(Self {
            client,
            streaming_client,
            base_url: base_url.trim_end_matches('/').to_string(),
            token: token.map(SecretString::from),
            request_policy: RequestPolicy::default(),
        })
    }

    /// Create a new API client with an explicit first-party request policy.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::InvalidToken`] if `token` contains characters invalid in HTTP headers.
    /// Returns [`ApiError::Server`] if the request policy contains invalid HTTP header data.
    /// Returns [`ApiError::Http`] if either HTTP client cannot be constructed.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    pub fn with_request_policy(
        base_url: &str,
        token: Option<String>,
        request_policy: RequestPolicy,
    ) -> Result<Self> {
        // kanon:ignore RUST/pub-visibility
        let client = build_http_client_with_policy(token.as_deref(), &request_policy)?;
        let streaming_client =
            build_streaming_client_with_policy(token.as_deref(), &request_policy)?;

        Ok(Self {
            client,
            streaming_client,
            base_url: base_url.trim_end_matches('/').to_string(),
            token: token.map(SecretString::from),
            request_policy,
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

    /// First-party request policy attached to this client.
    #[must_use]
    pub fn request_policy(&self) -> &RequestPolicy {
        // kanon:ignore RUST/pub-visibility
        &self.request_policy
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

    /// Fetch the request policy clients should use for state-changing requests.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::Http`] if the request fails or the response cannot be decoded.
    /// Returns [`ApiError::Auth`] if the server rejects the authentication token.
    /// Returns [`ApiError::Server`] if the server returns a non-success status.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn request_policy_metadata(&self) -> Result<RequestPolicy> {
        let resp = self
            .request(reqwest::Method::GET, "/api/v1/system/request-policy")
            .send()
            .await
            .context(HttpSnafu {
                operation: "load request policy",
            })?;
        Self::check_auth(&resp)?;
        let resp = Self::check_status(resp, "request policy request").await?;
        let wrapper: RequestPolicyResponse = resp.json().await.context(HttpSnafu {
            operation: "request policy response",
        })?;
        Ok(wrapper.request_policy)
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
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn sessions(&self, nous_id: &str) -> Result<Vec<Session>> {
        let encoded = keryx::url::encode_path_segment(nous_id);
        let resp = self
            .request(
                reqwest::Method::GET,
                &format!("/api/v1/sessions?nous_id={encoded}"),
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

    /// Fetch sessions with pagination, search, and status filtering.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::Http`] if the request fails or the response cannot be decoded.
    /// Returns [`ApiError::Auth`] if the server rejects the authentication token.
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
        let mut path = String::from("/api/v1/sessions");
        let mut sep = '?';

        let mut push_param = |name: &str, value: &str| {
            path.push(sep);
            sep = '&';
            path.push_str(name);
            path.push('=');
            path.extend(form_urlencoded::byte_serialize(value.as_bytes()));
        };

        if let Some(nous_id) = &params.nous_id {
            push_param("nous_id", nous_id);
        }
        if let Some(search) = &params.search {
            push_param("search", search);
        }
        if let Some(status) = &params.status {
            push_param("status", status);
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
        Self::check_auth(&resp)?;
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
    /// Returns [`ApiError::Auth`] if the server rejects the authentication token.
    /// Returns [`ApiError::Server`] if the server returns a non-success status.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn history(&self, session_id: &str) -> Result<Vec<HistoryMessage>> {
        let encoded = keryx::url::encode_path_segment(session_id);
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
        Self::check_auth(&resp)?;
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
        let encoded = keryx::url::encode_path_segment(session_id);
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

    /// Unarchive a previously archived session.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn unarchive_session(&self, session_id: &str) -> Result<()> {
        let encoded = keryx::url::encode_path_segment(session_id);
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

    /// Rename a session.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn rename_session(&self, session_id: &str, name: &str) -> Result<()> {
        let encoded = keryx::url::encode_path_segment(session_id);
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
    /// Returns [`ApiError::Auth`] if the server rejects the authentication token.
    /// Returns [`ApiError::Server`] if the server returns a non-success status.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn tools(&self, nous_id: &str) -> Result<Vec<NousTool>> {
        let encoded = keryx::url::encode_path_segment(nous_id);
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

    /// Fetch the server configuration.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::Http`] if the request fails or the response cannot be decoded.
    /// Returns [`ApiError::Auth`] if the server rejects the authentication token.
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
        Self::check_auth(&resp)?;
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
        let encoded = keryx::url::encode_path_segment(section);
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

    fn check_auth(resp: &Response) -> Result<()> {
        if resp.status() == StatusCode::UNAUTHORIZED || resp.status() == StatusCode::FORBIDDEN {
            return AuthSnafu.fail();
        }
        Ok(())
    }

    /// Consumes a response, returning it unchanged if 2xx.
    ///
    /// On non-2xx:
    /// - 429 → [`ApiError::RateLimited`] with `retry_after_secs` parsed
    ///   from the `Retry-After` header (delta-seconds form only).
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
            return RateLimitedSnafu {
                operation,
                retry_after_secs,
            }
            .fail();
        }

        let reason = status.canonical_reason().unwrap_or("Unknown");
        // kanon:ignore RUST/no-result-unwrap-or-default — empty body on text() failure is acceptable; status code is the primary error signal
        let body = resp.text().await.unwrap_or_default();
        let message = parse_pylon_error_body(&body)
            .map_or_else(|| format!("{} {}", status.as_u16(), reason), |d| d.message);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rest_client_builds_with_timeout() {
        let client = build_http_client(None);
        assert!(client.is_ok(), "REST client must build");
    }

    #[test]
    fn streaming_client_builds_without_total_timeout() {
        let client = build_streaming_client(None);
        assert!(client.is_ok(), "streaming client must build");
    }

    #[test]
    fn invalid_token_fails_for_rest_and_streaming() {
        let invalid = "\n";
        assert!(build_http_client(Some(invalid)).is_err());
        assert!(build_streaming_client(Some(invalid)).is_err());
    }

    #[test]
    fn default_headers_use_custom_request_policy() {
        let policy = RequestPolicy {
            csrf: super::super::request_policy::CsrfRequestPolicy {
                enabled: true,
                header_name: "x-aletheia-csrf".to_owned(),
                header_value: "custom-csrf-value".to_owned(),
            },
        };

        let Ok(headers) = default_headers(None, &policy) else {
            panic!("custom policy is valid");
        };

        assert_eq!(
            headers
                .get("x-aletheia-csrf")
                .and_then(|value| value.to_str().ok()),
            Some("custom-csrf-value")
        );
        assert!(headers.get("x-requested-with").is_none());
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
}
