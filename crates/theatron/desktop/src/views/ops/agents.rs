//! Agent status cards grid with live SSE updates.

use dioxus::prelude::*;

use crate::state::ops::{AgentCardData, AgentStatusStore};

const GRID_STYLE: &str = "\
    display: flex; \
    flex-wrap: wrap; \
    gap: 12px;\
";

const CARD_STYLE: &str = "\
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 8px; \
    padding: 16px 20px; \
    min-width: 200px; \
    flex: 1; \
    max-width: 320px;\
";

const CARD_HEADER: &str = "\
    display: flex; \
    align-items: center; \
    gap: 8px; \
    margin-bottom: 12px;\
";

const CARD_NAME: &str = "\
    font-size: 15px; \
    font-weight: bold; \
    color: #e0e0e0;\
";

const CARD_ROW: &str = "\
    display: flex; \
    justify-content: space-between; \
    align-items: center; \
    padding: 4px 0; \
    font-size: 12px;\
";

const CARD_LABEL: &str = "\
    color: #888;\
";

const CARD_VALUE: &str = "\
    color: #e0e0e0;\
";

const DOT_BASE: &str = "\
    width: 10px; \
    height: 10px; \
    border-radius: 50%; \
    flex-shrink: 0; \
    margin-left: auto;\
";

const EMPTY_STATE: &str = "\
    color: #555; \
    font-size: 13px; \
    padding: 12px 0;\
";

#[component]
pub(crate) fn AgentCards(store: Signal<AgentStatusStore>) -> Element {
    let cards = store.read();
    let ordered = cards.ordered();

    if ordered.is_empty() {
        return rsx! {
            div { style: "{EMPTY_STATE}", "No agents registered" }
        };
    }

    rsx! {
        div {
            style: "{GRID_STYLE}",
            for card in ordered {
                {render_card(card)}
            }
        }
    }
}

fn render_card(card: &AgentCardData) -> Element {
    let dot_color = card.health.dot_color();
    let health_label = card.health.label();
    let turn_color = if card.active_turns > 0 {
        "#4a4aff"
    } else {
        "#555"
    };
    let conn_color = if card.connected { "#22c55e" } else { "#ef4444" };
    let conn_label = if card.connected {
        "connected"
    } else {
        "disconnected"
    };
    let last_activity = card.last_activity.as_deref().unwrap_or("\u{2014}");
    let dot_style = format!("{DOT_BASE} background: {dot_color};");
    let health_style = format!("color: {dot_color};");
    let turn_style = format!("color: {turn_color}; font-weight: bold;");
    let conn_style = format!("color: {conn_color};");

    rsx! {
        div {
            key: "{card.id}",
            style: "{CARD_STYLE}",

            div {
                style: "{CARD_HEADER}",
                if let Some(ref emoji) = card.emoji {
                    span { style: "font-size: 18px;", "{emoji}" }
                }
                span { style: "{CARD_NAME}", "{card.name}" }
                span {
                    style: "{dot_style}",
                    title: "{health_label}",
                }
            }

            div {
                style: "{CARD_ROW}",
                span { style: "{CARD_LABEL}", "Health" }
                span { style: "{health_style}", "{health_label}" }
            }

            div {
                style: "{CARD_ROW}",
                span { style: "{CARD_LABEL}", "Model" }
                span { style: "{CARD_VALUE}", "{card.model}" }
            }

            div {
                style: "{CARD_ROW}",
                span { style: "{CARD_LABEL}", "Active turns" }
                span { style: "{turn_style}", "{card.active_turns}" }
            }

            div {
                style: "{CARD_ROW}",
                span { style: "{CARD_LABEL}", "Last activity" }
                span { style: "color: #666;", "{last_activity}" }
            }

            div {
                style: "{CARD_ROW}",
                span { style: "{CARD_LABEL}", "Connection" }
                span { style: "{conn_style}", "{conn_label}" }
            }
        }
    }
}
