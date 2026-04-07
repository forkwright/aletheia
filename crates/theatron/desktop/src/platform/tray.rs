//! System tray icon and context menu for the desktop application.
//!
//! Provides logic for deriving tray state from agent statuses, generating
//! context menu items, and handling tray events. The actual tray icon rendering
//! is performed by the Dioxus desktop runtime; this module supplies the
//! data model and event handlers.

use crate::state::agents::{AgentStatus, AgentStore};
use crate::state::platform::{TrayIconStatus, TrayState};

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
}
