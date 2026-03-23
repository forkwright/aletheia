//! System tray icon and context menu for the desktop application.
//!
//! Provides logic for deriving tray state from agent statuses, generating
//! context menu items, and handling tray events. The actual tray icon rendering
//! is performed by the Dioxus desktop runtime; this module supplies the
//! data model and event handlers.

use crate::state::agents::{AgentRecord, AgentStatus, AgentStore};
use crate::state::platform::{CloseBehavior, TrayIconStatus, TrayState};

/// A tray context menu item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TrayMenuItem {
    /// Toggle window visibility.
    ToggleWindow,
    /// Separator line.
    Separator,
    /// An agent entry with status indicator.
    Agent {
        /// Agent display name.
        name: String,
        /// Status label (idle/active/error).
        status_label: &'static str,
        /// CSS color for the status indicator.
        status_color: &'static str,
    },
    /// Open the quick input overlay.
    QuickInput,
    /// Abort all active streaming.
    AbortAll,
    /// Open the settings view.
    OpenSettings,
    /// Quit the application.
    Quit,
}

/// Build the tray context menu items from current state.
#[must_use]
pub(crate) fn build_menu(
    _tray: &TrayState,
    agents: &AgentStore,
    has_active_streams: bool,
) -> Vec<TrayMenuItem> {
    let mut items = Vec::with_capacity(10 + agents.all().len());

    // WHY: Show/Hide is the most common tray action, so it goes first.
    items.push(TrayMenuItem::ToggleWindow);
    items.push(TrayMenuItem::Separator);

    // NOTE: Agent list with status indicators.
    for record in agents.all() {
        items.push(agent_menu_item(record));
    }

    if !agents.is_empty() {
        items.push(TrayMenuItem::Separator);
    }

    items.push(TrayMenuItem::QuickInput);

    // NOTE: Abort is only meaningful when something is streaming.
    if has_active_streams {
        items.push(TrayMenuItem::AbortAll);
    }

    items.push(TrayMenuItem::Separator);
    items.push(TrayMenuItem::OpenSettings);
    items.push(TrayMenuItem::Quit);

    items
}

/// Build a single agent menu item from an agent record.
fn agent_menu_item(record: &AgentRecord) -> TrayMenuItem {
    TrayMenuItem::Agent {
        name: record.display_name().to_string(),
        status_label: record.status.label(),
        status_color: record.status.dot_color(),
    }
}

/// Derive `TrayState` from the current agent store and connection state.
#[must_use]
pub(crate) fn derive_tray_state(
    agents: &AgentStore,
    connected: bool,
    window_visible: bool,
) -> TrayState {
    let all = agents.all();
    let statuses: Vec<AgentStatus> = all.iter().map(|r| r.status.clone()).collect();
    let processing_count = statuses
        .iter()
        .filter(|s| matches!(s, AgentStatus::Active))
        .count();

    TrayState {
        icon_status: TrayIconStatus::from_agents(&statuses, connected),
        agent_count: all.len(),
        processing_count,
        window_visible,
    }
}

/// Actions that can result from tray events.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub(crate) enum TrayAction {
    /// Toggle window visibility.
    ToggleWindow,
    /// Show window (from double-click when hidden).
    ShowWindow,
    /// Open the quick input overlay.
    OpenQuickInput,
    /// Abort all active streams.
    AbortAll,
    /// Navigate to settings view.
    OpenSettings,
    /// Quit the application.
    Quit,
}

/// Determine the action for a double-click on the tray icon.
#[must_use]
pub(crate) fn on_double_click(window_visible: bool) -> TrayAction {
    if window_visible {
        TrayAction::ToggleWindow
    } else {
        TrayAction::ShowWindow
    }
}

/// Determine the window behavior when the close button is clicked.
#[must_use]
pub(crate) fn on_close_request(behavior: CloseBehavior) -> TrayAction {
    match behavior {
        CloseBehavior::MinimizeToTray => TrayAction::ToggleWindow,
        CloseBehavior::Quit => TrayAction::Quit,
    }
}

#[cfg(test)]
mod tests {
    use theatron_core::api::types::Agent;
    use theatron_core::id::NousId;

    use super::*;

    fn make_store(agents: Vec<(&str, AgentStatus)>) -> AgentStore {
        let mut store = AgentStore::new();
        let agent_list: Vec<Agent> = agents
            .iter()
            .map(|(id, _)| Agent {
                id: NousId::from(*id),
                name: Some(id.to_string()),
                model: None,
                emoji: None,
            })
            .collect();
        store.load_agents(agent_list);
        for (id, status) in &agents {
            store.update_status(&NousId::from(*id), status.clone());
        }
        store
    }

    #[test]
    fn build_menu_with_agents() {
        let store = make_store(vec![
            ("syn", AgentStatus::Active),
            ("arc", AgentStatus::Idle),
        ]);
        let tray = derive_tray_state(&store, true, true);
        let menu = build_menu(&tray, &store, true);

        assert!(matches!(menu[0], TrayMenuItem::ToggleWindow));
        assert!(matches!(menu[1], TrayMenuItem::Separator));
        assert!(matches!(menu[2], TrayMenuItem::Agent { .. }));
        assert!(matches!(menu[3], TrayMenuItem::Agent { .. }));
        assert!(matches!(menu[4], TrayMenuItem::Separator));
        assert!(matches!(menu[5], TrayMenuItem::QuickInput));
        assert!(matches!(menu[6], TrayMenuItem::AbortAll));
        assert!(matches!(menu[7], TrayMenuItem::Separator));
        assert!(matches!(menu[8], TrayMenuItem::OpenSettings));
        assert!(matches!(menu[9], TrayMenuItem::Quit));
    }

    #[test]
    fn build_menu_no_abort_when_no_streams() {
        let store = make_store(vec![("syn", AgentStatus::Idle)]);
        let tray = derive_tray_state(&store, true, true);
        let menu = build_menu(&tray, &store, false);

        assert!(!menu.iter().any(|m| matches!(m, TrayMenuItem::AbortAll)));
    }

    #[test]
    fn build_menu_empty_agents() {
        let store = AgentStore::new();
        let tray = derive_tray_state(&store, true, true);
        let menu = build_menu(&tray, &store, false);

        // NOTE: Should not have agent entries or agent separator.
        assert_eq!(
            menu.iter()
                .filter(|m| matches!(m, TrayMenuItem::Agent { .. }))
                .count(),
            0
        );
    }

    #[test]
    fn derive_tray_state_connected_mixed() {
        let store = make_store(vec![
            ("syn", AgentStatus::Active),
            ("arc", AgentStatus::Idle),
        ]);
        let tray = derive_tray_state(&store, true, false);
        assert_eq!(tray.icon_status, TrayIconStatus::Active);
        assert_eq!(tray.agent_count, 2);
        assert_eq!(tray.processing_count, 1);
        assert!(!tray.window_visible);
    }

    #[test]
    fn derive_tray_state_disconnected() {
        let store = make_store(vec![("syn", AgentStatus::Active)]);
        let tray = derive_tray_state(&store, false, true);
        assert_eq!(tray.icon_status, TrayIconStatus::Disconnected);
    }

    #[test]
    fn on_double_click_toggles() {
        assert_eq!(on_double_click(true), TrayAction::ToggleWindow);
        assert_eq!(on_double_click(false), TrayAction::ShowWindow);
    }

    #[test]
    fn on_close_request_behavior() {
        assert_eq!(
            on_close_request(CloseBehavior::MinimizeToTray),
            TrayAction::ToggleWindow
        );
        assert_eq!(on_close_request(CloseBehavior::Quit), TrayAction::Quit);
    }

    #[test]
    fn agent_menu_item_captures_status() {
        let record = AgentRecord {
            agent: Agent {
                id: NousId::from("syn"),
                name: Some("Synthesis".to_string()),
                model: None,
                emoji: None,
            },
            status: AgentStatus::Active,
        };
        let item = agent_menu_item(&record);
        match item {
            TrayMenuItem::Agent {
                name,
                status_label,
                status_color,
            } => {
                assert_eq!(name, "Synthesis");
                assert_eq!(status_label, "active");
                assert_eq!(status_color, "#22c55e");
            }
            _ => panic!("expected Agent menu item"),
        }
    }
}
