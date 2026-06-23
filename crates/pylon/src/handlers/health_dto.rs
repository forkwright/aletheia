// WHY: wire DTO
//! Health endpoint response wire shapes.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Public liveness response for unauthenticated health probes.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct LivenessResponse {
    /// Minimal process status. If this response is returned, pylon is alive.
    pub status: String,
}

/// Operator-only health response combining all subsystem checks.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct HealthResponse {
    /// Aggregate status: `"healthy"`, `"degraded"`, or `"unhealthy"`.
    pub status: String,
    /// Crate version from `Cargo.toml`.
    pub version: String,
    /// Build git SHA when available from the build environment.
    pub git_sha: String,
    /// Seconds since server start.
    pub uptime_seconds: u64,
    /// Individual subsystem check results.
    pub checks: Vec<HealthCheck>,
    /// Absolute path to the instance data directory.
    pub data_dir: String,
}

/// Result of a single subsystem health check.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct HealthCheck {
    /// Subsystem name (e.g. `"session_store"`, `"providers"`).
    pub name: String,
    /// Check outcome: `"pass"`, `"warn"`, `"fail"`, or `"timeout"`.
    pub status: String,
    /// Diagnostic message when status is not `"pass"`.
    pub message: Option<String>,
    /// Structured per-subsystem details that are safe to expose to the
    /// control plane. For `provider_reachability` this contains the per-provider
    /// status list; other checks may leave it empty.
    #[schema(value_type = Object, nullable = true)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}
