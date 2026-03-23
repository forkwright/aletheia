//! Agent presence sidebar with status indicators.
//!
//! Reads `Signal<AgentStore>` and `Signal<EventState>` from context. Clicking
//! an agent calls `AgentStore::set_active`, switching the active chat session.

use dioxus::prelude::*;
use theatron_core::id::NousId;

use crate::state::agents::{AgentStatus, AgentStore};
use crate::state::events::EventState;

const SIDEBAR_SECTION_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    gap: 2px; \
    margin-bottom: 8px;\
";

const SECTION_LABEL_STYLE: &str = "\
    font-size: 10px; \
    text-transform: uppercase; \
    letter-spacing: 0.08em; \
    color: #555; \
    padding: 4px 12px 2px;\
";

const AGENT_ROW_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 8px; \
    padding: 7px 12px; \
    border-radius: 6px; \
    cursor: pointer; \
    color: #e0e0e0; \
    font-size: 13px;\
";

const AGENT_ROW_ACTIVE_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 8px; \
    padding: 7px 12px; \
    border-radius: 6px; \
    cursor: pointer; \
    color: #ffffff; \
    font-size: 13px; \
    background: #2a2a4a;\
";

const EMPTY_AGENTS_STYLE: &str = "\
    padding: 8px 12px; \
    font-size: 12px; \
    color: #555;\
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
pub(crate) fn AgentSidebarView() -> Element {
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
            style: "{SIDEBAR_SECTION_STYLE}",
            div { style: "{SECTION_LABEL_STYLE}", "Agents" }
            if agents.is_empty() {
                div { style: "{EMPTY_AGENTS_STYLE}", "No agents" }
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
                                    style: "color: {color}; font-size: 9px; flex-shrink: 0;",
                                    "●"
                                }
                                if !emoji.is_empty() {
                                    span { style: "font-size: 14px;", "{emoji}" }
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
