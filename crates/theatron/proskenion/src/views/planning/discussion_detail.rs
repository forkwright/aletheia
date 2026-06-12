//! Expanded discussion detail view with full context, options, and history.

use dioxus::prelude::*;

use crate::components::option_card::OptionCard;
use crate::state::discussion::{Discussion, DiscussionStatus, DiscussionStore};

/// Expanded discussion detail view.
///
/// Shows the full context, all options, discussion history, and the current answer.
#[component]
pub(crate) fn DiscussionDetailView(
    project_id: String,
    discussion_id: String,
    on_back: EventHandler<()>,
) -> Element {
    let _ = (&project_id, &discussion_id);
    let discussion = use_signal(|| None::<Discussion>);

    rsx! {
        div {
            style: "display: flex; flex-direction: column; height: 100%; padding: var(--space-4); gap: var(--space-3); overflow-y: auto;",

            button {
                style: "background: transparent; color: var(--accent); border: none; font-size: var(--text-sm); cursor: pointer; padding: 0; margin-bottom: var(--space-2); transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick);",
                onclick: move |_| on_back.call(()),
                "<- Back to discussions"
            }

            if discussion.read().is_none() {
                div {
                    style: "display: flex; align-items: center; justify-content: center; flex: 1; color: var(--text-secondary);",
                    "Discussion details not available"
                }
            } else if let Some(disc) = discussion.read().as_ref() {
                div { style: "font-size: var(--text-lg); font-weight: var(--weight-semibold); color: var(--text-primary); margin-bottom: var(--space-2);", "{disc.question}" }

                if !disc.context.is_empty() {
                    div { style: "font-size: var(--text-base); color: var(--text-secondary); padding: var(--space-3) var(--space-4); background: var(--bg-surface-dim); border: 1px solid var(--border); border-radius: var(--radius-md); margin-bottom: var(--space-3);", "{disc.context}" }
                }

                if disc.status == DiscussionStatus::Answered {
                    if let Some(summary) = DiscussionStore::answer_summary(disc) {
                        div {
                            style: "font-size: var(--text-xs); font-weight: var(--weight-semibold); color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.5px; margin: var(--space-3) 0 var(--space-2);",
                            "Current Answer"
                        }
                        div { style: "font-size: var(--text-base); color: var(--status-success); padding: var(--space-2) var(--space-3); background: var(--status-success-bg); border: 1px solid var(--status-success); border-radius: var(--radius-md);", "{summary}" }
                    }
                }

                div { style: "font-size: var(--text-xs); font-weight: var(--weight-semibold); color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.5px; margin: var(--space-3) 0 var(--space-2);", "Options" }
                div {
                    style: "display: flex; flex-direction: column; gap: var(--space-2);",
                    for opt in &disc.options {
                        OptionCard {
                            key: "{opt.id}",
                            option: opt.clone(),
                            selected: disc.selected_option_id.as_deref() == Some(opt.id.as_str()),
                            on_select: move |_: String| {
                                // NOTE: read-only in detail view; selection happens in the list view card.
                            },
                        }
                    }
                }

                if !disc.history.is_empty() {
                    div { style: "font-size: var(--text-xs); font-weight: var(--weight-semibold); color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.5px; margin: var(--space-3) 0 var(--space-2);", "History" }
                    for (i, entry) in disc.history.iter().enumerate() {
                        div {
                            key: "{i}",
                            style: "display: flex; align-items: flex-start; gap: var(--space-2); padding: var(--space-2) var(--space-3); border-left: 2px solid var(--border); margin-bottom: var(--space-1);",
                            div {
                                span { style: "font-size: var(--text-xs); font-weight: var(--weight-semibold); color: var(--text-primary);", "{entry.action}" }
                                span { style: "font-size: var(--text-xs); color: var(--text-muted);", " by {entry.actor} at {entry.timestamp}" }
                                if !entry.detail.is_empty() {
                                    div { style: "font-size: var(--text-xs); color: var(--text-secondary); font-style: italic;", "\"{entry.detail}\"" }
                                }
                            }
                        }
                    }
                }
            } else {
                div {
                    style: "display: flex; align-items: center; justify-content: center; flex: 1; color: var(--text-secondary);",
                    "Loading discussion..."
                }
            }
        }
    }
}
