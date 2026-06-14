// kanon:ignore RUST/file-too-long — cohesive HTTP client; extracting now would fragment request/response handling
//! HTTP client for the Aletheia gateway REST API.
use reqwest::{Client, Response, StatusCode, header};
use snafu::prelude::*;

use koina::secret::SecretString;

use super::error::{
    ApiError, AuthSnafu, HttpSnafu, RateLimitedSnafu, Result, ServerSnafu, parse_pylon_error_body,
    parse_retry_after_secs,
};
use super::types::{
    Agent, AgentsResponse, AuthMode, DailyResponse, EntitiesQuery, EntitiesResponse, Entity,
    EntityMemory, FactDetailResponse, FactsQuery, FactsResponse, FlagRequest, FlagSeverity,
    ForgetRequest, GraphCheckReport, HealthResponse, HistoryMessage, HistoryResponse,
    LoginResponse, MergeRequest, NousTool, NousToolsResponse, RelationshipsResponse, Session,
    SessionsResponse, TimelineQuery, TimelineResponse, UpdateConfidenceRequest,
    UpdateSensitivityRequest,
};

/// Build the shared reqwest client used by all API paths (REST, streaming, SSE).
///
/// Default headers set here apply to every request made with this client:
/// - Authorization: Bearer <token> (if a token is configured)
/// - x-requested-with: aletheia (CSRF mitigation: server rejects absent header)
/// - Content-Type: application/json
/// - Accept: application/json (SSE callers override this per-request to text/event-stream)
///
/// WHY: A single client shares the connection pool and TLS session cache across all
/// request types. Building one per call creates
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
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    pub fn new(base_url: &str, token: Option<String>) -> Result<Self> {
        // kanon:ignore RUST/pub-visibility
        let client = build_http_client(token.as_deref())?;

        Ok(Self {
            client,
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
            Self::check_status(resp, "health details request").await?;
            unreachable!("check_status returns Ok only for success status codes")
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

    /// Fetch knowledge facts with sorting, filtering, and pagination.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn knowledge_facts(&self, query: &FactsQuery) -> Result<FactsResponse> {
        let limit = query.limit.to_string();
        let offset = query.offset.to_string();
        let include_forgotten = query.include_forgotten.to_string();
        let mut params: Vec<(&str, &str)> = vec![
            ("sort", &query.sort),
            ("order", &query.order),
            ("limit", &limit),
            ("offset", &offset),
            ("include_forgotten", &include_forgotten),
        ];
        if let Some(ref nous_id) = query.nous_id {
            params.push(("nous_id", nous_id));
        }
        if let Some(ref filter) = query.filter {
            params.push(("filter", filter));
        }
        if let Some(ref fact_type) = query.fact_type {
            params.push(("fact_type", fact_type));
        }
        if let Some(ref tier) = query.tier {
            params.push(("tier", tier));
        }

        let resp = self
            .request(reqwest::Method::GET, "/api/v1/knowledge/facts")
            .query(&params)
            .send()
            .await
            .context(HttpSnafu {
                operation: "load facts",
            })?;
        Self::check_auth(&resp)?;
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
        Self::check_auth(&resp)?;
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
        let body = ForgetRequest {
            reason: "user_requested".to_string(),
        };
        let resp = self
            .request(
                reqwest::Method::POST,
                &format!("/api/v1/knowledge/facts/{encoded}/forget"),
            )
            .json(&body)
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

    /// Fetch a single knowledge entity by ID.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn knowledge_entity(&self, entity_id: &str) -> Result<Entity> {
        let encoded = keryx::url::encode_path_segment(entity_id);
        let resp = self
            .request(
                reqwest::Method::GET,
                &format!("/api/v1/knowledge/entities/{encoded}"),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "load entity detail",
            })?;
        Self::check_auth(&resp)?;
        let resp = Self::check_status(resp, "entity detail request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "entity detail response",
        })
    }

    /// Fetch knowledge entities with sorting, filtering, and pagination.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn knowledge_entities(&self, query: &EntitiesQuery) -> Result<EntitiesResponse> {
        let limit = query.limit.to_string();
        let offset = query.offset.to_string();
        let mut params: Vec<(&str, &str)> = vec![
            ("limit", &limit),
            ("offset", &offset),
            ("sort", &query.sort),
            ("order", &query.order),
        ];
        if let Some(ref q) = query.q {
            params.push(("q", q));
        }
        let min_confidence = query.min_confidence.map(|value| value.to_string());
        if let Some(ref min_confidence) = min_confidence {
            params.push(("min_confidence", min_confidence));
        }
        for ty in &query.entity_type {
            params.push(("entity_type", ty));
        }
        for agent in &query.agent {
            params.push(("agent", agent));
        }

        let resp = self
            .request(reqwest::Method::GET, "/api/v1/knowledge/entities")
            .query(&params)
            .send()
            .await
            .context(HttpSnafu {
                operation: "load entities",
            })?;
        Self::check_auth(&resp)?;
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
        Self::check_auth(&resp)?;
        let resp = Self::check_status(resp, "entity relationships request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "entity relationships response",
        })
    }

    /// Fetch memories linked to a specific entity.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn knowledge_entity_memories(&self, entity_id: &str) -> Result<Vec<EntityMemory>> {
        let encoded = keryx::url::encode_path_segment(entity_id);
        let resp = self
            .request(
                reqwest::Method::GET,
                &format!("/api/v1/knowledge/entities/{encoded}/memories"),
            )
            .send()
            .await
            .context(HttpSnafu {
                operation: "load entity memories",
            })?;
        Self::check_auth(&resp)?;
        let resp = Self::check_status(resp, "entity memories request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "entity memories response",
        })
    }

    /// Fetch the knowledge activity timeline.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn knowledge_timeline(&self, query: &TimelineQuery) -> Result<TimelineResponse> {
        let limit = query.limit.to_string();
        let offset = query.offset.to_string();
        let mut params: Vec<(&str, &str)> = vec![("limit", &limit), ("offset", &offset)];
        if let Some(ref nous_id) = query.nous_id {
            params.push(("nous_id", nous_id));
        }

        let resp = self
            .request(reqwest::Method::GET, "/api/v1/knowledge/timeline")
            .query(&params)
            .send()
            .await
            .context(HttpSnafu {
                operation: "load timeline",
            })?;
        Self::check_auth(&resp)?;
        let resp = Self::check_status(resp, "timeline request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "timeline response",
        })
    }

    /// Run server-side knowledge graph consistency checks.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn knowledge_graph_check(&self) -> Result<GraphCheckReport> {
        let resp = self
            .request(reqwest::Method::GET, "/api/v1/knowledge/check")
            .send()
            .await
            .context(HttpSnafu {
                operation: "graph check",
            })?;
        Self::check_auth(&resp)?;
        let resp = Self::check_status(resp, "graph check request").await?;
        resp.json().await.context(HttpSnafu {
            operation: "graph check response",
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
        let body = UpdateConfidenceRequest { confidence };
        let resp = self
            .request(
                reqwest::Method::PUT,
                &format!("/api/v1/knowledge/facts/{encoded}/confidence"),
            )
            .json(&body)
            .send()
            .await
            .context(HttpSnafu {
                operation: "update confidence",
            })?;
        Self::check_status(resp, "confidence request").await?;
        Ok(())
    }

    /// Update the data-sovereignty sensitivity for a knowledge fact.
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
        let body = UpdateSensitivityRequest {
            sensitivity: sensitivity.to_string(),
        };
        let resp = self
            .request(
                reqwest::Method::PUT,
                &format!("/api/v1/knowledge/facts/{encoded}/sensitivity"),
            )
            .json(&body)
            .send()
            .await
            .context(HttpSnafu {
                operation: "update sensitivity",
            })?;
        Self::check_status(resp, "sensitivity request").await?;
        Ok(())
    }

    /// Flag an entity for operator review.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn knowledge_flag_entity(
        &self,
        entity_id: &str,
        reason: &str,
        severity: FlagSeverity,
    ) -> Result<()> {
        let encoded = keryx::url::encode_path_segment(entity_id);
        let body = FlagRequest {
            reason: reason.to_string(),
            severity,
        };
        let resp = self
            .request(
                reqwest::Method::POST,
                &format!("/api/v1/knowledge/entities/{encoded}/flag"),
            )
            .json(&body)
            .send()
            .await
            .context(HttpSnafu {
                operation: "flag entity",
            })?;
        Self::check_status(resp, "flag request").await?;
        Ok(())
    }

    /// Merge two entities, keeping the canonical entity and removing the other.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self))]
    pub async fn knowledge_merge_entities(
        &self,
        canonical_id: &str,
        merged_id: &str,
    ) -> Result<()> {
        let body = MergeRequest {
            canonical_id: canonical_id.to_string(),
            merged_id: merged_id.to_string(),
        };
        let resp = self
            .request(reqwest::Method::POST, "/api/v1/knowledge/entities/merge")
            .json(&body)
            .send()
            .await
            .context(HttpSnafu {
                operation: "merge entities",
            })?;
        Self::check_status(resp, "merge request").await?;
        Ok(())
    }

    /// Queue a message for asynchronous processing.
    #[must_use]
    #[expect(
        clippy::double_must_use,
        reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
    )]
    #[tracing::instrument(skip(self, text))]
    pub async fn queue_message(&self, session_id: &str, text: &str) -> Result<()> {
        let encoded = keryx::url::encode_path_segment(session_id);
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

    /// The shared HTTP client, pre-configured with auth and default headers.
    #[must_use]
    pub fn raw_client(&self) -> &Client {
        // kanon:ignore RUST/pub-visibility
        &self.client
    }
}
