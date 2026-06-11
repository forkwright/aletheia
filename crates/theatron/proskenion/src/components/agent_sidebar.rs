//! Agent presence sidebar with status indicators.
//!
//! Reads `Signal<AgentStore>` and `Signal<EventState>` from context. Clicking
//! an agent calls `AgentStore::set_active`, switching the active chat session.

use dioxus::prelude::*;
use skene::id::NousId;

use crate::state::agents::{AgentStatus, AgentStore};
use crate::state::events::EventState;

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
                class: "agent-roster-rail",
                span { class: "agent-roster-rail-label", "NOUS" }
                if agent_count > 0 {
                    span { class: "agent-roster-rail-count", "● {agent_count}" }
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
            class: "agent-roster-section",
            h2 { class: "agent-roster-label", "Nous" }
            if agents.is_empty() {
                div { class: "agent-roster-empty", "No nous" }
            } else {
                for (id , name , emoji , status , is_active) in agents {
                    {
                        let id_clone = id.clone();
                        let color = status.dot_color();
                        let row_class = if is_active { "agent-row active" } else { "agent-row" };
                        let dot_class = if matches!(status, AgentStatus::Active) {
                            "agent-dot status-dot-active"
                        } else {
                            "agent-dot"
                        };
                        let status_label = status.label();
                        rsx! {
                            div {
                                key: "{id}",
                                class: "{row_class}",
                                title: "Status: {status_label}",
                                onclick: move |_| {
                                    store.write().set_active(&id_clone);
                                },
                                span {
                                    class: "{dot_class}",
                                    style: "color: {color};",
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
