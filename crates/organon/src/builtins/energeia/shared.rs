//! Shared helpers and types used across energeia tool submodules.

use std::sync::Arc;

use energeia::cron::CronLockStore;
use energeia::orchestrator::Orchestrator;
use energeia::store::EnergeiaStore;

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
    /// Cron fire state store for scheduler observability.
    pub cron_lock_store: Option<Arc<CronLockStore>>,
    /// Configured cron task names to include in status output.
    pub cron_task_names: Vec<String>,
}

impl EnergeiaServices {
    /// Create a service bundle for Energeia tool executors.
    #[must_use]
    pub fn new(orchestrator: Arc<Orchestrator>, store: Arc<EnergeiaStore>) -> Self {
        Self {
            orchestrator,
            store,
            cron_lock_store: None,
            cron_task_names: Vec::new(),
        }
    }

    /// Attach cron fire state used by status reporting and the cron executor.
    #[must_use]
    pub fn with_cron_lock_store(
        mut self,
        cron_lock_store: Arc<CronLockStore>,
        cron_task_names: Vec<String>,
    ) -> Self {
        self.cron_lock_store = Some(cron_lock_store);
        self.cron_task_names = cron_task_names;
        self
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Extract a required string field from tool arguments.
pub(super) fn require_str<'a>(
    args: &'a serde_json::Value,
    field: &str,
) -> std::result::Result<&'a str, String> {
    args.get(field)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("missing required field '{field}'"))
}

/// Extract an optional string field from tool arguments.
pub(super) fn opt_str<'a>(args: &'a serde_json::Value, field: &str) -> Option<&'a str> {
    args.get(field).and_then(|v| v.as_str())
}

/// Extract an optional u64 field from tool arguments.
pub(super) fn opt_u64(args: &serde_json::Value, field: &str) -> Option<u64> {
    args.get(field).and_then(serde_json::Value::as_u64)
}

/// Extract an optional finite f64 field from tool arguments.
pub(super) fn opt_f64(
    args: &serde_json::Value,
    field: &str,
) -> std::result::Result<Option<f64>, String> {
    match args.get(field) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(value) => value
            .as_f64()
            .filter(|number| number.is_finite())
            .map(Some)
            .ok_or_else(|| format!("field '{field}' must be a finite number")),
    }
}

/// Extract an optional bool field from tool arguments.
pub(super) fn opt_bool(args: &serde_json::Value, field: &str) -> Option<bool> {
    args.get(field).and_then(serde_json::Value::as_bool)
}

/// Serialize a value to a pretty-printed JSON `ToolResult`.
pub(super) fn to_json_text<T: serde::Serialize>(value: &T) -> ToolResult {
    match serde_json::to_string_pretty(value) {
        Ok(text) => ToolResult::text(text),
        Err(e) => ToolResult::error(format!("serialization error: {e}")),
    }
}
