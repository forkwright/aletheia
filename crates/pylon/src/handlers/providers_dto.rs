// WHY: wire DTO
//! Provider control-plane response shapes.

use serde::Serialize;
use utoipa::ToSchema;

/// List of registered LLM providers and their runtime readiness.
#[derive(Debug, Serialize, ToSchema)]
pub struct ProviderListResponse {
    /// Provider entries in registration order.
    pub providers: Vec<ProviderInfo>,
}

/// Single provider inventory and health snapshot.
#[derive(Debug, Serialize, ToSchema)]
pub struct ProviderInfo {
    /// Provider identifier from configuration.
    pub name: String,
    /// Provider kind (e.g. `"openai"`, `"anthropic"`, `"openai-compatible"`).
    pub kind: String,
    /// Deployment target class.
    pub deployment_target: String,
    /// Redacted base URL: scheme + host + path only; credentials and query
    /// strings are stripped before serialization.
    pub base_url: String,
    /// Models the runtime provider reports it can serve.
    pub supported_models: Vec<String>,
    /// Models explicitly listed for this provider in configuration.
    pub configured_models: Vec<String>,
    /// Current health status: `"up"`, `"degraded"`, or `"down"`.
    pub health: String,
    /// Diagnostic reason when health is not `"up"`.
    pub health_reason: Option<String>,
    /// Credential source class: `"env"` or `"none"`.
    pub auth_source: String,
    /// Whether the provider is currently available for routing.
    pub available: bool,
}

/// Provider selected for a requested model.
#[derive(Debug, Serialize, ToSchema)]
pub struct ProviderRouteResponse {
    /// Model that was looked up.
    pub model: String,
    /// Name of the provider that would handle the model, if any.
    pub provider: Option<String>,
    /// Health status of the resolved provider, if any.
    pub health: Option<String>,
    /// Whether the resolved provider is currently available.
    pub available: Option<bool>,
}

/// Readiness of a single model route.
#[derive(Debug, Serialize, ToSchema)]
pub struct ModelProviderReadiness {
    /// Model identifier.
    pub model: String,
    /// Provider that would handle the model, if resolved.
    pub provider: Option<String>,
    /// Provider health status, if resolved.
    pub health: Option<String>,
    /// Whether the route is currently available.
    pub available: bool,
}
