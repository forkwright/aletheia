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

/// Effective capability limits for one agent, as reported by
/// `GET /api/v1/nous/{id}`.
///
/// WHY: the list endpoint (`GET /api/v1/nous`) returns `NousSummary`, which
/// carries no capability fields. These come from the per-agent detail
/// endpoint, so they are absent whenever that fetch has not completed or
/// failed for this agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentCapabilities {
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub thinking_enabled: bool,
    pub thinking_budget: u32,
    pub max_tool_iterations: u32,
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
    /// `None` when the per-agent detail fetch has not completed or failed;
    /// the card renders without the capability block in that case.
    pub capabilities: Option<AgentCapabilities>,
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
    /// Crate version reported by the backend. `None` when unreachable.
    pub version: Option<String>,
    /// Build git SHA reported by the backend. `None` when unreachable.
    pub git_sha: Option<String>,
    /// Seconds since backend process start. `None` when unreachable.
    pub uptime_seconds: Option<u64>,
}

impl ServiceHealthStore {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Build store from a parsed health response.
    ///
    /// WHY(#5177): `data_dir` on the response is a local filesystem path;
    /// the issue asks it stay out of the desktop UI unless the server
    /// gates/redacts it, so unlike the other fields it is not carried
    /// through here.
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
                })
                .collect(),
            error: None,
            version: Some(response.version),
            git_sha: Some(response.git_sha.to_string()),
            uptime_seconds: Some(response.uptime_seconds),
        }
    }

    /// Build store for an unreachable or unparseable health response.
    pub(crate) fn unreachable(message: String) -> Self {
        Self {
            status: HealthStatus::Unknown,
            checks: Vec::new(),
            error: Some(message),
            version: None,
            git_sha: None,
            uptime_seconds: None,
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
    /// Human-readable error from the last failed update. Kept visible until
    /// a later update succeeds, mirroring `FeatureFlag::error`.
    pub error: Option<String>,
}

/// A tool toggle for a specific agent.
#[derive(Debug, Clone)]
pub(crate) struct ToolToggle {
    pub agent_id: NousId,
    pub tool_name: String,
    pub enabled: bool,
    pub pending: bool,
    pub apply_state: ToggleApplyState,
    /// Human-readable error from the last failed update. Kept visible until
    /// a later update succeeds, mirroring `FeatureFlag::error`.
    pub error: Option<String>,
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

/// Outcome of the last manual config reload (`POST /api/v1/config/reload`).
#[derive(Debug, Clone)]
pub(crate) enum ReloadOutcome {
    /// Reload applied; counts summarize the server-reported diff.
    Applied { hot_reloaded: usize, changed: usize },
    /// Reload failed; carries a human-readable message.
    Failed(String),
}

/// Outcome of the last manual agent recovery
/// (`POST /api/v1/nous/{id}/recover`).
#[derive(Debug, Clone)]
pub(crate) enum RecoverOutcome {
    /// Server accepted the request; `recovered` is its own report of whether
    /// the actor actually left the degraded state.
    Applied { recovered: bool },
    /// Recovery failed; carries a human-readable message.
    Failed(String),
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

/// A single flag as returned in the canonical `config` section of
/// `ConfigUpdateResponse` (pylon `crates/pylon/src/handlers/config_dto.rs`).
///
/// WHY(#4986): read back after every write so normalization, defaults, or a
/// server-rejected value are reflected instead of trusting the client's
/// optimistic guess.
#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct FeatureFlagConfigEntry {
    pub key: String,
    pub description: String,
    #[serde(default)]
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
    /// Whether a manual config reload is currently in flight.
    pub reload_pending: bool,
    /// Outcome of the last manual config reload, kept visible until the next
    /// reload attempt or a full panel refresh.
    pub reload_outcome: Option<ReloadOutcome>,
    /// Agent whose manual recovery is currently in flight.
    ///
    /// WHY store-level rather than a field on `AgentToggle`: the toggle list
    /// is rebuilt wholesale from the server on every panel refresh, which
    /// would discard per-toggle in-flight state. Recovery is a deliberate
    /// operator action on a degraded actor, so one in flight at a time is
    /// the honest model.
    pub recover_pending: Option<NousId>,
    /// Outcome of the last manual recovery, paired with the agent it
    /// targeted. Kept visible until the next recovery attempt.
    pub recover_outcome: Option<(NousId, RecoverOutcome)>,
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
    pub(crate) fn resolve_agent(
        &mut self,
        id: &NousId,
        success: bool,
        prev: bool,
        error: Option<String>,
    ) {
        let result = if success {
            ToggleActionResult::synced()
        } else {
            ToggleActionResult::failed()
        };
        self.resolve_agent_result(id, prev, None, None, result, error);
    }

    /// Resolve an in-flight agent toggle using server-reported runtime effects.
    ///
    /// On failure the optimistic flip is rolled back to `prev` (unlike feature
    /// flags), but `error` is surfaced the same way so the operator sees why
    /// the write did not land instead of an unexplained switch bounce.
    pub(crate) fn resolve_agent_result(
        &mut self,
        id: &NousId,
        prev: bool,
        applied_enabled: Option<bool>,
        live_status: Option<String>,
        result: ToggleActionResult,
        error: Option<String>,
    ) {
        if let Some(t) = self.agent_toggles.iter_mut().find(|t| t.id == *id) {
            t.pending = false;
            if result.config_applied {
                if let Some(enabled) = applied_enabled {
                    t.enabled = enabled;
                }
                t.live_status = live_status;
                t.error = None;
            } else {
                t.enabled = prev;
                t.live_status = None;
                t.error = Some(error.unwrap_or_else(|| "Update failed".to_string()));
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
        error: Option<String>,
    ) {
        let result = if success {
            ToggleActionResult::synced()
        } else {
            ToggleActionResult::failed()
        };
        self.resolve_tool_result(agent_id, tool_name, prev, None, result, error);
    }

    /// Resolve an in-flight tool toggle using server-reported runtime effects.
    ///
    /// On failure the optimistic flip is rolled back to `prev` (unlike feature
    /// flags), but `error` is surfaced the same way so the operator sees why
    /// the write did not land instead of an unexplained switch bounce.
    pub(crate) fn resolve_tool_result(
        &mut self,
        agent_id: &NousId,
        tool_name: &str,
        prev: bool,
        applied_enabled: Option<bool>,
        result: ToggleActionResult,
        error: Option<String>,
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
                t.error = None;
            } else {
                t.enabled = prev;
                t.error = Some(error.unwrap_or_else(|| "Update failed".to_string()));
            }
            t.apply_state = ToggleApplyState::from_action(result);
        }
    }

    /// Optimistically flip a feature flag.
    ///
    /// Returns `None` (and flips nothing) both when `key` is unknown and
    /// when another feature-flag write is already in flight -- the backend
    /// write is whole-section (see [`Self::feature_flags_payload`]), so two
    /// overlapping writes can complete out of order and the later-sent one
    /// can stomp the earlier-sent one's just-persisted value (#4986).
    pub(crate) fn flip_feature(&mut self, key: &str) -> Option<bool> {
        if self.any_feature_flag_pending() {
            return None;
        }
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

    /// Whether any feature flag currently has a write in flight.
    ///
    /// Used both to gate a new write from starting (see [`Self::flip_feature`])
    /// and by the panel to disable every flag's control while one write is
    /// outstanding, since the backend section write is not scoped to a
    /// single key.
    #[must_use]
    pub(crate) fn any_feature_flag_pending(&self) -> bool {
        self.feature_flags.iter().any(|f| f.pending)
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
    /// On success with a canonical section (`Some`), local state is replaced
    /// wholesale from the server's response instead of trusting the
    /// optimistic guess -- this is what lets server-side normalization, a
    /// default, or a write that landed differently than requested reach the
    /// UI (#4986). On success without a canonical section (a 2xx the client
    /// could not otherwise interpret), only the resolved flag's pending/error
    /// state is cleared. On failure the optimistic flip is reverted to `prev`
    /// so a rejected write cannot render as though it persisted.
    pub(crate) fn resolve_feature(
        &mut self,
        key: &str,
        success: bool,
        prev: bool,
        error: Option<String>,
        restart_required: Vec<String>,
        canonical: Option<Vec<FeatureFlagConfigEntry>>,
    ) {
        self.restart_required = restart_required;

        if success {
            if let Some(entries) = canonical {
                self.feature_flags = entries
                    .into_iter()
                    .map(|e| FeatureFlag {
                        key: e.key,
                        description: e.description,
                        enabled: e.enabled,
                        pending: false,
                        error: None,
                    })
                    .collect();
                return;
            }
            if let Some(f) = self.feature_flags.iter_mut().find(|f| f.key == key) {
                f.pending = false;
                f.error = None;
            }
            return;
        }

        if let Some(f) = self.feature_flags.iter_mut().find(|f| f.key == key) {
            f.pending = false;
            f.enabled = prev;
            f.error = error.or(Some("Update failed".to_string()));
        }
    }

    /// Mark a manual config reload as in flight.
    pub(crate) fn begin_reload(&mut self) {
        self.reload_pending = true;
    }

    /// Resolve a successful config reload.
    ///
    /// WHY: `restart_required` is replaced wholesale rather than merged --
    /// reload re-reads the entire config file from disk, so its diff
    /// supersedes any partial list left over from an earlier section update.
    pub(crate) fn resolve_reload_success(
        &mut self,
        hot_reloaded: usize,
        changed: usize,
        restart_required: Vec<String>,
    ) {
        self.reload_pending = false;
        self.restart_required = restart_required;
        self.reload_outcome = Some(ReloadOutcome::Applied {
            hot_reloaded,
            changed,
        });
    }

    /// Resolve a failed config reload.
    pub(crate) fn resolve_reload_failure(&mut self, message: String) {
        self.reload_pending = false;
        self.reload_outcome = Some(ReloadOutcome::Failed(message));
    }

    /// Mark a manual recovery of one agent as in flight.
    ///
    /// WHY the stale outcome is cleared: it belongs to the previous attempt,
    /// and leaving it rendered beside a spinner reads as the result of the
    /// attempt now running.
    pub(crate) fn begin_recover(&mut self, id: &NousId) {
        self.recover_pending = Some(id.clone());
        self.recover_outcome = None;
    }

    /// Resolve a successful manual recovery.
    pub(crate) fn resolve_recover_success(&mut self, id: &NousId, recovered: bool) {
        self.recover_pending = None;
        self.recover_outcome = Some((id.clone(), RecoverOutcome::Applied { recovered }));
    }

    /// Resolve a failed manual recovery.
    pub(crate) fn resolve_recover_failure(&mut self, id: &NousId, message: String) {
        self.recover_pending = None;
        self.recover_outcome = Some((id.clone(), RecoverOutcome::Failed(message)));
    }

    /// Whether a manual recovery is in flight for one agent.
    #[must_use]
    pub(crate) fn is_recovering(&self, id: &NousId) -> bool {
        self.recover_pending.as_ref() == Some(id)
    }

    /// The last recovery outcome, if it targeted this agent.
    #[must_use]
    pub(crate) fn recover_outcome_for(&self, id: &NousId) -> Option<&RecoverOutcome> {
        self.recover_outcome
            .as_ref()
            .filter(|(target, _)| target == id)
            .map(|(_, outcome)| outcome)
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
            capabilities: None,
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
            error: None,
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
            error: None,
        });

        store.resolve_agent(
            &nid("syn"),
            false,
            true,
            Some("server returned 500".to_string()),
        );

        let toggle = &store.agent_toggles[0];
        assert!(toggle.enabled, "failure must rollback to previous state");
        assert!(!toggle.pending, "must clear pending");
        assert_eq!(
            toggle.error.as_deref(),
            Some("server returned 500"),
            "failure must surface error"
        );
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
            error: None,
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
            None,
        );

        let toggle = &store.agent_toggles[0];
        assert!(!toggle.enabled, "persisted desired state must stay visible");
        assert!(!toggle.pending);
        assert_eq!(toggle.apply_state, ToggleApplyState::RestartRequired);
        assert_eq!(toggle.live_status.as_deref(), Some("unknown"));
        assert!(
            toggle.error.is_none(),
            "config_applied success must clear error"
        );
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
            error: None,
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
            None,
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
            error: None,
        });
        store.tool_toggles.push(ToolToggle {
            agent_id: nid("mneme"),
            tool_name: "write".to_string(),
            enabled: true,
            pending: false,
            apply_state: ToggleApplyState::Synced,
            error: None,
        });
        store.tool_toggles.push(ToolToggle {
            agent_id: nid("syn"),
            tool_name: "exec".to_string(),
            enabled: false,
            pending: false,
            apply_state: ToggleApplyState::Synced,
            error: None,
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
            }],
            data_dir: "/tmp/data".to_string(),
        };
        let store = ServiceHealthStore::from_response(response);
        assert_eq!(store.status, HealthStatus::Degraded);
        assert_eq!(store.checks.len(), 1);
        assert_eq!(store.checks[0].name, "providers");
        assert!(store.error.is_none());
        assert_eq!(store.version.as_deref(), Some("0.13.1"));
        assert_eq!(store.git_sha.as_deref(), Some("abc123"));
        assert_eq!(store.uptime_seconds, Some(300));
    }

    #[test]
    fn service_health_store_unreachable_keeps_error() {
        let store = ServiceHealthStore::unreachable("connection refused".to_string());
        assert_eq!(store.status, HealthStatus::Unknown);
        assert!(store.checks.is_empty());
        assert_eq!(store.error.as_deref(), Some("connection refused"));
        assert!(store.version.is_none());
        assert!(store.git_sha.is_none());
        assert!(store.uptime_seconds.is_none());
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
        assert!(s.version.is_none());
        assert!(s.git_sha.is_none());
        assert!(s.uptime_seconds.is_none());
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
            error: None,
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
            error: None,
        });
        store.resolve_tool(&nid("syn"), "read", true, false, None);
        let t = &store.tool_toggles[0];
        assert!(t.enabled, "success keeps optimistic state");
        assert!(!t.pending, "pending must be cleared");
        assert!(t.error.is_none(), "success must clear error");
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
            error: None,
        });
        store.resolve_tool(
            &nid("syn"),
            "read",
            false,
            false,
            Some("connection error: timed out".to_string()),
        );
        let t = &store.tool_toggles[0];
        assert!(!t.enabled, "failure restores prev state");
        assert!(!t.pending);
        assert_eq!(
            t.error.as_deref(),
            Some("connection error: timed out"),
            "failure must surface error"
        );
    }

    #[test]
    fn toggle_store_resolve_tool_failure_without_error_gets_default_message() {
        let mut store = ToggleStore::new();
        store.tool_toggles.push(ToolToggle {
            agent_id: nid("syn"),
            tool_name: "read".to_string(),
            enabled: true,
            pending: true,
            apply_state: ToggleApplyState::Synced,
            error: None,
        });
        store.resolve_tool(&nid("syn"), "read", false, false, None);
        assert_eq!(
            store.tool_toggles[0].error.as_deref(),
            Some("Update failed")
        );
    }

    #[test]
    fn toggle_store_resolve_agent_failure_without_error_gets_default_message() {
        let mut store = ToggleStore::new();
        store.agent_toggles.push(AgentToggle {
            id: nid("syn"),
            name: "syn".to_string(),
            enabled: true,
            pending: true,
            apply_state: ToggleApplyState::Synced,
            live_status: Some("idle".to_string()),
            error: None,
        });
        store.resolve_agent(&nid("syn"), false, true, None);
        assert_eq!(
            store.agent_toggles[0].error.as_deref(),
            Some("Update failed")
        );
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
            error: None,
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
            None,
        );

        let toggle = &store.tool_toggles[0];
        assert!(
            !toggle.enabled,
            "persisted allowlist state must stay visible"
        );
        assert!(!toggle.pending);
        assert_eq!(toggle.apply_state, ToggleApplyState::ReloadRequired);
        assert!(toggle.error.is_none(), "success must clear error");
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
    fn toggle_store_resolve_feature_failure_reverts_to_persisted_value() {
        // WHY(#4986): a rejected write must not render as though it
        // persisted -- revert to `prev` rather than keeping the optimistic
        // guess.
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
            None,
        );
        assert!(
            !store.feature_flags[0].enabled,
            "failure must revert to the pre-optimistic value"
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
        store.resolve_feature(
            "k",
            true,
            false,
            None,
            vec!["feature_flags.k".to_string()],
            None,
        );
        assert!(store.feature_flags[0].enabled);
        assert!(!store.feature_flags[0].pending);
        assert!(
            store.feature_flags[0].error.is_none(),
            "success must clear error"
        );
        assert_eq!(store.restart_required, vec!["feature_flags.k".to_string()]);
    }

    #[test]
    fn toggle_store_resolve_feature_success_applies_canonical_config() {
        // WHY(#4986): the regression this issue is actually about -- a
        // canonical response that disagrees with the optimistic guess (here,
        // the server normalized the description and left the flag off
        // despite the optimistic flip to on) must win.
        let mut store = ToggleStore::new();
        store.feature_flags.push(FeatureFlag {
            key: "k".to_string(),
            description: "stale local copy".to_string(),
            enabled: true,
            pending: true,
            error: None,
        });
        store.resolve_feature(
            "k",
            true,
            false,
            None,
            Vec::new(),
            Some(vec![FeatureFlagConfigEntry {
                key: "k".to_string(),
                description: "canonical from server".to_string(),
                enabled: false,
            }]),
        );
        assert_eq!(store.feature_flags.len(), 1);
        assert!(
            !store.feature_flags[0].enabled,
            "canonical server value must win over the optimistic guess"
        );
        assert_eq!(store.feature_flags[0].description, "canonical from server");
        assert!(!store.feature_flags[0].pending);
    }

    #[test]
    fn toggle_store_resolve_feature_success_canonical_replaces_whole_list() {
        // A canonical response describes the whole section: a flag the
        // client held locally but the server no longer reports must not
        // survive the replace.
        let mut store = ToggleStore::new();
        store.feature_flags.push(FeatureFlag {
            key: "kept".to_string(),
            description: String::new(),
            enabled: false,
            pending: true,
            error: None,
        });
        store.feature_flags.push(FeatureFlag {
            key: "removed-server-side".to_string(),
            description: String::new(),
            enabled: true,
            pending: false,
            error: None,
        });
        store.resolve_feature(
            "kept",
            true,
            false,
            None,
            Vec::new(),
            Some(vec![FeatureFlagConfigEntry {
                key: "kept".to_string(),
                description: String::new(),
                enabled: true,
            }]),
        );
        assert_eq!(store.feature_flags.len(), 1);
        assert_eq!(store.feature_flags[0].key, "kept");
    }

    #[test]
    fn toggle_store_any_feature_flag_pending_reflects_any_row() {
        let mut store = ToggleStore::new();
        assert!(!store.any_feature_flag_pending());
        store.feature_flags.push(FeatureFlag {
            key: "a".to_string(),
            description: String::new(),
            enabled: false,
            pending: false,
            error: None,
        });
        store.feature_flags.push(FeatureFlag {
            key: "b".to_string(),
            description: String::new(),
            enabled: false,
            pending: true,
            error: None,
        });
        assert!(store.any_feature_flag_pending());
    }

    #[test]
    fn toggle_store_flip_feature_refuses_while_another_write_is_pending() {
        // WHY(#4986): the write is whole-section, so overlapping writes can
        // race and stomp each other's persisted value.
        let mut store = ToggleStore::new();
        store.feature_flags.push(FeatureFlag {
            key: "a".to_string(),
            description: String::new(),
            enabled: false,
            pending: true,
            error: None,
        });
        store.feature_flags.push(FeatureFlag {
            key: "b".to_string(),
            description: String::new(),
            enabled: false,
            pending: false,
            error: None,
        });
        assert_eq!(
            store.flip_feature("b"),
            None,
            "must refuse a second flip while `a` is still in flight"
        );
        assert!(!store.feature_flags[1].enabled, "b must not have flipped");
    }

    #[test]
    fn toggle_store_resolve_unknown_id_no_panic() {
        let mut store = ToggleStore::new();
        // Should not panic when id is missing.
        store.resolve_agent(&nid("ghost"), true, false, None);
        store.resolve_tool(&nid("ghost"), "missing", true, false, None);
        store.resolve_feature("missing", true, false, None, Vec::new(), None);
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
    fn toggle_store_begin_reload_sets_pending() {
        let mut store = ToggleStore::new();
        assert!(!store.reload_pending);
        store.begin_reload();
        assert!(store.reload_pending, "must set pending flag");
        assert!(store.reload_outcome.is_none());
    }

    #[test]
    fn toggle_store_resolve_reload_success_clears_pending_and_sets_outcome() {
        let mut store = ToggleStore::new();
        store.begin_reload();
        store.resolve_reload_success(2, 5, vec!["gateway.port".to_string()]);

        assert!(!store.reload_pending, "must clear pending");
        assert_eq!(
            store.restart_required,
            vec!["gateway.port".to_string()],
            "must replace restart_required with the reload's diff"
        );
        match store.reload_outcome {
            Some(ReloadOutcome::Applied {
                hot_reloaded,
                changed,
            }) => {
                assert_eq!(hot_reloaded, 2);
                assert_eq!(changed, 5);
            }
            other => panic!("expected Applied outcome, got {other:?}"),
        }
    }

    #[test]
    fn toggle_store_resolve_reload_success_replaces_stale_restart_required() {
        let mut store = ToggleStore::new();
        store.restart_required = vec!["stale.path".to_string()];
        store.begin_reload();
        store.resolve_reload_success(0, 0, Vec::new());

        assert!(
            store.restart_required.is_empty(),
            "a clean reload must clear a stale restart_required list"
        );
    }

    #[test]
    fn toggle_store_resolve_reload_failure_clears_pending_and_sets_message() {
        let mut store = ToggleStore::new();
        store.begin_reload();
        store.resolve_reload_failure("connection error: timed out".to_string());

        assert!(!store.reload_pending, "must clear pending");
        match store.reload_outcome {
            Some(ReloadOutcome::Failed(message)) => {
                assert_eq!(message, "connection error: timed out");
            }
            other => panic!("expected Failed outcome, got {other:?}"),
        }
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
        store.resolve_feature("k", false, true, None, Vec::new(), None);
        assert_eq!(
            store.feature_flags[0].error,
            Some("Update failed".to_string())
        );
    }
}
