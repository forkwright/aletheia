//! Runtime feature flag configuration.

use serde::{Deserialize, Serialize};

/// A single feature flag exposed through the config API.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureFlagConfig {
    /// Stable flag identifier.
    pub key: String,
    /// Human-readable description of what the flag controls.
    pub description: String,
    /// Whether the feature is currently enabled.
    #[serde(default)]
    pub enabled: bool,
}
