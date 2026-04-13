//! Agent presence sidebar with status indicators.
//!
//! Reads `Signal<AgentStore>` and `Signal<EventState>` from context. Clicking
//! an agent calls `AgentStore::set_active`, switching the active chat session.

use dioxus::prelude::*;
use skene::id::NousId;

use crate::state::agents::{AgentStatus, AgentStore};
use crate::state::events::EventState;

const SIDEBAR_SECTION_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    gap: 2px; \
    margin-bottom: var(--space-2);\
";

const SECTION_LABEL_STYLE: &str = "\
    font-size: var(--text-xs); \
    text-transform: uppercase; \
    letter-spacing: 0.08em; \
    color: var(--text-muted); \
    padding: var(--space-1) var(--space-3) 2px;\
";

const AGENT_ROW_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    padding: var(--space-2) var(--space-3); \
    border-radius: var(--radius-md); \
    cursor: pointer; transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick); \
    color: var(--text-primary); \
    font-size: var(--text-sm);\
";

const AGENT_ROW_ACTIVE_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    padding: var(--space-2) var(--space-3); \
    border-radius: var(--radius-md); \
    cursor: pointer; transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick); \
    color: var(--text-primary); \
    font-size: var(--text-sm); \
    background: var(--border);\
";

const EMPTY_AGENTS_STYLE: &str = "\
    padding: var(--space-2) var(--space-3); \
    font-size: var(--text-xs); \
    color: var(--text-muted);\
";

/// Derives the display status for an agent from the current `EventState`.
///
/// Active turns take priority over the status string in `agent_statuses`.
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

/// Agent presence sidebar.
///
/// Reads `Signal<AgentStore>` and `Signal<EventState>` from context. Provides
/// the section of the sidebar that lists agents with colored status indicators.
#[component]
pub(crate) fn AgentSidebarView(collapsed: bool) -> Element {
    let mut store = use_context::<Signal<AgentStore>>();
    let event_state = use_context::<Signal<EventState>>();

    // When collapsed, just show a minimal indicator.
    if collapsed {
        let agent_count = store.read().all().len();
        return rsx! {
            div {
                style: "display: flex; flex-direction: column; align-items: center; padding: var(--space-2) 0;",
                span {
                    style: "font-size: var(--text-xs); color: var(--text-muted); writing-mode: vertical-rl; text-orientation: mixed;",
                    "NOUS"
                }
                if agent_count > 0 {
                    span {
                        style: "font-size: var(--text-xs); color: var(--status-success); margin-top: var(--space-1);",
                        "● {agent_count}"
                    }
                }
            }
        };
    }

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
            style: "{SIDEBAR_SECTION_STYLE}",
            div { style: "{SECTION_LABEL_STYLE}", "NOUS" }
            if agents.is_empty() {
                div { style: "{EMPTY_AGENTS_STYLE}", "No nous" }
            } else {
                for (id , name , emoji , status , is_active) in agents {
                    {
                        let id_clone = id.clone();
                        let color = status.dot_color();
                        let row_style = if is_active {
                            AGENT_ROW_ACTIVE_STYLE
                        } else {
                            AGENT_ROW_STYLE
                        };
                        let status_label = status.label();
                        rsx! {
                            div {
                                key: "{id}",
                                style: "{row_style}",
                                title: "Status: {status_label}",
                                onclick: move |_| {
                                    store.write().set_active(&id_clone);
                                },
                                span {
                                    style: "color: {color}; font-size: var(--text-xs); flex-shrink: 0;",
                                    "●"
                                }
                                if !emoji.is_empty() {
                                    span { style: "font-size: var(--text-base);", "{emoji}" }
                                }
                                span { "{name}" }
                            }
                        }
                    }
                }
            }
        }
    }
}
