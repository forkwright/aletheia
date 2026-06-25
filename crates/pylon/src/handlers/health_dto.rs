// WHY: wire DTO
//! Health endpoint response wire shapes.

use serde::Serialize;
use utoipa::ToSchema;

/// Public liveness response for unauthenticated health probes.
#[derive(Debug, Serialize, ToSchema)]
pub struct LivenessResponse {
    /// Minimal process status. If this response is returned, pylon is alive.
    #[schema(value_type = String)]
    pub status: &'static str,
}

/// Operator-only health response combining all subsystem checks.
#[derive(Debug, Serialize, ToSchema)]
pub struct HealthResponse {
    /// Aggregate status: `"healthy"`, `"degraded"`, or `"unhealthy"`.
    #[schema(value_type = String)]
    pub status: &'static str,
    /// Crate version from `Cargo.toml`.
    #[schema(value_type = String)]
    pub version: &'static str,
    /// Build git SHA when available from the build environment.
    #[schema(value_type = String)]
    pub git_sha: &'static str,
    /// Seconds since server start.
    pub uptime_seconds: u64,
    /// Individual subsystem check results.
    pub checks: Vec<HealthCheck>,
    /// Absolute path to the instance data directory.
    pub data_dir: String,
}

/// Result of a single subsystem health check.
#[derive(Debug, Serialize, ToSchema)]
pub struct HealthCheck {
    /// Subsystem name (e.g. `"session_store"`, `"providers"`).
    #[schema(value_type = String)]
    pub name: &'static str,
    /// Check outcome: `"pass"`, `"warn"`, `"fail"`, or `"timeout"`.
    #[schema(value_type = String)]
    pub status: &'static str,
    /// Diagnostic message when status is not `"pass"`.
    pub message: Option<String>,
    /// Structured per-subsystem details that are safe to expose to the
    /// control plane. For `provider_reachability` this contains the per-provider
    /// status list; other checks may leave it empty.
    #[schema(value_type = Object, nullable = true)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}
