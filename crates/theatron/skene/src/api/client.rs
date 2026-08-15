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
    Agent, AgentsResponse, CostMetricsResponse, EntitiesResponse, FactDetailResponse,
    FactsResponse, HealthResponse, HistoryMessage, HistoryResponse, ListSessionsRequest,
    NousTool, NousToolsResponse, PaginatedSessionsResponse, ProviderListResponse,
    ProviderRouteResponse, RelationshipsResponse, Session, SessionReplayResponse,
    SessionsResponse, TimelineResponse, TokenMetricsResponse,
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
    ///
    /// WHY(#4925): crate-private — skene is the sole typed protocol boundary
    /// for first-party clients; a public escape hatch let a consumer bypass
    /// route/DTO/error semantics while still looking like it used the shared
    /// client. Confirmed zero external callers before tightening visibility.
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
}
