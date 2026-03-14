//! Root application state.
//!
//! In Dioxus, `Store<AppState>` is provided via `use_context_provider` at the
//! app root. Child components project into individual fields. This replaces
//! the monolithic `App` struct from the ratatui TUI.

use super::agent::{AgentEntry, ConnectionStatus};
use super::chat::NousId;

/// Root state provided to the entire component tree.
///
/// In Dioxus each field maps to a signal or sub-store:
/// - `agents`, `focused_agent`, `overlay`, `connection` become `Signal<_>` fields.
/// - `tabs` wraps `Signal<TabBar>` (same concept as TUI's `TabBar`).
/// - Per-tab chat state lives in `AgentChatState` stores owned by tab components,
///   not in `AppState` directly.
#[derive(Debug, Clone)]
pub struct AppState {
    /// All registered agents, fetched on startup and updated via SSE.
    pub agents: Vec<AgentEntry>,
    /// Currently focused agent. `None` before first agent load.
    pub focused_agent: Option<NousId>,
    /// Tab bar managing multiple open conversations.
    pub tabs: TabBar,
    /// SSE connection health.
    pub connection: ConnectionStatus,
    /// Modal overlay (help, pickers, approvals). `None` when no overlay.
    pub overlay: Option<Overlay>,
    /// Sidebar visibility toggle.
    pub sidebar_visible: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            agents: Vec::new(),
            focused_agent: None,
            tabs: TabBar::new(),
            connection: ConnectionStatus::default(),
            overlay: None,
            sidebar_visible: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Tab bar (simplified from TUI's TabBar, state lives in AgentChatState)
// ---------------------------------------------------------------------------

/// Unique tab identifier, distinct from session IDs.
pub type TabId = u64;

/// Metadata for a tab. The actual chat state lives in `AgentChatState`,
/// owned by the tab's component, not here.
#[derive(Debug, Clone)]
pub struct TabEntry {
    pub id: TabId,
    pub agent_id: NousId,
    pub title: String,
    pub unread: bool,
}

/// Tab bar tracking open tabs and active index.
#[derive(Debug, Clone)]
pub struct TabBar {
    pub tabs: Vec<TabEntry>,
    pub active: usize,
    next_id: TabId,
}

impl TabBar {
    #[must_use]
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active: 0,
            next_id: 1,
        }
    }

    /// Create a new tab, returning its index.
    pub fn create(&mut self, agent_id: NousId, title: impl Into<String>) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.push(TabEntry {
            id,
            agent_id,
            title: title.into(),
            unread: false,
        });
        self.tabs.len() - 1
    }

    /// Close a tab by index. Returns the removed entry.
    pub fn close(&mut self, index: usize) -> Option<TabEntry> {
        if index >= self.tabs.len() {
            return None;
        }
        let entry = self.tabs.remove(index);
        if self.tabs.is_empty() {
            self.active = 0;
        } else if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        } else if self.active > index {
            self.active -= 1;
        }
        Some(entry)
    }

    /// Number of open tabs.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    /// Whether the tab bar is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// Get the active tab entry.
    #[must_use]
    pub fn active_tab(&self) -> Option<&TabEntry> {
        self.tabs.get(self.active)
    }
}

impl Default for TabBar {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Overlay
// ---------------------------------------------------------------------------

/// Modal overlays that block interaction with the main UI.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum Overlay {
    Help,
    AgentPicker { cursor: usize },
    SessionPicker { cursor: usize, show_archived: bool },
    ToolApproval(ToolApprovalOverlay),
    Settings,
}

/// Tool approval dialog data.
#[derive(Debug, Clone)]
pub struct ToolApprovalOverlay {
    pub tool_name: String,
    pub input_json: String,
    pub risk: String,
    pub reason: String,
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    #[test]
    fn app_state_default() {
        let state = AppState::default();
        assert!(state.agents.is_empty());
        assert!(state.focused_agent.is_none());
        assert!(state.tabs.is_empty());
        assert!(state.connection.is_connected());
        assert!(state.overlay.is_none());
        assert!(state.sidebar_visible);
    }

    #[test]
    fn tab_bar_create_and_close() {
        let mut bar = TabBar::new();
        let idx = bar.create(NousId::from("syn"), "main");
        assert_eq!(idx, 0);
        assert_eq!(bar.len(), 1);
        assert!(!bar.is_empty());

        let entry = bar.close(0);
        assert!(entry.is_some());
        assert!(bar.is_empty());
    }

    #[test]
    fn tab_bar_close_adjusts_active() {
        let mut bar = TabBar::new();
        bar.create(NousId::from("syn"), "a");
        bar.create(NousId::from("syn"), "b");
        bar.create(NousId::from("syn"), "c");
        bar.active = 2;
        bar.close(0); // remove "a", active should adjust from 2 to 1
        assert_eq!(bar.active, 1);
    }

    #[test]
    fn tab_bar_close_out_of_range() {
        let mut bar = TabBar::new();
        bar.create(NousId::from("syn"), "a");
        assert!(bar.close(5).is_none());
        assert_eq!(bar.len(), 1);
    }

    #[test]
    fn tab_bar_active_tab() {
        let mut bar = TabBar::new();
        bar.create(NousId::from("syn"), "first");
        let tab = bar.active_tab();
        assert!(tab.is_some());
        assert_eq!(tab.unwrap().title, "first");
    }

    #[test]
    fn tab_bar_empty_active_tab_is_none() {
        let bar = TabBar::new();
        assert!(bar.active_tab().is_none());
    }

    #[test]
    fn tab_ids_are_unique() {
        let mut bar = TabBar::new();
        bar.create(NousId::from("a"), "t1");
        bar.create(NousId::from("b"), "t2");
        bar.create(NousId::from("c"), "t3");
        let ids: Vec<TabId> = bar.tabs.iter().map(|t| t.id).collect();
        let unique: std::collections::HashSet<TabId> = ids.iter().copied().collect();
        assert_eq!(ids.len(), unique.len());
    }
}
