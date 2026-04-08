//! Discussion panel: gray-area questions, option cards, and answer flow.

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::components::option_card::OptionCard;
use crate::state::connection::ConnectionConfig;
use crate::state::discussion::{
    Discussion, DiscussionAnswerRequest, DiscussionPriority, DiscussionStatus, DiscussionStore,
};

/// Fetch state for discussions, with a 404 variant for unavailable endpoints.
#[derive(Debug, Clone)]
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
    padding: 16px; \
    gap: 12px; \
    overflow-y: auto;\
";

const HEADER_ROW: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between;\
";

const REFRESH_BTN: &str = "\
    background: #2a2a4a; \
    color: #e0e0e0; \
    border: 1px solid #444; \
    border-radius: 6px; \
    padding: 4px 12px; \
    font-size: 12px; \
    cursor: pointer;\
";

const CARD_BASE: &str = "\
    border-radius: 8px; \
    border: 1px solid; \
    padding: 14px 18px;\
";

const QUESTION_STYLE: &str = "\
    font-size: 15px; \
    font-weight: 600; \
    color: #e0e0e0; \
    margin-bottom: 6px;\
";

const CONTEXT_STYLE: &str = "\
    font-size: 13px; \
    color: #999; \
    margin-bottom: 10px;\
";

const BADGE_BASE: &str = "\
    display: inline-block; \
    font-size: 10px; \
    font-weight: 600; \
    padding: 2px 8px; \
    border-radius: 10px; \
    text-transform: uppercase; \
    letter-spacing: 0.3px; \
    margin-left: 8px;\
";

const OPTIONS_GRID: &str = "\
    display: flex; \
    flex-direction: column; \
    gap: 8px; \
    margin-top: 8px;\
";

const FREE_TEXT_INPUT: &str = "\
    width: 100%; \
    background: #0f0f1a; \
    border: 1px solid #333; \
    border-radius: 4px; \
    padding: 8px 10px; \
    color: #e0e0e0; \
    font-size: 13px; \
    font-family: inherit; \
    resize: vertical; \
    min-height: 50px; \
    box-sizing: border-box;\
";

const SUBMIT_BTN: &str = "\
    background: #4a4aff; \
    color: white; \
    border: none; \
    border-radius: 6px; \
    padding: 6px 16px; \
    font-size: 13px; \
    font-weight: 600; \
    cursor: pointer;\
";

const SUBMIT_BTN_DISABLED: &str = "\
    background: #333; \
    color: #666; \
    border: none; \
    border-radius: 6px; \
    padding: 6px 16px; \
    font-size: 13px; \
    font-weight: 600; \
    cursor: not-allowed;\
";

const UNDO_BTN: &str = "\
    background: transparent; \
    color: #4a9aff; \
    border: 1px solid #4a9aff; \
    border-radius: 6px; \
    padding: 4px 12px; \
    font-size: 12px; \
    cursor: pointer;\
";

const ANSWER_SUMMARY: &str = "\
    font-size: 13px; \
    color: #22c55e; \
    padding: 6px 10px; \
    background: #0f1a0f; \
    border: 1px solid #1a3a1a; \
    border-radius: 4px; \
    margin-top: 8px;\
";

const PLACEHOLDER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    align-items: center; \
    justify-content: center; \
    flex: 1; \
    gap: 12px; \
    color: #555;\
";

const ERROR_STYLE: &str = "color: #ef4444; font-size: 12px; margin-top: 6px;";

/// Discussion panel listing all discussions for a project.
#[component]
pub(crate) fn DiscussionView(project_id: String) -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut fetch_state = use_signal(|| DiscussionFetchState::Loading);
    let mut fetch_trigger = use_signal(|| 0u32);

    let project_id_effect = project_id.clone();
    use_effect(move || {
        let _trigger = *fetch_trigger.read();
        let cfg = config.read().clone();
        let pid = project_id_effect.clone();
        fetch_state.set(DiscussionFetchState::Loading);

        spawn(async move {
            let client = authenticated_client(&cfg);
            let url = format!(
                "{}/api/planning/projects/{pid}/discussions",
                cfg.server_url.trim_end_matches('/')
            );

            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    match resp.json::<Vec<Discussion>>().await {
                        Ok(discussions) => {
                            fetch_state.set(DiscussionFetchState::Loaded(discussions));
                        }
                        Err(e) => {
                            fetch_state
                                .set(DiscussionFetchState::Error(format!("parse error: {e}")));
                        }
                    }
                }
                // WHY: 404 means discussions endpoint not available on this pylon version.
                Ok(resp) if resp.status().as_u16() == 404 => {
                    fetch_state.set(DiscussionFetchState::NotAvailable);
                }
                Ok(resp) => {
                    let status = resp.status();
                    fetch_state.set(DiscussionFetchState::Error(format!(
                        "server returned {status}"
                    )));
                }
                Err(e) => {
                    fetch_state.set(DiscussionFetchState::Error(format!(
                        "connection error: {e}"
                    )));
                }
            }
        });
    });

    rsx! {
        div {
            style: "{CONTAINER_STYLE}",
            div {
                style: "{HEADER_ROW}",
                h3 { style: "font-size: 16px; margin: 0; color: #e0e0e0;", "Discussions" }
                button {
                    style: "{REFRESH_BTN}",
                    onclick: move |_| fetch_trigger.set(fetch_trigger() + 1),
                    "Refresh"
                }
            }

            match &*fetch_state.read() {
                DiscussionFetchState::Loading => rsx! {
                    div {
                        style: "display: flex; align-items: center; justify-content: center; flex: 1; color: #888;",
                        "Loading discussions..."
                    }
                },
                DiscussionFetchState::Error(err) => rsx! {
                    div {
                        style: "display: flex; align-items: center; justify-content: center; flex: 1; color: #ef4444;",
                        "Error: {err}"
                    }
                },
                DiscussionFetchState::NotAvailable => rsx! {
                    div {
                        style: "{PLACEHOLDER_STYLE}",
                        div { style: "font-size: 48px;", "[?]" }
                        div { style: "font-size: 16px;", "Discussions not available" }
                        div { style: "font-size: 13px; max-width: 400px; text-align: center;",
                            "The discussions API is not available on this pylon instance."
                        }
                    }
                },
                DiscussionFetchState::Loaded(discussions) => {
                    if discussions.is_empty() {
                        rsx! {
                            div {
                                style: "{PLACEHOLDER_STYLE}",
                                div { style: "font-size: 16px;", "No discussions" }
                                div { style: "font-size: 13px;",
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
                                    project_id: project_id.clone(),
                                    on_change: move |_| fetch_trigger.set(fetch_trigger() + 1),
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// A single discussion card with question, options, and answer flow.
#[component]
fn DiscussionCard(
    discussion: Discussion,
    project_id: String,
    on_change: EventHandler<()>,
) -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut selected_option_id: Signal<Option<String>> = use_signal(|| None);
    let mut free_text = use_signal(String::new);
    let mut show_free_text = use_signal(|| false);
    let mut submitting = use_signal(|| false);
    let mut error_msg: Signal<Option<String>> = use_signal(|| None);

    let is_open = discussion.status == DiscussionStatus::Open;
    let is_answered = discussion.status == DiscussionStatus::Answered;

    let (card_bg, card_border) = discussion_card_colors(discussion.priority, discussion.status);
    let card_style = format!("{CARD_BASE} background: {card_bg}; border-color: {card_border};");

    let can_submit = *selected_option_id.read() != None || !free_text.read().is_empty();

    // Clone ids for closures.
    let disc_id_submit = discussion.id.clone();
    let project_id_submit = project_id.clone();

    let disc_id_undo = discussion.id.clone();
    let project_id_undo = project_id.clone();

    let do_submit = move |_| {
        if !can_submit || *submitting.read() {
            return;
        }
        let cfg = config.read().clone();
        let did = disc_id_submit.clone();
        let pid = project_id_submit.clone();
        let opt_id = selected_option_id.read().clone();
        let ft = if free_text.read().is_empty() {
            None
        } else {
            Some(free_text.read().clone())
        };

        submitting.set(true);
        error_msg.set(None);

        spawn(async move {
            let client = authenticated_client(&cfg);
            let url = format!(
                "{}/api/planning/projects/{pid}/discussions/{did}/answer",
                cfg.server_url.trim_end_matches('/')
            );
            let req = DiscussionAnswerRequest {
                option_id: opt_id,
                free_text: ft,
            };
            match client.post(&url).json(&req).send().await {
                Ok(resp) if resp.status().is_success() => {
                    on_change.call(());
                }
                Ok(resp) => {
                    let status = resp.status();
                    error_msg.set(Some(format!("server error: {status}")));
                    submitting.set(false);
                }
                Err(e) => {
                    error_msg.set(Some(format!("connection error: {e}")));
                    submitting.set(false);
                }
            }
        });
    };

    let do_undo = move |_| {
        let cfg = config.read().clone();
        let did = disc_id_undo.clone();
        let pid = project_id_undo.clone();

        submitting.set(true);
        error_msg.set(None);

        spawn(async move {
            let client = authenticated_client(&cfg);
            // WHY: reopen is POST to the same answer endpoint with empty body.
            let url = format!(
                "{}/api/planning/projects/{pid}/discussions/{did}/reopen",
                cfg.server_url.trim_end_matches('/')
            );
            match client.post(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    on_change.call(());
                }
                Ok(resp) => {
                    let status = resp.status();
                    error_msg.set(Some(format!("server error: {status}")));
                    submitting.set(false);
                }
                Err(e) => {
                    error_msg.set(Some(format!("connection error: {e}")));
                    submitting.set(false);
                }
            }
        });
    };

    rsx! {
        div {
            style: "{card_style}",

            // Header: question + status + priority
            div {
                style: "display: flex; align-items: flex-start; justify-content: space-between; margin-bottom: 6px;",
                span { style: "{QUESTION_STYLE}", "{discussion.question}" }
                div {
                    style: "display: flex; align-items: center; flex-shrink: 0;",
                    span { style: "{status_badge_style(discussion.status)}", "{status_label(discussion.status)}" }
                    span { style: "{priority_badge_style(discussion.priority)}", "{priority_label(discussion.priority)}" }
                }
            }

            // Context
            if !discussion.context.is_empty() {
                div { style: "{CONTEXT_STYLE}", "{discussion.context}" }
            }

            // Answered: show summary and undo button
            if is_answered {
                if let Some(summary) = DiscussionStore::answer_summary(&discussion) {
                    div { style: "{ANSWER_SUMMARY}", "Answer: {summary}" }
                }
                button {
                    style: "{UNDO_BTN}",
                    disabled: *submitting.read(),
                    onclick: do_undo,
                    if *submitting.read() { "Reopening..." } else { "Reopen" }
                }
            }

            // Open: show options and answer flow
            if is_open {
                div {
                    style: "{OPTIONS_GRID}",
                    for opt in &discussion.options {
                        OptionCard {
                            key: "{opt.id}",
                            option: opt.clone(),
                            selected: *selected_option_id.read() == Some(opt.id.clone()),
                            on_select: move |id: String| {
                                selected_option_id.set(Some(id));
                                show_free_text.set(false);
                                free_text.set(String::new());
                            },
                        }
                    }
                }

                // Free-text override
                div {
                    style: "margin-top: 10px;",
                    button {
                        style: "background: transparent; border: none; color: #4a9aff; font-size: 12px; cursor: pointer; padding: 0;",
                        onclick: move |_| {
                            let current = *show_free_text.read();
                            show_free_text.set(!current);
                            if current {
                                free_text.set(String::new());
                            } else {
                                selected_option_id.set(None);
                            }
                        },
                        if *show_free_text.read() { "Cancel free-text" } else { "Provide custom answer" }
                    }

                    if *show_free_text.read() {
                        textarea {
                            style: "{FREE_TEXT_INPUT}",
                            placeholder: "Type your custom answer...",
                            rows: "3",
                            value: "{free_text.read()}",
                            oninput: move |evt: Event<FormData>| free_text.set(evt.value().clone()),
                        }
                    }
                }

                // Submit
                div {
                    style: "display: flex; gap: 8px; margin-top: 10px;",
                    button {
                        style: if can_submit { "{SUBMIT_BTN}" } else { "{SUBMIT_BTN_DISABLED}" },
                        disabled: !can_submit || *submitting.read(),
                        onclick: do_submit,
                        if *submitting.read() { "Submitting..." } else { "Submit Answer" }
                    }
                }
            }

            // Error feedback
            if let Some(ref err) = *error_msg.read() {
                div { style: "{ERROR_STYLE}", "{err}" }
            }
        }
    }
}

fn discussion_card_colors(
    priority: DiscussionPriority,
    status: DiscussionStatus,
) -> (&'static str, &'static str) {
    if status == DiscussionStatus::Answered {
        return ("#0f1a0f", "#2a4a2a");
    }
    match priority {
        DiscussionPriority::Blocking => ("#1e0f0f", "#ef4444"),
        DiscussionPriority::Important => ("#1e1a10", "#f59e0b"),
        DiscussionPriority::NiceToHave => ("#1a1a2e", "#2a2a3a"),
    }
}

fn status_badge_style(status: DiscussionStatus) -> String {
    let (bg, color) = match status {
        DiscussionStatus::Open => ("#1e1e5a", "#4a9aff"),
        DiscussionStatus::Answered => ("#0f2a0f", "#22c55e"),
        DiscussionStatus::Deferred => ("#2a2a3a", "#888"),
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
        DiscussionPriority::Blocking => ("#3a0f0f", "#ef4444"),
        DiscussionPriority::Important => ("#2a1f05", "#f59e0b"),
        DiscussionPriority::NiceToHave => ("#2a2a3a", "#888"),
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
        assert_eq!(border, "#ef4444", "blocking should have red border");
    }

    #[test]
    fn discussion_card_colors_answered_overrides_priority() {
        let (_, border) =
            discussion_card_colors(DiscussionPriority::Blocking, DiscussionStatus::Answered);
        assert_eq!(
            border, "#2a4a2a",
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
