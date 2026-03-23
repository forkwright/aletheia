//! Ops dashboard state: agent status cards, service health, and toggle controls.

use std::collections::HashMap;

use theatron_core::id::NousId;

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
    /// CSS hex color for the status dot.
    #[must_use]
    pub(crate) fn dot_color(&self) -> &'static str {
        match self {
            Self::Healthy => "#22c55e",
            Self::Degraded => "#eab308",
            Self::Error => "#ef4444",
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
    #[expect(dead_code, reason = "wired when SSE activity events are plumbed")]
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

/// Result of the last run of a cron job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum JobResult {
    #[default]
    Unknown,
    Success,
    Failure,
}

impl JobResult {
    #[must_use]
    pub(crate) fn dot_color(&self) -> &'static str {
        match self {
            Self::Unknown => "#555",
            Self::Success => "#22c55e",
            Self::Failure => "#ef4444",
        }
    }
}

/// Status of a daemon task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum TaskStatus {
    #[default]
    Running,
    Stopped,
    Failed,
}

impl TaskStatus {
    #[must_use]
    pub(crate) fn dot_color(&self) -> &'static str {
        match self {
            Self::Running => "#22c55e",
            Self::Stopped => "#555",
            Self::Failed => "#ef4444",
        }
    }

    #[must_use]
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
        }
    }
}

/// A single cron job entry in the service health panel.
#[derive(Debug, Clone)]
pub(crate) struct CronJobInfo {
    pub name: String,
    pub schedule: String,
    pub last_run: Option<String>,
    #[expect(dead_code, reason = "deserialized from API but not yet rendered")]
    pub next_run: Option<String>,
    pub last_result: JobResult,
}

/// A single daemon task entry.
#[derive(Debug, Clone)]
pub(crate) struct DaemonTaskInfo {
    pub name: String,
    pub status: TaskStatus,
    pub uptime: Option<String>,
    pub restart_count: u32,
}

/// Trend direction for failure counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Trend {
    Up,
    Down,
    #[default]
    Stable,
}

impl Trend {
    #[must_use]
    pub(crate) fn indicator(&self) -> &'static str {
        match self {
            Self::Up => "\u{25b2}",     // ▲
            Self::Down => "\u{25bc}",   // ▼
            Self::Stable => "\u{2014}", // —
        }
    }

    #[must_use]
    pub(crate) fn color(&self) -> &'static str {
        match self {
            Self::Up => "#ef4444",
            Self::Down => "#22c55e",
            Self::Stable => "#888",
        }
    }
}

/// Aggregate service health data.
#[derive(Debug, Clone, Default)]
pub(crate) struct ServiceHealthStore {
    pub cron_jobs: Vec<CronJobInfo>,
    pub daemon_tasks: Vec<DaemonTaskInfo>,
    pub failure_count: u32,
    pub failure_trend: Trend,
}

impl ServiceHealthStore {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
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
}

/// A tool toggle for a specific agent.
#[derive(Debug, Clone)]
pub(crate) struct ToolToggle {
    pub agent_id: NousId,
    pub tool_name: String,
    pub enabled: bool,
    pub pending: bool,
}

/// A system-wide feature flag.
#[derive(Debug, Clone)]
pub(crate) struct FeatureFlag {
    pub key: String,
    pub description: String,
    pub enabled: bool,
    pub pending: bool,
}

/// Aggregate toggle state with optimistic update support.
#[derive(Debug, Clone, Default)]
pub(crate) struct ToggleStore {
    pub agent_toggles: Vec<AgentToggle>,
    pub tool_toggles: Vec<ToolToggle>,
    pub feature_flags: Vec<FeatureFlag>,
    pub expanded_agent: Option<NousId>,
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
                prev
            })
    }

    /// Resolve an in-flight agent toggle (clear pending state).
    pub(crate) fn resolve_agent(&mut self, id: &NousId, success: bool, prev: bool) {
        if let Some(t) = self.agent_toggles.iter_mut().find(|t| t.id == *id) {
            t.pending = false;
            if !success {
                t.enabled = prev;
            }
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
        if let Some(t) = self
            .tool_toggles
            .iter_mut()
            .find(|t| t.agent_id == *agent_id && t.tool_name == tool_name)
        {
            t.pending = false;
            if !success {
                t.enabled = prev;
            }
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

    /// Resolve an in-flight feature flag toggle.
    pub(crate) fn resolve_feature(&mut self, key: &str, success: bool, prev: bool) {
        if let Some(f) = self.feature_flags.iter_mut().find(|f| f.key == key) {
            f.pending = false;
            if !success {
                f.enabled = prev;
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
        });

        store.resolve_agent(&nid("syn"), false, true);

        let toggle = &store.agent_toggles[0];
        assert!(toggle.enabled, "failure must rollback to previous state");
        assert!(!toggle.pending, "must clear pending");
    }

    #[test]
    fn toggle_store_tools_for_agent() {
        let mut store = ToggleStore::new();
        store.tool_toggles.push(ToolToggle {
            agent_id: nid("syn"),
            tool_name: "read".to_string(),
            enabled: true,
            pending: false,
        });
        store.tool_toggles.push(ToolToggle {
            agent_id: nid("mneme"),
            tool_name: "write".to_string(),
            enabled: true,
            pending: false,
        });
        store.tool_toggles.push(ToolToggle {
            agent_id: nid("syn"),
            tool_name: "exec".to_string(),
            enabled: false,
            pending: false,
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
    fn trend_indicators() {
        assert_eq!(Trend::Up.indicator(), "\u{25b2}");
        assert_eq!(Trend::Down.indicator(), "\u{25bc}");
        assert_eq!(Trend::Stable.indicator(), "\u{2014}");
    }
}
