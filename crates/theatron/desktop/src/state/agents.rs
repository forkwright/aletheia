//! Agent registry state — status tracking and active-agent selection.

use std::collections::HashMap;

use theatron_core::api::types::Agent;
use theatron_core::id::NousId;

/// Runtime status of an agent, derived from SSE events.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum AgentStatus {
    /// Agent is available and not processing a turn.
    #[default]
    Idle,
    /// Agent is currently processing a turn.
    Active,
    /// Agent is in an error state.
    Error,
}

impl AgentStatus {
    /// CSS hex color for the status indicator dot.
    #[must_use]
    pub(crate) fn dot_color(&self) -> &'static str {
        match self {
            Self::Active => "#22c55e",
            Self::Idle => "#555",
            Self::Error => "#ef4444",
        }
    }

    /// Human-readable status label.
    #[must_use]
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Idle => "idle",
            Self::Error => "error",
        }
    }
}

/// An agent entry in the sidebar with runtime status.
#[derive(Debug, Clone)]
pub struct AgentRecord {
    /// Core agent data from the API.
    pub agent: Agent,
    /// Current runtime status.
    pub status: AgentStatus,
}

impl AgentRecord {
    /// Display name, delegating to [`Agent::display_name`].
    #[must_use]
    pub(crate) fn display_name(&self) -> &str {
        self.agent.display_name()
    }
}

/// Manages all known agents and which one is currently active.
#[derive(Debug, Clone, Default)]
pub struct AgentStore {
    /// All known agents, keyed by NousId.
    agents: HashMap<NousId, AgentRecord>,
    /// Ordered list of agent IDs (preserves server order).
    order: Vec<NousId>,
    /// Currently active agent ID.
    pub active_id: Option<NousId>,
}

impl AgentStore {
    /// Create an empty store.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Replace all agents from a fresh API fetch.
    ///
    /// Auto-selects the first agent if no agent is currently active.
    pub(crate) fn load_agents(&mut self, agents: Vec<Agent>) {
        self.agents.clear();
        self.order.clear();
        for agent in agents {
            let id = agent.id.clone();
            self.order.push(id.clone());
            self.agents.insert(
                id,
                AgentRecord {
                    agent,
                    status: AgentStatus::Idle,
                },
            );
        }
        if self.active_id.is_none() {
            self.active_id = self.order.first().cloned();
        }
    }

    /// Set the active agent. Returns `false` if the ID is unknown.
    pub(crate) fn set_active(&mut self, id: &NousId) -> bool {
        if self.agents.contains_key(id) {
            self.active_id = Some(id.clone());
            true
        } else {
            false
        }
    }

    /// Update an agent's runtime status from SSE events.
    pub(crate) fn update_status(&mut self, id: &NousId, status: AgentStatus) {
        if let Some(record) = self.agents.get_mut(id) {
            record.status = status;
        }
    }

    /// All agents in server order.
    #[must_use]
    pub(crate) fn all(&self) -> Vec<&AgentRecord> {
        self.order
            .iter()
            .filter_map(|id| self.agents.get(id))
            .collect()
    }

    /// Get a specific agent by ID.
    #[must_use]
    pub(crate) fn get(&self, id: &NousId) -> Option<&AgentRecord> {
        self.agents.get(id)
    }

    /// Whether the store has no agents loaded.
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    fn make_agent(id: &str) -> Agent {
        Agent {
            id: id.into(),
            name: Some(id.to_string()),
            model: None,
            emoji: None,
        }
    }

    #[test]
    fn agent_store_starts_empty() {
        let store = AgentStore::new();
        assert!(store.is_empty());
        assert!(store.active_id.is_none());
    }

    #[test]
    fn agent_store_loads_agents() {
        let mut store = AgentStore::new();
        store.load_agents(vec![make_agent("syn"), make_agent("arc")]);
        assert_eq!(store.all().len(), 2);
        assert!(!store.is_empty());
    }

    #[test]
    fn agent_store_auto_selects_first_on_load() {
        let mut store = AgentStore::new();
        store.load_agents(vec![make_agent("syn"), make_agent("arc")]);
        assert_eq!(store.active_id.as_deref(), Some("syn"));
    }

    #[test]
    fn agent_store_set_active_valid() {
        let mut store = AgentStore::new();
        store.load_agents(vec![make_agent("syn"), make_agent("arc")]);
        assert!(store.set_active(&NousId::from("arc")));
        assert_eq!(store.active_id.as_deref(), Some("arc"));
    }

    #[test]
    fn agent_store_set_active_unknown_returns_false() {
        let mut store = AgentStore::new();
        store.load_agents(vec![make_agent("syn")]);
        assert!(!store.set_active(&NousId::from("unknown")));
        assert_eq!(store.active_id.as_deref(), Some("syn"));
    }

    #[test]
    fn agent_store_update_status() {
        let mut store = AgentStore::new();
        store.load_agents(vec![make_agent("syn")]);
        store.update_status(&NousId::from("syn"), AgentStatus::Active);
        let rec = store.get(&NousId::from("syn")).unwrap();
        assert_eq!(rec.status, AgentStatus::Active);
    }

    #[test]
    fn agent_store_update_status_unknown_is_noop() {
        let mut store = AgentStore::new();
        store.load_agents(vec![make_agent("syn")]);
        // Should not panic or error.
        store.update_status(&NousId::from("ghost"), AgentStatus::Error);
        assert_eq!(store.all().len(), 1);
    }

    #[test]
    fn agent_status_labels() {
        assert_eq!(AgentStatus::Active.label(), "active");
        assert_eq!(AgentStatus::Idle.label(), "idle");
        assert_eq!(AgentStatus::Error.label(), "error");
    }

    #[test]
    fn agent_status_dot_colors() {
        assert_eq!(AgentStatus::Active.dot_color(), "#22c55e");
        assert_eq!(AgentStatus::Idle.dot_color(), "#555");
        assert_eq!(AgentStatus::Error.dot_color(), "#ef4444");
    }

    #[test]
    fn agent_record_display_name() {
        let rec = AgentRecord {
            agent: make_agent("syn"),
            status: AgentStatus::default(),
        };
        assert_eq!(rec.display_name(), "syn");
    }

    #[test]
    fn agent_store_preserves_order() {
        let mut store = AgentStore::new();
        store.load_agents(vec![make_agent("c"), make_agent("a"), make_agent("b")]);
        let names: Vec<&str> = store.all().iter().map(|r| r.display_name()).collect();
        assert_eq!(names, vec!["c", "a", "b"]);
    }
}
