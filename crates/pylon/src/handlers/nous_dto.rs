// WHY: wire DTO
//! Nous endpoint request and response wire shapes.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Payload for creating a new nous agent via `POST /api/v1/nous`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AgentDefinition {
    /// Agent identifier. Case folds to lowercase; underscores normalize to hyphens.
    pub id: String,
    /// Human-readable display name. Falls back to a capitalized `id`.
    #[serde(default)]
    pub name: Option<String>,
    /// LLM model identifier. Falls back to the workspace default.
    #[serde(default)]
    pub model: Option<String>,
}

/// Response from a successful agent creation.
#[derive(Debug, Serialize, ToSchema)]
pub struct CreateAgentResponse {
    /// Agent identifier.
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// LLM model assigned to this agent.
    pub model: String,
    /// Whether the agent requires a server restart to become active.
    pub restart_required: bool,
}

/// Response from a recovery attempt.
#[derive(Debug, Serialize, ToSchema)]
pub struct RecoverResponse {
    /// Agent identifier.
    pub id: String,
    /// Whether recovery was performed (false if agent was not degraded).
    pub recovered: bool,
}

/// Response listing all registered nous agents.
#[derive(Debug, Serialize, ToSchema)]
pub struct NousListResponse {
    /// Agent summaries.
    pub nous: Vec<NousSummary>,
}

/// Brief overview of a registered nous agent.
#[derive(Debug, Serialize, ToSchema)]
pub struct NousSummary {
    /// Agent identifier.
    pub id: String,
    /// Human-readable display name (falls back to `id`).
    pub name: String,
    /// Whether the agent is enabled in the operator surface.
    pub enabled: bool,
    /// Primary LLM model assigned to this agent.
    pub model: String,
    /// Fallback models tried after primary-model failures.
    pub fallback_models: Vec<String>,
    /// Per-model provider readiness for the agent's model chain.
    pub provider_readiness: Vec<crate::handlers::providers::ModelProviderReadiness>,
    /// Lifecycle status (e.g. `"active"`).
    pub status: String,
    /// Tool toggle summaries for the agent.
    pub tools: Vec<ToolSummary>,
    /// Whether the requested config change was persisted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_applied: Option<bool>,
    /// Whether the running actor/runtime now reflects the requested state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_applied: Option<bool>,
    /// Whether a config reload is required before the requested state is live.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reload_required: Option<bool>,
    /// Whether a process restart is required before the requested state is live.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restart_required: Option<bool>,
}

/// Detailed status of a single nous agent.
#[derive(Debug, Serialize, ToSchema)]
pub struct NousStatus {
    /// Agent identifier.
    pub id: String,
    /// Primary LLM model assigned to this agent.
    pub model: String,
    /// Fallback models tried after primary-model failures.
    pub fallback_models: Vec<String>,
    /// Number of primary-model attempts before moving to the fallback chain.
    pub retries_before_fallback: u32,
    /// Whether complexity-based model routing is enabled.
    pub complexity_routing_enabled: bool,
    /// Per-model provider readiness for the agent's model chain.
    pub provider_readiness: Vec<crate::handlers::providers::ModelProviderReadiness>,
    /// Maximum context window in tokens.
    pub context_window: u32,
    /// Maximum output tokens per turn.
    pub max_output_tokens: u32,
    /// Whether extended thinking is enabled.
    pub thinking_enabled: bool,
    /// Token budget for extended thinking.
    pub thinking_budget: u32,
    /// Maximum tool iterations per turn.
    pub max_tool_iterations: u32,
    /// Actor lifecycle status.
    pub status: String,
    /// Total number of background failures recorded since actor start.
    pub background_failure_total_count: u32,
    /// Number of background failures recorded in the recent window.
    pub background_failure_recent_count: u32,
    /// Human-readable message from the most recent background failure, if any.
    pub background_failure_latest_message: Option<String>,
    /// Classification kind of the most recent background failure, if any.
    pub background_failure_latest_kind: Option<String>,
    /// Whether recent background failures have pushed the actor into degraded background health.
    ///
    /// This does not change the actor lifecycle; it is an independent signal used by the
    /// detailed health endpoint to surface repeated background-work failures.
    pub background_health_degraded: bool,
    /// Current cross-nous inbound address mask for this agent.
    pub address_mask: AddressMaskStatus,
}

/// Diagnostic view of a cross-nous inbound address mask.
#[derive(Debug, Serialize, ToSchema)]
pub struct AddressMaskStatus {
    /// Stable mask kind: `public`, `operator_only`, or `allow_list`.
    pub kind: String,
    /// Sender ids allowed by an `allow_list` mask.
    pub allowed_senders: Vec<String>,
}

/// Response listing tools available to a nous agent.
#[derive(Debug, Serialize, ToSchema)]
pub struct ToolsResponse {
    /// Tool summaries.
    pub tools: Vec<ToolSummary>,
    /// Whether the requested config change was persisted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_applied: Option<bool>,
    /// Whether the running actor/runtime now reflects the requested state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_applied: Option<bool>,
    /// Whether a config reload is required before the requested state is live.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reload_required: Option<bool>,
    /// Whether a process restart is required before the requested state is live.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restart_required: Option<bool>,
}

/// Brief description of a registered tool.
#[derive(Debug, Serialize, ToSchema)]
pub struct ToolSummary {
    /// Tool name as sent to the LLM.
    pub name: String,
    /// Whether the tool is enabled for this agent.
    pub enabled: bool,
    /// Human-readable description.
    pub description: String,
    /// Tool category (e.g. `"Builtin"`, `"Pack"`).
    pub category: String,
    /// Whether the tool activates automatically without explicit configuration.
    ///
    /// When `false` the tool is lazy and must be activated via `enable_tool`
    /// before the agent can use it.
    pub auto_activate: bool,
}

/// Request body for toggling a nous agent.
#[derive(Debug, Deserialize, ToSchema)]
pub struct NousToggleRequest {
    /// Whether the agent should be enabled.
    pub enabled: bool,
}

/// Request body for toggling a nous tool.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ToolToggleRequest {
    /// Tool name to toggle.
    pub tool: String,
    /// Whether the tool should be enabled.
    pub enabled: bool,
}
