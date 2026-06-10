//! Top bar with agent status pills and connection/theme controls.
//!
//! Sits above the main content area, providing at-a-glance agent status
//! and quick theme switching. The sidebar owns the single brand mark.

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
    gap: var(--space-2); \
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
    gap: var(--space-2); \
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
    box-shadow: var(--shadow-glow);\
";

const STATUS_PILL_STYLE: &str = "\
    display: inline-flex; \
    align-items: center; \
    gap: var(--space-1); \
    padding: 0 var(--space-2); \
    border-radius: var(--radius-full); \
    font-size: var(--text-xs); \
    line-height: var(--leading-normal); \
    white-space: nowrap; \
    flex-shrink: 0;\
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

/// Tinted background token for a status pill, paired with [`AgentStatus::dot_color`].
fn status_pill_bg(status: &AgentStatus) -> &'static str {
    match status {
        AgentStatus::Active => "var(--status-success-bg)",
        AgentStatus::Idle => "var(--status-warning-bg)",
        AgentStatus::Error => "var(--status-error-bg)",
    }
}

/// Top bar with agent pills and controls.
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
                        let status_pill_style = format!(
                            "{STATUS_PILL_STYLE} background: {}; color: {color};",
                            status_pill_bg(&status),
                        );
                        let dot_style = if matches!(status, AgentStatus::Active) {
                            format!("width: var(--space-2); height: var(--space-2); border-radius: var(--radius-full); background: {color}; flex-shrink: 0; animation: status-pulse 2s ease-in-out infinite;")
                        } else {
                            format!("width: var(--space-2); height: var(--space-2); border-radius: var(--radius-full); background: {color}; flex-shrink: 0;")
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
                                if !emoji.is_empty() {
                                    span { style: "font-size: var(--text-base);", "{emoji}" }
                                }
                                span { "{name}" }
                                span {
                                    style: "{status_pill_style}",
                                    span {
                                        style: "{dot_style}",
                                        aria_hidden: "true",
                                    }
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
