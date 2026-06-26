//! Request and response types for the Aletheia REST API.

pub mod verification;
pub use verification::{
    ProjectVerificationResult, RequirementPriority, RequirementVerification, VerificationEvidence,
    VerificationGap, VerificationStatus,
};

use serde::{Deserialize, Serialize};

use koina::secret::SecretString;

use crate::id::{GitSha, NousId, PlanId, SessionId, TurnId};

use super::request_policy::RequestPolicy;

/// A registered agent (nous) in the system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    /// Agent identifier.
    pub id: NousId,
    /// Display name: falls back to `id` if absent.
    #[serde(default)]
    pub name: Option<String>,
    /// Model backing this agent.
    #[serde(default)]
    pub model: Option<String>,
    /// Emoji icon for the agent.
    #[serde(default)]
    pub emoji: Option<String>,
}

impl Agent {
    /// Display name: uses `name` if set, otherwise `id`.
    #[must_use]
    pub fn display_name(&self) -> &str {
        // kanon:ignore RUST/pub-visibility
        self.name.as_deref().unwrap_or(&self.id)
    }
}

/// A session within an agent.
#[derive(Debug, Clone, Serialize, Deserialize)] // kanon:ignore RUST/no-debug-derive-on-public-types
pub struct Session {
    /// Session identifier.
    pub id: SessionId,
    /// Agent this session belongs to.
    pub nous_id: NousId,
    /// Session key (human-readable slug, not a secret).
    #[serde(rename = "session_key")]
    pub key: String, // kanon:ignore RUST/plain-string-secret
    /// Session status (e.g. "active", "archived").
    #[serde(default)]
    pub status: Option<String>,
    /// Number of messages in the session.
    #[serde(default)]
    pub message_count: u32,
    /// Session type (e.g. "background").
    #[serde(default)]
    pub session_type: Option<String>,
    /// Last-updated timestamp.
    #[serde(default)]
    pub updated_at: Option<String>,
    /// User-assigned display name.
    #[serde(default, alias = "name")]
    pub display_name: Option<String>,
}

impl Session {
    /// Label for display: prefers `display_name`, falls back to `key`.
    #[must_use]
    pub fn label(&self) -> &str {
        // kanon:ignore RUST/pub-visibility
        self.display_name
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.key)
    }

    /// Whether this session has been archived.
    #[must_use]
    pub fn is_archived(&self) -> bool {
        // kanon:ignore RUST/pub-visibility
        self.status.as_deref() == Some("archived") || self.key.contains(":archived:")
    }

    /// Whether this session accepts interactive user input.
    #[must_use]
    pub fn is_interactive(&self) -> bool {
        // kanon:ignore RUST/pub-visibility
        !self.is_archived()
            && self.session_type.as_deref() != Some("background")
            && !self.key.starts_with("cron:")
            && !self.key.starts_with("daemon:")
            && !self.key.starts_with("prosoche")
            && !self.key.starts_with("agent:")
    }
}

/// Query parameters for listing sessions with pagination and filtering.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListSessionsRequest {
    /// Filter to sessions belonging to this agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nous_id: Option<String>,
    /// Free-text search across session key and display name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
    /// Filter to sessions in this status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Maximum number of sessions to return.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Cursor for the next page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

/// A single message from session history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryMessage {
    /// Role: "user", "assistant", or "tool".
    pub role: String,
    /// Message content (text or structured).
    #[serde(default)]
    pub content: Option<serde_json::Value>,
    /// When the message was created.
    #[serde(default)]
    pub created_at: Option<String>,
    /// Model that generated this message (assistant messages only).
    #[serde(default)]
    pub model: Option<String>,
    /// Tool name if this is a tool-result message.
    #[serde(default)]
    pub tool_name: Option<String>,
}

/// Wrapper for the history endpoint response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryResponse {
    /// Messages in chronological order.
    pub messages: Vec<HistoryMessage>,
}

/// Summary of a completed turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnOutcome {
    /// Final text output.
    pub text: String,
    /// Agent that processed this turn.
    #[serde(rename = "nousId", alias = "nous_id")]
    pub nous_id: NousId,
    /// Session this turn belongs to.
    #[serde(rename = "sessionId", alias = "session_id")]
    pub session_id: SessionId,
    /// Model used for this turn; `None` when the gateway could not resolve it.
    #[serde(default)]
    pub model: Option<String>,
    /// Number of tool calls made.
    #[serde(rename = "toolCalls", alias = "tool_calls", default)]
    pub tool_calls: u32,
    /// Input tokens consumed.
    #[serde(rename = "inputTokens", alias = "input_tokens", default)]
    pub input_tokens: u32,
    /// Output tokens generated.
    #[serde(rename = "outputTokens", alias = "output_tokens", default)]
    pub output_tokens: u32,
    /// Tokens read from cache.
    #[serde(rename = "cacheReadTokens", alias = "cache_read_tokens", default)]
    pub cache_read_tokens: u32,
    /// Tokens written to cache.
    #[serde(rename = "cacheWriteTokens", alias = "cache_write_tokens", default)]
    pub cache_write_tokens: u32,
    /// Provider stop reason reported by the terminal completion event.
    #[serde(rename = "stopReason", alias = "stop_reason", default)]
    pub stop_reason: Option<String>,
    /// Error message, if the turn errored.
    #[serde(default)]
    pub error: Option<String>,
}

/// A single step within a plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    /// Step index.
    pub id: u32,
    /// Human-readable label.
    pub label: String,
    /// Role responsible for this step.
    pub role: String,
    /// Steps that can run in parallel with this one.
    #[serde(default)]
    pub parallel: Option<Vec<u32>>,
    /// Current status of this step.
    pub status: String,
    /// Result summary after completion.
    #[serde(default)]
    pub result: Option<String>,
}

/// A multi-step execution plan proposed by the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    /// Plan identifier.
    pub id: PlanId,
    /// Session this plan was proposed in.
    #[serde(rename = "sessionId")]
    pub session_id: SessionId,
    /// Agent that proposed the plan.
    #[serde(rename = "nousId")]
    pub nous_id: NousId,
    /// Ordered list of plan steps.
    pub steps: Vec<PlanStep>,
    /// Estimated total cost in cents.
    #[serde(rename = "totalEstimatedCostCents", default)]
    pub total_estimated_cost_cents: u32,
    /// Plan status.
    pub status: String,
}

/// Application-level SSE events from `GET /api/v1/events/subscribe`.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum SseEvent {
    /// SSE connection established.
    Connected,
    /// SSE connection lost (will auto-reconnect).
    Disconnected,
    /// Initial state dump with currently active turns.
    Init {
        /// Turns that are currently in progress.
        active_turns: Vec<ActiveTurn>,
    },
    /// A turn is about to start.
    TurnBefore {
        /// Agent processing the turn.
        nous_id: NousId,
        /// Session the turn belongs to.
        session_id: SessionId,
        /// Turn identifier.
        turn_id: TurnId,
    },
    /// A turn has completed.
    TurnAfter {
        /// Agent that processed the turn.
        nous_id: NousId,
        /// Session the turn belongs to.
        session_id: SessionId,
    },
    /// A domain event reporting that a turn completed, emitted on the
    /// `EventBus` topic `turn.complete`.
    TurnComplete {
        /// Session the turn belongs to.
        session_id: SessionId,
        /// Agent that processed the turn.
        nous_id: NousId,
        /// Turn identifier.
        turn_id: TurnId,
        /// Input tokens consumed.
        input_tokens: u32,
        /// Output tokens generated.
        output_tokens: u32,
    },
    /// A domain event reporting that a knowledge fact was created, emitted on
    /// the `EventBus` topic `fact.created`.
    FactCreated {
        /// Fact identifier.
        fact_id: String,
        /// Agent that owns the fact.
        nous_id: NousId,
        /// Short preview of the fact content.
        content_preview: String,
    },
    /// A domain event reporting a `nous` lifecycle change, emitted on the
    /// `EventBus` topic `nous.lifecycle`.
    NousLifecycle {
        /// Agent whose lifecycle changed.
        nous_id: NousId,
        /// Lifecycle event name (e.g. "created").
        event: String,
        /// Whether the server requires a restart to activate the change.
        restart_required: bool,
    },
    /// A tool was invoked during a turn.
    ToolCalled {
        /// Agent invoking the tool.
        nous_id: NousId,
        /// Name of the tool.
        tool_name: String,
    },
    /// A tool invocation failed.
    ToolFailed {
        /// Agent whose tool failed.
        nous_id: NousId,
        /// Name of the failed tool.
        tool_name: String,
        /// Error description.
        error: String,
    },
    /// Agent status changed.
    StatusUpdate {
        /// Agent whose status changed.
        nous_id: NousId,
        /// New status value.
        status: String,
    },
    /// A new session was created.
    SessionCreated {
        /// Agent the session was created for.
        nous_id: NousId,
        /// New session identifier.
        session_id: SessionId,
    },
    /// A session was archived.
    SessionArchived {
        /// Agent the session belongs to.
        nous_id: NousId,
        /// Archived session identifier.
        session_id: SessionId,
    },
    /// Memory distillation is about to start.
    DistillBefore {
        /// Agent undergoing distillation.
        nous_id: NousId,
    },
    /// Memory distillation progressed to a new stage.
    DistillStage {
        /// Agent undergoing distillation.
        nous_id: NousId,
        /// Current distillation stage.
        stage: String,
    },
    /// Memory distillation completed.
    DistillAfter {
        /// Agent that completed distillation.
        nous_id: NousId,
    },
    /// A new checkpoint was created in a planning project.
    CheckpointCreated {
        /// Project the checkpoint belongs to.
        project_id: String,
        /// Identifier of the created checkpoint.
        checkpoint_id: String,
    },
    /// A checkpoint's status changed (approved, skipped, overridden).
    CheckpointUpdated {
        /// Project the checkpoint belongs to.
        project_id: String,
        /// Identifier of the updated checkpoint.
        checkpoint_id: String,
        /// New status value (e.g. "approved", "skipped", "overridden").
        status: String,
    },
    /// Server heartbeat.
    Ping,
    /// Server reports the client fell behind and events were dropped.
    StreamLagged {
        /// Number of events dropped by the server due to client lag.
        dropped: u64,
    },
    /// Error event from the server.
    Error {
        /// Error message.
        message: String,
    },
    /// An SSE event payload could not be decoded.
    ///
    /// Surfaces JSON parse failures as a typed event instead of
    /// silently dropping them, so UIs can log or render protocol-drift
    /// diagnostics without losing raw data.
    DecodeError {
        /// Wire event type string from the SSE `event:` field.
        event_type: String,
        /// Raw `data:` payload that failed to decode.
        raw_data: String,
        /// Decode error description.
        error: String,
    },
    /// An SSE event type not recognized by this client was received.
    ///
    /// Surfaces unknown event types as a typed variant instead of
    /// silently dropping them, so UIs can observe server-side additions
    /// without losing the raw event data.
    UnknownEvent {
        /// Wire event type string from the SSE `event:` field.
        event_type: String,
        /// Raw `data:` payload.
        raw_data: String,
    },
}

/// A turn currently in progress, reported in the `init` SSE event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveTurn {
    /// Agent processing this turn.
    #[serde(rename = "nousId")]
    pub nous_id: NousId,
    /// Session this turn belongs to.
    #[serde(rename = "sessionId")]
    pub session_id: SessionId,
    /// Turn identifier.
    #[serde(rename = "turnId")]
    pub turn_id: TurnId,
}

/// Server authentication mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthMode {
    /// Authentication mode (e.g. "token", "none").
    pub mode: String,
}

/// Response from the login endpoint.
#[derive(Clone, Serialize, Deserialize)]
pub struct LoginResponse {
    /// Authentication token.
    pub token: SecretString,
}

impl std::fmt::Debug for LoginResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginResponse")
            .field("token", &self.token)
            .finish()
    }
}

/// Cost summary across agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostSummary {
    /// Total cost across all agents.
    #[serde(rename = "totalCost", default)]
    pub total_cost: f64,
    /// Per-agent cost breakdown.
    #[serde(default)]
    pub agents: Vec<AgentCost>,
}

/// Cost for a single agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCost {
    /// Agent identifier.
    #[serde(rename = "agentId")]
    pub agent_id: NousId,
    /// Total cost for this agent.
    #[serde(rename = "totalCost", default)]
    pub total_cost: f64,
    /// Number of turns processed.
    #[serde(default)]
    pub turns: u32,
}

/// Response from the daily costs endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyResponse {
    /// Daily cost entries.
    pub daily: Vec<DailyEntry>,
}

/// A single day's cost and usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyEntry {
    /// Date string (YYYY-MM-DD).
    pub date: String,
    /// Cost in dollars.
    pub cost: f64,
    /// Total tokens consumed.
    #[serde(default)]
    pub tokens: u64,
    /// Number of turns.
    #[serde(default)]
    pub turns: u32,
}

/// Wrapper for the agents list endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentsResponse {
    /// Server returns `{"nous": [...]}`: accept both keys for resilience.
    #[serde(alias = "agents")]
    pub nous: Vec<Agent>,
}

/// Wrapper for the sessions list endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionsResponse {
    /// List of sessions.
    #[serde(alias = "items")]
    pub sessions: Vec<Session>,
}

/// Paginated response from the sessions list endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedSessionsResponse {
    /// Sessions in the current page.
    #[serde(alias = "sessions")]
    pub items: Vec<Session>,
    /// Whether more pages are available.
    pub has_more: bool,
    /// Cursor for fetching the next page.
    #[serde(default)]
    pub next_cursor: Option<String>,
    /// Total number of sessions matching the query.
    #[serde(default)]
    pub total: Option<u64>,
}

/// A tool available to an agent, with its enablement state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NousTool {
    /// Tool name.
    pub name: String,
    /// Whether the tool is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// Wrapper for the tools list endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NousToolsResponse {
    /// List of tools.
    pub tools: Vec<NousTool>,
}

/// Server health response from `GET /api/health`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    /// Aggregate status: `"healthy"`, `"degraded"`, or `"unhealthy"`.
    pub status: String,
    /// Crate version from `Cargo.toml`.
    pub version: String,
    /// Build git SHA when available.
    pub git_sha: GitSha,
    /// Seconds since server start.
    pub uptime_seconds: u64,
    /// Individual subsystem check results.
    pub checks: Vec<HealthCheck>,
    /// Absolute path to the instance data directory.
    pub data_dir: String,
}

/// Result of a single subsystem health check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    /// Subsystem name (e.g. `"session_store"`, `"providers"`).
    pub name: String,
    /// Check outcome: `"pass"`, `"warn"`, `"fail"`, or `"timeout"`.
    pub status: String,
    /// Diagnostic message when status is not `"pass"`.
    pub message: Option<String>,
}

/// Response from `GET /api/v1/system/request-policy`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestPolicyResponse {
    /// First-party request policy clients should use for state-changing requests.
    pub request_policy: RequestPolicy,
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test assertions may panic on failure"
)]
mod tests;
