//! Expanded discussion detail view with full context, options, and history.

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::components::option_card::OptionCard;
use crate::state::connection::ConnectionConfig;
use crate::state::discussion::{Discussion, DiscussionStatus, DiscussionStore};

/// Fetch state for a single discussion detail.
#[derive(Debug, Clone)]
enum DetailFetchState {
    Loading,
    Loaded(Discussion),
    Error(String),
}

const CONTAINER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    height: 100%; \
    padding: 16px; \
    gap: 12px; \
    overflow-y: auto;\
";

const BACK_BTN: &str = "\
    background: transparent; \
    color: #4a9aff; \
    border: none; \
    font-size: 13px; \
    cursor: pointer; \
    padding: 0; \
    margin-bottom: 8px;\
";

const QUESTION_STYLE: &str = "\
    font-size: 18px; \
    font-weight: 600; \
    color: #e0e0e0; \
    margin-bottom: 8px;\
";

const CONTEXT_STYLE: &str = "\
    font-size: 14px; \
    color: #aaa; \
    padding: 10px 14px; \
    background: #0f0f1a; \
    border: 1px solid #2a2a3a; \
    border-radius: 6px; \
    margin-bottom: 12px;\
";

const SECTION_LABEL: &str = "\
    font-size: 12px; \
    font-weight: 600; \
    color: #666; \
    text-transform: uppercase; \
    letter-spacing: 0.5px; \
    margin: 12px 0 6px;\
";

const HISTORY_ENTRY: &str = "\
    display: flex; \
    align-items: flex-start; \
    gap: 8px; \
    padding: 6px 10px; \
    border-left: 2px solid #2a2a3a; \
    margin-bottom: 4px;\
";

const HISTORY_ACTION: &str = "\
    font-size: 12px; \
    font-weight: 600; \
    color: #c0c0e0;\
";

const HISTORY_META: &str = "\
    font-size: 11px; \
    color: #666;\
";

const HISTORY_DETAIL: &str = "\
    font-size: 12px; \
    color: #999; \
    font-style: italic;\
";

const OPTIONS_GRID: &str = "\
    display: flex; \
    flex-direction: column; \
    gap: 8px;\
";

const ANSWER_SUMMARY: &str = "\
    font-size: 14px; \
    color: #22c55e; \
    padding: 8px 12px; \
    background: #0f1a0f; \
    border: 1px solid #1a3a1a; \
    border-radius: 6px;\
";

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
    let mut fetch_state = use_signal(|| DetailFetchState::Loading);
    let fetch_trigger = use_signal(|| 0u32);

    let project_id_effect = project_id.clone();
    let discussion_id_effect = discussion_id.clone();

    use_effect(move || {
        let _trigger = *fetch_trigger.read();
        let cfg = config.read().clone();
        let pid = project_id_effect.clone();
        let did = discussion_id_effect.clone();
        fetch_state.set(DetailFetchState::Loading);

        spawn(async move {
            let client = authenticated_client(&cfg);
            let url = format!(
                "{}/api/planning/projects/{pid}/discussions/{did}",
                cfg.server_url.trim_end_matches('/')
            );

            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => match resp.json::<Discussion>().await {
                    Ok(disc) => fetch_state.set(DetailFetchState::Loaded(disc)),
                    Err(e) => {
                        fetch_state.set(DetailFetchState::Error(format!("parse error: {e}")));
                    }
                },
                Ok(resp) => {
                    let status = resp.status();
                    fetch_state.set(DetailFetchState::Error(format!("server returned {status}")));
                }
                Err(e) => {
                    fetch_state.set(DetailFetchState::Error(format!("connection error: {e}")));
                }
            }
        });
    });

    rsx! {
        div {
            style: "{CONTAINER_STYLE}",

            button {
                style: "{BACK_BTN}",
                onclick: move |_| on_back.call(()),
                "<- Back to discussions"
            }

            match &*fetch_state.read() {
                DetailFetchState::Loading => rsx! {
                    div {
                        style: "display: flex; align-items: center; justify-content: center; flex: 1; color: #888;",
                        "Loading discussion..."
                    }
                },
                DetailFetchState::Error(err) => rsx! {
                    div {
                        style: "display: flex; align-items: center; justify-content: center; flex: 1; color: #ef4444;",
                        "Error: {err}"
                    }
                },
                DetailFetchState::Loaded(disc) => rsx! {
                    // Question
                    div { style: "{QUESTION_STYLE}", "{disc.question}" }

                    // Context
                    if !disc.context.is_empty() {
                        div { style: "{CONTEXT_STYLE}", "{disc.context}" }
                    }

                    // Current answer (if answered)
                    if disc.status == DiscussionStatus::Answered {
                        if let Some(summary) = DiscussionStore::answer_summary(disc) {
                            div {
                                style: "{SECTION_LABEL}",
                                "Current Answer"
                            }
                            div { style: "{ANSWER_SUMMARY}", "{summary}" }
                        }
                    }

                    // All options
                    div { style: "{SECTION_LABEL}", "Options" }
                    div {
                        style: "{OPTIONS_GRID}",
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
                        div { style: "{SECTION_LABEL}", "History" }
                        for (i, entry) in disc.history.iter().enumerate() {
                            div {
                                key: "{i}",
                                style: "{HISTORY_ENTRY}",
                                div {
                                    span { style: "{HISTORY_ACTION}", "{entry.action}" }
                                    span { style: "{HISTORY_META}", " by {entry.actor} at {entry.timestamp}" }
                                    if !entry.detail.is_empty() {
                                        div { style: "{HISTORY_DETAIL}", "\"{entry.detail}\"" }
                                    }
                                }
                            }
                        }
                    }
                },
            }
        }
    }
}
