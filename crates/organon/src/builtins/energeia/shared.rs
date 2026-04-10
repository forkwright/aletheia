//! Shared helpers and types used across energeia tool submodules.

use std::sync::Arc;

use aletheia_energeia::orchestrator::Orchestrator;
use aletheia_energeia::store::EnergeiaStore;

use crate::types::ToolResult;

// ── Services ────────────────────────────────────────────────────────────────

/// Services injected at registration time for energeia tool executors.
///
/// The orchestrator handles dispatch (dromeus), and the store backs lessons,
/// observations, and metrics (mathesis, parateresis, metron, diorthosis).
pub struct EnergeiaServices {
    /// Top-level dispatch orchestrator wiring engine, QA, and store.
    pub orchestrator: Arc<Orchestrator>,
    /// State persistence store for lessons, observations, and CI validations.
    pub store: Arc<EnergeiaStore>,
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Extract a required string field from tool arguments.
pub(super) fn require_str<'a>(
    args: &'a serde_json::Value,
    field: &str,
) -> std::result::Result<&'a str, ToolResult> {
    args.get(field)
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolResult::error(format!("missing required field '{field}'")))
}

/// Extract an optional string field from tool arguments.
pub(super) fn opt_str<'a>(args: &'a serde_json::Value, field: &str) -> Option<&'a str> {
    args.get(field).and_then(|v| v.as_str())
}

/// Extract an optional u64 field from tool arguments.
pub(super) fn opt_u64(args: &serde_json::Value, field: &str) -> Option<u64> {
    args.get(field).and_then(|v| v.as_u64())
}

/// Extract an optional bool field from tool arguments.
pub(super) fn opt_bool(args: &serde_json::Value, field: &str) -> Option<bool> {
    args.get(field).and_then(|v| v.as_bool())
}

/// Serialize a value to a pretty-printed JSON ToolResult.
pub(super) fn to_json_text<T: serde::Serialize>(value: &T) -> ToolResult {
    match serde_json::to_string_pretty(value) {
        Ok(text) => ToolResult::text(text),
        Err(e) => ToolResult::error(format!("serialization error: {e}")),
    }
}
