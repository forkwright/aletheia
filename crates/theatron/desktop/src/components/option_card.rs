//! Option card component for planning discussion choices.

use dioxus::prelude::*;

use crate::state::discussion::DiscussionOption;

const CARD_STYLE: &str = "\
    background: #1a1a2e; \
    border: 1px solid #2a2a3a; \
    border-radius: 8px; \
    padding: 14px 16px; \
    cursor: pointer; \
    transition: border-color 0.15s;\
";

const CARD_RECOMMENDED: &str = "\
    background: #1a1a2e; \
    border: 2px solid #4a9aff; \
    border-radius: 8px; \
    padding: 14px 16px; \
    cursor: pointer; \
    transition: border-color 0.15s;\
";

const CARD_SELECTED: &str = "\
    background: #1a2a3a; \
    border: 2px solid #22c55e; \
    border-radius: 8px; \
    padding: 14px 16px; \
    cursor: pointer; \
    transition: border-color 0.15s;\
";

const HEADER_ROW: &str = "\
    display: flex; \
    align-items: center; \
    gap: 8px; \
    margin-bottom: 6px;\
";

const TITLE_STYLE: &str = "\
    font-size: 14px; \
    font-weight: 600; \
    color: #e0e0e0;\
";

const BADGE_RECOMMENDED: &str = "\
    display: inline-block; \
    font-size: 10px; \
    font-weight: 600; \
    padding: 2px 6px; \
    border-radius: 8px; \
    background: #1a2a4a; \
    color: #4a9aff; \
    text-transform: uppercase; \
    letter-spacing: 0.3px;\
";

const BADGE_SELECTED: &str = "\
    display: inline-block; \
    font-size: 10px; \
    font-weight: 600; \
    padding: 2px 6px; \
    border-radius: 8px; \
    background: #0f2a0f; \
    color: #22c55e; \
    text-transform: uppercase; \
    letter-spacing: 0.3px;\
";

const DESCRIPTION_STYLE: &str = "\
    font-size: 13px; \
    color: #aaa; \
    margin-bottom: 8px;\
";

const RATIONALE_STYLE: &str = "\
    font-size: 12px; \
    color: #999; \
    font-style: italic; \
    margin-bottom: 8px;\
";

const TRADE_OFF_SECTION: &str = "\
    display: flex; \
    gap: 16px; \
    font-size: 12px;\
";

const PRO_ITEM: &str = "color: #22c55e; padding: 2px 0;";

const CON_ITEM: &str = "color: #ef4444; padding: 2px 0;";

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
                            div { style: "color: #22c55e; font-weight: 600; margin-bottom: 4px;", "Pros" }
                            for (i, pro) in option.pros.iter().enumerate() {
                                div { key: "{i}", style: "{PRO_ITEM}", "+ {pro}" }
                            }
                        }
                    }

                    if !option.cons.is_empty() {
                        div {
                            style: "flex: 1;",
                            div { style: "color: #ef4444; font-weight: 600; margin-bottom: 4px;", "Cons" }
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

/// Determine the card border style string based on state.
#[must_use]
pub(crate) fn card_style_for(recommended: bool, selected: bool) -> &'static str {
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
