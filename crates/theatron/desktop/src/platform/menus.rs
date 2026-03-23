//! Native menu bar definition and event mapping.
//!
//! Defines the application menu structure with keyboard accelerators.
//! The actual menu rendering is handled by the Dioxus desktop runtime
//! (backed by the `muda` crate); this module provides the menu model
//! and maps menu item IDs to application actions.

use crate::state::agents::AgentStore;

/// A menu item with label, accelerator, and action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MenuItem {
    /// Unique identifier for action dispatch.
    pub id: &'static str,
    /// Display label.
    pub label: String,
    /// Keyboard accelerator (display string, e.g. "Ctrl+N").
    pub accelerator: Option<&'static str>,
    /// Whether this item is currently enabled.
    pub enabled: bool,
}

/// A submenu containing a label and child items.
#[derive(Debug, Clone)]
pub(crate) struct Submenu {
    /// Submenu display label.
    pub label: &'static str,
    /// Child items (items or separators).
    pub items: Vec<MenuEntry>,
}

/// A single entry in a menu: either an item, separator, or submenu.
#[derive(Debug, Clone)]
pub(crate) enum MenuEntry {
    /// A clickable menu item.
    Item(MenuItem),
    /// A visual separator.
    Separator,
    /// A nested submenu.
    Sub(Submenu),
}

/// Build the complete application menu bar.
#[must_use]
pub(crate) fn build_menu_bar(agents: &AgentStore, has_active_streams: bool) -> Vec<Submenu> {
    vec![
        file_menu(),
        edit_menu(),
        view_menu(),
        agent_menu(agents, has_active_streams),
        help_menu(),
    ]
}

fn file_menu() -> Submenu {
    Submenu {
        label: "File",
        items: vec![
            MenuEntry::Item(MenuItem {
                id: "file.new_session",
                label: "New Session".to_string(),
                accelerator: Some("Ctrl+N"),
                enabled: true,
            }),
            MenuEntry::Item(MenuItem {
                id: "file.close_session",
                label: "Close Session".to_string(),
                accelerator: Some("Ctrl+W"),
                enabled: true,
            }),
            MenuEntry::Separator,
            MenuEntry::Item(MenuItem {
                id: "file.quit",
                label: "Quit".to_string(),
                accelerator: Some("Ctrl+Q"),
                enabled: true,
            }),
        ],
    }
}

fn edit_menu() -> Submenu {
    Submenu {
        label: "Edit",
        items: vec![
            MenuEntry::Item(MenuItem {
                id: "edit.copy",
                label: "Copy".to_string(),
                accelerator: Some("Ctrl+C"),
                enabled: true,
            }),
            MenuEntry::Item(MenuItem {
                id: "edit.paste",
                label: "Paste".to_string(),
                accelerator: Some("Ctrl+V"),
                enabled: true,
            }),
            MenuEntry::Item(MenuItem {
                id: "edit.select_all",
                label: "Select All".to_string(),
                accelerator: Some("Ctrl+A"),
                enabled: true,
            }),
            MenuEntry::Separator,
            MenuEntry::Item(MenuItem {
                id: "edit.find",
                label: "Find".to_string(),
                accelerator: Some("Ctrl+F"),
                enabled: true,
            }),
        ],
    }
}

fn view_menu() -> Submenu {
    Submenu {
        label: "View",
        items: vec![
            MenuEntry::Item(MenuItem {
                id: "view.chat",
                label: "Chat".to_string(),
                accelerator: Some("Ctrl+1"),
                enabled: true,
            }),
            MenuEntry::Item(MenuItem {
                id: "view.files",
                label: "Files".to_string(),
                accelerator: Some("Ctrl+2"),
                enabled: true,
            }),
            MenuEntry::Item(MenuItem {
                id: "view.planning",
                label: "Planning".to_string(),
                accelerator: Some("Ctrl+3"),
                enabled: true,
            }),
            MenuEntry::Item(MenuItem {
                id: "view.ops",
                label: "Ops".to_string(),
                accelerator: Some("Ctrl+4"),
                enabled: true,
            }),
            MenuEntry::Item(MenuItem {
                id: "view.sessions",
                label: "Sessions".to_string(),
                accelerator: Some("Ctrl+5"),
                enabled: true,
            }),
            MenuEntry::Item(MenuItem {
                id: "view.memory",
                label: "Memory".to_string(),
                accelerator: Some("Ctrl+6"),
                enabled: true,
            }),
            MenuEntry::Item(MenuItem {
                id: "view.metrics",
                label: "Metrics".to_string(),
                accelerator: Some("Ctrl+7"),
                enabled: true,
            }),
            MenuEntry::Separator,
            MenuEntry::Item(MenuItem {
                id: "view.toggle_sidebar",
                label: "Toggle Sidebar".to_string(),
                accelerator: Some("Ctrl+B"),
                enabled: true,
            }),
            MenuEntry::Item(MenuItem {
                id: "view.zoom_in",
                label: "Zoom In".to_string(),
                accelerator: Some("Ctrl+="),
                enabled: true,
            }),
            MenuEntry::Item(MenuItem {
                id: "view.zoom_out",
                label: "Zoom Out".to_string(),
                accelerator: Some("Ctrl+-"),
                enabled: true,
            }),
            MenuEntry::Item(MenuItem {
                id: "view.zoom_reset",
                label: "Reset Zoom".to_string(),
                accelerator: Some("Ctrl+0"),
                enabled: true,
            }),
        ],
    }
}

fn agent_menu(agents: &AgentStore, has_active_streams: bool) -> Submenu {
    let mut items = Vec::new();

    // NOTE: Dynamic agent switch submenu.
    let agent_items: Vec<MenuEntry> = agents
        .all()
        .iter()
        .map(|record| {
            MenuEntry::Item(MenuItem {
                // WHY: Static str not possible for dynamic IDs. The caller matches on
                // the "agent.switch" id and uses the label to identify the agent.
                id: "agent.switch",
                label: format!("{} ({})", record.display_name(), record.status.label()),
                accelerator: None,
                enabled: true,
            })
        })
        .collect();

    if !agent_items.is_empty() {
        items.push(MenuEntry::Sub(Submenu {
            label: "Switch Agent",
            items: agent_items,
        }));
        items.push(MenuEntry::Separator);
    }

    items.push(MenuEntry::Item(MenuItem {
        id: "agent.abort_streaming",
        label: "Abort Streaming".to_string(),
        accelerator: Some("Ctrl+."),
        enabled: has_active_streams,
    }));

    Submenu {
        label: "Agent",
        items,
    }
}

fn help_menu() -> Submenu {
    Submenu {
        label: "Help",
        items: vec![
            MenuEntry::Item(MenuItem {
                id: "help.about",
                label: "About Aletheia".to_string(),
                accelerator: None,
                enabled: true,
            }),
            MenuEntry::Item(MenuItem {
                id: "help.docs",
                label: "Documentation".to_string(),
                accelerator: None,
                enabled: true,
            }),
        ],
    }
}

/// Application action triggered by a menu item.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub(crate) enum MenuAction {
    /// Create a new session.
    NewSession,
    /// Close the current session.
    CloseSession,
    /// Quit the application.
    Quit,
    /// Navigate to a view by route path.
    NavigateView(&'static str),
    /// Toggle sidebar visibility.
    ToggleSidebar,
    /// Zoom in.
    ZoomIn,
    /// Zoom out.
    ZoomOut,
    /// Reset zoom to default.
    ZoomReset,
    /// Switch to a specific agent.
    SwitchAgent(String),
    /// Abort all active streaming.
    AbortStreaming,
    /// Show the about dialog.
    ShowAbout,
    /// Open documentation in the browser.
    OpenDocs,
}

/// Map a menu item ID to an application action.
#[must_use]
pub(crate) fn resolve_action(menu_id: &str) -> Option<MenuAction> {
    match menu_id {
        "file.new_session" => Some(MenuAction::NewSession),
        "file.close_session" => Some(MenuAction::CloseSession),
        "file.quit" => Some(MenuAction::Quit),
        "view.chat" => Some(MenuAction::NavigateView("/")),
        "view.files" => Some(MenuAction::NavigateView("/files")),
        "view.planning" => Some(MenuAction::NavigateView("/planning")),
        "view.ops" => Some(MenuAction::NavigateView("/ops")),
        "view.sessions" => Some(MenuAction::NavigateView("/sessions")),
        "view.memory" => Some(MenuAction::NavigateView("/memory")),
        "view.metrics" => Some(MenuAction::NavigateView("/metrics")),
        "view.toggle_sidebar" => Some(MenuAction::ToggleSidebar),
        "view.zoom_in" => Some(MenuAction::ZoomIn),
        "view.zoom_out" => Some(MenuAction::ZoomOut),
        "view.zoom_reset" => Some(MenuAction::ZoomReset),
        "agent.abort_streaming" => Some(MenuAction::AbortStreaming),
        "help.about" => Some(MenuAction::ShowAbout),
        "help.docs" => Some(MenuAction::OpenDocs),
        _ => None,
    }
}

/// Count of view menu items (for testing completeness).
pub(crate) const VIEW_ROUTE_COUNT: usize = 7;

#[cfg(test)]
mod tests {
    use theatron_core::api::types::Agent;
    use theatron_core::id::NousId;

    use super::*;

    fn make_store(names: &[&str]) -> AgentStore {
        let mut store = AgentStore::new();
        let agents: Vec<Agent> = names
            .iter()
            .map(|n| Agent {
                id: NousId::from(*n),
                name: Some(n.to_string()),
                model: None,
                emoji: None,
            })
            .collect();
        store.load_agents(agents);
        store
    }

    #[test]
    fn build_menu_bar_has_five_menus() {
        let store = make_store(&["syn"]);
        let bar = build_menu_bar(&store, false);
        assert_eq!(bar.len(), 5);
        assert_eq!(bar[0].label, "File");
        assert_eq!(bar[1].label, "Edit");
        assert_eq!(bar[2].label, "View");
        assert_eq!(bar[3].label, "Agent");
        assert_eq!(bar[4].label, "Help");
    }

    #[test]
    fn file_menu_has_quit() {
        let menu = file_menu();
        let has_quit = menu
            .items
            .iter()
            .any(|e| matches!(e, MenuEntry::Item(i) if i.id == "file.quit"));
        assert!(has_quit);
    }

    #[test]
    fn view_menu_has_all_routes() {
        let menu = view_menu();
        let route_items = menu
            .items
            .iter()
            .filter(|e| matches!(e, MenuEntry::Item(i) if i.id.starts_with("view.") && i.accelerator.is_some() && i.id != "view.toggle_sidebar" && i.id != "view.zoom_in" && i.id != "view.zoom_out" && i.id != "view.zoom_reset"))
            .count();
        assert_eq!(route_items, VIEW_ROUTE_COUNT);
    }

    #[test]
    fn view_menu_items_have_accelerators() {
        let menu = view_menu();
        for entry in &menu.items {
            if let MenuEntry::Item(item) = entry {
                assert!(
                    item.accelerator.is_some(),
                    "menu item '{}' missing accelerator",
                    item.id
                );
            }
        }
    }

    #[test]
    fn agent_menu_abort_disabled_when_no_streams() {
        let store = make_store(&["syn"]);
        let menu = agent_menu(&store, false);
        let abort = menu.items.iter().find_map(|e| match e {
            MenuEntry::Item(i) if i.id == "agent.abort_streaming" => Some(i),
            _ => None,
        });
        assert!(abort.is_some());
        assert!(!abort.expect("abort item").enabled);
    }

    #[test]
    fn agent_menu_abort_enabled_when_streaming() {
        let store = make_store(&["syn"]);
        let menu = agent_menu(&store, true);
        let abort = menu.items.iter().find_map(|e| match e {
            MenuEntry::Item(i) if i.id == "agent.abort_streaming" => Some(i),
            _ => None,
        });
        assert!(abort.expect("abort item").enabled);
    }

    #[test]
    fn agent_menu_dynamic_agents() {
        let store = make_store(&["syn", "arc", "mneme"]);
        let menu = agent_menu(&store, false);
        // NOTE: Should have a Switch Agent submenu with 3 entries.
        let switch_sub = menu.items.iter().find_map(|e| match e {
            MenuEntry::Sub(s) if s.label == "Switch Agent" => Some(s),
            _ => None,
        });
        assert!(switch_sub.is_some());
        assert_eq!(switch_sub.expect("switch sub").items.len(), 3);
    }

    #[test]
    fn agent_menu_empty_agents_no_submenu() {
        let store = AgentStore::new();
        let menu = agent_menu(&store, false);
        let has_switch = menu
            .items
            .iter()
            .any(|e| matches!(e, MenuEntry::Sub(s) if s.label == "Switch Agent"));
        assert!(!has_switch);
    }

    #[test]
    fn resolve_action_known_ids() {
        assert_eq!(
            resolve_action("file.new_session"),
            Some(MenuAction::NewSession)
        );
        assert_eq!(resolve_action("file.quit"), Some(MenuAction::Quit));
        assert_eq!(
            resolve_action("view.chat"),
            Some(MenuAction::NavigateView("/"))
        );
        assert_eq!(
            resolve_action("view.files"),
            Some(MenuAction::NavigateView("/files"))
        );
        assert_eq!(
            resolve_action("view.toggle_sidebar"),
            Some(MenuAction::ToggleSidebar)
        );
        assert_eq!(
            resolve_action("agent.abort_streaming"),
            Some(MenuAction::AbortStreaming)
        );
        assert_eq!(resolve_action("help.about"), Some(MenuAction::ShowAbout));
        assert_eq!(resolve_action("help.docs"), Some(MenuAction::OpenDocs));
    }

    #[test]
    fn resolve_action_unknown_id() {
        assert!(resolve_action("unknown.action").is_none());
        assert!(resolve_action("").is_none());
    }

    #[test]
    fn resolve_action_zoom() {
        assert_eq!(resolve_action("view.zoom_in"), Some(MenuAction::ZoomIn));
        assert_eq!(resolve_action("view.zoom_out"), Some(MenuAction::ZoomOut));
        assert_eq!(
            resolve_action("view.zoom_reset"),
            Some(MenuAction::ZoomReset)
        );
    }
}
