// kanon:ignore RUST/file-too-long — cohesive HTTP client; extracting now would fragment request/response handling
//! HTTP client for the Aletheia gateway REST API.
use std::time::Duration;

use reqwest::{Client, Response, StatusCode, header};
use snafu::prelude::*;

use koina::http::{CSRF_HEADER_NAME, DEFAULT_CSRF_HEADER_VALUE};
use koina::secret::SecretString;

use super::error::{
    ApiError, HttpSnafu, RateLimitedSnafu, Result, ServerSnafu, format_http_error_body,
    parse_pylon_error_body, parse_retry_after_secs,
};
use super::health::{HealthFetchError, parse_health_body};
use super::types::{
    AddCredentialRequest, Agent, AgentPerformance, AgentPerformanceListResponse, AgentsResponse,
    ConfigReloadResponse, ConfigUpdateResponse, CostMetricsResponse, CredentialRemoveResponse,
    CredentialResponse, CredentialsListResponse, DaemonTask, DaemonTaskListResponse,
    EntitiesResponse, ExplainResponse, FactDetailResponse, FactsResponse, FileEntry, FlagRequest,
    FlagSeverity, GitStatusEntry, HealthResponse, HistoryMessage, HistoryResponse, JournalResponse,
    ListSessionsRequest, MergeRequest, NousStatus, NousTool, NousToolsResponse, OpenFileResponse,
    PaginatedSessionsResponse, PendingApprovalsResponse, ProjectVerificationResult,
    ProviderListResponse, ProviderRouteResponse, QualityMetricsResponse, RecoverResponse,
    RelationshipsResponse, SearchResponse, Session, SessionReplayResponse, SessionsResponse,
    TimelineResponse, TokenMetricsResponse, WorkspaceSearchResult, WriteContentRequest,
    WriteContentResponse,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const REST_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

fn default_headers(token: Option<&str>) -> Result<header::HeaderMap> {
    let mut headers = header::HeaderMap::new();

    if let Some(t) = token {
        let auth_value = header::HeaderValue::from_str(&format!("Bearer {t}"))
            .map_err(|_invalid| ApiError::InvalidToken)?;
        headers.insert(header::AUTHORIZATION, auth_value);
    }

    // WHY(#4823, #5059): CSRF header name/value come from the shared
    // `koina::http` constants so this client matches
    // `taxis::config::CsrfConfig::default()` without independently
    // restating the string.
    headers.insert(
        CSRF_HEADER_NAME,
        header::HeaderValue::from_static(DEFAULT_CSRF_HEADER_VALUE),
    );
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
    Client::builder()
        .cookie_store(true)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REST_REQUEST_TIMEOUT)
        .default_headers(default_headers(token)?)
        .build()
        .context(HttpSnafu {
            operation: "build REST HTTP client",
        })
}

/// Build the reqwest client used for long-lived SSE/streaming connections.
pub(crate) fn build_streaming_client(token: Option<&str>) -> Result<Client> {
    // kanon:ignore RUST/missing-http-timeout — SSE connections are long-lived; a request-level timeout would terminate the stream prematurely; connect_timeout guards against connection hang
    Client::builder()
        .cookie_store(true)
        .connect_timeout(CONNECT_TIMEOUT)
        .default_headers(default_headers(token)?)
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
        let client = build_http_client(token.as_deref())?;
        let streaming_client = build_streaming_client(token.as_deref())?;

        Ok(Self {
            client,
            streaming_client,
            base_url: base_url.trim_end_matches('/').to_string(),
            token: token.map(SecretString::from),
        })
    }

    /// Replace the authentication token and rebuild the underlying HTTP
    /// clients so subsequent requests actually carry it.
    ///
    /// WHY(#6818): the token is baked into each client's default
    /// `Authorization` header at construction (`request()` above injects no
    /// per-request header), so mutating only the `token` field left the old
    /// header in place on every live connection — this method previously
    /// did exactly that and had zero call sites anywhere, dead code that
    /// would not have worked had anything called it. Rebuilding both
    /// clients here is what makes in-session re-authentication (a fresh
    /// token replacing an expired one, without restarting) actually take
    /// effect.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::InvalidToken`] if `token` contains characters
    /// invalid in an HTTP header value.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    pub fn set_token(&mut self, token: SecretString) -> Result<()> {
        // kanon:ignore RUST/pub-visibility
        let raw = token.expose_secret();
        self.client = build_http_client(Some(raw))?;
        self.streaming_client = build_streaming_client(Some(raw))?;
        self.token = Some(token);
        Ok(())
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
    /// Hits the operator-only `/api/v1/system/health` route — the
    /// unauthenticated `/api/health` liveness probe carries only `status`
    /// and cannot satisfy [`HealthResponse`] (see [`super::health::parse_liveness_body`]
    /// for that contract).
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
            .get(self.url("/api/v1/system/health"))
            .send()
            .await
            .context(HttpSnafu {
                operation: "health details",
            })?;

        let status = resp.status();
        if status.is_success() || status == StatusCode::SERVICE_UNAVAILABLE {
            let body = resp.text().await.context(HttpSnafu {
                operation: "health details response",
            })?;
            return parse_health_body(status, &body).map_err(|err| match err {
                HealthFetchError::Malformed(message) => {
                    match serde_json::from_str::<HealthResponse>(&body) {
                        Err(source) => ApiError::BadResponse {
                            operation: "health details response",
                            source,
                        },
                        Ok(_unexpected) => ApiError::Server {
                            operation: "health details response",
                            status: status.as_u16(),
                            message,
                        },
                    }
                }
                HealthFetchError::Connection(message) => ApiError::Server {
                    operation: "health details response",
                    status: status.as_u16(),
                    message,
                },
                HealthFetchError::Status(status) => ApiError::Server {
                    operation: "health details request",
                    status: status.as_u16(),
                    message: status.to_string(),
                },
            });
        }

        let resp = Self::check_status(resp, "health details request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "health details response",
        })
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

    /// Fetch the registered LLM provider inventory and readiness.
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
    pub async fn providers(&self) -> Result<ProviderListResponse> {
        let resp = self
            .request(
                reqwest::Method::GET,
                super::routes::providers::providers_path(),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "load providers",
            })?;
        let resp = Self::check_status(resp, "providers request").await?;
        let wrapper: ProviderListResponse = resp.json().await.context(HttpSnafu {
            operation: "providers response",
        })?;
        Ok(wrapper)
    }

    /// Resolve which provider would handle a given model.
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
    pub async fn provider_route(&self, model: &str) -> Result<ProviderRouteResponse> {
        let resp = self
            .request(
                reqwest::Method::GET,
                &super::routes::providers::providers_route_path(model),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "load provider route",
            })?;
        let resp = Self::check_status(resp, "provider route request").await?;
        let wrapper: ProviderRouteResponse = resp.json().await.context(HttpSnafu {
            operation: "provider route response",
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

    /// Fetch the replay-faithful export for a session.
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
    pub async fn session_replay(&self, session_id: &str) -> Result<SessionReplayResponse> {
        let resp = self
            .request(
                reqwest::Method::GET,
                &super::routes::sessions::session_replay_path(session_id),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "session replay export",
            })?;
        let resp = Self::check_status(resp, "session replay export").await?;
        resp.json().await.context(HttpSnafu {
            operation: "session replay response",
        })
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

    /// Resolve a pending tool approval via the session-scoped,
    /// ownership-verifying route (#7202): `POST
    /// /api/v1/sessions/{session_id}/approvals`.
    ///
    /// WHY(#7202): replaces the legacy `approve_tool`/`deny_tool` methods,
    /// which `POST`ed `/api/v1/turns/{turn_id}/tools/{tool_id}/{approve,deny}`
    /// -- a route with no session id at all, so pylon rejects it outright
    /// for any token carrying a `nous_id` (`SECURITY(#5340)` at
    /// `crates/pylon/src/handlers/sessions/approvals.rs`). This route
    /// carries `session_id` so pylon can verify the caller's token owns the
    /// session's agent before routing the decision, working for both scoped
    /// and unscoped tokens -- there is no remaining first-party use for the
    /// legacy route, so it was not kept as a fallback.
    ///
    /// `decision` is the wire vocabulary pylon expects: `"approved"` or
    /// `"denied"`.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn resolve_session_approval(
        &self,
        session_id: &str,
        turn_id: &str,
        tool_id: &str,
        decision: &str,
    ) -> Result<()> {
        let resp = self
            .request(
                reqwest::Method::POST,
                &super::routes::sessions::session_approvals_path(session_id),
            )
            .json(&serde_json::json!({
                "turn_id": turn_id,
                "tool_id": tool_id,
                "decision": decision,
            }))
            .send()
            .await
            .context(HttpSnafu {
                operation: "resolve session approval",
            })?;
        Self::check_status(resp, "session approval request").await?;
        Ok(())
    }

    /// List pending tool approvals for a session (#7207): the read half of
    /// [`Self::resolve_session_approval`]'s model. Lets a client that
    /// connects late, restarts, or reconnects after missing the live
    /// `tool.approval_required` event discover what is still waiting,
    /// instead of depending on having seen it.
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
    pub async fn pending_session_approvals(
        &self,
        session_id: &str,
    ) -> Result<PendingApprovalsResponse> {
        let resp = self
            .request(
                reqwest::Method::GET,
                &super::routes::sessions::session_approvals_path(session_id),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "list session pending approvals",
            })?;
        let resp = Self::check_status(resp, "list session pending approvals request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "list session pending approvals response",
        })
    }

    /// List pending tool approvals across every session belonging to
    /// `nous_id` (#7207): the scoped-token shape of
    /// [`Self::pending_session_approvals`], for a caller that has no
    /// session id to enumerate against.
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
    pub async fn pending_approvals_for_nous(
        &self,
        nous_id: &str,
    ) -> Result<PendingApprovalsResponse> {
        let resp = self
            .request(reqwest::Method::GET, "/api/v1/approvals")
            .query(&[("nous_id", nous_id)])
            .send()
            .await
            .context(HttpSnafu {
                operation: "list nous pending approvals",
            })?;
        let resp = Self::check_status(resp, "list nous pending approvals request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "list nous pending approvals response",
        })
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
    // WHY(#4925): deliberate `Value` exception — config is a dynamic,
    // section-defined settings bag (arbitrary keys per section, no fixed
    // schema Skene can type without duplicating Taxis's config surface).
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
    ) -> Result<FactsResponse> {
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
    pub async fn knowledge_fact_detail(&self, fact_id: &str) -> Result<FactDetailResponse> {
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
    pub async fn knowledge_entities(&self) -> Result<EntitiesResponse> {
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
    ) -> Result<RelationshipsResponse> {
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
    pub async fn knowledge_timeline(&self) -> Result<TimelineResponse> {
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

    /// Semantic/relevance search over the knowledge store (#7197).
    ///
    /// WHY(#7197): pylon has registered `GET /api/v1/knowledge/search` since
    /// before this method existed; the endpoint was never the gap. Both
    /// first-party UIs disabled their search entry points citing a missing
    /// pylon route that was never true — the actual gap was this client
    /// method. `nous_id` scopes results to one agent; `None` searches across
    /// every agent's facts, matching how the rest of the memory inspector
    /// (`knowledge_facts`, `knowledge_timeline`) already reads globally.
    /// `limit` is left to pylon's own default (20) when `None`.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn knowledge_search(
        &self,
        q: &str,
        nous_id: Option<&str>,
        limit: Option<u32>,
    ) -> Result<SearchResponse> {
        let mut params: Vec<(&str, String)> = vec![("q", q.to_string())];
        if let Some(id) = nous_id {
            params.push(("nous_id", id.to_string()));
        }
        if let Some(l) = limit {
            params.push(("limit", l.to_string()));
        }
        let resp = self
            .request(reqwest::Method::GET, "/api/v1/knowledge/search")
            .query(&params)
            .send()
            .await
            .context(HttpSnafu {
                operation: "knowledge search",
            })?;
        let resp = Self::check_status(resp, "knowledge search request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "knowledge search response",
        })
    }

    /// Explainable recall scoring for the same query [`Self::knowledge_search`]
    /// runs, reporting every candidate's per-factor score and why it was
    /// selected, filtered, or dropped (#7197).
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn knowledge_search_explain(
        &self,
        q: &str,
        nous_id: Option<&str>,
        limit: Option<u32>,
    ) -> Result<ExplainResponse> {
        let mut params: Vec<(&str, String)> = vec![("q", q.to_string())];
        if let Some(id) = nous_id {
            params.push(("nous_id", id.to_string()));
        }
        if let Some(l) = limit {
            params.push(("limit", l.to_string()));
        }
        let resp = self
            .request(reqwest::Method::GET, "/api/v1/knowledge/search/explain")
            .query(&params)
            .send()
            .await
            .context(HttpSnafu {
                operation: "knowledge search explain",
            })?;
        let resp = Self::check_status(resp, "knowledge search explain request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "knowledge search explain response",
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

    /// Update the data-sovereignty sensitivity classification for a knowledge
    /// fact. Accepted values (lowercase): `public`, `internal`, `confidential`
    /// — validated server-side by pylon's `PUT .../sensitivity` handler, the
    /// same way `knowledge_update_confidence` leaves range validation to
    /// pylon rather than duplicating it here.
    ///
    /// WHY(#4622): forget/restore/confidence had peer client methods but
    /// sensitivity — the one classification that gates which deployment
    /// targets a fact may reach — had none, so a caller wanting to correct a
    /// fact's sensitivity through skene had no path to pylon's existing
    /// `PUT /api/v1/knowledge/facts/{id}/sensitivity` endpoint.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn knowledge_update_sensitivity(
        &self,
        fact_id: &str,
        sensitivity: &str,
    ) -> Result<()> {
        let encoded = keryx::url::encode_path_segment(fact_id);
        let resp = self
            .request(
                reqwest::Method::PUT,
                &format!("/api/v1/knowledge/facts/{encoded}/sensitivity"),
            )
            .json(&serde_json::json!({ "sensitivity": sensitivity }))
            .send()
            .await
            .context(HttpSnafu {
                operation: "update sensitivity",
            })?;
        Self::check_status(resp, "sensitivity request").await?;
        Ok(())
    }

    /// Fetch canonical backend-wide token usage telemetry (#4987).
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
    pub async fn token_metrics(&self) -> Result<TokenMetricsResponse> {
        let resp = self
            .request(reqwest::Method::GET, "/api/v1/metrics/tokens")
            .send()
            .await
            .context(HttpSnafu {
                operation: "load token metrics",
            })?;
        let resp = Self::check_status(resp, "token metrics request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "token metrics response",
        })
    }

    /// Fetch canonical backend-wide cost telemetry (#4987).
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
    pub async fn cost_metrics(&self) -> Result<CostMetricsResponse> {
        let resp = self
            .request(reqwest::Method::GET, "/api/v1/metrics/costs")
            .send()
            .await
            .context(HttpSnafu {
                operation: "load cost metrics",
            })?;
        let resp = Self::check_status(resp, "cost metrics request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "cost metrics response",
        })
    }

    /// List every daemon task with persisted execution history, across every
    /// attached runner (#7206).
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
    pub async fn daemon_tasks(&self) -> Result<Vec<DaemonTask>> {
        let resp = self
            .request(
                reqwest::Method::GET,
                super::routes::system::daemon_tasks_path(),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "load daemon tasks",
            })?;
        let resp = Self::check_status(resp, "daemon tasks request").await?;
        let wrapper: DaemonTaskListResponse = resp.json().await.context(HttpSnafu {
            operation: "daemon tasks response",
        })?;
        Ok(wrapper.tasks)
    }

    /// Fully re-enable a daemon task, resetting its failure history (#7206).
    ///
    /// Equivalent to the `aletheia maintenance reset` CLI command.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::Http`] if the request fails or the response cannot be decoded.
    /// Returns [`ApiError::Server`] if the server returns a non-success status
    /// (including 404 for an unknown `runner`/`task_id`).
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn enable_daemon_task(&self, runner: &str, task_id: &str) -> Result<DaemonTask> {
        let resp = self
            .request(
                reqwest::Method::POST,
                &super::routes::system::daemon_task_enable_path(runner, task_id),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "enable daemon task",
            })?;
        let resp = Self::check_status(resp, "enable daemon task request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "enable daemon task response",
        })
    }

    /// Explicitly disable a daemon task (#7206).
    ///
    /// Unlike an auto-disable, this persists an `operator` cause that is
    /// never re-armed on the daemon's own restart -- only [`Self::enable_daemon_task`]
    /// or [`Self::retry_daemon_task`] re-enables it. `reason`, when given, is
    /// recorded as the task's operator-facing `last_error`.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::Http`] if the request fails or the response cannot be decoded.
    /// Returns [`ApiError::Server`] if the server returns a non-success status
    /// (including 404 for an unknown `runner`/`task_id`).
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn disable_daemon_task(
        &self,
        runner: &str,
        task_id: &str,
        reason: Option<&str>,
    ) -> Result<DaemonTask> {
        let resp = self
            .request(
                reqwest::Method::POST,
                &super::routes::system::daemon_task_disable_path(runner, task_id),
            )
            .json(&serde_json::json!({ "reason": reason }))
            .send()
            .await
            .context(HttpSnafu {
                operation: "disable daemon task",
            })?;
        let resp = Self::check_status(resp, "disable daemon task request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "disable daemon task response",
        })
    }

    /// Give a disabled daemon task exactly one more attempt now, without
    /// resetting its failure history (#7206).
    ///
    /// Unlike [`Self::enable_daemon_task`], a subsequent failure re-disables
    /// the task immediately rather than after three fresh strikes -- see the
    /// pylon handler's doc comment for why.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::Http`] if the request fails or the response cannot be decoded.
    /// Returns [`ApiError::Server`] if the server returns a non-success status
    /// (including 404 for an unknown `runner`/`task_id`).
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn retry_daemon_task(&self, runner: &str, task_id: &str) -> Result<DaemonTask> {
        let resp = self
            .request(
                reqwest::Method::POST,
                &super::routes::system::daemon_task_retry_path(runner, task_id),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "retry daemon task",
            })?;
        let resp = Self::check_status(resp, "retry daemon task request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "retry daemon task response",
        })
    }

    // ── Workspace (#4565) ──────────────────────────────────────────────
    //
    // WHY(#4565): proskenion's file browser, viewer, and diff/search views
    // called these routes directly through a duplicate HTTP client
    // (`crates/theatron/proskenion/src/api/client.rs`) because skene wrapped
    // none of pylon's seven workspace routes. These wrap all seven so the
    // desktop can move onto skene domain by domain without a capability gap.

    /// List workspace files and directories, optionally scoped to `path`.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn workspace_files(&self, path: Option<&str>) -> Result<Vec<FileEntry>> {
        let resp = self
            .request(
                reqwest::Method::GET,
                &super::routes::workspace::files_path(path),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "load workspace files",
            })?;
        let resp = Self::check_status(resp, "workspace files request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "workspace files response",
        })
    }

    /// Fetch normalized git-status entries for the workspace.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn workspace_git_status(&self) -> Result<Vec<GitStatusEntry>> {
        let resp = self
            .request(
                reqwest::Method::GET,
                super::routes::workspace::git_status_path(),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "load workspace git status",
            })?;
        let resp = Self::check_status(resp, "workspace git status request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "workspace git status response",
        })
    }

    /// Fetch raw content for one workspace file.
    ///
    /// Returns the raw response bytes rather than a typed body: pylon's
    /// `Content-Type` is guessed from the file extension and the content may
    /// be binary, so text/binary handling is left to the caller -- the same
    /// split proskenion's file viewer already makes on the client side.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn workspace_file_content(&self, path: &str) -> Result<Vec<u8>> {
        let resp = self
            .request(
                reqwest::Method::GET,
                &super::routes::workspace::content_path(path),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "load workspace file content",
            })?;
        let resp = Self::check_status(resp, "workspace file content request").await?;
        let bytes = resp.bytes().await.context(HttpSnafu {
            operation: "workspace file content response",
        })?;
        Ok(bytes.to_vec())
    }

    /// Write UTF-8 text content back to a workspace file.
    ///
    /// `if_match_mtime_ms`, when set, is an optimistic-concurrency guard:
    /// pylon rejects the write with `409` if the on-disk mtime has since
    /// changed.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self, content))]
    pub async fn workspace_write_file(
        &self,
        path: &str,
        content: &str,
        if_match_mtime_ms: Option<i64>,
    ) -> Result<WriteContentResponse> {
        let resp = self
            .request(
                reqwest::Method::PUT,
                super::routes::workspace::content_write_path(),
            )
            .json(&WriteContentRequest {
                path: path.to_owned(),
                content: content.to_owned(),
                if_match_mtime_ms,
            })
            .send()
            .await
            .context(HttpSnafu {
                operation: "write workspace file",
            })?;
        let resp = Self::check_status(resp, "workspace write request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "workspace write response",
        })
    }

    /// Dispatch a workspace file to the system default application.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn workspace_open_file(&self, path: &str) -> Result<OpenFileResponse> {
        let resp = self
            .request(reqwest::Method::POST, super::routes::workspace::open_path())
            .json(&serde_json::json!({ "path": path }))
            .send()
            .await
            .context(HttpSnafu {
                operation: "open workspace file",
            })?;
        let resp = Self::check_status(resp, "workspace open request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "workspace open response",
        })
    }

    /// Fetch a unified `git diff` for one workspace file.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn workspace_diff(&self, path: &str) -> Result<String> {
        let resp = self
            .request(
                reqwest::Method::GET,
                &super::routes::workspace::diff_path(path),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "load workspace diff",
            })?;
        let resp = Self::check_status(resp, "workspace diff request").await?;
        resp.text().await.context(HttpSnafu {
            operation: "workspace diff response",
        })
    }

    /// Search workspace filenames and content for `q`, capped at `limit`
    /// results.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn workspace_search(
        &self,
        q: &str,
        limit: usize,
    ) -> Result<Vec<WorkspaceSearchResult>> {
        let resp = self
            .request(
                reqwest::Method::GET,
                &super::routes::workspace::search_path(q, limit),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "search workspace",
            })?;
        let resp = Self::check_status(resp, "workspace search request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "workspace search response",
        })
    }

    // ── Credentials (#4565) ────────────────────────────────────────────
    //
    // WHY(#4565): route templates for all five credential routes already
    // existed in `routes::system` with zero client methods wrapping them;
    // proskenion's operator credentials view called pylon directly instead.

    /// List managed provider credentials (secret-safe metadata only).
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn list_credentials(&self) -> Result<CredentialsListResponse> {
        let resp = self
            .request(
                reqwest::Method::GET,
                super::routes::system::credentials_path(),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "load credentials",
            })?;
        let resp = Self::check_status(resp, "credentials request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "credentials response",
        })
    }

    /// Add a managed provider credential.
    ///
    /// `key` is sent once, in the request body, to be stored encrypted at
    /// rest server-side; skene does not itself persist it (see
    /// `secret_store` for the client-local credential cache this method does
    /// not touch).
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self, key))]
    pub async fn add_credential(
        &self,
        provider: &str,
        key: SecretString,
        role: &str,
    ) -> Result<CredentialResponse> {
        let resp = self
            .request(
                reqwest::Method::POST,
                super::routes::system::credentials_path(),
            )
            .json(&AddCredentialRequest {
                provider: provider.to_owned(),
                key,
                role: role.to_owned(),
            })
            .send()
            .await
            .context(HttpSnafu {
                operation: "add credential",
            })?;
        let resp = Self::check_status(resp, "add credential request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "add credential response",
        })
    }

    /// Remove one managed credential.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn remove_credential(&self, id: &str) -> Result<CredentialRemoveResponse> {
        let resp = self
            .request(
                reqwest::Method::DELETE,
                &super::routes::system::credential_path(id),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "remove credential",
            })?;
        let resp = Self::check_status(resp, "remove credential request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "remove credential response",
        })
    }

    /// Validate one managed credential against its provider.
    ///
    /// A real network round trip for providers skene knows how to reach
    /// live; the outcome is persisted server-side (see
    /// [`super::types::CredentialValidationState`]).
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn validate_credential(&self, id: &str) -> Result<CredentialResponse> {
        let resp = self
            .request(
                reqwest::Method::POST,
                &super::routes::system::credential_validate_path(id),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "validate credential",
            })?;
        let resp = Self::check_status(resp, "validate credential request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "validate credential response",
        })
    }

    /// Swap the primary and backup credentials for `provider`.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn rotate_credentials(&self, provider: &str) -> Result<CredentialsListResponse> {
        let resp = self
            .request(
                reqwest::Method::POST,
                &super::routes::system::credential_rotate_path(provider),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "rotate credentials",
            })?;
        let resp = Self::check_status(resp, "rotate credentials request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "rotate credentials response",
        })
    }

    // ── Planning (#4565) ───────────────────────────────────────────────
    //
    // WHY(#4565): `ProjectVerificationResult` and its route templates
    // already existed with zero client methods wrapping them; proskenion's
    // planning verification view called pylon directly instead.

    /// Fetch the verification result for one planning project.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn project_verification(
        &self,
        project_id: &str,
    ) -> Result<ProjectVerificationResult> {
        let resp = self
            .request(
                reqwest::Method::GET,
                &super::routes::planning::project_verification_path(project_id),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "load project verification",
            })?;
        let resp = Self::check_status(resp, "project verification request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "project verification response",
        })
    }

    /// Re-run verification for one planning project and fetch the refreshed
    /// result.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn refresh_project_verification(
        &self,
        project_id: &str,
    ) -> Result<ProjectVerificationResult> {
        let resp = self
            .request(
                reqwest::Method::POST,
                &super::routes::planning::project_verification_refresh_path(project_id),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "refresh project verification",
            })?;
        let resp = Self::check_status(resp, "project verification refresh request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "project verification refresh response",
        })
    }

    // ── Feature flags and config reload (#4565) ─────────────────────────

    /// Replace the entire `feature_flags` config section.
    ///
    /// WHY: sends the complete section rather than a partial patch --
    /// pylon's `PUT /api/v1/config/{section}` replaces the section wholesale,
    /// so a partial payload here would silently drop sibling flags, exactly
    /// as proskenion's own feature-flags view already documents.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self, flags))]
    pub async fn update_feature_flags(
        &self,
        flags: &serde_json::Value,
    ) -> Result<ConfigUpdateResponse> {
        let resp = self
            .request(
                reqwest::Method::PUT,
                &super::routes::config::feature_flags_path(),
            )
            .json(flags)
            .send()
            .await
            .context(HttpSnafu {
                operation: "update feature flags",
            })?;
        let resp = Self::check_status(resp, "feature flags update request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "feature flags update response",
        })
    }

    /// Re-read `aletheia.toml` from disk and apply hot-reloadable changes.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn reload_config(&self) -> Result<ConfigReloadResponse> {
        let resp = self
            .request(reqwest::Method::POST, super::routes::config::reload_path())
            .send()
            .await
            .context(HttpSnafu {
                operation: "reload config",
            })?;
        let resp = Self::check_status(resp, "config reload request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "config reload response",
        })
    }

    // ── Nous get-one / recover (#4565) ──────────────────────────────────

    /// Fetch detailed status for one nous agent.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn agent_status(&self, id: &str) -> Result<NousStatus> {
        let resp = self
            .request(reqwest::Method::GET, &super::routes::nous::agent_path(id))
            .send()
            .await
            .context(HttpSnafu {
                operation: "load agent status",
            })?;
        let resp = Self::check_status(resp, "agent status request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "agent status response",
        })
    }

    /// Reset a degraded nous agent back to idle.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn agent_recover(&self, id: &str) -> Result<RecoverResponse> {
        let resp = self
            .request(
                reqwest::Method::POST,
                &super::routes::nous::agent_recover_path(id),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "recover agent",
            })?;
        let resp = Self::check_status(resp, "agent recover request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "agent recover response",
        })
    }

    // ── Metrics dashboards (#4565) ───────────────────────────────────────
    //
    // WHY(#4565): rounds out `token_metrics`/`cost_metrics` with the
    // remaining dashboard views (agent performance, quality, journal)
    // proskenion's meta/ops views fetched directly instead.

    /// Fetch performance metrics for every agent, with cross-agent anomaly
    /// alerts.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn agent_performance(&self) -> Result<AgentPerformanceListResponse> {
        let resp = self
            .request(reqwest::Method::GET, super::routes::metrics::agents_path())
            .send()
            .await
            .context(HttpSnafu {
                operation: "load agent performance",
            })?;
        let resp = Self::check_status(resp, "agent performance request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "agent performance response",
        })
    }

    /// Fetch performance metrics for a single agent.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn agent_performance_one(&self, id: &str) -> Result<AgentPerformance> {
        let resp = self
            .request(
                reqwest::Method::GET,
                &super::routes::metrics::agent_performance_path(id),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "load agent performance detail",
            })?;
        let resp = Self::check_status(resp, "agent performance detail request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "agent performance detail response",
        })
    }

    /// Fetch conversation-quality time series metrics.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn quality_metrics(&self) -> Result<QualityMetricsResponse> {
        let resp = self
            .request(reqwest::Method::GET, super::routes::metrics::quality_path())
            .send()
            .await
            .context(HttpSnafu {
                operation: "load quality metrics",
            })?;
        let resp = Self::check_status(resp, "quality metrics request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "quality metrics response",
        })
    }

    /// Fetch recent system journal events.
    ///
    /// WHY: pylon has never had a persistent event journal to back this
    /// endpoint (`data_unavailable` on every response says so honestly); the
    /// method is wired anyway so a future backing store needs no new client
    /// surface.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn journal(&self) -> Result<JournalResponse> {
        let resp = self
            .request(reqwest::Method::GET, super::routes::metrics::journal_path())
            .send()
            .await
            .context(HttpSnafu {
                operation: "load journal",
            })?;
        let resp = Self::check_status(resp, "journal request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "journal response",
        })
    }

    // ── Entity merge / delete / flag (#4565) ────────────────────────────
    //
    // WHY(#4565): rounds out the knowledge-entities surface
    // (`knowledge_entities`, `knowledge_entity_relationships`) with the three
    // operator mutation routes proskenion's entity actions view called
    // directly instead. All three return `204 No Content` on success.

    /// Merge `merged_id` into `canonical_id`, removing the merged entity.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn merge_entities(&self, canonical_id: &str, merged_id: &str) -> Result<()> {
        let resp = self
            .request(reqwest::Method::POST, "/api/v1/knowledge/entities/merge")
            .json(&MergeRequest {
                canonical_id: canonical_id.to_owned(),
                merged_id: merged_id.to_owned(),
            })
            .send()
            .await
            .context(HttpSnafu {
                operation: "merge entities",
            })?;
        Self::check_status(resp, "merge entities request").await?;
        Ok(())
    }

    /// Persist an operator review flag against an entity. The latest flag
    /// for an entity overwrites any previous flag.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn flag_entity(
        &self,
        entity_id: &str,
        reason: &str,
        severity: FlagSeverity,
    ) -> Result<()> {
        let encoded = keryx::url::encode_path_segment(entity_id);
        let resp = self
            .request(
                reqwest::Method::POST,
                &format!("/api/v1/knowledge/entities/{encoded}/flag"),
            )
            .json(&FlagRequest {
                reason: reason.to_owned(),
                severity,
            })
            .send()
            .await
            .context(HttpSnafu {
                operation: "flag entity",
            })?;
        Self::check_status(resp, "flag entity request").await?;
        Ok(())
    }

    /// Delete an entity and its relationship links.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn delete_entity(&self, entity_id: &str) -> Result<()> {
        let encoded = keryx::url::encode_path_segment(entity_id);
        let resp = self
            .request(
                reqwest::Method::DELETE,
                &format!("/api/v1/knowledge/entities/{encoded}"),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "delete entity",
            })?;
        Self::check_status(resp, "delete entity request").await?;
        Ok(())
    }

    /// Consumes a response, returning it unchanged if 2xx.
    ///
    /// On non-2xx:
    /// - 401/403 → [`ApiError::Auth`], distinct from the generic
    ///   [`ApiError::Server`] variant so callers can tell "credentials
    ///   rejected" apart from every other server-side failure and offer
    ///   re-authentication instead of generic troubleshooting advice.
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

        // WHY(#6818): matches keryx's own `response::ensure_success` contract
        // (401/403 -> ApiError::Auth) rather than the doc-only claim it made
        // before this fix -- this client reimplements status classification
        // instead of calling that helper, and had never actually produced
        // ApiError::Auth, so every credential rejection fell into the generic
        // Server variant and every consumer's error message read like a
        // server-side failure with nothing to distinguish it from one.
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(ApiError::Auth);
        }

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
    ///
    /// WHY(#4925): crate-private — skene is the sole typed protocol boundary
    /// for first-party clients; a public escape hatch let a consumer bypass
    /// route/DTO/error semantics while still looking like it used the shared
    /// client. Confirmed zero external callers before tightening visibility.
    ///
    /// WHY test-only: narrowing the visibility left the tests as its sole
    /// caller, so a lib build reports it dead -- an error under `-D warnings`.
    /// Gating on `test` states that honestly rather than re-widening the
    /// boundary this method was narrowed to enforce.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn raw_client(&self) -> &Client {
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
    #![expect(clippy::expect_used, reason = "test helper failures should panic")]
    #![expect(
        clippy::indexing_slicing,
        reason = "test: each index is guarded by an asserted len on the line above"
    )]

    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    use crate::api::types::{CredentialMutationEffect, CredentialValidationState, ExplainDecision};

    use super::*;

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

    /// Like [`serve_http_error_once`], but returns the raw request the
    /// client sent instead of driving an error path — for asserting a
    /// wrapper method built the wire request (method, path, body) the
    /// server actually expects, rather than only that it did not error.
    fn serve_http_capture_once(
        status_line: &'static str,
        body: &'static str,
    ) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local test server");
        let addr = listener.local_addr().expect("read local test server addr");
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept test request");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("set read timeout");
            let mut buf = [0_u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            // WHY get(): `n` comes from a read whose contract does not bind it
            // to buf's length, so indexing is a panic clippy::indexing_slicing
            // correctly refuses in a mock server a test depends on.
            let request = String::from_utf8_lossy(buf.get(..n).unwrap_or(&[])).into_owned();
            let response = format!(
                "HTTP/1.1 {status_line}\r\ncontent-type: application/json\r\nconnection: close\r\n\r\n{body}"
            );
            stream
                .write_all(response.as_bytes())
                .expect("write HTTP capture test response");
            request
        });
        (format!("http://{addr}"), handle)
    }

    #[tokio::test]
    async fn knowledge_update_sensitivity_puts_the_pylon_sensitivity_route() {
        // WHY(#4622): before this method existed, nothing verified skene would
        // even form the right request — a typo'd path or verb here would 404
        // or 405 silently against a real pylon instance with no compile-time
        // or test-time signal, since this was previously untestable (the
        // method did not exist to call).
        crate::install_test_crypto_provider();
        let (base_url, server) = serve_http_capture_once("200 OK", r#"{"status":"updated"}"#);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        client
            .knowledge_update_sensitivity("f-abc123", "confidential")
            .await
            .expect("sensitivity update should succeed");

        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("PUT /api/v1/knowledge/facts/f-abc123/sensitivity"),
            "must PUT pylon's sensitivity route, got: {request}"
        );
        assert!(
            request.contains(r#"{"sensitivity":"confidential"}"#),
            "body must carry the requested sensitivity value, got: {request}"
        );
    }

    #[tokio::test]
    async fn knowledge_search_gets_the_pylon_search_route_with_query_params() {
        // WHY(#7197): before this method existed, koilon's `/` and `:recall`
        // both claimed pylon had no search endpoint. It always did
        // (`GET /api/v1/knowledge/search`) -- this asserts skene actually
        // reaches it with the caller's query text, agent scope, and limit as
        // URL-encoded query parameters, and parses the ranked result set.
        crate::install_test_crypto_provider();
        let body = r#"{"results":[{"id":"f-1","content":"hello world","confidence":0.9,"tier":"verified","fact_type":"knowledge","score":1.5}]}"#;
        let (base_url, server) = serve_http_capture_once("200 OK", body);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        let response = client
            .knowledge_search("hello world", Some("syn"), Some(5))
            .await
            .expect("knowledge search should succeed");

        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("GET /api/v1/knowledge/search?"),
            "must GET pylon's search route, got: {request}"
        );
        assert!(request.contains("q=hello+world") || request.contains("q=hello%20world"));
        assert!(request.contains("nous_id=syn"));
        assert!(request.contains("limit=5"));
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].id, "f-1");
        assert!((response.results[0].score - 1.5).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn knowledge_search_omits_optional_params_when_absent() {
        crate::install_test_crypto_provider();
        let (base_url, server) = serve_http_capture_once("200 OK", r#"{"results":[]}"#);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        client
            .knowledge_search("q", None, None)
            .await
            .expect("knowledge search should succeed");

        let request = server.join().expect("test server thread should finish");
        assert!(!request.contains("nous_id="), "got: {request}");
        assert!(!request.contains("limit="), "got: {request}");
    }

    #[tokio::test]
    async fn knowledge_search_explain_gets_the_pylon_explain_route() {
        crate::install_test_crypto_provider();
        // WHY: pylon serializes these DTOs with plain (snake_case) field
        // names (crates/pylon/src/handlers/knowledge/dto.rs has no
        // `rename_all`) -- this body uses that wire casing directly so the
        // test fails if skene's types ever drifted to expect camelCase.
        let body = r#"{
            "query": "hello",
            "weights": {
                "vector_similarity": 0.3, "decay": 0.1, "relevance": 0.2,
                "epistemic_tier": 0.1, "access_frequency": 0.1,
                "relationship_proximity": 0.1, "graph_importance": 0.1,
                "serendipity": 0.0, "surprise": 0.0, "evidence_coverage": 0.0,
                "convergence": 0.0
            },
            "total_candidates": 1,
            "selected": [{
                "id": "f-1", "content": "hello", "confidence": 0.9,
                "tier": "verified", "fact_type": "knowledge", "score": 1.2,
                "decision": "selected", "reasons": ["matched"],
                "factors": {
                    "vector_similarity": 0.3, "decay": 0.1, "relevance": 0.2,
                    "epistemic_tier": 0.1, "access_frequency": 0.1,
                    "relationship_proximity": 0.1, "graph_importance": 0.1
                }
            }],
            "dropped": []
        }"#;
        let (base_url, server) = serve_http_capture_once("200 OK", body);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        let response = client
            .knowledge_search_explain("hello", None, None)
            .await
            .expect("knowledge search explain should succeed");

        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("GET /api/v1/knowledge/search/explain?"),
            "must GET pylon's explain route, got: {request}"
        );
        assert_eq!(response.total_candidates, 1);
        assert_eq!(response.selected.len(), 1);
        assert!(matches!(
            response.selected[0].decision,
            ExplainDecision::Selected
        ));
    }

    #[tokio::test]
    async fn resolve_session_approval_posts_the_session_scoped_route() {
        // WHY(#7202): before this method existed, koilon and proskenion both
        // called approve_tool/deny_tool against the legacy
        // /api/v1/turns/{turn_id}/tools/{tool_id}/{approve,deny} route, which
        // pylon rejects for any token carrying a nous_id (SECURITY(#5340)).
        // This asserts the session-scoped route is actually hit, with the
        // session id in the path and turn_id/tool_id/decision in the body.
        crate::install_test_crypto_provider();
        let (base_url, server) = serve_http_capture_once("200 OK", r#"{"decision":"approved"}"#);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        client
            .resolve_session_approval("ses-1", "turn-1", "tool-1", "approved")
            .await
            .expect("session approval should succeed");

        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("POST /api/v1/sessions/ses-1/approvals"),
            "must POST pylon's session-scoped approval route, got: {request}"
        );
        assert!(request.contains(r#""turn_id":"turn-1""#), "got: {request}");
        assert!(request.contains(r#""tool_id":"tool-1""#), "got: {request}");
        assert!(
            request.contains(r#""decision":"approved""#),
            "got: {request}"
        );
    }

    #[tokio::test]
    async fn pending_session_approvals_gets_the_session_scoped_route() {
        // WHY(#7207): the read half of `resolve_session_approval` -- a
        // client that reconnects must be able to list what is still
        // pending on the same session-scoped route the write uses.
        crate::install_test_crypto_provider();
        let body = r#"{"approvals":[{"session_id":"ses-1","turn_id":"turn-1","tool_id":"tool-1","tool_name":"shell_execute","risk":"critical","requested_at":"2026-01-01T00:00:00Z","deadline":"2026-01-01T00:02:00Z"}]}"#;
        let (base_url, server) = serve_http_capture_once("200 OK", body);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        let response = client
            .pending_session_approvals("ses-1")
            .await
            .expect("pending session approvals should succeed");

        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("GET /api/v1/sessions/ses-1/approvals"),
            "must GET pylon's session-scoped approval route, got: {request}"
        );
        assert_eq!(response.approvals.len(), 1);
        assert_eq!(response.approvals[0].tool_id, "tool-1");
        assert_eq!(response.approvals[0].turn_id, "turn-1");
        assert_eq!(response.approvals[0].risk, "critical");
        assert_eq!(response.approvals[0].deadline, "2026-01-01T00:02:00Z");
    }

    #[tokio::test]
    async fn pending_approvals_for_nous_gets_the_nous_scoped_route_with_query_param() {
        // WHY(#7207): the scoped-token shape -- a caller with no session id
        // to enumerate against lists by agent instead.
        crate::install_test_crypto_provider();
        let (base_url, server) = serve_http_capture_once("200 OK", r#"{"approvals":[]}"#);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        let response = client
            .pending_approvals_for_nous("syn")
            .await
            .expect("pending nous approvals should succeed");

        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("GET /api/v1/approvals?"),
            "must GET pylon's nous-scoped approval route, got: {request}"
        );
        assert!(request.contains("nous_id=syn"), "got: {request}");
        assert!(response.approvals.is_empty());
    }

    #[tokio::test]
    async fn session_replay_gets_the_pylon_replay_route_and_parses_the_full_dto() {
        // WHY(#4913): the koilon TUI export previously had no way to fetch a
        // replay-faithful export at all -- session_replay did not exist on
        // this client. This asserts both the wire route (GET .../replay) and
        // that fields the TUI's Markdown export cannot carry -- tool error/
        // approval detail, usage records -- survive the round trip.
        crate::install_test_crypto_provider();
        let body = r#"{
            "version": 1,
            "exportType": "replay",
            "exportedAt": "2026-01-01T00:00:00Z",
            "session": {
                "id": "s1",
                "nousId": "syn",
                "sessionKey": "key",
                "status": "active",
                "sessionType": "chat",
                "messageCount": 1,
                "tokenCountEstimate": 10,
                "distillationCount": 0,
                "createdAt": "2026-01-01T00:00:00Z",
                "updatedAt": "2026-01-01T00:00:00Z",
                "lastInputTokens": 5,
                "computedContextTokens": 5
            },
            "messages": [{
                "id": 1,
                "seq": 1,
                "role": "assistant",
                "content": "hi",
                "tokenEstimate": 2,
                "isDistilled": false,
                "createdAt": "2026-01-01T00:00:00Z"
            }],
            "usageRecords": [{
                "turnSeq": 1,
                "inputTokens": 5,
                "outputTokens": 5,
                "cacheReadTokens": 0,
                "cacheWriteTokens": 0
            }],
            "toolAuditRecords": [{
                "id": 1,
                "nousId": "syn",
                "turnSeq": 1,
                "toolCallId": "tc1",
                "toolName": "read_file",
                "durationMs": 10,
                "isError": true,
                "outcome": "error",
                "result": "boom",
                "approval": "auto",
                "createdAt": "2026-01-01T00:00:00Z"
            }],
            "turnAttempts": [{
                "version": 1,
                "turnId": "t1",
                "sessionId": "s1",
                "nousId": "syn",
                "status": "complete",
                "createdAt": "2026-01-01T00:00:00Z"
            }]
        }"#;
        let (base_url, server) = serve_http_capture_once("200 OK", body);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        let replay = client
            .session_replay("s1")
            .await
            .expect("session replay export should succeed");

        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("GET /api/v1/sessions/s1/replay"),
            "must GET pylon's replay route, got: {request}"
        );
        assert_eq!(replay.messages.len(), 1);
        let usage = replay.usage_records.first().expect("one usage record");
        assert_eq!(usage.input_tokens, 5);
        let audit = replay
            .tool_audit_records
            .first()
            .expect("one tool audit record");
        assert!(audit.is_error);
        assert_eq!(audit.approval.as_deref(), Some("auto"));
        let attempt = replay.turn_attempts.first().expect("one turn attempt");
        assert_eq!(attempt.status, "complete");
    }

    #[test]
    fn rest_client_builds_with_timeout() {
        crate::install_test_crypto_provider();
        let client = build_http_client(None);
        assert!(client.is_ok(), "REST client must build");
    }

    #[test]
    fn streaming_client_builds_without_total_timeout() {
        crate::install_test_crypto_provider();
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
    fn api_client_provides_distinct_rest_and_streaming_clients() {
        crate::install_test_crypto_provider();
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
        crate::install_test_crypto_provider();
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
        crate::install_test_crypto_provider();
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
        crate::install_test_crypto_provider();
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

    #[tokio::test]
    async fn rest_401_maps_to_auth_error_not_generic_server() {
        crate::install_test_crypto_provider();
        let (base_url, server) = serve_http_error_once(
            "401 Unauthorized",
            "",
            r#"{"error":{"code":"auth_failed","message":"invalid token"}}"#,
        );
        let client =
            ApiClient::new(&base_url, Some("stale-token".to_string())).expect("build test client");

        let Err(err) = client.agents().await else {
            panic!("agents request should fail on 401");
        };
        server.join().expect("test server thread should finish");

        assert!(
            matches!(err, ApiError::Auth),
            "expected ApiError::Auth, got {err:?}"
        );
    }

    #[tokio::test]
    async fn rest_403_maps_to_auth_error_not_generic_server() {
        crate::install_test_crypto_provider();
        let (base_url, server) =
            serve_http_error_once("403 Forbidden", "", r#"{"message":"forbidden"}"#);
        let client =
            ApiClient::new(&base_url, Some("stale-token".to_string())).expect("build test client");

        let Err(err) = client.agents().await else {
            panic!("agents request should fail on 403");
        };
        server.join().expect("test server thread should finish");

        assert!(
            matches!(err, ApiError::Auth),
            "expected ApiError::Auth, got {err:?}"
        );
    }

    #[tokio::test]
    async fn set_token_rebuilds_headers_used_by_subsequent_requests() {
        crate::install_test_crypto_provider();
        let (base_url, server) = serve_http_capture_once("200 OK", "{}");
        let mut client =
            ApiClient::new(&base_url, Some("old-token".to_string())).expect("build test client");

        client
            .set_token(SecretString::from("fresh-token"))
            .expect("set_token should accept a valid header value");
        assert_eq!(client.token(), Some("fresh-token"));

        // WHY(#6818): before this fix, set_token only mutated the `token`
        // field -- the reqwest clients built at construction kept the old
        // Authorization header baked into their default headers, so a
        // caller who replaced an expired token still sent the expired one
        // on every subsequent request. This asserts the wire request, not
        // just the field, so a regression to field-only mutation fails
        // here even though `client.token()` above would still read right.
        let _ = client.agents().await;
        let request = server.join().expect("test server thread should finish");
        // NOTE: header name is asserted lowercase -- `http`'s HeaderName
        // stores/emits the canonical lowercase form on the wire (hyper
        // does not title-case HTTP/1.1 headers unless
        // `http1_title_case_headers()` is set, which this client does
        // not), so a mock server's raw capture reads
        // "authorization: ..." rather than "Authorization: ...".
        assert!(
            request.contains("authorization: Bearer fresh-token"),
            "request should carry the token set via set_token, got: {request}"
        );
        assert!(
            !request.contains("Bearer old-token"),
            "request must not carry the stale token set_token replaced, got: {request}"
        );
    }

    #[tokio::test]
    async fn health_details_hits_the_operator_readiness_route_not_liveness() {
        crate::install_test_crypto_provider();
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let addr = listener.local_addr().expect("read test server addr");
        let body = serde_json::to_string(&HealthResponse {
            status: "healthy".to_string(),
            version: "0.1.0".to_string(),
            git_sha: "deadbeef".to_string().into(),
            uptime_seconds: 42,
            checks: vec![],
            data_dir: "/tmp".to_string(),
        })
        .expect("HealthResponse must serialize");
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept test request");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("set read timeout");
            let mut buf = [0_u8; 2048];
            let n = stream.read(&mut buf).expect("read request");
            // WHY get(): `n` comes from a read whose contract does not bind it
            // to buf's length, so indexing is a panic clippy::indexing_slicing
            // correctly refuses in a mock server a test depends on.
            let request_line = String::from_utf8_lossy(buf.get(..n).unwrap_or(&[]))
                .lines()
                .next()
                .unwrap_or_default()
                .to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\ncontent-length: {}\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .expect("write health details test response");
            request_line
        });

        let base_url = format!("http://{addr}");
        let client = ApiClient::new(&base_url, None).expect("build test client");
        let health = client
            .health_details()
            .await
            .expect("real HealthResponse body must parse");
        assert_eq!(health.status, "healthy");

        let request_line = handle.join().expect("test server thread should finish");
        // WHY: a mock server answers any path with the same canned body, so the
        // only way to catch a regression back to the liveness-only route is to
        // inspect the path the client actually requested.
        assert!(
            request_line.contains("/api/v1/system/health"),
            "health_details must request the operator readiness route, got: {request_line}"
        );
        assert!(
            !request_line.contains("GET /api/health "),
            "health_details must not request the liveness-only route, got: {request_line}"
        );
    }

    #[tokio::test]
    async fn providers_gets_the_pylon_providers_route_and_parses_the_dto() {
        // WHY(#4890): before this method existed, no client surface could call
        // GET /api/v1/providers at all -- proskenion had no way to render
        // provider inventory. Asserts both the wire route and that
        // health/auth_source/available survive the round trip.
        crate::install_test_crypto_provider();
        let body = r#"{"providers":[{
            "name": "anthropic-primary",
            "kind": "anthropic",
            "deployment_target": "cloud",
            "base_url": "https://api.anthropic.com",
            "supported_models": ["claude-opus-4-6"],
            "configured_models": ["claude-opus-4-6"],
            "health": "up",
            "health_reason": null,
            "auth_source": "env:ANTHROPIC_API_KEY",
            "available": true
        }]}"#;
        let (base_url, server) = serve_http_capture_once("200 OK", body);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        let resp = client
            .providers()
            .await
            .expect("real ProviderListResponse body must parse");

        assert_eq!(resp.providers.len(), 1);
        assert_eq!(resp.providers[0].name, "anthropic-primary");
        assert_eq!(resp.providers[0].health, "up");
        assert!(resp.providers[0].available);
        assert_eq!(resp.providers[0].auth_source, "env:ANTHROPIC_API_KEY");

        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("GET /api/v1/providers "),
            "providers must GET pylon's providers route, got: {request}"
        );
    }

    #[tokio::test]
    async fn provider_route_gets_the_pylon_route_endpoint_with_model_query() {
        // WHY(#4890): pins the query-param encoding for the model lookup and
        // that the resolved provider/health/available fields round-trip.
        crate::install_test_crypto_provider();
        let body = r#"{
            "model": "claude-opus-4-6",
            "provider": "anthropic-primary",
            "health": "up",
            "available": true
        }"#;
        let (base_url, server) = serve_http_capture_once("200 OK", body);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        let resp = client
            .provider_route("claude-opus-4-6")
            .await
            .expect("real ProviderRouteResponse body must parse");

        assert_eq!(resp.model, "claude-opus-4-6");
        assert_eq!(resp.provider.as_deref(), Some("anthropic-primary"));
        assert_eq!(resp.available, Some(true));

        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("GET /api/v1/providers/route?model=claude-opus-4-6 "),
            "provider_route must GET pylon's route endpoint with the model query param, got: {request}"
        );
    }

    // ── Workspace (#4565) ──────────────────────────────────────────────

    #[tokio::test]
    async fn workspace_files_gets_the_pylon_route_with_the_path_query() {
        crate::install_test_crypto_provider();
        let body = r#"[{"name":"a.rs","path":"src/a.rs","is_dir":false,"size":12}]"#;
        let (base_url, server) = serve_http_capture_once("200 OK", body);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        let entries = client
            .workspace_files(Some("src"))
            .await
            .expect("workspace files should parse");

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "src/a.rs");
        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("GET /api/v1/workspace/files?path=src "),
            "got: {request}"
        );
    }

    #[tokio::test]
    async fn workspace_files_omits_the_path_query_when_absent() {
        crate::install_test_crypto_provider();
        let (base_url, server) = serve_http_capture_once("200 OK", "[]");
        let client = ApiClient::new(&base_url, None).expect("build test client");

        client
            .workspace_files(None)
            .await
            .expect("workspace files should succeed");

        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("GET /api/v1/workspace/files "),
            "got: {request}"
        );
    }

    #[tokio::test]
    async fn workspace_git_status_gets_the_pylon_route() {
        crate::install_test_crypto_provider();
        let body = r#"[{"path":"src/a.rs","status":"M"}]"#;
        let (base_url, server) = serve_http_capture_once("200 OK", body);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        let entries = client
            .workspace_git_status()
            .await
            .expect("git status should parse");
        assert_eq!(entries[0].status, "M");
        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("GET /api/v1/workspace/git-status "),
            "got: {request}"
        );
    }

    #[tokio::test]
    async fn workspace_file_content_gets_raw_bytes() {
        crate::install_test_crypto_provider();
        let (base_url, server) = serve_http_capture_once("200 OK", "hello world");
        let client = ApiClient::new(&base_url, None).expect("build test client");

        let bytes = client
            .workspace_file_content("src/a.rs")
            .await
            .expect("file content should succeed");
        assert_eq!(bytes, b"hello world");
        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("GET /api/v1/workspace/files/content?path=src%2Fa.rs "),
            "got: {request}"
        );
    }

    #[tokio::test]
    async fn workspace_write_file_puts_content_and_mtime_guard() {
        crate::install_test_crypto_provider();
        let body = r#"{"path":"src/a.rs","size":5,"mtime_ms":1000}"#;
        let (base_url, server) = serve_http_capture_once("200 OK", body);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        let resp = client
            .workspace_write_file("src/a.rs", "hello", Some(999))
            .await
            .expect("write should succeed");
        assert_eq!(resp.size, 5);
        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("PUT /api/v1/workspace/files/content "),
            "must PUT the bare content route (path is in the body), got: {request}"
        );
        assert!(request.contains(r#""path":"src/a.rs""#));
        assert!(request.contains(r#""content":"hello""#));
        assert!(request.contains(r#""if_match_mtime_ms":999"#));
    }

    #[tokio::test]
    async fn workspace_open_file_posts_the_path() {
        crate::install_test_crypto_provider();
        let body = r#"{"ok":true,"path":"src/a.rs"}"#;
        let (base_url, server) = serve_http_capture_once("200 OK", body);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        let resp = client
            .workspace_open_file("src/a.rs")
            .await
            .expect("open should succeed");
        assert!(resp.ok);
        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("POST /api/v1/workspace/open "),
            "got: {request}"
        );
        assert!(request.contains(r#""path":"src/a.rs""#));
    }

    #[tokio::test]
    async fn workspace_diff_gets_raw_text() {
        crate::install_test_crypto_provider();
        let (base_url, server) = serve_http_capture_once("200 OK", "diff --git a b\n");
        let client = ApiClient::new(&base_url, None).expect("build test client");

        let diff = client
            .workspace_diff("src/a.rs")
            .await
            .expect("diff should succeed");
        assert!(diff.starts_with("diff --git"));
        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("GET /api/v1/workspace/diff?path=src%2Fa.rs "),
            "got: {request}"
        );
    }

    #[tokio::test]
    async fn workspace_search_gets_query_and_limit() {
        crate::install_test_crypto_provider();
        let body = r#"[{"path":"src/a.rs","line":1,"snippet":"fn main"}]"#;
        let (base_url, server) = serve_http_capture_once("200 OK", body);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        let results = client
            .workspace_search("main", 10)
            .await
            .expect("search should succeed");
        assert_eq!(results[0].snippet, "fn main");
        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("GET /api/v1/workspace/search?q=main&limit=10 "),
            "got: {request}"
        );
    }

    // ── Credentials (#4565) ────────────────────────────────────────────

    #[tokio::test]
    async fn list_credentials_gets_the_pylon_route() {
        crate::install_test_crypto_provider();
        let body = r#"{"credentials":[{"id":"anthropic:primary","provider":"anthropic","role":"primary","masked_key":"sk-...ab12","status":"valid","provider_verified":false,"usage_counters_available":false}]}"#;
        let (base_url, server) = serve_http_capture_once("200 OK", body);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        let resp = client
            .list_credentials()
            .await
            .expect("credentials should parse");
        assert_eq!(resp.credentials[0].id, "anthropic:primary");
        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("GET /api/v1/system/credentials "),
            "got: {request}"
        );
    }

    #[tokio::test]
    async fn add_credential_posts_provider_key_and_role() {
        crate::install_test_crypto_provider();
        let body = r#"{"id":"anthropic:primary","provider":"anthropic","role":"primary","masked_key":"sk-...ab12","status":"valid","provider_verified":false,"usage_counters_available":false}"#;
        let (base_url, server) = serve_http_capture_once("201 Created", body);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        let resp = client
            .add_credential("anthropic", SecretString::from("sk-test"), "primary")
            .await
            .expect("add credential should succeed");
        assert_eq!(resp.provider, "anthropic");
        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("POST /api/v1/system/credentials "),
            "got: {request}"
        );
        assert!(request.contains(r#""provider":"anthropic""#));
        assert!(request.contains(r#""role":"primary""#));
        // WHY: `SecretString`'s default `Serialize` impl always emits the
        // literal "[REDACTED]", never the real value. `AddCredentialRequest`
        // overrides that for this one field with a custom serializer so the
        // actual key reaches pylon; this asserts the override held, not just
        // that some `key` field is present.
        assert!(
            request.contains(r#""key":"sk-test""#),
            "the real key must reach the wire, not a redacted placeholder, got: {request}"
        );
        assert!(
            !request.contains("[REDACTED]"),
            "credential add must never send the redacted placeholder as the key value, got: {request}"
        );
    }

    #[tokio::test]
    async fn remove_credential_deletes_the_encoded_id() {
        crate::install_test_crypto_provider();
        let body = r#"{"runtime_effect":"applied"}"#;
        let (base_url, server) = serve_http_capture_once("200 OK", body);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        let resp = client
            .remove_credential("anthropic:primary")
            .await
            .expect("remove should succeed");
        assert_eq!(resp.runtime_effect, CredentialMutationEffect::Applied);
        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("DELETE /api/v1/system/credentials/anthropic%3Aprimary "),
            "got: {request}"
        );
    }

    #[tokio::test]
    async fn validate_credential_posts_the_encoded_id() {
        crate::install_test_crypto_provider();
        let body = r#"{"id":"anthropic:primary","provider":"anthropic","role":"primary","masked_key":"sk-...ab12","status":"valid","provider_verified":true,"validation_state":"accepted","usage_counters_available":false}"#;
        let (base_url, server) = serve_http_capture_once("200 OK", body);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        let resp = client
            .validate_credential("anthropic:primary")
            .await
            .expect("validate should succeed");
        assert_eq!(
            resp.validation_state,
            Some(CredentialValidationState::Accepted)
        );
        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("POST /api/v1/system/credentials/anthropic%3Aprimary/validate "),
            "got: {request}"
        );
    }

    #[tokio::test]
    async fn rotate_credentials_posts_the_provider_query() {
        crate::install_test_crypto_provider();
        let body = r#"{"credentials":[],"runtime_effect":"restart_required"}"#;
        let (base_url, server) = serve_http_capture_once("200 OK", body);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        let resp = client
            .rotate_credentials("anthropic")
            .await
            .expect("rotate should succeed");
        assert_eq!(
            resp.runtime_effect,
            Some(CredentialMutationEffect::RestartRequired)
        );
        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("POST /api/v1/system/credentials/rotate?provider=anthropic "),
            "got: {request}"
        );
    }

    // ── Planning (#4565) ───────────────────────────────────────────────

    #[tokio::test]
    async fn project_verification_gets_the_pylon_route() {
        crate::install_test_crypto_provider();
        let body =
            r#"{"project_id":"p1","requirements":[],"last_verified_at":"2026-01-01T00:00:00Z"}"#;
        let (base_url, server) = serve_http_capture_once("200 OK", body);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        let resp = client
            .project_verification("p1")
            .await
            .expect("verification should parse");
        assert_eq!(resp.project_id, "p1");
        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("GET /api/v1/planning/projects/p1/verification "),
            "got: {request}"
        );
    }

    #[tokio::test]
    async fn refresh_project_verification_posts_the_refresh_route() {
        crate::install_test_crypto_provider();
        let body =
            r#"{"project_id":"p1","requirements":[],"last_verified_at":"2026-01-02T00:00:00Z"}"#;
        let (base_url, server) = serve_http_capture_once("200 OK", body);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        let resp = client
            .refresh_project_verification("p1")
            .await
            .expect("refresh should parse");
        assert_eq!(resp.last_verified_at, "2026-01-02T00:00:00Z");
        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("POST /api/v1/planning/projects/p1/verification/refresh "),
            "got: {request}"
        );
    }

    // ── Feature flags and config reload (#4565) ─────────────────────────

    #[tokio::test]
    async fn update_feature_flags_puts_the_whole_section() {
        crate::install_test_crypto_provider();
        let body = r#"{"section":"feature_flags","config":[{"key":"foo","description":"d","enabled":true}],"restart_required":[]}"#;
        let (base_url, server) = serve_http_capture_once("200 OK", body);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        let flags = serde_json::json!([{"key": "foo", "description": "d", "enabled": true}]);
        let resp = client
            .update_feature_flags(&flags)
            .await
            .expect("update should succeed");
        assert_eq!(resp.section, "feature_flags");
        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("PUT /api/v1/config/feature_flags "),
            "got: {request}"
        );
        assert!(request.contains(r#""key":"foo""#));
    }

    #[tokio::test]
    async fn reload_config_posts_with_no_body() {
        crate::install_test_crypto_provider();
        let body = r#"{"hot_reloaded":2,"restart_required":[],"changed":["gateway.port"]}"#;
        let (base_url, server) = serve_http_capture_once("200 OK", body);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        let resp = client.reload_config().await.expect("reload should succeed");
        assert_eq!(resp.hot_reloaded, 2);
        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("POST /api/v1/config/reload "),
            "got: {request}"
        );
    }

    // ── Nous get-one / recover (#4565) ──────────────────────────────────

    #[tokio::test]
    async fn agent_status_gets_the_pylon_route() {
        crate::install_test_crypto_provider();
        let body = r#"{
            "id": "syn",
            "model": "claude-opus-4-6",
            "fallback_models": [],
            "fallback_providers": [],
            "retries_before_fallback": 1,
            "complexity_routing_enabled": false,
            "complexity_no_llm_threshold": 0,
            "complexity_low_threshold": 0,
            "complexity_high_threshold": 0,
            "provider_readiness": [],
            "context_window": 200000,
            "max_output_tokens": 8192,
            "thinking_enabled": false,
            "thinking_budget": 0,
            "max_tool_iterations": 10,
            "status": "idle",
            "background_failure_total_count": 0,
            "background_failure_recent_count": 0,
            "background_health_degraded": false,
            "address_mask": {"kind": "public", "allowed_senders": []}
        }"#;
        let (base_url, server) = serve_http_capture_once("200 OK", body);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        let status = client
            .agent_status("syn")
            .await
            .expect("agent status should parse");
        assert_eq!(status.id, "syn");
        assert_eq!(status.status, "idle");
        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("GET /api/v1/nous/syn "),
            "got: {request}"
        );
    }

    #[tokio::test]
    async fn agent_recover_posts_the_recover_route() {
        crate::install_test_crypto_provider();
        let body = r#"{"id":"syn","recovered":true}"#;
        let (base_url, server) = serve_http_capture_once("200 OK", body);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        let resp = client
            .agent_recover("syn")
            .await
            .expect("recover should succeed");
        assert!(resp.recovered);
        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("POST /api/v1/nous/syn/recover "),
            "got: {request}"
        );
    }

    // ── Metrics dashboards (#4565) ───────────────────────────────────────

    #[tokio::test]
    async fn agent_performance_gets_the_pylon_route() {
        crate::install_test_crypto_provider();
        let body = r#"{"agents":[],"anomalies":[]}"#;
        let (base_url, server) = serve_http_capture_once("200 OK", body);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        client
            .agent_performance()
            .await
            .expect("agent performance should parse");
        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("GET /api/v1/metrics/agents "),
            "got: {request}"
        );
    }

    #[tokio::test]
    async fn agent_performance_one_gets_the_encoded_id_route() {
        crate::install_test_crypto_provider();
        let body = r#"{
            "agent_id": "syn",
            "agent_name": "Syn",
            "avg_tokens_per_response": 1.0,
            "tool_calls_per_session": 1.0,
            "tool_success_rate": 1.0,
            "distillation_frequency": 0.0,
            "avg_context_before_distill": 0.0,
            "messages_per_session": 1.0,
            "sessions_per_day": 1.0,
            "errors_per_session": 0.0
        }"#;
        let (base_url, server) = serve_http_capture_once("200 OK", body);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        let perf = client
            .agent_performance_one("syn")
            .await
            .expect("agent performance detail should parse");
        assert_eq!(perf.agent_id, "syn");
        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("GET /api/v1/metrics/agents/syn "),
            "got: {request}"
        );
    }

    #[tokio::test]
    async fn quality_metrics_gets_the_pylon_route() {
        crate::install_test_crypto_provider();
        let body = r#"{"series":{"avg_turn_length":[],"response_to_question_ratio":[],"tool_call_density":[],"thinking_time_ratio":[]}}"#;
        let (base_url, server) = serve_http_capture_once("200 OK", body);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        client
            .quality_metrics()
            .await
            .expect("quality metrics should parse");
        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("GET /api/v1/metrics/quality "),
            "got: {request}"
        );
    }

    #[tokio::test]
    async fn journal_gets_the_pylon_route() {
        crate::install_test_crypto_provider();
        let body = r#"{"events":[],"data_unavailable":[{"metric":"journal","reason":"no persistent event journal is available in pylon"}]}"#;
        let (base_url, server) = serve_http_capture_once("200 OK", body);
        let client = ApiClient::new(&base_url, None).expect("build test client");

        let resp = client.journal().await.expect("journal should parse");
        assert_eq!(resp.data_unavailable.len(), 1);
        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("GET /api/v1/journal "),
            "got: {request}"
        );
    }

    // ── Entity merge / delete / flag (#4565) ────────────────────────────

    #[tokio::test]
    async fn merge_entities_posts_canonical_and_merged_ids() {
        crate::install_test_crypto_provider();
        let (base_url, server) = serve_http_capture_once("204 No Content", "");
        let client = ApiClient::new(&base_url, None).expect("build test client");

        client
            .merge_entities("e-1", "e-2")
            .await
            .expect("merge should succeed");
        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("POST /api/v1/knowledge/entities/merge "),
            "got: {request}"
        );
        assert!(request.contains(r#""canonical_id":"e-1""#));
        assert!(request.contains(r#""merged_id":"e-2""#));
    }

    #[tokio::test]
    async fn flag_entity_posts_the_encoded_id_and_reason() {
        crate::install_test_crypto_provider();
        let (base_url, server) = serve_http_capture_once("204 No Content", "");
        let client = ApiClient::new(&base_url, None).expect("build test client");

        client
            .flag_entity("entity/1", "looks wrong", FlagSeverity::High)
            .await
            .expect("flag should succeed");
        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("POST /api/v1/knowledge/entities/entity%2F1/flag "),
            "got: {request}"
        );
        assert!(request.contains(r#""reason":"looks wrong""#));
        assert!(request.contains(r#""severity":"high""#));
    }

    #[tokio::test]
    async fn delete_entity_deletes_the_encoded_id() {
        crate::install_test_crypto_provider();
        let (base_url, server) = serve_http_capture_once("204 No Content", "");
        let client = ApiClient::new(&base_url, None).expect("build test client");

        client
            .delete_entity("entity/1")
            .await
            .expect("delete should succeed");
        let request = server.join().expect("test server thread should finish");
        assert!(
            request.starts_with("DELETE /api/v1/knowledge/entities/entity%2F1 "),
            "got: {request}"
        );
    }
}
