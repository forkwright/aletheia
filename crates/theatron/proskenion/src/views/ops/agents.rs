//! Agent status cards grid with live SSE updates.

use dioxus::prelude::*;
use skeue::EmptyState;

use crate::state::ops::{AgentCapabilities, AgentCardData, AgentStatusStore};

const GRID_STYLE: &str = "\
    display: flex; \
    flex-wrap: wrap; \
    gap: var(--space-3);\
";

const CARD_STYLE: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-4) var(--space-5); \
    min-width: 200px; \
    flex: 1; \
    max-width: 320px;\
";

const CARD_HEADER: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    margin-bottom: var(--space-3);\
";

const CARD_NAME: &str = "\
    font-size: var(--text-md); \
    font-weight: var(--weight-bold); \
    color: var(--text-primary);\
";

const CARD_ROW: &str = "\
    display: flex; \
    justify-content: space-between; \
    align-items: center; \
    padding: var(--space-1) 0; \
    font-size: var(--text-xs);\
";

const CARD_LABEL: &str = "\
    color: var(--text-secondary);\
";

const CARD_VALUE: &str = "\
    color: var(--text-primary);\
";

const DOT_BASE: &str = "\
    width: 10px; \
    height: 10px; \
    border-radius: 50%; \
    flex-shrink: 0; \
    margin-left: auto;\
";

const CARD_DETAILS: &str = "\
    margin-top: var(--space-2); \
    border-top: 1px solid var(--border); \
    padding-top: var(--space-2);\
";

const CARD_SUMMARY: &str = "\
    cursor: pointer; \
    font-size: var(--text-xs); \
    color: var(--text-secondary);\
";

/// Render a token count with thousands separators.
///
/// WHY: context windows are six- and seven-digit values; unseparated digits
/// are the reported failure mode operators hit when comparing agents.
fn thousands(value: u32) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    let first = digits.len() % 3;
    for (idx, ch) in digits.chars().enumerate() {
        if idx > 0 && idx % 3 == first {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn render_capabilities(caps: &AgentCapabilities) -> Element {
    let context_window = thousands(caps.context_window);
    let max_output_tokens = thousands(caps.max_output_tokens);
    let thinking_budget = thousands(caps.thinking_budget);
    let thinking_enabled = if caps.thinking_enabled {
        "enabled"
    } else {
        "disabled"
    };

    rsx! {
        details {
            style: "{CARD_DETAILS}",
            summary { style: "{CARD_SUMMARY}", "Capabilities" }

            div {
                style: "{CARD_ROW}",
                span { style: "{CARD_LABEL}", "Context window" }
                span { style: "{CARD_VALUE}", "{context_window}" }
            }

            div {
                style: "{CARD_ROW}",
                span { style: "{CARD_LABEL}", "Max output tokens" }
                span { style: "{CARD_VALUE}", "{max_output_tokens}" }
            }

            div {
                style: "{CARD_ROW}",
                span { style: "{CARD_LABEL}", "Extended thinking" }
                span { style: "{CARD_VALUE}", "{thinking_enabled}" }
            }

            div {
                style: "{CARD_ROW}",
                span { style: "{CARD_LABEL}", "Thinking budget" }
                span { style: "{CARD_VALUE}", "{thinking_budget}" }
            }

            div {
                style: "{CARD_ROW}",
                span { style: "{CARD_LABEL}", "Max tool iterations" }
                span { style: "{CARD_VALUE}", "{caps.max_tool_iterations}" }
            }
        }
    }
}

#[component]
pub(crate) fn AgentCards(store: Signal<AgentStatusStore>) -> Element {
    let cards = store.read();
    let ordered = cards.ordered();

    if ordered.is_empty() {
        return rsx! {
            EmptyState { title: "No agents registered".to_string() }
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
        "var(--accent)"
    } else {
        "var(--text-muted)"
    };
    let conn_color = if card.connected {
        "var(--status-success)"
    } else {
        "var(--status-error)"
    };
    let conn_label = if card.connected {
        "connected"
    } else {
        "disconnected"
    };
    let last_activity = card.last_activity.as_deref().unwrap_or("\u{2014}");
    let dot_style = format!("{DOT_BASE} background: {dot_color};");
    let health_style = format!("color: {dot_color};");
    let turn_style = format!("color: {turn_color}; font-weight: var(--weight-bold);");
    let conn_style = format!("color: {conn_color};");

    rsx! {
        div {
            key: "{card.id}",
            style: "{CARD_STYLE}",

            div {
                style: "{CARD_HEADER}",
                if let Some(ref emoji) = card.emoji {
                    span { style: "font-size: var(--text-lg);", "{emoji}" }
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
                span { style: "color: var(--text-muted);", "{last_activity}" }
            }

            div {
                style: "{CARD_ROW}",
                span { style: "{CARD_LABEL}", "Connection" }
                span { style: "{conn_style}", "{conn_label}" }
            }

            if let Some(ref caps) = card.capabilities {
                {render_capabilities(caps)}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_groups_from_the_right() {
        assert_eq!(thousands(0), "0", "zero is ungrouped");
        assert_eq!(thousands(25), "25", "two digits are ungrouped");
        assert_eq!(thousands(999), "999", "three digits are ungrouped");
        assert_eq!(thousands(1_000), "1,000", "four digits take one separator");
        assert_eq!(thousands(64_000), "64,000", "five digits group correctly");
        assert_eq!(thousands(200_000), "200,000", "six digits group correctly");
        assert_eq!(thousands(1_048_576), "1,048,576", "seven digits take two");
    }
}
