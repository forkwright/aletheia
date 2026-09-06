//! Nous agent detail/recovery DTOs mirroring Pylon's wire shapes
//! (`crates/pylon/src/handlers/nous_dto.rs`,
//! `crates/pylon/src/handlers/providers_dto.rs`). Skene has no dependency on
//! pylon, so these are independent structs kept in sync by the contract
//! tests in `super::tests`.

use serde::Deserialize;

/// Per-model provider readiness for an agent's model chain.
///
/// Mirrors `pylon::handlers::providers_dto::ModelProviderReadiness`.
#[derive(Debug, Clone, Deserialize)]
#[expect(
    missing_docs,
    reason = "fields mirror pylon's ModelProviderReadiness; self-documenting by name"
)]
pub struct ModelProviderReadiness {
    pub model: String,
    pub provider: Option<String>,
    pub health: Option<String>,
    pub available: bool,
}

/// Diagnostic view of a cross-nous inbound address mask.
///
/// Mirrors `pylon::handlers::nous_dto::AddressMaskStatus`.
#[derive(Debug, Clone, Deserialize)]
pub struct AddressMaskStatus {
    /// Stable mask kind: `public`, `operator_only`, or `allow_list`.
    pub kind: String,
    /// Sender ids allowed by an `allow_list` mask.
    pub allowed_senders: Vec<String>,
}

/// Detailed status of a single nous agent (`GET /api/v1/nous/{id}`).
///
/// Mirrors `pylon::handlers::nous_dto::NousStatus`.
#[derive(Debug, Clone, Deserialize)]
#[expect(
    missing_docs,
    reason = "fields mirror pylon's NousStatus; self-documenting by name"
)]
pub struct NousStatus {
    pub id: String,
    pub model: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub fallback_models: Vec<String>,
    #[serde(default)]
    pub fallback_providers: Vec<Option<String>>,
    pub retries_before_fallback: u32,
    pub complexity_routing_enabled: bool,
    pub complexity_no_llm_threshold: u32,
    pub complexity_low_threshold: u32,
    pub complexity_high_threshold: u32,
    #[serde(default)]
    pub provider_readiness: Vec<ModelProviderReadiness>,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub thinking_enabled: bool,
    pub thinking_budget: u32,
    pub max_tool_iterations: u32,
    pub status: String,
    pub background_failure_total_count: u32,
    pub background_failure_recent_count: u32,
    #[serde(default)]
    pub background_failure_latest_message: Option<String>,
    #[serde(default)]
    pub background_failure_latest_kind: Option<String>,
    pub background_health_degraded: bool,
    pub address_mask: AddressMaskStatus,
}

/// Response from a recovery attempt (`POST /api/v1/nous/{id}/recover`).
///
/// Mirrors `pylon::handlers::nous_dto::RecoverResponse`.
#[derive(Debug, Clone, Deserialize)]
pub struct RecoverResponse {
    /// Agent identifier.
    pub id: String,
    /// Whether recovery was performed (`false` if the agent was not
    /// degraded).
    pub recovered: bool,
}
