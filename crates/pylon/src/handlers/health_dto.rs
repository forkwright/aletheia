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

/// Operator-only diagnostics response combining all subsystem checks.
///
/// WHY(#5312): full local paths are intentionally gated behind the bearer-auth
/// `/api/v1/system/health` route. The unauthenticated `/api/health` and
/// `/health` endpoints return [`LivenessResponse`] only.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct HealthResponse {
    /// Aggregate status: `"healthy"`, `"degraded"`, or `"unhealthy"`.
    pub status: String,
    /// Crate version from `Cargo.toml`.
    pub version: String,
    /// Build git SHA when available from the build environment.
    // kanon:ignore RUST/primitive-for-domain-id — wire DTO field; git SHA is sourced from build env, not a first-party domain ID
    pub git_sha: String,
    /// Seconds since server start.
    pub uptime_seconds: u64,
    /// Individual subsystem check results.
    pub checks: Vec<HealthCheck>,
    /// Absolute path to the instance data directory.
    ///
    /// Operator-only: this field is not present on public liveness responses.
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

/// Authoritative, operator-grade subsystem status record (#5313).
///
/// Distinct from [`HealthCheck`]: each record names an explicit code owner,
/// uses a smaller and more meaningful status vocabulary
/// (`"healthy"`/`"degraded"`/`"failed"`/`"unknown"`), and — critically — is
/// allowed to report `"unknown"` rather than defaulting an unreachable
/// subsystem to `"healthy"`. A control plane that lies toward optimism when
/// it cannot actually see a subsystem is worse than one that says so.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SubsystemStatus {
    /// Stable machine identifier (e.g. `"session_store"`).
    pub id: String,
    /// Human-readable name for the control-plane UI.
    pub name: String,
    /// `"healthy"`, `"degraded"`, `"failed"`, or `"unknown"`.
    pub status: String,
    /// Crate/module that owns this subsystem's behavior — one code owner
    /// per record, per #5313's acceptance criteria.
    pub owner: String,
    /// When this record was computed (ISO 8601, UTC).
    pub last_checked: String,
    /// Most recent known success timestamp, when tracked. Absent (not
    /// fabricated) for subsystems that only report point-in-time status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_success: Option<String>,
    /// Most recent known failure timestamp, when tracked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_failure: Option<String>,
    /// Explanation when status is `"degraded"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
    /// Explanation when status is `"failed"` or `"unknown"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
    /// Redacted structured diagnostics: counts, backlog depth, per-item
    /// detail. Never credentials, tokens, or absolute host paths.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Object, nullable = true)]
    pub details: Option<serde_json::Value>,
    /// A known remediation step, when Aletheia can suggest one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_action: Option<String>,
}

/// Response for `GET /api/v1/system/status` (#5313).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SubsystemStatusResponse {
    /// Aggregate status across every subsystem: `"healthy"` (all healthy or
    /// unknown), `"degraded"` (any degraded, none failed), or `"failed"`
    /// (any failed). `"unknown"` subsystems do not by themselves elevate
    /// the aggregate — a subsystem this endpoint cannot yet see is a gap to
    /// close, not evidence the system is down — but every subsystem is
    /// always listed so the gap stays visible rather than silently absent.
    pub status: String,
    /// When this snapshot was computed (ISO 8601, UTC).
    pub generated_at: String,
    /// One record per tracked subsystem.
    pub subsystems: Vec<SubsystemStatus>,
}
