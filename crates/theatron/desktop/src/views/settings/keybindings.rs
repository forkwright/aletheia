//! Keybindings settings panel.
//!
//! Shows all actions grouped by category with their current key combos.
//! Supports click-to-record, conflict detection, and per-category reset.

use dioxus::prelude::*;

use crate::services::settings_config;
use crate::state::settings::{KeyAction, KeyCategory, KeyCombo, KeybindingStore, default_actions};

/// Keybindings panel with recording overlay and conflict detection.
#[component]
pub(crate) fn KeybindingsPanel() -> Element {
    let mut keybindings: Signal<KeybindingStore> = use_context();
    let server_store = use_context::<Signal<crate::state::settings::ServerConfigStore>>();
    let appearance = use_context::<Signal<crate::state::settings::AppearanceSettings>>();

    let actions = use_hook(default_actions);

    // Recording state: Some(action_id) when waiting for a keypress.
    let mut recording_id: Signal<Option<String>> = use_signal(|| None);
    // Conflict dialog: (pending_combo, conflicting_action_id, target_action_id).
    let mut conflict_state: Signal<Option<(KeyCombo, String, String)>> = use_signal(|| None);

    rsx! {
        div {
            style: "display: flex; flex-direction: column; gap: 24px; max-width: 700px;",

            // Global reset
            div {
                style: "display: flex; justify-content: flex-end;",
                button {
                    style: "padding: 5px 14px; background: none; border: 1px solid #444; \
                            border-radius: 5px; color: #888; font-size: 12px; cursor: pointer;",
                    onclick: move |_| {
                        keybindings.write().overrides.clear();
                        let store = server_store.read();
                        let app = appearance.read();
                        let keys = keybindings.read();
                        settings_config::save_state(&store, &app, &keys);
                    },
                    "Reset all to defaults"
                }
            }

            // Capture overlay when recording
            if recording_id.read().is_some() {
                div {
                    style: "position: fixed; inset: 0; background: rgba(0,0,0,0.6); \
                            display: flex; align-items: center; justify-content: center; z-index: 100;",
                    tabindex: "0",
                    autofocus: true,
                    onkeydown: move |evt| {
                        let key = evt.data().key().to_string();
                        // Ignore bare modifier keypresses.
                        if matches!(key.as_str(), "Control" | "Alt" | "Shift" | "Meta") {
                            return;
                        }
                        if key == "Escape" {
                            recording_id.set(None);
                            return;
                        }
                        let mods = evt.data().modifiers();
                        let combo = KeyCombo {
                            ctrl: mods.ctrl(),
                            alt: mods.alt(),
                            shift: mods.shift(),
                            key,
                        };
                        // WHY: Clone out of the read guard before calling set() to
                        // avoid holding an immutable borrow when the mutable one fires.
                        let target_id_opt = recording_id.read().clone();
                        if let Some(target_id) = target_id_opt {
                            let acts = default_actions();
                            if let Some(conflict) = keybindings.read().conflict(&combo, &target_id, &acts) {
                                conflict_state.set(Some((combo, conflict.id.to_string(), target_id.clone())));
                            } else {
                                keybindings.write().set(&target_id, combo);
                                let store = server_store.read();
                                let app = appearance.read();
                                let keys = keybindings.read();
                                settings_config::save_state(&store, &app, &keys);
                            }
                            recording_id.set(None);
                        }
                    },
                    div {
                        style: "background: #1a1a2e; border: 1px solid #5b6af0; border-radius: 12px; \
                                padding: 32px 48px; text-align: center; color: #e0e0e0;",
                        div {
                            style: "font-size: 16px; margin-bottom: 8px;",
                            "Press a key combination"
                        }
                        div {
                            style: "font-size: 12px; color: #666;",
                            "Esc to cancel"
                        }
                    }
                }
            }

            // Conflict dialog
            if let Some((ref combo, ref conflict_id, ref target_id)) = conflict_state.read().clone() {
                {
                    let conflict_label = default_actions()
                        .iter()
                        .find(|a| a.id == conflict_id.as_str())
                        .map(|a| a.label)
                        .unwrap_or("another action");
                    let combo_display = combo.display();
                    let combo_clone = combo.clone();
                    let target_clone = target_id.clone();
                    let conflict_clone = conflict_id.clone();

                    rsx! {
                        div {
                            style: "position: fixed; inset: 0; background: rgba(0,0,0,0.6); \
                                    display: flex; align-items: center; justify-content: center; z-index: 110;",
                            div {
                                style: "background: #1a1a2e; border: 1px solid #f59e0b; border-radius: 10px; \
                                        padding: 24px 32px; max-width: 380px; width: 90%;",
                                div {
                                    style: "font-size: 14px; color: #e0e0e0; margin-bottom: 12px;",
                                    "{combo_display} is already used by \"{conflict_label}\"."
                                }
                                div {
                                    style: "font-size: 12px; color: #888; margin-bottom: 20px;",
                                    "Reassign will remove it from that action."
                                }
                                div {
                                    style: "display: flex; gap: 8px; justify-content: flex-end;",
                                    button {
                                        style: "padding: 6px 14px; background: none; border: 1px solid #444; \
                                                border-radius: 5px; color: #888; font-size: 12px; cursor: pointer;",
                                        onclick: move |_| { conflict_state.set(None); },
                                        "Cancel"
                                    }
                                    button {
                                        style: "padding: 6px 14px; background: #f59e0b; border: none; \
                                                border-radius: 5px; color: #000; font-size: 12px; cursor: pointer;",
                                        onclick: move |_| {
                                            keybindings.write().reset(&conflict_clone);
                                            keybindings.write().set(&target_clone, combo_clone.clone());
                                            let store = server_store.read();
                                            let app = appearance.read();
                                            let keys = keybindings.read();
                                            settings_config::save_state(&store, &app, &keys);
                                            conflict_state.set(None);
                                        },
                                        "Reassign"
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Action table grouped by category
            for category in KeyCategory::all() {
                {
                    let cat = *category;
                    let cat_actions: Vec<KeyAction> = actions
                        .iter()
                        .filter(|a| a.category == cat)
                        .cloned()
                        .collect();

                    rsx! {
                        div {
                            key: "{cat:?}",
                            style: "background: #1a1a2e; border: 1px solid #333; border-radius: 8px; overflow: hidden;",

                            // Category header
                            div {
                                style: "display: flex; justify-content: space-between; align-items: center; \
                                        padding: 10px 16px; background: #161626; border-bottom: 1px solid #333;",
                                span {
                                    style: "font-size: 11px; font-weight: bold; color: #666; \
                                            text-transform: uppercase; letter-spacing: 0.6px;",
                                    "{cat.label()}"
                                }
                                button {
                                    style: "padding: 3px 10px; background: none; border: 1px solid #333; \
                                            border-radius: 4px; color: #666; font-size: 11px; cursor: pointer;",
                                    onclick: move |_| {
                                        let all_actions = default_actions();
                                        keybindings.write().reset_category(cat, &all_actions);
                                        let store = server_store.read();
                                        let app = appearance.read();
                                        let keys = keybindings.read();
                                        settings_config::save_state(&store, &app, &keys);
                                    },
                                    "Reset"
                                }
                            }

                            // Action rows
                            for action in cat_actions.iter() {
                                {
                                    let action_id = action.id;
                                    let combo_str = keybindings.read().effective(action).display();
                                    let is_recording = recording_id.read().as_deref() == Some(action_id);
                                    let row_bg = if is_recording { "#1e1e3a" } else { "transparent" };
                                    let combo_style = if is_recording {
                                        "padding: 4px 10px; background: #5b6af0; border: none; \
                                         border-radius: 4px; color: #fff; font-size: 12px; cursor: pointer; min-width: 90px;"
                                    } else {
                                        "padding: 4px 10px; background: #0d0d1a; border: 1px solid #333; \
                                         border-radius: 4px; color: #ccc; font-size: 12px; cursor: pointer; min-width: 90px; \
                                         font-family: monospace;"
                                    };
                                    let aid_str = action_id.to_string();

                                    rsx! {
                                        div {
                                            key: "{action_id}",
                                            style: "display: flex; justify-content: space-between; align-items: center; \
                                                    padding: 8px 16px; background: {row_bg}; border-bottom: 1px solid #222;",
                                            span {
                                                style: "font-size: 13px; color: #ccc;",
                                                "{action.label}"
                                            }
                                            div {
                                                style: "display: flex; gap: 6px; align-items: center;",
                                                button {
                                                    style: "{combo_style}",
                                                    onclick: move |_| {
                                                        if is_recording {
                                                            recording_id.set(None);
                                                        } else {
                                                            recording_id.set(Some(aid_str.clone()));
                                                        }
                                                    },
                                                    if is_recording { "Recording…" } else { "{combo_str}" }
                                                }
                                                button {
                                                    style: "padding: 3px 8px; background: none; border: 1px solid #333; \
                                                            border-radius: 4px; color: #555; font-size: 11px; cursor: pointer;",
                                                    title: "Reset to default",
                                                    onclick: move |_| {
                                                        keybindings.write().reset(action_id);
                                                        let store = server_store.read();
                                                        let app = appearance.read();
                                                        let keys = keybindings.read();
                                                        settings_config::save_state(&store, &app, &keys);
                                                    },
                                                    "↺"
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
