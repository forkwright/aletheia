//! Quick input overlay — a floating input for sending messages to agents.
//!
//! Opened via global hotkey (`Ctrl+Shift+Space`) or the tray context menu.
//! Provides a minimal input field with agent selector that sends a message
//! to the selected agent's active session. Designed to feel like a
//! Spotlight/Raycast-style launcher.

use dioxus::prelude::*;

use crate::state::agents::AgentStore;
use crate::state::platform::QuickInputState;

const OVERLAY_BACKDROP: &str = "\
    position: fixed; \
    top: 0; \
    left: 0; \
    right: 0; \
    bottom: 0; \
    background: rgba(0, 0, 0, 0.5); \
    display: flex; \
    align-items: flex-start; \
    justify-content: center; \
    padding-top: 20vh; \
    z-index: 9999;\
";

const OVERLAY_PANEL: &str = "\
    width: 600px; \
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 12px; \
    box-shadow: 0 16px 48px rgba(0, 0, 0, 0.5); \
    padding: 16px; \
    display: flex; \
    flex-direction: column; \
    gap: 12px;\
";

const INPUT_ROW: &str = "\
    display: flex; \
    gap: 8px; \
    align-items: center;\
";

const INPUT_STYLE: &str = "\
    flex: 1; \
    background: #0f0f1a; \
    border: 1px solid #444; \
    border-radius: 8px; \
    padding: 12px 16px; \
    color: #e0e0e0; \
    font-size: 15px; \
    outline: none;\
";

const SELECT_STYLE: &str = "\
    background: #2a2a4a; \
    border: 1px solid #444; \
    border-radius: 8px; \
    padding: 12px; \
    color: #e0e0e0; \
    font-size: 13px; \
    cursor: pointer;\
";

const HINT_STYLE: &str = "\
    color: #555; \
    font-size: 12px; \
    text-align: center;\
";

/// Quick input overlay component.
///
/// Renders a floating panel with a text input and agent selector dropdown.
/// Enter submits the message, Escape or backdrop click closes the overlay.
#[component]
pub(crate) fn QuickInputOverlay() -> Element {
    let mut quick_input: Signal<QuickInputState> = use_context();
    let agents: Signal<AgentStore> = use_context();

    let is_visible = quick_input.read().visible;

    if !is_visible {
        return rsx! {};
    }

    let agent_list = agents
        .read()
        .all()
        .iter()
        .map(|r| (r.agent.id.clone(), r.display_name().to_string()))
        .collect::<Vec<_>>();

    let selected_id = quick_input.read().selected_agent.clone();

    rsx! {
        div {
            style: "{OVERLAY_BACKDROP}",
            // NOTE: Clicking the backdrop closes the overlay.
            onclick: move |_| {
                quick_input.write().close();
            },
            div {
                style: "{OVERLAY_PANEL}",
                // NOTE: Stop propagation so clicking the panel does not close it.
                onclick: move |evt| {
                    evt.stop_propagation();
                },
                div {
                    style: "{INPUT_ROW}",
                    select {
                        style: "{SELECT_STYLE}",
                        value: selected_id.as_deref().unwrap_or(""),
                        onchange: move |evt| {
                            let val = evt.value();
                            if !val.is_empty() {
                                quick_input.write().selected_agent = Some(val.into());
                            }
                        },
                        for (id, name) in agent_list {
                            option {
                                value: "{id}",
                                selected: selected_id.as_deref() == Some(id.as_ref()),
                                "{name}"
                            }
                        }
                    }
                    input {
                        style: "{INPUT_STYLE}",
                        r#type: "text",
                        placeholder: "Send a message...",
                        autofocus: true,
                        value: quick_input.read().input_text.clone(),
                        oninput: move |evt| {
                            quick_input.write().input_text = evt.value();
                        },
                        onkeydown: move |evt| {
                            match evt.key() {
                                Key::Escape => {
                                    quick_input.write().close();
                                }
                                Key::Enter => {
                                    // NOTE: Check if there is input, then take and close
                                    // in a single write lock to avoid double borrow.
                                    let mut guard = quick_input.write();
                                    if !guard.input_text.trim().is_empty() {
                                        guard.close();
                                    }
                                }
                                _ => {}
                            }
                        },
                    }
                }
                div {
                    style: "{HINT_STYLE}",
                    "Enter to send \u{2022} Escape to close"
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use theatron_core::id::NousId;

    use super::*;

    #[test]
    fn quick_input_state_lifecycle() {
        let mut state = QuickInputState::default();
        assert!(!state.visible);

        state.open(Some(NousId::from("syn")));
        assert!(state.visible);
        assert_eq!(state.selected_agent.as_deref(), Some("syn"));

        state.input_text = "hello agent".to_string();
        let text = state.take_input();
        assert_eq!(text.as_deref(), Some("hello agent"));
        assert!(state.input_text.is_empty());

        state.close();
        assert!(!state.visible);
    }
}
