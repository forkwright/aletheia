//! Top bar with brand, agent status pills, and connection/theme controls.
//!
//! Sits above the main content area, providing at-a-glance agent status
//! and quick theme switching. Mirrors the Svelte webui's TopBar pattern.

use dioxus::prelude::*;
use skene::id::NousId;

use crate::state::agents::{AgentStatus, AgentStore};
use crate::state::events::EventState;

const TOPBAR_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    height: 52px; \
    padding: 0 var(--space-4); \
    background: var(--bg-surface); \
    border-bottom: 1px solid var(--border-separator); \
    flex-shrink: 0; \
    gap: var(--space-4);\
";

const BRAND_STYLE: &str = "\
    font-family: var(--font-display); \
    font-size: var(--text-lg); \
    font-weight: var(--weight-semibold); \
    color: var(--accent); \
    flex-shrink: 0; \
    letter-spacing: 0.02em;\
";

const PILLS_CONTAINER_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    flex: 1; \
    overflow-x: auto; \
    scrollbar-width: none;\
";

const PILL_STYLE: &str = "\
    display: inline-flex; \
    align-items: center; \
    gap: var(--space-1); \
    padding: 6px var(--space-3); \
    border-radius: var(--radius-full); \
    border: 1px solid var(--border); \
    background: var(--bg); \
    color: var(--text-secondary); \
    font-size: var(--text-sm); \
    cursor: pointer; \
    white-space: nowrap; \
    transition: border-color var(--transition-quick), \
                background-color var(--transition-quick);\
";

const PILL_ACTIVE_STYLE: &str = "\
    display: inline-flex; \
    align-items: center; \
    gap: var(--space-1); \
    padding: 6px var(--space-3); \
    border-radius: var(--radius-full); \
    border: 1px solid var(--accent); \
    background: var(--bg-surface-bright); \
    color: var(--text-primary); \
    font-size: var(--text-sm); \
    cursor: pointer; \
    white-space: nowrap; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick); \
    box-shadow: 0 0 0 3px rgb(154 123 79 / 0.2);\
";

const CONTROLS_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    flex-shrink: 0;\
";

/// Derive the display status for an agent from the current `EventState`.
fn derive_status(nous_id: &NousId, event_state: &EventState) -> AgentStatus {
    if event_state
        .active_turns
        .iter()
        .any(|t| &t.nous_id == nous_id)
    {
        return AgentStatus::Active;
    }
    match event_state.agent_statuses.get(nous_id).map(String::as_str) {
        Some("error") => AgentStatus::Error,
        _ => AgentStatus::Idle,
    }
}

/// Top bar with brand, agent pills, and controls.
#[component]
pub(crate) fn TopBar() -> Element {
    let mut store = use_context::<Signal<AgentStore>>();
    let event_state = use_context::<Signal<EventState>>();

    let agents: Vec<(NousId, String, String, AgentStatus, bool)> = {
        let s = store.read();
        let es = event_state.read();
        s.all()
            .iter()
            .map(|rec| {
                let status = derive_status(&rec.agent.id, &es);
                let is_active = s.active_id.as_ref() == Some(&rec.agent.id);
                (
                    rec.agent.id.clone(),
                    rec.display_name().to_string(),
                    rec.agent.emoji.clone().unwrap_or_default(),
                    status,
                    is_active,
                )
            })
            .collect()
    };

    rsx! {
        div {
            style: "{TOPBAR_STYLE}",
            role: "banner",
            "aria-label": "Top bar",

            // Brand
            span { style: "{BRAND_STYLE}", "Aletheia" }

            // Agent pills
            div {
                style: "{PILLS_CONTAINER_STYLE}",
                "aria-label": "Agent status",
                for (id , name , emoji , status , is_active) in agents {
                    {
                        let id_clone = id.clone();
                        let color = status.dot_color();
                        let pill_style = if is_active {
                            PILL_ACTIVE_STYLE
                        } else {
                            PILL_STYLE
                        };
                        let status_label = status.label();
                        let dot_style = if matches!(status, AgentStatus::Active) {
                            format!("width: 10px; height: 10px; border-radius: 50%; background: {color}; flex-shrink: 0; animation: status-pulse 2s ease-in-out infinite;")
                        } else {
                            format!("width: 10px; height: 10px; border-radius: 50%; background: {color}; flex-shrink: 0;")
                        };
                        rsx! {
                            button {
                                key: "{id}",
                                r#type: "button",
                                style: "{pill_style}",
                                title: "{name} — {status_label}",
                                "aria-label": "Agent {name}, status {status_label}",
                                onclick: move |_| {
                                    store.write().set_active(&id_clone);
                                },
                                span {
                                    style: "{dot_style}",
                                    aria_hidden: "true",
                                }
                                if !emoji.is_empty() {
                                    span { style: "font-size: var(--text-base);", "{emoji}" }
                                }
                                span { "{name} " }
                                span {
                                    style: "font-size: var(--text-xs); color: var(--text-muted);",
                                    "{status_label}"
                                }
                            }
                        }
                    }
                }
            }

            // Controls
            div {
                style: "{CONTROLS_STYLE}",
                crate::components::theme_toggle::ThemeToggle {}
                crate::components::connection_indicator::ConnectionIndicatorView {}
            }
        }
    }
}
