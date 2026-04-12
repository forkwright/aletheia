//! Option card component for planning discussion choices.

use dioxus::prelude::*;

use crate::state::discussion::DiscussionOption;

const CARD_STYLE: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-4) var(--space-4); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const CARD_RECOMMENDED: &str = "\
    background: var(--bg-surface); \
    border: 2px solid #4a9aff; \
    border-radius: var(--radius-md); \
    padding: var(--space-4) var(--space-4); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const CARD_SELECTED: &str = "\
    background: #1a2a3a; \
    border: 2px solid var(--status-success); \
    border-radius: var(--radius-md); \
    padding: var(--space-4) var(--space-4); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const HEADER_ROW: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    margin-bottom: var(--space-2);\
";

const TITLE_STYLE: &str = "\
    font-size: var(--text-base); \
    font-weight: var(--weight-semibold); \
    color: var(--text-primary);\
";

const BADGE_RECOMMENDED: &str = "\
    display: inline-block; \
    font-size: var(--text-xs); \
    font-weight: var(--weight-semibold); \
    padding: var(--space-1) var(--space-2); \
    border-radius: var(--radius-md); \
    background: #1a2a4a; \
    color: #4a9aff; \
    text-transform: uppercase; \
    letter-spacing: 0.3px;\
";

const BADGE_SELECTED: &str = "\
    display: inline-block; \
    font-size: var(--text-xs); \
    font-weight: var(--weight-semibold); \
    padding: var(--space-1) var(--space-2); \
    border-radius: var(--radius-md); \
    background: #0f2a0f; \
    color: var(--status-success); \
    text-transform: uppercase; \
    letter-spacing: 0.3px;\
";

const DESCRIPTION_STYLE: &str = "\
    font-size: var(--text-sm); \
    color: var(--text-secondary); \
    margin-bottom: var(--space-2);\
";

const RATIONALE_STYLE: &str = "\
    font-size: var(--text-xs); \
    color: var(--text-secondary); \
    font-style: italic; \
    margin-bottom: var(--space-2);\
";

const TRADE_OFF_SECTION: &str = "\
    display: flex; \
    gap: var(--space-4); \
    font-size: var(--text-xs);\
";

const PRO_ITEM: &str = "color: var(--status-success); padding: var(--space-1) 0;";

const CON_ITEM: &str = "color: var(--status-error); padding: var(--space-1) 0;";

/// Option card for a single discussion choice.
///
/// Displays title, description, rationale, trade-offs, and recommendation badge.
/// Fires `on_select` when clicked.
#[component]
pub(crate) fn OptionCard(
    option: DiscussionOption,
    selected: bool,
    on_select: EventHandler<String>,
) -> Element {
    let card_style = if selected {
        CARD_SELECTED
    } else if option.recommended {
        CARD_RECOMMENDED
    } else {
        CARD_STYLE
    };

    let option_id = option.id.clone();

    rsx! {
        div {
            style: "{card_style}",
            onclick: move |_| on_select.call(option_id.clone()),

            // Header: title + badges
            div {
                style: "{HEADER_ROW}",
                span { style: "{TITLE_STYLE}", "{option.title}" }
                if option.recommended {
                    span { style: "{BADGE_RECOMMENDED}", "recommended" }
                }
                if selected {
                    span { style: "{BADGE_SELECTED}", "selected" }
                }
            }

            // Description
            if !option.description.is_empty() {
                div { style: "{DESCRIPTION_STYLE}", "{option.description}" }
            }

            // Rationale
            if !option.rationale.is_empty() {
                div { style: "{RATIONALE_STYLE}", "{option.rationale}" }
            }

            // Trade-offs
            if !option.pros.is_empty() || !option.cons.is_empty() {
                div {
                    style: "{TRADE_OFF_SECTION}",

                    if !option.pros.is_empty() {
                        div {
                            style: "flex: 1;",
                            div { style: "color: var(--status-success); font-weight: var(--weight-semibold); margin-bottom: var(--space-1);", "Pros" }
                            for (i, pro) in option.pros.iter().enumerate() {
                                div { key: "{i}", style: "{PRO_ITEM}", "+ {pro}" }
                            }
                        }
                    }

                    if !option.cons.is_empty() {
                        div {
                            style: "flex: 1;",
                            div { style: "color: var(--status-error); font-weight: var(--weight-semibold); margin-bottom: var(--space-1);", "Cons" }
                            for (i, con) in option.cons.iter().enumerate() {
                                div { key: "{i}", style: "{CON_ITEM}", "- {con}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Select the appropriate card CSS style based on recommended and selected flags.
///
/// Priority: selected > recommended > default.
#[cfg_attr(not(test), expect(dead_code, reason = "used in tests"))]
fn card_style_for(recommended: bool, selected: bool) -> &'static str {
    if selected {
        CARD_SELECTED
    } else if recommended {
        CARD_RECOMMENDED
    } else {
        CARD_STYLE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_style_selected_takes_priority() {
        assert_eq!(
            card_style_for(true, true),
            CARD_SELECTED,
            "selected overrides recommended"
        );
    }

    #[test]
    fn card_style_recommended_when_not_selected() {
        assert_eq!(card_style_for(true, false), CARD_RECOMMENDED);
    }

    #[test]
    fn card_style_default_when_neither() {
        assert_eq!(card_style_for(false, false), CARD_STYLE);
    }
}
