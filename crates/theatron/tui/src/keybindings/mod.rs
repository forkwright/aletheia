//! Context-aware keybinding registry: single source of truth for help overlay and status bar hints.

mod helpers;
mod keymap;
mod registry;

pub(crate) use helpers::{context_label, current_contexts, grouped_keybindings, status_bar_hints};
pub(crate) use keymap::KeyMap;

#[cfg(test)]
pub(crate) use helpers::parse_key_combo;
#[cfg(test)]
pub(crate) use keymap::Action;
#[cfg(test)]
pub(crate) use registry::{KeyContext, all_keybindings};

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;
    use crate::app::test_helpers::*;
    use crate::app::{Overlay, SelectionContext};
    use crossterm::event::{KeyCode, KeyModifiers};
    use std::collections::HashMap;

    #[test]
    fn all_keybindings_is_not_empty() {
        let bindings = all_keybindings();
        assert!(bindings.len() > 30);
    }

    #[test]
    fn every_keybinding_has_context() {
        for kb in all_keybindings() {
            assert!(
                !kb.contexts.is_empty(),
                "keybinding '{}' has no contexts",
                kb.keys
            );
        }
    }

    #[test]
    fn section_label_covers_all_variants() {
        let contexts = [
            KeyContext::Global,
            KeyContext::Chat,
            KeyContext::Selection,
            KeyContext::Filter,
            KeyContext::CommandPalette,
            KeyContext::SessionList,
            KeyContext::Input,
            KeyContext::Overlay,
            KeyContext::ToolApproval,
            KeyContext::PlanApproval,
            KeyContext::Settings,
        ];
        for ctx in contexts {
            let label = ctx.section_label();
            assert!(!label.is_empty());
        }
    }

    #[test]
    fn display_order_returns_valid_values() {
        let contexts = [
            KeyContext::Global,
            KeyContext::Chat,
            KeyContext::Input,
            KeyContext::Overlay,
            KeyContext::Selection,
        ];
        for ctx in contexts {
            let order = ctx.display_order();
            assert!(order <= 4);
        }
    }

    #[test]
    fn current_contexts_default_includes_global_chat_input() {
        let app = test_app();
        let contexts = current_contexts(&app);
        assert!(contexts.contains(&KeyContext::Global));
        assert!(contexts.contains(&KeyContext::Chat));
        assert!(contexts.contains(&KeyContext::Input));
    }

    #[test]
    fn current_contexts_command_palette() {
        let mut app = test_app();
        app.interaction.command_palette.active = true;
        let contexts = current_contexts(&app);
        assert!(contexts.contains(&KeyContext::CommandPalette));
        assert!(!contexts.contains(&KeyContext::Chat));
    }

    #[test]
    fn current_contexts_filter_mode() {
        let mut app = test_app();
        app.interaction.filter.active = true;
        app.interaction.filter.editing = true;
        let contexts = current_contexts(&app);
        assert!(contexts.contains(&KeyContext::Filter));
        assert!(!contexts.contains(&KeyContext::Chat));
    }

    #[test]
    fn current_contexts_selection_mode() {
        let mut app = test_app();
        app.interaction.selection = SelectionContext::UserMessage { index: 0 };
        let contexts = current_contexts(&app);
        assert!(contexts.contains(&KeyContext::Selection));
        assert!(!contexts.contains(&KeyContext::Chat));
    }

    #[test]
    fn current_contexts_tool_approval_overlay() {
        let mut app = test_app();
        app.layout.overlay = Some(Overlay::ToolApproval(crate::state::ToolApprovalOverlay {
            turn_id: "t1".into(),
            tool_id: "tool1".into(),
            tool_name: "test_tool".to_string(),
            input: serde_json::Value::Null,
            risk: "low".to_string(),
            reason: "test".to_string(),
        }));
        let contexts = current_contexts(&app);
        assert!(contexts.contains(&KeyContext::ToolApproval));
        assert!(contexts.contains(&KeyContext::Overlay));
    }

    #[test]
    fn current_contexts_settings_overlay() {
        let mut app = test_app();
        let settings = crate::state::settings::SettingsOverlay::from_config(&serde_json::json!({}));
        app.layout.overlay = Some(Overlay::Settings(settings));
        let contexts = current_contexts(&app);
        assert!(contexts.contains(&KeyContext::Settings));
        assert!(!contexts.contains(&KeyContext::Overlay));
    }

    #[test]
    fn context_label_default() {
        let app = test_app();
        assert_eq!(context_label(&app), "Chat");
    }

    #[test]
    fn context_label_command_palette() {
        let mut app = test_app();
        app.interaction.command_palette.active = true;
        assert_eq!(context_label(&app), "Command Palette");
    }

    #[test]
    fn context_label_overlay_variants() {
        let mut app = test_app();
        app.layout.overlay = Some(Overlay::Help);
        // Help overlay is transparent: shows underlying context
        assert_eq!(context_label(&app), "Chat");

        app.layout.overlay = Some(Overlay::AgentPicker { cursor: 0 });
        assert_eq!(context_label(&app), "Agent Picker");

        app.layout.overlay = Some(Overlay::SystemStatus);
        assert_eq!(context_label(&app), "System Status");
    }

    #[test]
    fn grouped_keybindings_deduplicates_keys() {
        let contexts = vec![KeyContext::Global, KeyContext::Chat, KeyContext::Input];
        let groups = grouped_keybindings(&contexts);
        let mut all_keys: Vec<&str> = Vec::new();
        for (_, bindings) in &groups {
            for kb in bindings {
                assert!(
                    !all_keys.contains(&kb.keys),
                    "duplicate key '{}' in grouped_keybindings",
                    kb.keys
                );
                all_keys.push(kb.keys);
            }
        }
    }

    #[test]
    fn grouped_keybindings_empty_contexts_returns_empty() {
        let groups = grouped_keybindings(&[]);
        assert!(groups.is_empty());
    }

    #[test]
    fn status_bar_hints_max_eight() {
        let app = test_app();
        let hints = status_bar_hints(&app);
        assert!(hints.len() <= 8);
    }

    #[test]
    fn status_bar_hints_includes_chat_bindings() {
        let app = test_app();
        let hints = status_bar_hints(&app);
        let keys: Vec<&str> = hints.iter().map(|(k, _)| *k).collect();
        assert!(keys.contains(&"?"), "should include ? for help");
        assert!(keys.contains(&":"), "should include : for command palette");
    }

    // --- KeyMap and parse_key_combo tests ---

    #[test]
    fn parse_key_combo_ctrl_letter() {
        let (mods, code) = parse_key_combo("Ctrl+F").expect("should parse");
        assert_eq!(mods, KeyModifiers::CONTROL);
        assert_eq!(code, KeyCode::Char('f'));
    }

    #[test]
    fn parse_key_combo_shift_arrow() {
        let (mods, code) = parse_key_combo("Shift+Up").expect("should parse");
        assert_eq!(mods, KeyModifiers::SHIFT);
        assert_eq!(code, KeyCode::Up);
    }

    #[test]
    fn parse_key_combo_function_key() {
        let (mods, code) = parse_key_combo("F1").expect("should parse");
        assert_eq!(mods, KeyModifiers::NONE);
        assert_eq!(code, KeyCode::F(1));
    }

    #[test]
    fn parse_key_combo_pageup() {
        let (mods, code) = parse_key_combo("PageUp").expect("should parse");
        assert_eq!(mods, KeyModifiers::NONE);
        assert_eq!(code, KeyCode::PageUp);
    }

    #[test]
    fn parse_key_combo_invalid_returns_none() {
        assert!(parse_key_combo("InvalidKey").is_none());
    }

    #[test]
    fn parse_key_combo_ctrl_alt_combo() {
        let (mods, code) = parse_key_combo("Ctrl+Alt+X").expect("should parse");
        assert!(mods.contains(KeyModifiers::CONTROL));
        assert!(mods.contains(KeyModifiers::ALT));
        assert_eq!(code, KeyCode::Char('x'));
    }

    #[test]
    fn keymap_defaults_include_quit() {
        let keymap = KeyMap::build(&HashMap::new());
        let action = keymap.lookup(KeyModifiers::CONTROL, KeyCode::Char('c'));
        assert_eq!(action, Some(Action::Quit));
    }

    #[test]
    fn keymap_defaults_include_toggle_sidebar() {
        let keymap = KeyMap::build(&HashMap::new());
        let action = keymap.lookup(KeyModifiers::CONTROL, KeyCode::Char('f'));
        assert_eq!(action, Some(Action::ToggleSidebar));
    }

    #[test]
    fn keymap_override_replaces_default() {
        let mut overrides = HashMap::new();
        overrides.insert("toggle_sidebar".to_string(), "Ctrl+G".to_string());
        let keymap = KeyMap::build(&overrides);
        // Old binding should be gone
        assert!(
            keymap
                .lookup(KeyModifiers::CONTROL, KeyCode::Char('f'))
                .is_none()
        );
        // New binding should work
        assert_eq!(
            keymap.lookup(KeyModifiers::CONTROL, KeyCode::Char('g')),
            Some(Action::ToggleSidebar)
        );
    }

    #[test]
    fn keymap_invalid_override_ignored() {
        let mut overrides = HashMap::new();
        overrides.insert("quit".to_string(), "InvalidKey".to_string());
        let keymap = KeyMap::build(&overrides);
        // Default Ctrl+C should still work since the override was invalid
        assert_eq!(
            keymap.lookup(KeyModifiers::CONTROL, KeyCode::Char('c')),
            Some(Action::Quit)
        );
    }

    #[test]
    fn keymap_lookup_unbound_returns_none() {
        let keymap = KeyMap::build(&HashMap::new());
        assert!(
            keymap
                .lookup(KeyModifiers::CONTROL, KeyCode::Char('z'))
                .is_none()
        );
    }

    #[test]
    fn action_to_msg_round_trips() {
        // Verify every action produces a Msg without panicking.
        for action in Action::all() {
            let _msg = action.to_msg();
        }
    }

    #[test]
    fn action_config_key_unique() {
        let keys: Vec<&str> = Action::all().iter().map(|a| a.config_key()).collect();
        let mut deduped = keys.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(
            keys.len(),
            deduped.len(),
            "config_key values must be unique"
        );
    }
}
