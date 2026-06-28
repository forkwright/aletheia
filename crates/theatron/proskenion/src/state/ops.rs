// kanon:ignore RUST/file-too-long — state module with co-located tests; splitting would fragment invariants from their assertions
//! Ops dashboard state: agent status cards, service health, and toggle controls.

use std::collections::HashMap;

use skene::id::NousId;

// -- Agent card data ----------------------------------------------------------

/// Health tier for an agent, derived from SSE status strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum HealthTier {
    /// Agent is operating normally.
    #[default]
    Healthy,
    /// Agent has warnings or partial failures.
    Degraded,
    /// Agent is in an error state or unreachable.
    Error,
}

impl HealthTier {
    /// CSS color for the status dot.
    #[must_use]
    pub(crate) fn dot_color(&self) -> &'static str {
        match self {
            Self::Healthy => "var(--status-success)",
            Self::Degraded => "var(--status-warning)",
            Self::Error => "var(--status-error)",
        }
    }

    /// Human-readable label.
    #[must_use]
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Degraded => "degraded",
            Self::Error => "error",
        }
    }
}

/// Display data for a single agent status card.
#[derive(Debug, Clone)]
pub(crate) struct AgentCardData {
    pub id: NousId,
    pub name: String,
    pub emoji: Option<String>,
    pub health: HealthTier,
    pub model: String,
    pub active_turns: u32,
    pub last_activity: Option<String>,
    pub connected: bool,
}

/// Store for agent status card data, keyed by NousId.
#[derive(Debug, Clone, Default)]
pub(crate) struct AgentStatusStore {
    pub cards: HashMap<NousId, AgentCardData>,
    pub order: Vec<NousId>,
}

impl AgentStatusStore {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Replace all agent cards from a fresh API fetch.
    pub(crate) fn load(&mut self, cards: Vec<AgentCardData>) {
        self.cards.clear();
        self.order.clear();
        for card in cards {
            let id = card.id.clone();
            self.order.push(id.clone());
            self.cards.insert(id, card);
        }
    }

    /// Update active turn count for an agent.
    pub(crate) fn set_active_turns(&mut self, id: &NousId, count: u32) {
        if let Some(card) = self.cards.get_mut(id) {
            card.active_turns = count;
        }
    }

    /// Update health tier for an agent from SSE status string.
    pub(crate) fn set_health(&mut self, id: &NousId, health: HealthTier) {
        if let Some(card) = self.cards.get_mut(id) {
            card.health = health;
        }
    }

    /// Update last activity timestamp.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "wired when SSE activity events are plumbed")
    )]
    pub(crate) fn set_last_activity(&mut self, id: &NousId, activity: String) {
        if let Some(card) = self.cards.get_mut(id) {
            card.last_activity = Some(activity);
        }
    }

    /// All cards in server order.
    #[must_use]
    pub(crate) fn ordered(&self) -> Vec<&AgentCardData> {
        self.order
            .iter()
            .filter_map(|id| self.cards.get(id))
            .collect()
    }
}

// -- Service health -----------------------------------------------------------

/// Aggregate health status derived from the server's `/api/health` response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum HealthStatus {
    /// No health data has been loaded or the response was unparseable.
    #[default]
    Unknown,
    /// All subsystem checks pass.
    Healthy,
    /// One or more checks warn; no hard failures.
    Degraded,
    /// One or more checks fail or time out.
    Unhealthy,
}

impl HealthStatus {
    #[must_use]
    pub(crate) fn from_status(status: &str) -> Self {
        match status {
            "healthy" | "pass" => Self::Healthy,
            "degraded" | "warn" => Self::Degraded,
            "unhealthy" | "fail" | "timeout" => Self::Unhealthy,
            _ => Self::Unknown,
        }
    }

    #[must_use]
    pub(crate) fn dot_color(&self) -> &'static str {
        match self {
            Self::Healthy => "var(--status-success)",
            Self::Degraded => "var(--status-warning)",
            Self::Unhealthy => "var(--status-error)",
            Self::Unknown => "var(--text-muted)",
        }
    }

    #[must_use]
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Degraded => "degraded",
            Self::Unhealthy => "unhealthy",
            Self::Unknown => "unknown",
        }
    }
}

/// A single check row from the server's `/api/health` response.
#[derive(Debug, Clone)]
pub(crate) struct HealthCheckInfo {
    pub name: String,
    pub status: String,
    pub message: Option<String>,
    pub details: Option<serde_json::Value>,
}

/// Aggregate service health data.
#[derive(Debug, Clone, Default)]
pub(crate) struct ServiceHealthStore {
    /// Aggregate status reported by the server.
    pub status: HealthStatus,
    /// Individual subsystem checks.
    pub checks: Vec<HealthCheckInfo>,
    /// Reachability or parse error when health data could not be loaded.
    pub error: Option<String>,
}

impl ServiceHealthStore {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Build store from a parsed health response.
    pub(crate) fn from_response(response: skene::api::types::HealthResponse) -> Self {
        Self {
            status: HealthStatus::from_status(&response.status),
            checks: response
                .checks
                .into_iter()
                .map(|c| HealthCheckInfo {
                    name: c.name,
                    status: c.status,
                    message: c.message,
                    details: c.details,
                })
                .collect(),
            error: None,
        }
    }

    /// Build store for an unreachable or unparseable health response.
    pub(crate) fn unreachable(message: String) -> Self {
        Self {
            status: HealthStatus::Unknown,
            checks: Vec::new(),
            error: Some(message),
        }
    }
}

// -- Toggle controls ----------------------------------------------------------

/// An agent toggle entry: enabled/disabled with in-flight state.
#[derive(Debug, Clone)]
pub(crate) struct AgentToggle {
    pub id: NousId,
    pub name: String,
    pub enabled: bool,
    pub pending: bool,
    pub apply_state: ToggleApplyState,
    pub live_status: Option<String>,
}

/// A tool toggle for a specific agent.
#[derive(Debug, Clone)]
pub(crate) struct ToolToggle {
    pub agent_id: NousId,
    pub tool_name: String,
    pub enabled: bool,
    pub pending: bool,
    pub apply_state: ToggleApplyState,
}

/// Runtime effect state for a persisted toggle request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ToggleApplyState {
    #[default]
    Synced,
    Pending,
    Degraded,
    ReloadRequired,
    RestartRequired,
    Failed,
}

/// Server-reported effect fields for a toggle mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ToggleActionResult {
    pub config_applied: bool,
    pub live_applied: bool,
    pub reload_required: bool,
    pub restart_required: bool,
}

impl ToggleActionResult {
    #[must_use]
    pub(crate) fn synced() -> Self {
        Self {
            config_applied: true,
            live_applied: true,
            reload_required: false,
            restart_required: false,
        }
    }

    #[must_use]
    pub(crate) fn failed() -> Self {
        Self {
            config_applied: false,
            live_applied: false,
            reload_required: false,
            restart_required: false,
        }
    }
}

impl ToggleApplyState {
    #[must_use]
    pub(crate) fn from_action(result: ToggleActionResult) -> Self {
        if !result.config_applied {
            Self::Failed
        } else if result.restart_required {
            Self::RestartRequired
        } else if result.reload_required {
            Self::ReloadRequired
        } else if result.live_applied {
            Self::Synced
        } else {
            Self::Pending
        }
    }
}

/// A system-wide feature flag.
#[derive(Debug, Clone)]
pub(crate) struct FeatureFlag {
    pub key: String, // kanon:ignore RUST/plain-string-secret -- feature flag identifier, not credential material (#3988)
    pub description: String,
    pub enabled: bool,
    pub pending: bool,
    /// Human-readable error from the last failed update. Kept visible until
    /// a later update succeeds or the user refreshes the panel.
    pub error: Option<String>,
}

/// Wire payload for a single feature flag entry sent to
/// `PUT /api/v1/config/feature_flags`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FeatureFlagPayloadEntry {
    pub key: String,
    pub description: String,
    pub enabled: bool,
}

/// Aggregate toggle state with optimistic update support.
#[derive(Debug, Clone, Default)]
pub(crate) struct ToggleStore {
    pub agent_toggles: Vec<AgentToggle>,
    pub tool_toggles: Vec<ToolToggle>,
    pub feature_flags: Vec<FeatureFlag>,
    pub expanded_agent: Option<NousId>,
    /// Paths returned by the last config update that require a server restart.
    /// Surfaced in the feature-flag panel so operators know when a restart is
    /// needed for a change to take effect.
    pub restart_required: Vec<String>,
}

impl ToggleStore {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Optimistically flip an agent toggle. Returns the previous state for rollback.
    pub(crate) fn flip_agent(&mut self, id: &NousId) -> Option<bool> {
        self.agent_toggles
            .iter_mut()
            .find(|t| t.id == *id)
            .map(|t| {
                let prev = t.enabled;
                t.enabled = !prev;
                t.pending = true;
                t.apply_state = ToggleApplyState::Synced;
                prev
            })
    }

    /// Resolve an in-flight agent toggle (clear pending state).
    pub(crate) fn resolve_agent(&mut self, id: &NousId, success: bool, prev: bool) {
        let result = if success {
            ToggleActionResult::synced()
        } else {
            ToggleActionResult::failed()
        };
        self.resolve_agent_result(id, prev, None, None, result);
    }

    /// Resolve an in-flight agent toggle using server-reported runtime effects.
    pub(crate) fn resolve_agent_result(
        &mut self,
        id: &NousId,
        prev: bool,
        applied_enabled: Option<bool>,
        live_status: Option<String>,
        result: ToggleActionResult,
    ) {
        if let Some(t) = self.agent_toggles.iter_mut().find(|t| t.id == *id) {
            t.pending = false;
            if result.config_applied {
                if let Some(enabled) = applied_enabled {
                    t.enabled = enabled;
                }
                t.live_status = live_status;
            } else {
                t.enabled = prev;
                t.live_status = None;
            }
            t.apply_state = if t.live_status.as_deref() == Some("degraded")
                && !result.live_applied
                && result.restart_required
            {
                ToggleApplyState::Degraded
            } else {
                ToggleApplyState::from_action(result)
            };
        }
    }

    /// Optimistically flip a tool toggle.
    pub(crate) fn flip_tool(&mut self, agent_id: &NousId, tool_name: &str) -> Option<bool> {
        self.tool_toggles
            .iter_mut()
            .find(|t| t.agent_id == *agent_id && t.tool_name == tool_name)
            .map(|t| {
                let prev = t.enabled;
                t.enabled = !prev;
                t.pending = true;
                t.apply_state = ToggleApplyState::Synced;
                prev
            })
    }

    /// Resolve an in-flight tool toggle.
    pub(crate) fn resolve_tool(
        &mut self,
        agent_id: &NousId,
        tool_name: &str,
        success: bool,
        prev: bool,
    ) {
        let result = if success {
            ToggleActionResult::synced()
        } else {
            ToggleActionResult::failed()
        };
        self.resolve_tool_result(agent_id, tool_name, prev, None, result);
    }

    /// Resolve an in-flight tool toggle using server-reported runtime effects.
    pub(crate) fn resolve_tool_result(
        &mut self,
        agent_id: &NousId,
        tool_name: &str,
        prev: bool,
        applied_enabled: Option<bool>,
        result: ToggleActionResult,
    ) {
        if let Some(t) = self
            .tool_toggles
            .iter_mut()
            .find(|t| t.agent_id == *agent_id && t.tool_name == tool_name)
        {
            t.pending = false;
            if result.config_applied {
                if let Some(enabled) = applied_enabled {
                    t.enabled = enabled;
                }
            } else {
                t.enabled = prev;
            }
            t.apply_state = ToggleApplyState::from_action(result);
        }
    }

    /// Optimistically flip a feature flag.
    pub(crate) fn flip_feature(&mut self, key: &str) -> Option<bool> {
        self.feature_flags
            .iter_mut()
            .find(|f| f.key == key)
            .map(|f| {
                let prev = f.enabled;
                f.enabled = !prev;
                f.pending = true;
                prev
            })
    }

    /// Build the complete `Vec<FeatureFlagPayloadEntry>` that must be sent to
    /// `PUT /api/v1/config/feature_flags`. Sending the whole section preserves
    /// every flag's state instead of PATCH-ing a single key.
    #[must_use]
    pub(crate) fn feature_flags_payload(&self) -> Vec<FeatureFlagPayloadEntry> {
        self.feature_flags
            .iter()
            .map(|f| FeatureFlagPayloadEntry {
                key: f.key.clone(),
                description: f.description.clone(),
                enabled: f.enabled,
            })
            .collect()
    }

    /// Resolve an in-flight feature flag toggle.
    ///
    /// On failure the optimistic flip is *not* silently reverted; instead the
    /// flag keeps its new state and `error` is populated so the UI can show a
    /// visible failure state. On success the error is cleared.
    pub(crate) fn resolve_feature(
        &mut self,
        key: &str,
        success: bool,
        _prev: bool,
        error: Option<String>,
        restart_required: Vec<String>,
    ) {
        self.restart_required = restart_required;
        if let Some(f) = self.feature_flags.iter_mut().find(|f| f.key == key) {
            f.pending = false;
            if success {
                f.error = None;
            } else {
                // Keep the optimistic state (do not roll back) and surface the
                // failure so the operator sees the write did not land.
                f.error = error.or(Some("Update failed".to_string()));
            }
        }
    }

    /// Get tools filtered by the currently expanded agent.
    #[must_use]
    pub(crate) fn tools_for_agent(&self, agent_id: &NousId) -> Vec<&ToolToggle> {
        self.tool_toggles
            .iter()
            .filter(|t| t.agent_id == *agent_id)
            .collect()
    }
}

// -- SSE status parsing -------------------------------------------------------

/// Derive a [`HealthTier`] from an SSE status string.
#[must_use]
pub(crate) fn health_from_status(status: &str) -> HealthTier {
    match status {
        s if s.starts_with("tool-failed:") => HealthTier::Degraded,
        "error" | "failed" => HealthTier::Error,
        _ => HealthTier::Healthy,
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    fn nid(s: &str) -> NousId {
        NousId::from(s)
    }

    fn sample_card(id: &str) -> AgentCardData {
        AgentCardData {
            id: nid(id),
            name: id.to_string(),
            emoji: None,
            health: HealthTier::Healthy,
            model: "test-model".to_string(),
            active_turns: 0,
            last_activity: None,
            connected: true,
        }
    }

    #[test]
    fn agent_status_store_starts_empty() {
        let store = AgentStatusStore::new();
        assert!(store.ordered().is_empty(), "new store must be empty");
    }

    #[test]
    fn agent_status_store_load_preserves_order() {
        let mut store = AgentStatusStore::new();
        store.load(vec![sample_card("b"), sample_card("a"), sample_card("c")]);
        let names: Vec<&str> = store.ordered().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["b", "a", "c"], "order must match insertion");
    }

    #[test]
    fn agent_status_store_set_active_turns() {
        let mut store = AgentStatusStore::new();
        store.load(vec![sample_card("syn")]);
        store.set_active_turns(&nid("syn"), 3);
        assert_eq!(
            store.cards.get(&nid("syn")).map(|c| c.active_turns),
            Some(3),
            "active turns must update"
        );
    }

    #[test]
    fn agent_status_store_unknown_id_is_noop() {
        let mut store = AgentStatusStore::new();
        store.load(vec![sample_card("syn")]);
        store.set_active_turns(&nid("ghost"), 5);
        store.set_health(&nid("ghost"), HealthTier::Error);
        store.set_last_activity(&nid("ghost"), "now".to_string());
        assert_eq!(store.ordered().len(), 1, "unknown id must not create entry");
    }

    #[test]
    fn toggle_store_flip_agent_optimistic() {
        let mut store = ToggleStore::new();
        store.agent_toggles.push(AgentToggle {
            id: nid("syn"),
            name: "syn".to_string(),
            enabled: true,
            pending: false,
            apply_state: ToggleApplyState::Synced,
            live_status: Some("idle".to_string()),
        });

        let prev = store.flip_agent(&nid("syn"));
        assert_eq!(prev, Some(true), "must return previous state");

        let toggle = &store.agent_toggles[0];
        assert!(!toggle.enabled, "must flip enabled state");
        assert!(toggle.pending, "must set pending flag");
    }

    #[test]
    fn toggle_store_resolve_agent_rollback() {
        let mut store = ToggleStore::new();
        store.agent_toggles.push(AgentToggle {
            id: nid("syn"),
            name: "syn".to_string(),
            enabled: false,
            pending: true,
            apply_state: ToggleApplyState::Synced,
            live_status: Some("idle".to_string()),
        });

        store.resolve_agent(&nid("syn"), false, true);

        let toggle = &store.agent_toggles[0];
        assert!(toggle.enabled, "failure must rollback to previous state");
        assert!(!toggle.pending, "must clear pending");
    }

    #[test]
    fn toggle_store_resolve_agent_live_failure_keeps_desired_state() {
        let mut store = ToggleStore::new();
        store.agent_toggles.push(AgentToggle {
            id: nid("syn"),
            name: "syn".to_string(),
            enabled: true,
            pending: true,
            apply_state: ToggleApplyState::Synced,
            live_status: Some("idle".to_string()),
        });

        store.resolve_agent_result(
            &nid("syn"),
            true,
            Some(false),
            Some("unknown".to_string()),
            ToggleActionResult {
                config_applied: true,
                live_applied: false,
                reload_required: false,
                restart_required: true,
            },
        );

        let toggle = &store.agent_toggles[0];
        assert!(!toggle.enabled, "persisted desired state must stay visible");
        assert!(!toggle.pending);
        assert_eq!(toggle.apply_state, ToggleApplyState::RestartRequired);
        assert_eq!(toggle.live_status.as_deref(), Some("unknown"));
    }

    #[test]
    fn toggle_store_resolve_agent_degraded_live_status() {
        let mut store = ToggleStore::new();
        store.agent_toggles.push(AgentToggle {
            id: nid("syn"),
            name: "syn".to_string(),
            enabled: false,
            pending: true,
            apply_state: ToggleApplyState::Synced,
            live_status: None,
        });

        store.resolve_agent_result(
            &nid("syn"),
            false,
            Some(true),
            Some("degraded".to_string()),
            ToggleActionResult {
                config_applied: true,
                live_applied: false,
                reload_required: false,
                restart_required: true,
            },
        );

        let toggle = &store.agent_toggles[0];
        assert!(toggle.enabled);
        assert_eq!(toggle.apply_state, ToggleApplyState::Degraded);
        assert_eq!(toggle.live_status.as_deref(), Some("degraded"));
    }

    #[test]
    fn toggle_store_tools_for_agent() {
        let mut store = ToggleStore::new();
        store.tool_toggles.push(ToolToggle {
            agent_id: nid("syn"),
            tool_name: "read".to_string(),
            enabled: true,
            pending: false,
            apply_state: ToggleApplyState::Synced,
        });
        store.tool_toggles.push(ToolToggle {
            agent_id: nid("mneme"),
            tool_name: "write".to_string(),
            enabled: true,
            pending: false,
            apply_state: ToggleApplyState::Synced,
        });
        store.tool_toggles.push(ToolToggle {
            agent_id: nid("syn"),
            tool_name: "exec".to_string(),
            enabled: false,
            pending: false,
            apply_state: ToggleApplyState::Synced,
        });

        let syn_tools = store.tools_for_agent(&nid("syn"));
        assert_eq!(syn_tools.len(), 2, "must filter by agent id");
    }

    #[test]
    fn health_from_status_mapping() {
        assert_eq!(health_from_status("idle"), HealthTier::Healthy);
        assert_eq!(health_from_status("working"), HealthTier::Healthy);
        assert_eq!(health_from_status("tool-failed:exec"), HealthTier::Degraded);
        assert_eq!(health_from_status("error"), HealthTier::Error);
        assert_eq!(health_from_status("failed"), HealthTier::Error);
    }

    #[test]
    fn health_tier_dot_color() {
        assert_eq!(HealthTier::Healthy.dot_color(), "var(--status-success)");
        assert_eq!(HealthTier::Degraded.dot_color(), "var(--status-warning)");
        assert_eq!(HealthTier::Error.dot_color(), "var(--status-error)");
    }

    #[test]
    fn health_tier_label() {
        assert_eq!(HealthTier::Healthy.label(), "healthy");
        assert_eq!(HealthTier::Degraded.label(), "degraded");
        assert_eq!(HealthTier::Error.label(), "error");
    }

    #[test]
    fn health_tier_default_healthy() {
        assert_eq!(HealthTier::default(), HealthTier::Healthy);
    }

    #[test]
    fn health_status_from_string() {
        assert_eq!(HealthStatus::from_status("healthy"), HealthStatus::Healthy);
        assert_eq!(HealthStatus::from_status("pass"), HealthStatus::Healthy);
        assert_eq!(
            HealthStatus::from_status("degraded"),
            HealthStatus::Degraded
        );
        assert_eq!(HealthStatus::from_status("warn"), HealthStatus::Degraded);
        assert_eq!(
            HealthStatus::from_status("unhealthy"),
            HealthStatus::Unhealthy
        );
        assert_eq!(HealthStatus::from_status("fail"), HealthStatus::Unhealthy);
        assert_eq!(HealthStatus::from_status("unknown"), HealthStatus::Unknown);
    }

    #[test]
    fn health_status_dot_color() {
        assert_eq!(HealthStatus::Healthy.dot_color(), "var(--status-success)");
        assert_eq!(HealthStatus::Degraded.dot_color(), "var(--status-warning)");
        assert_eq!(HealthStatus::Unhealthy.dot_color(), "var(--status-error)");
        assert_eq!(HealthStatus::Unknown.dot_color(), "var(--text-muted)");
    }

    #[test]
    fn health_status_label() {
        assert_eq!(HealthStatus::Healthy.label(), "healthy");
        assert_eq!(HealthStatus::Degraded.label(), "degraded");
        assert_eq!(HealthStatus::Unhealthy.label(), "unhealthy");
        assert_eq!(HealthStatus::Unknown.label(), "unknown");
    }

    #[test]
    fn service_health_store_from_response() {
        let response = skene::api::types::HealthResponse {
            status: "degraded".to_string(),
            version: "0.13.1".to_string(),
            git_sha: "abc123".into(),
            uptime_seconds: 300,
            checks: vec![skene::api::types::HealthCheck {
                name: "providers".to_string(),
                status: "warn".to_string(),
                message: Some("no LLM providers registered".to_string()),
                details: Some(serde_json::json!({"providers": []})),
            }],
            data_dir: "/tmp/data".to_string(),
        };
        let store = ServiceHealthStore::from_response(response);
        assert_eq!(store.status, HealthStatus::Degraded);
        assert_eq!(store.checks.len(), 1);
        assert_eq!(store.checks[0].name, "providers");
        assert_eq!(
            store.checks[0]
                .details
                .as_ref()
                .and_then(|details| details.get("providers"))
                .and_then(serde_json::Value::as_array)
                .map(Vec::len),
            Some(0)
        );
        assert!(store.error.is_none());
    }

    #[test]
    fn service_health_store_unreachable_keeps_error() {
        let store = ServiceHealthStore::unreachable("connection refused".to_string());
        assert_eq!(store.status, HealthStatus::Unknown);
        assert!(store.checks.is_empty());
        assert_eq!(store.error.as_deref(), Some("connection refused"));
    }

    #[test]
    fn agent_status_store_set_health() {
        let mut store = AgentStatusStore::new();
        store.load(vec![sample_card("syn")]);
        store.set_health(&nid("syn"), HealthTier::Error);
        assert_eq!(
            store.cards.get(&nid("syn")).map(|c| c.health),
            Some(HealthTier::Error)
        );
    }

    #[test]
    fn agent_status_store_set_last_activity() {
        let mut store = AgentStatusStore::new();
        store.load(vec![sample_card("syn")]);
        store.set_last_activity(&nid("syn"), "2024-01-01T00:00:00Z".to_string());
        assert_eq!(
            store
                .cards
                .get(&nid("syn"))
                .and_then(|c| c.last_activity.as_deref()),
            Some("2024-01-01T00:00:00Z"),
        );
    }

    #[test]
    fn agent_status_store_load_replaces_existing() {
        let mut store = AgentStatusStore::new();
        store.load(vec![sample_card("a"), sample_card("b")]);
        store.load(vec![sample_card("c")]);
        assert_eq!(store.ordered().len(), 1);
        assert_eq!(store.ordered()[0].name, "c");
    }

    #[test]
    fn service_health_store_default_empty() {
        let s = ServiceHealthStore::new();
        assert_eq!(s.status, HealthStatus::Unknown);
        assert!(s.checks.is_empty());
        assert!(s.error.is_none());
    }

    #[test]
    fn toggle_store_flip_unknown_agent_returns_none() {
        let mut store = ToggleStore::new();
        assert_eq!(store.flip_agent(&nid("ghost")), None);
    }

    #[test]
    fn toggle_store_flip_tool_optimistic() {
        let mut store = ToggleStore::new();
        store.tool_toggles.push(ToolToggle {
            agent_id: nid("syn"),
            tool_name: "read".to_string(),
            enabled: false,
            pending: false,
            apply_state: ToggleApplyState::Synced,
        });
        let prev = store.flip_tool(&nid("syn"), "read");
        assert_eq!(prev, Some(false));
        let t = &store.tool_toggles[0];
        assert!(t.enabled);
        assert!(t.pending);
    }

    #[test]
    fn toggle_store_flip_tool_unknown_returns_none() {
        let mut store = ToggleStore::new();
        assert_eq!(store.flip_tool(&nid("ghost"), "read"), None);
    }

    #[test]
    fn toggle_store_resolve_tool_success_keeps_state() {
        let mut store = ToggleStore::new();
        store.tool_toggles.push(ToolToggle {
            agent_id: nid("syn"),
            tool_name: "read".to_string(),
            enabled: true,
            pending: true,
            apply_state: ToggleApplyState::Synced,
        });
        store.resolve_tool(&nid("syn"), "read", true, false);
        let t = &store.tool_toggles[0];
        assert!(t.enabled, "success keeps optimistic state");
        assert!(!t.pending, "pending must be cleared");
    }

    #[test]
    fn toggle_store_resolve_tool_failure_rolls_back() {
        let mut store = ToggleStore::new();
        store.tool_toggles.push(ToolToggle {
            agent_id: nid("syn"),
            tool_name: "read".to_string(),
            enabled: true,
            pending: true,
            apply_state: ToggleApplyState::Synced,
        });
        store.resolve_tool(&nid("syn"), "read", false, false);
        let t = &store.tool_toggles[0];
        assert!(!t.enabled, "failure restores prev state");
        assert!(!t.pending);
    }

    #[test]
    fn toggle_store_resolve_tool_reload_required_keeps_desired_state() {
        let mut store = ToggleStore::new();
        store.tool_toggles.push(ToolToggle {
            agent_id: nid("syn"),
            tool_name: "read".to_string(),
            enabled: true,
            pending: true,
            apply_state: ToggleApplyState::Synced,
        });

        store.resolve_tool_result(
            &nid("syn"),
            "read",
            true,
            Some(false),
            ToggleActionResult {
                config_applied: true,
                live_applied: false,
                reload_required: true,
                restart_required: false,
            },
        );

        let toggle = &store.tool_toggles[0];
        assert!(
            !toggle.enabled,
            "persisted allowlist state must stay visible"
        );
        assert!(!toggle.pending);
        assert_eq!(toggle.apply_state, ToggleApplyState::ReloadRequired);
    }

    #[test]
    fn toggle_store_flip_feature_optimistic() {
        let mut store = ToggleStore::new();
        store.feature_flags.push(FeatureFlag {
            key: "experimental".to_string(),
            description: "Beta features".to_string(),
            enabled: false,
            pending: false,
            error: None,
        });
        let prev = store.flip_feature("experimental");
        assert_eq!(prev, Some(false));
        let f = &store.feature_flags[0];
        assert!(f.enabled);
        assert!(f.pending);
    }

    #[test]
    fn toggle_store_flip_unknown_feature_returns_none() {
        let mut store = ToggleStore::new();
        assert_eq!(store.flip_feature("nope"), None);
    }

    #[test]
    fn toggle_store_resolve_feature_failure_rolls_back() {
        let mut store = ToggleStore::new();
        store.feature_flags.push(FeatureFlag {
            key: "k".to_string(),
            description: String::new(),
            enabled: true,
            pending: true,
            error: None,
        });
        store.resolve_feature(
            "k",
            false,
            false,
            Some("server error".to_string()),
            Vec::new(),
        );
        assert!(
            store.feature_flags[0].enabled,
            "failure must keep optimistic state"
        );
        assert!(!store.feature_flags[0].pending);
        assert_eq!(
            store.feature_flags[0].error,
            Some("server error".to_string()),
            "failure must surface error"
        );
    }

    #[test]
    fn toggle_store_resolve_feature_success_keeps_state() {
        let mut store = ToggleStore::new();
        store.feature_flags.push(FeatureFlag {
            key: "k".to_string(),
            description: String::new(),
            enabled: true,
            pending: true,
            error: Some("old error".to_string()),
        });
        store.resolve_feature("k", true, false, None, vec!["feature_flags.k".to_string()]);
        assert!(store.feature_flags[0].enabled);
        assert!(!store.feature_flags[0].pending);
        assert!(
            store.feature_flags[0].error.is_none(),
            "success must clear error"
        );
        assert_eq!(store.restart_required, vec!["feature_flags.k".to_string()]);
    }

    #[test]
    fn toggle_store_resolve_unknown_id_no_panic() {
        let mut store = ToggleStore::new();
        // Should not panic when id is missing.
        store.resolve_agent(&nid("ghost"), true, false);
        store.resolve_tool(&nid("ghost"), "missing", true, false);
        store.resolve_feature("missing", true, false, None, Vec::new());
        assert!(store.agent_toggles.is_empty());
        assert!(store.tool_toggles.is_empty());
        assert!(store.feature_flags.is_empty());
    }

    #[test]
    fn toggle_store_tools_for_agent_empty_when_none_match() {
        let store = ToggleStore::new();
        assert!(store.tools_for_agent(&nid("syn")).is_empty());
    }

    #[test]
    fn feature_flags_payload_preserves_all_entries() {
        let mut store = ToggleStore::new();
        store.feature_flags.push(FeatureFlag {
            key: "alpha".to_string(),
            description: "Alpha flag".to_string(),
            enabled: true,
            pending: false,
            error: None,
        });
        store.feature_flags.push(FeatureFlag {
            key: "beta".to_string(),
            description: "Beta flag".to_string(),
            enabled: false,
            pending: false,
            error: None,
        });

        let payload = store.feature_flags_payload();
        assert_eq!(payload.len(), 2);
        assert_eq!(payload[0].key, "alpha");
        assert!(payload[0].enabled);
        assert_eq!(payload[1].key, "beta");
        assert!(!payload[1].enabled);
    }

    #[test]
    fn feature_flags_payload_serializes_camel_case() {
        let mut store = ToggleStore::new();
        store.feature_flags.push(FeatureFlag {
            key: "alpha".to_string(),
            description: "Alpha flag".to_string(),
            enabled: true,
            pending: false,
            error: None,
        });

        let json = serde_json::to_value(store.feature_flags_payload()).unwrap();
        let entry = json.as_array().unwrap()[0].as_object().unwrap();
        assert!(entry.contains_key("key"));
        assert!(entry.contains_key("description"));
        assert!(entry.contains_key("enabled"));
        assert!(!entry.contains_key("error"));
        assert!(!entry.contains_key("pending"));
    }

    #[test]
    fn toggle_store_resolve_feature_failure_without_error_gets_default_message() {
        let mut store = ToggleStore::new();
        store.feature_flags.push(FeatureFlag {
            key: "k".to_string(),
            description: String::new(),
            enabled: true,
            pending: true,
            error: None,
        });
        store.flip_feature("k");
        store.resolve_feature("k", false, true, None, Vec::new());
        assert_eq!(
            store.feature_flags[0].error,
            Some("Update failed".to_string())
        );
    }
}
