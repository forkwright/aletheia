//! Configuration endpoint DTOs mirroring Pylon's wire shapes
//! (`crates/pylon/src/handlers/config_dto.rs`). Skene has no dependency on
//! pylon, so these are independent structs kept in sync by the contract
//! tests in `super::tests`.

use serde::Deserialize;

/// Response wrapper for a config section update (`PUT /api/v1/config/{section}`).
///
/// Mirrors `pylon::handlers::config_dto::ConfigUpdateResponse`.
#[derive(Debug, Clone, Deserialize)]
pub struct ConfigUpdateResponse {
    /// Name of the config section that was updated.
    pub section: String,
    /// The updated config section value.
    pub config: serde_json::Value,
    /// Field paths that require a restart to take effect.
    pub restart_required: Vec<String>,
}

/// Response wrapper for a full config reload (`POST /api/v1/config/reload`).
///
/// Mirrors `pylon::handlers::config_dto::ConfigReloadResponse`.
#[derive(Debug, Clone, Deserialize)]
pub struct ConfigReloadResponse {
    /// Number of hot-reloadable values that were updated.
    pub hot_reloaded: usize,
    /// Field paths that changed but require a restart to take effect.
    pub restart_required: Vec<String>,
    /// All changed field paths (both hot and cold).
    pub changed: Vec<String>,
}
