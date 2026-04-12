//! Expanded discussion detail view with full context, options, and history.

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::components::option_card::OptionCard;
use crate::state::connection::ConnectionConfig;
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
    let config: Signal<ConnectionConfig> = use_context();
    let mut discussion = use_signal(|| None::<Discussion>);
    let mut error = use_signal(|| None::<String>);
    let fetch_trigger = use_signal(|| 0u32);

    let project_id_effect = project_id.clone();
    let discussion_id_effect = discussion_id.clone();

    use_effect(move || {
        let _trigger = *fetch_trigger.read();
        let cfg = config.read().clone();
        let pid = project_id_effect.clone();
        let did = discussion_id_effect.clone();
        discussion.set(None);
        error.set(None);

        spawn(async move {
            let client = authenticated_client(&cfg);
            let url = format!(
                "{}/api/planning/projects/{pid}/discussions/{did}",
                cfg.server_url.trim_end_matches('/')
            );

            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => match resp.json::<Discussion>().await {
                    Ok(disc) => discussion.set(Some(disc)),
                    Err(e) => {
                        error.set(Some(format!("parse error: {e}")));
                    }
                },
                Ok(resp) => {
                    let status = resp.status();
                    error.set(Some(format!("server returned {status}")));
                }
                Err(e) => {
                    error.set(Some(format!("connection error: {e}")));
                }
            }
        });
    });

    rsx! {
        div {
            style: "display: flex; flex-direction: column; height: 100%; padding: var(--space-4); gap: var(--space-3); overflow-y: auto;",

            button {
                style: "background: transparent; color: var(--accent); border: none; font-size: var(--text-sm); cursor: pointer; padding: 0; margin-bottom: var(--space-2); transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick);",
                onclick: move |_| on_back.call(()),
                "<- Back to discussions"
            }

            if let Some(err) = error.read().as_ref() {
                div {
                    style: "display: flex; align-items: center; justify-content: center; flex: 1; color: var(--status-error);",
                    "Error: {err}"
                }
            } else if let Some(disc) = discussion.read().as_ref() {
                // Question
                div { style: "font-size: var(--text-lg); font-weight: var(--weight-semibold); color: var(--text-primary); margin-bottom: var(--space-2);", "{disc.question}" }

                // Context
                if !disc.context.is_empty() {
                    div { style: "font-size: var(--text-base); color: var(--text-secondary); padding: var(--space-3) var(--space-4); background: var(--bg-surface-dim); border: 1px solid var(--border); border-radius: var(--radius-md); margin-bottom: var(--space-3);", "{disc.context}" }
                }

                // Current answer (if answered)
                if disc.status == DiscussionStatus::Answered {
                    if let Some(summary) = DiscussionStore::answer_summary(disc) {
                        div {
                            style: "font-size: var(--text-xs); font-weight: var(--weight-semibold); color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.5px; margin: var(--space-3) 0 var(--space-2);",
                            "Current Answer"
                        }
                        div { style: "font-size: var(--text-base); color: var(--status-success); padding: var(--space-2) var(--space-3); background: #0f1a0f; border: 1px solid #1a3a1a; border-radius: var(--radius-md);", "{summary}" }
                    }
                }

                // All options
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

                // Discussion history
                if !disc.history.is_empty() {
                    div { style: "font-size: var(--text-xs); font-weight: var(--weight-semibold); color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.5px; margin: var(--space-3) 0 var(--space-2);", "History" }
                    for (i, entry) in disc.history.iter().enumerate() {
                        div {
                            key: "{i}",
                            style: "display: flex; align-items: flex-start; gap: var(--space-2); padding: var(--space-2) var(--space-3); border-left: 2px solid var(--border); margin-bottom: var(--space-1);",
                            div {
                                span { style: "font-size: var(--text-xs); font-weight: var(--weight-semibold); color: #c0c0e0;", "{entry.action}" }
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
