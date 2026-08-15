//! Provider inventory and model-route resolution types.
//!
//! Field names mirror `pylon::handlers::providers_dto` verbatim (that DTO
//! carries no `rename_all`) so these deserialize without any wire-format
//! translation (#4890).

use serde::Deserialize;

/// List of registered LLM providers and their runtime readiness.
#[derive(Debug, Clone, Deserialize)]
pub struct ProviderListResponse {
    /// Provider entries in registration order.
    #[serde(default)]
    pub providers: Vec<ProviderInfo>,
}

/// Single provider inventory and health snapshot.
#[derive(Debug, Clone, Deserialize)]
pub struct ProviderInfo {
    /// Provider identifier from configuration.
    pub name: String,
    /// Provider kind (e.g. `"openai"`, `"anthropic"`, `"openai-compatible"`).
    pub kind: String,
    /// Deployment target class.
    pub deployment_target: String,
    /// Redacted base URL: scheme + host + path only.
    pub base_url: String,
    /// Models the runtime provider reports it can serve.
    #[serde(default)]
    pub supported_models: Vec<String>,
    /// Models explicitly listed for this provider in configuration.
    #[serde(default)]
    pub configured_models: Vec<String>,
    /// Current health status: `"up"`, `"degraded"`, or `"down"`.
    pub health: String,
    /// Diagnostic reason when health is not `"up"`.
    #[serde(default)]
    pub health_reason: Option<String>,
    /// Credential source class: `"env:<VAR>"` or `"none"`.
    pub auth_source: String,
    /// Whether the provider is currently available for routing.
    pub available: bool,
}

/// Provider selected for a requested model.
#[derive(Debug, Clone, Deserialize)]
pub struct ProviderRouteResponse {
    /// Model that was looked up.
    pub model: String,
    /// Name of the provider that would handle the model, if any.
    #[serde(default)]
    pub provider: Option<String>,
    /// Health status of the resolved provider, if any.
    #[serde(default)]
    pub health: Option<String>,
    /// Whether the resolved provider is currently available.
    #[serde(default)]
    pub available: Option<bool>,
}
