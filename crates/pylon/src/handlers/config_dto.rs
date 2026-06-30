// WHY: wire DTO
//! Config endpoint response and schema wire shapes.
#![expect(missing_docs, reason = "OpenAPI schema mirrors taxis config shapes")]

use serde_json::Value;
use utoipa::ToSchema;

/// Response wrapper for config section update metadata.
#[derive(serde::Serialize, ToSchema)]
pub struct ConfigUpdateResponse {
    /// Name of the config section that was updated.
    pub section: String,
    /// The updated config section value.
    pub config: Value,
    /// Field paths that require a restart to take effect.
    pub restart_required: Vec<String>,
}

/// Response wrapper for full config reload.
#[derive(serde::Serialize, ToSchema)]
pub struct ConfigReloadResponse {
    /// Number of hot-reloadable values that were updated.
    pub hot_reloaded: usize,
    /// Field paths that changed but require a restart to take effect.
    pub restart_required: Vec<String>,
    /// All changed field paths (both hot and cold).
    pub changed: Vec<String>,
}

/// Schema for a single feature flag entry.
#[derive(serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FeatureFlagConfig {
    pub key: String,
    pub description: String,
    #[serde(default)]
    pub enabled: bool,
}

/// Schema for the `agents` config section.
#[derive(serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentsConfig {
    #[schema(value_type = Object)]
    pub defaults: Option<Value>,
    pub list: Option<Vec<Value>>,
}

/// Schema for the `gateway` config section.
#[derive(serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GatewayConfig {
    pub port: Option<u16>,
    pub bind: Option<String>,
    #[schema(value_type = Object)]
    pub auth: Option<Value>,
    #[schema(value_type = Object)]
    pub tls: Option<Value>,
    #[schema(value_type = Object)]
    pub cors: Option<Value>,
    #[schema(value_type = Object)]
    pub body_limit: Option<Value>,
    #[schema(value_type = Object)]
    pub csrf: Option<Value>,
    #[schema(value_type = Object)]
    pub rate_limit: Option<Value>,
}

/// Schema for the `channels` config section.
#[derive(serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChannelsConfig {
    #[schema(value_type = Object)]
    pub signal: Option<Value>,
    #[schema(value_type = Object)]
    pub matrix: Option<Value>,
}

/// Schema for a single channel binding entry.
#[derive(serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChannelBinding {
    pub channel: Option<String>,
    pub source: Option<String>,
    pub nous_id: Option<String>,
    pub session_key: Option<String>,
}

/// Schema for the `embedding` config section.
#[derive(serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingSettings {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub dimension: Option<usize>,
    pub base_url: Option<String>,
    pub api_key_env: Option<String>,
}

/// Schema for the `data` config section.
#[derive(serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DataConfig {
    #[schema(value_type = Object)]
    pub retention: Option<Value>,
}

/// Schema for the `maintenance` config section.
#[derive(serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MaintenanceConfig {
    #[schema(value_type = Object)]
    pub trace_rotation: Option<Value>,
    #[schema(value_type = Object)]
    pub drift_detection: Option<Value>,
    #[schema(value_type = Object)]
    pub db_monitoring: Option<Value>,
    #[schema(value_type = Object)]
    pub disk_space: Option<Value>,
    #[schema(value_type = Object)]
    pub retention: Option<Value>,
    pub knowledge_maintenance_enabled: Option<bool>,
    #[schema(value_type = Object)]
    pub watchdog: Option<Value>,
    #[schema(value_type = Object)]
    pub cron_tasks: Option<Value>,
    #[schema(value_type = Object)]
    pub backup: Option<Value>,
    /// Prosoche heartbeat and self-audit scheduling configuration.
    ///
    /// The runtime scheduler owns these values through `[maintenance.prosoche]`.
    #[schema(value_type = Object)]
    pub prosoche: Option<Value>,
}

/// Schema for a single pricing entry.
#[derive(serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModelPricing {
    pub input_cost_per_mtok: Option<f64>,
    pub output_cost_per_mtok: Option<f64>,
}
