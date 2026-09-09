//! Discussion panel: gray-area questions and read-only option review.

use dioxus::prelude::*;

use crate::state::discussion::{
    Discussion, DiscussionOption, DiscussionPriority, DiscussionStatus, DiscussionStore,
};

// WHY(#7224): answering and reopening a discussion used to POST to pylon via
// `project_discussion_answer_url`/`project_discussion_reopen_url`, but
// neither route has ever had a pylon handler and dianoia has no persisted,
// mutable discussion entity to back one -- see the same-numbered note on
// `skene::api::routes::planning`. The answer/reopen actions (option
// selection, free-text entry, submit, undo) were deleted; this panel is now
// a read-only view of each discussion's question, options, and any recorded
// answer.

/// Fetch state for discussions, with a 404 variant for unavailable endpoints.
#[derive(Debug, Clone)]
#[expect(
    dead_code,
    reason = "discussion routes are pending B23 backend work (#4482)"
)]
enum DiscussionFetchState {
    Loading,
    Loaded(Vec<Discussion>),
    NotAvailable,
    Error(String),
}

const CONTAINER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    height: 100%; \
    padding: var(--space-4); \
    gap: var(--space-3); \
    overflow-y: auto;\
";

const HEADER_ROW: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between;\
";

const CARD_BASE: &str = "\
    border-radius: var(--radius-md); \
    border: 1px solid; \
    padding: var(--space-4) var(--space-4);\
";

const QUESTION_STYLE: &str = "\
    font-size: var(--text-md); \
    font-weight: var(--weight-semibold); \
    color: var(--text-primary); \
    margin-bottom: var(--space-2);\
";

const CONTEXT_STYLE: &str = "\
    font-size: var(--text-sm); \
    color: var(--text-secondary); \
    margin-bottom: var(--space-3);\
";

const BADGE_BASE: &str = "\
    display: inline-block; \
    font-size: var(--text-xs); \
    font-weight: var(--weight-semibold); \
    padding: var(--space-1) var(--space-2); \
    border-radius: var(--radius-lg); \
    text-transform: uppercase; \
    letter-spacing: 0.3px; \
    margin-left: var(--space-2);\
";

const OPTIONS_GRID: &str = "\
    display: flex; \
    flex-direction: column; \
    gap: var(--space-2); \
    margin-top: var(--space-2);\
";

const OPTION_CARD: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-4) var(--space-4);\
";

const OPTION_CARD_RECOMMENDED: &str = "\
    background: var(--bg-surface); \
    border: 2px solid var(--status-info); \
    border-radius: var(--radius-md); \
    padding: var(--space-4) var(--space-4);\
";

const OPTION_HEADER_ROW: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    margin-bottom: var(--space-2);\
";

const OPTION_TITLE_STYLE: &str = "\
    font-size: var(--text-base); \
    font-weight: var(--weight-semibold); \
    color: var(--text-primary);\
";

const OPTION_BADGE_RECOMMENDED: &str = "\
    display: inline-block; \
    font-size: var(--text-xs); \
    font-weight: var(--weight-semibold); \
    padding: var(--space-1) var(--space-2); \
    border-radius: var(--radius-md); \
    background: var(--status-info-bg); \
    color: var(--status-info); \
    text-transform: uppercase; \
    letter-spacing: 0.3px;\
";

const OPTION_DESCRIPTION_STYLE: &str = "\
    font-size: var(--text-sm); \
    color: var(--text-secondary); \
    margin-bottom: var(--space-2);\
";

const OPTION_RATIONALE_STYLE: &str = "\
    font-size: var(--text-xs); \
    color: var(--text-secondary); \
    font-style: italic; \
    margin-bottom: var(--space-2);\
";

const OPTION_TRADE_OFF_SECTION: &str = "\
    display: flex; \
    gap: var(--space-4); \
    font-size: var(--text-xs);\
";

const OPTION_PRO_ITEM: &str = "color: var(--status-success); padding: var(--space-1) 0;";

const OPTION_CON_ITEM: &str = "color: var(--status-error); padding: var(--space-1) 0;";

const ANSWER_SUMMARY: &str = "\
    font-size: var(--text-sm); \
    color: var(--status-success); \
    padding: var(--space-2) var(--space-3); \
    background: var(--status-success-bg); \
    border: 1px solid var(--status-success); \
    border-radius: var(--radius-sm); \
    margin-top: var(--space-2);\
";

const PLACEHOLDER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    align-items: center; \
    justify-content: center; \
    flex: 1; \
    gap: var(--space-3); \
    color: var(--text-muted);\
";

/// Discussion panel listing all discussions for a project.
#[component]
pub(crate) fn DiscussionView(project_id: String) -> Element {
    let _ = &project_id;
    let fetch_state = use_signal(|| DiscussionFetchState::NotAvailable);

    rsx! {
        div {
            style: "{CONTAINER_STYLE}",
            role: "region",
            "aria-label": "Discussions",
            div {
                style: "{HEADER_ROW}",
                h3 { style: "font-size: var(--text-md); margin: 0; color: var(--text-primary);", "Discussions" }
            }

            match &*fetch_state.read() {
                DiscussionFetchState::Loading => rsx! {
                    div {
                        style: "display: flex; align-items: center; justify-content: center; flex: 1; color: var(--text-secondary);",
                        "Loading discussions..."
                    }
                },
                DiscussionFetchState::Error(err) => rsx! {
                    div {
                        style: "display: flex; align-items: center; justify-content: center; flex: 1; color: var(--status-error);",
                        "Error: {err}"
                    }
                },
                DiscussionFetchState::NotAvailable => rsx! {
                    div {
                        style: "{PLACEHOLDER_STYLE}",
                        div { style: "font-size: var(--text-3xl);", "[?]" }
                        div { style: "font-size: var(--text-md);", "Discussions not available" }
                        div { style: "font-size: var(--text-sm); max-width: 400px; text-align: center;",
                            "The discussions API is not available on this pylon instance."
                        }
                    }
                },
                DiscussionFetchState::Loaded(discussions) => {
                    if discussions.is_empty() {
                        rsx! {
                            div {
                                style: "{PLACEHOLDER_STYLE}",
                                div { style: "font-size: var(--text-md);", "No discussions" }
                                div { style: "font-size: var(--text-sm);",
                                    "Gray-area questions will appear here when agents need human input."
                                }
                            }
                        }
                    } else {
                        let store = DiscussionStore { discussions: discussions.clone() };
                        let sorted = store.sorted();
                        rsx! {
                            for disc in sorted {
                                DiscussionCard {
                                    key: "{disc.id}",
                                    discussion: disc.clone(),
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// A single, read-only discussion card: question, options, and any recorded answer.
///
/// No answer/reopen actions -- pylon has never implemented the discussion
/// answer/reopen endpoints.
#[component]
fn DiscussionCard(discussion: Discussion) -> Element {
    let is_open = discussion.status == DiscussionStatus::Open;
    let is_answered = discussion.status == DiscussionStatus::Answered;

    let (card_bg, card_border) = discussion_card_colors(discussion.priority, discussion.status);
    let card_style = format!("{CARD_BASE} background: {card_bg}; border-color: {card_border};");

    rsx! {
        div {
            style: "{card_style}",
            role: "group",
            "aria-label": "{discussion.question}",

            div {
                style: "display: flex; align-items: flex-start; justify-content: space-between; margin-bottom: var(--space-2);",
                span { style: "{QUESTION_STYLE}", "{discussion.question}" }
                div {
                    style: "display: flex; align-items: center; flex-shrink: 0;",
                    span { style: "{status_badge_style(discussion.status)}", "{status_label(discussion.status)}" }
                    span { style: "{priority_badge_style(discussion.priority)}", "{priority_label(discussion.priority)}" }
                }
            }

            if !discussion.context.is_empty() {
                div { style: "{CONTEXT_STYLE}", "{discussion.context}" }
            }

            if is_answered {
                if let Some(summary) = DiscussionStore::answer_summary(&discussion) {
                    div { style: "{ANSWER_SUMMARY}", "Answer: {summary}" }
                }
            }

            if is_open && !discussion.options.is_empty() {
                div {
                    style: "{OPTIONS_GRID}",
                    "aria-label": "Proposed options",
                    for opt in &discussion.options {
                        ReadOnlyOptionCard {
                            key: "{opt.id}",
                            option: opt.clone(),
                        }
                    }
                }
            }
        }
    }
}

/// Read-only rendering of a discussion option: title, description,
/// rationale, and trade-offs, with a "recommended" badge when applicable.
#[component]
fn ReadOnlyOptionCard(option: DiscussionOption) -> Element {
    let card_style = if option.recommended {
        OPTION_CARD_RECOMMENDED
    } else {
        OPTION_CARD
    };

    rsx! {
        div {
            style: "{card_style}",
            "aria-label": "{option.title}",

            div {
                style: "{OPTION_HEADER_ROW}",
                span { style: "{OPTION_TITLE_STYLE}", "{option.title}" }
                if option.recommended {
                    span { style: "{OPTION_BADGE_RECOMMENDED}", "recommended" }
                }
            }

            if !option.description.is_empty() {
                div { style: "{OPTION_DESCRIPTION_STYLE}", "{option.description}" }
            }

            if !option.rationale.is_empty() {
                div { style: "{OPTION_RATIONALE_STYLE}", "{option.rationale}" }
            }

            if !option.pros.is_empty() || !option.cons.is_empty() {
                div {
                    style: "{OPTION_TRADE_OFF_SECTION}",

                    if !option.pros.is_empty() {
                        div {
                            style: "flex: 1;",
                            div { style: "color: var(--status-success); font-weight: var(--weight-semibold); margin-bottom: var(--space-1);", "Pros" }
                            for (i, pro) in option.pros.iter().enumerate() {
                                div { key: "{i}", style: "{OPTION_PRO_ITEM}", "+ {pro}" }
                            }
                        }
                    }

                    if !option.cons.is_empty() {
                        div {
                            style: "flex: 1;",
                            div { style: "color: var(--status-error); font-weight: var(--weight-semibold); margin-bottom: var(--space-1);", "Cons" }
                            for (i, con) in option.cons.iter().enumerate() {
                                div { key: "{i}", style: "{OPTION_CON_ITEM}", "- {con}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn discussion_card_colors(
    priority: DiscussionPriority,
    status: DiscussionStatus,
) -> (&'static str, &'static str) {
    if status == DiscussionStatus::Answered {
        return ("var(--status-success-bg)", "var(--status-success)");
    }
    match priority {
        DiscussionPriority::Blocking => ("var(--status-error-bg)", "var(--status-error)"),
        DiscussionPriority::Important => ("var(--status-warning-bg)", "var(--status-warning)"),
        DiscussionPriority::NiceToHave => ("var(--bg-surface)", "var(--border)"),
    }
}

fn status_badge_style(status: DiscussionStatus) -> String {
    let (bg, color) = match status {
        DiscussionStatus::Open => ("var(--bg-surface-dim)", "var(--accent)"),
        DiscussionStatus::Answered => ("var(--status-success-bg)", "var(--status-success)"),
        DiscussionStatus::Deferred => ("var(--border)", "var(--text-secondary)"),
    };
    format!("{BADGE_BASE} background: {bg}; color: {color};")
}

fn status_label(status: DiscussionStatus) -> &'static str {
    match status {
        DiscussionStatus::Open => "Open",
        DiscussionStatus::Answered => "Answered",
        DiscussionStatus::Deferred => "Deferred",
    }
}

fn priority_badge_style(priority: DiscussionPriority) -> String {
    let (bg, color) = match priority {
        DiscussionPriority::Blocking => ("var(--status-error-bg)", "var(--status-error)"),
        DiscussionPriority::Important => ("var(--status-warning-bg)", "var(--status-warning)"),
        DiscussionPriority::NiceToHave => ("var(--border)", "var(--text-secondary)"),
    };
    format!("{BADGE_BASE} background: {bg}; color: {color};")
}

fn priority_label(priority: DiscussionPriority) -> &'static str {
    match priority {
        DiscussionPriority::Blocking => "Blocking",
        DiscussionPriority::Important => "Important",
        DiscussionPriority::NiceToHave => "Nice to Have",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discussion_card_colors_blocking_has_red_border() {
        let (_, border) =
            discussion_card_colors(DiscussionPriority::Blocking, DiscussionStatus::Open);
        assert_eq!(
            border, "var(--status-error)",
            "blocking should have red border"
        );
    }

    #[test]
    fn discussion_card_colors_answered_overrides_priority() {
        let (_, border) =
            discussion_card_colors(DiscussionPriority::Blocking, DiscussionStatus::Answered);
        assert_eq!(
            border, "var(--status-success)",
            "answered status should override blocking priority color"
        );
    }

    #[test]
    fn status_labels_are_distinct() {
        let labels = [
            status_label(DiscussionStatus::Open),
            status_label(DiscussionStatus::Answered),
            status_label(DiscussionStatus::Deferred),
        ];
        let unique: std::collections::HashSet<_> = labels.iter().collect();
        assert_eq!(
            unique.len(),
            labels.len(),
            "all status labels must be distinct"
        );
    }

    #[test]
    fn priority_labels_are_distinct() {
        let labels = [
            priority_label(DiscussionPriority::Blocking),
            priority_label(DiscussionPriority::Important),
            priority_label(DiscussionPriority::NiceToHave),
        ];
        let unique: std::collections::HashSet<_> = labels.iter().collect();
        assert_eq!(
            unique.len(),
            labels.len(),
            "all priority labels must be distinct"
        );
    }
}
