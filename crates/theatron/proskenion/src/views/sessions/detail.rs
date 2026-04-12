//! Session detail panel: stats, distillation history, message preview.

use dioxus::prelude::*;
use skene::id::SessionId;

use crate::state::fetch::FetchState;
use crate::state::sessions::{
    SessionDetailStore, format_relative_time, session_display_status, status_color,
};

const DETAIL_CONTAINER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    height: 100%; \
    overflow-y: auto; \
    padding: var(--space-4); \
    gap: var(--space-4);\
";

const HEADER_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    gap: var(--space-3);\
";

const TITLE_STYLE: &str = "\
    font-size: var(--text-lg); \
    font-weight: var(--weight-bold); \
    color: var(--text-primary);\
";

const STATUS_BADGE_STYLE: &str = "\
    display: inline-block; \
    padding: var(--space-1) var(--space-2); \
    border-radius: var(--radius-sm); \
    font-size: var(--text-xs);\
";

const AGENT_BADGE_STYLE: &str = "\
    display: inline-block; \
    padding: var(--space-1) var(--space-2); \
    border-radius: var(--radius-sm); \
    font-size: var(--text-xs); \
    background: var(--border); \
    color: #7a7aff;\
";

const STATS_GRID_STYLE: &str = "\
    display: grid; \
    grid-template-columns: repeat(auto-fit, minmax(160px, 1fr)); \
    gap: var(--space-3);\
";

const STAT_CARD_STYLE: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-3);\
";

const STAT_LABEL_STYLE: &str = "\
    font-size: var(--text-xs); \
    color: var(--text-secondary); \
    margin-bottom: var(--space-1);\
";

const STAT_VALUE_STYLE: &str = "\
    font-size: var(--text-xl); \
    font-weight: var(--weight-bold); \
    color: var(--text-primary);\
";

const STAT_SUB_STYLE: &str = "\
    font-size: var(--text-xs); \
    color: var(--text-muted); \
    margin-top: var(--space-1);\
";

const TOKEN_BAR_BG_STYLE: &str = "\
    width: 100%; \
    height: 6px; \
    background: #222; \
    border-radius: var(--radius-sm); \
    margin-top: var(--space-2); \
    overflow: hidden;\
";

const SECTION_TITLE_STYLE: &str = "\
    font-size: var(--text-base); \
    font-weight: var(--weight-bold); \
    color: var(--text-primary); \
    margin: 0;\
";

const DISTILL_ROW_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-3); \
    padding: var(--space-2) var(--space-3); \
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    font-size: var(--text-xs);\
";

const DISTILL_TRIGGER_STYLE: &str = "\
    display: inline-block; \
    padding: 1px 6px; \
    border-radius: var(--radius-sm); \
    font-size: var(--text-xs); \
    background: var(--border); \
    color: var(--text-secondary);\
";

const MSG_PREVIEW_STYLE: &str = "\
    display: flex; \
    gap: var(--space-2); \
    padding: var(--space-2) 0; \
    border-bottom: 1px solid var(--bg-surface); \
    font-size: var(--text-sm);\
";

const ROLE_BADGE_USER: &str = "\
    flex-shrink: 0; \
    width: 60px; \
    font-size: var(--text-xs); \
    color: #4a9aff; \
    font-weight: var(--weight-bold);\
";

const ROLE_BADGE_ASSISTANT: &str = "\
    flex-shrink: 0; \
    width: 60px; \
    font-size: var(--text-xs); \
    color: var(--status-success); \
    font-weight: var(--weight-bold);\
";

const OPEN_CHAT_BTN: &str = "\
    background: var(--accent); \
    color: white; \
    border: none; \
    border-radius: var(--radius-md); \
    padding: var(--space-2) var(--space-4); \
    font-size: var(--text-sm); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const ARCHIVE_BTN: &str = "\
    background: var(--border); \
    color: var(--text-primary); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-2) var(--space-4); \
    font-size: var(--text-sm); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const EMPTY_DETAIL_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    align-items: center; \
    justify-content: center; \
    flex: 1; \
    gap: var(--space-3); \
    color: var(--text-muted);\
";

/// Empty state shown when no session is selected.
#[component]
pub(crate) fn SessionDetailEmpty() -> Element {
    rsx! {
        div {
            style: "{EMPTY_DETAIL_STYLE}",
            div { style: "font-size: var(--text-3xl);", "[S]" }
            div { style: "font-size: var(--text-md);", "Select a session" }
            div { style: "font-size: var(--text-sm);",
                "Click a session in the list to view details."
            }
        }
    }
}

/// Session detail panel showing stats, distillation history, and message preview.
#[component]
pub(crate) fn SessionDetail(
    detail_state: Signal<FetchState<SessionDetailStore>>,
    on_open_chat: EventHandler<SessionId>,
    on_archive: EventHandler<SessionId>,
    on_restore: EventHandler<SessionId>,
) -> Element {
    rsx! {
        match &*detail_state.read() {
            FetchState::Loading => rsx! {
                div {
                    style: "{EMPTY_DETAIL_STYLE}",
                    "Loading session details..."
                }
            },
            FetchState::Error(err) => rsx! {
                div {
                    style: "{EMPTY_DETAIL_STYLE} color: var(--status-error);",
                    "Error: {err}"
                }
            },
            FetchState::Loaded(detail) => {
                match &detail.session {
                    None => rsx! { SessionDetailEmpty {} },
                    Some(session) => {
                        let session_id = session.id.clone();
                        let status = session_display_status(session);
                        let status_bg = match status {
                            "active" => "background: #1a3a1a;",
                            "archived" => "background: var(--border);",
                            _ => "background: #2a2a1a;",
                        };
                        let s_color = status_color(status);
                        let is_archived = session.is_archived();

                        let id_for_chat = session_id.clone();
                        let id_for_action = session_id.clone();

                        rsx! {
                            div {
                                style: "{DETAIL_CONTAINER_STYLE}",
                                // Header
                                div {
                                    style: "{HEADER_STYLE}",
                                    div {
                                        h2 { style: "{TITLE_STYLE}", "{session.label()}" }
                                        div {
                                            style: "display: flex; gap: var(--space-2); align-items: center; margin-top: var(--space-1);",
                                            span { style: "{AGENT_BADGE_STYLE}", "{session.nous_id}" }
                                            span {
                                                style: "{STATUS_BADGE_STYLE} {status_bg} color: {s_color};",
                                                "{status}"
                                            }
                                            if let Some(ref updated) = session.updated_at {
                                                span {
                                                    style: "font-size: var(--text-xs); color: var(--text-muted);",
                                                    "Last active: {format_relative_time(updated)}"
                                                }
                                            }
                                        }
                                    }
                                    div {
                                        style: "display: flex; gap: var(--space-2);",
                                        button {
                                            style: "{OPEN_CHAT_BTN}",
                                            onclick: move |_| on_open_chat.call(id_for_chat.clone()),
                                            "Open in Chat"
                                        }
                                        if is_archived {
                                            button {
                                                style: "{ARCHIVE_BTN}",
                                                onclick: {
                                                    let id = id_for_action.clone();
                                                    move |_| on_restore.call(id.clone())
                                                },
                                                "Restore"
                                            }
                                        } else {
                                            button {
                                                style: "{ARCHIVE_BTN}",
                                                onclick: {
                                                    let id = id_for_action.clone();
                                                    move |_| on_archive.call(id.clone())
                                                },
                                                "Archive"
                                            }
                                        }
                                    }
                                }
                                // Stats grid
                                div {
                                    style: "{STATS_GRID_STYLE}",
                                    // Message count
                                    div {
                                        style: "{STAT_CARD_STYLE}",
                                        div { style: "{STAT_LABEL_STYLE}", "Messages" }
                                        div { style: "{STAT_VALUE_STYLE}", "{session.message_count}" }
                                        if detail.user_messages > 0 || detail.assistant_messages > 0 {
                                            div { style: "{STAT_SUB_STYLE}",
                                                "{detail.user_messages} user / {detail.assistant_messages} assistant"
                                            }
                                        }
                                    }
                                    // Token usage
                                    div {
                                        style: "{STAT_CARD_STYLE}",
                                        div { style: "{STAT_LABEL_STYLE}", "Token Usage" }
                                        div { style: "{STAT_VALUE_STYLE}",
                                            "{format_tokens(detail.total_tokens())}"
                                        }
                                        if detail.has_token_breakdown() {
                                            div { style: "{STAT_SUB_STYLE}",
                                                "{format_tokens(detail.input_tokens)} in / {format_tokens(detail.output_tokens)} out"
                                            }
                                            // Token bar
                                            div {
                                                style: "{TOKEN_BAR_BG_STYLE}",
                                                div {
                                                    style: "height: 100%; background: #4a9aff; border-radius: var(--radius-sm); width: {token_input_pct(detail)}%;",
                                                }
                                            }
                                        }
                                    }
                                    // Duration
                                    if detail.started_at.is_some() {
                                        div {
                                            style: "{STAT_CARD_STYLE}",
                                            div { style: "{STAT_LABEL_STYLE}", "Duration" }
                                            div { style: "{STAT_VALUE_STYLE}",
                                                "{format_duration_between(detail.started_at.as_deref(), detail.ended_at.as_deref())}"
                                            }
                                        }
                                    }
                                    // Model
                                    if let Some(ref model) = detail.model {
                                        div {
                                            style: "{STAT_CARD_STYLE}",
                                            div { style: "{STAT_LABEL_STYLE}", "Model" }
                                            div {
                                                style: "font-size: var(--text-sm); color: var(--text-primary); word-break: break-all;",
                                                "{model}"
                                            }
                                        }
                                    }
                                }
                                // Distillation history
                                if !detail.distillation_events.is_empty() {
                                    div {
                                        style: "display: flex; flex-direction: column; gap: var(--space-2);",
                                        h3 { style: "{SECTION_TITLE_STYLE}", "Distillation History" }
                                        for (i , event) in detail.distillation_events.iter().enumerate() {
                                            div {
                                                key: "{i}",
                                                style: "{DISTILL_ROW_STYLE}",
                                                span { style: "color: var(--text-secondary);",
                                                    "{format_relative_time(&event.timestamp)}"
                                                }
                                                span { style: "{DISTILL_TRIGGER_STYLE}", "{event.trigger}" }
                                                span { style: "color: var(--text-primary);",
                                                    "{format_tokens(event.tokens_before)} -> {format_tokens(event.tokens_after)}"
                                                }
                                                span { style: "color: var(--status-success);",
                                                    "-{format_tokens(event.tokens_saved())} ({format_pct(1.0 - event.compression_ratio())})"
                                                }
                                            }
                                        }
                                    }
                                }
                                // Message preview
                                if !detail.message_previews.is_empty() {
                                    div {
                                        style: "display: flex; flex-direction: column; gap: var(--space-1);",
                                        h3 { style: "{SECTION_TITLE_STYLE}", "Messages" }
                                        for (i , msg) in detail.message_previews.iter().enumerate() {
                                            div {
                                                key: "{i}",
                                                style: "{MSG_PREVIEW_STYLE}",
                                                span {
                                                    style: if msg.role == "user" { "{ROLE_BADGE_USER}" } else { "{ROLE_BADGE_ASSISTANT}" },
                                                    "{msg.role}"
                                                }
                                                span {
                                                    style: "color: var(--text-primary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap;",
                                                    "{msg.summary}"
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Format a token count with K/M suffixes.
fn format_tokens(count: u32) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", f64::from(count) / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}K", f64::from(count) / 1_000.0)
    } else {
        count.to_string()
    }
}

fn format_pct(ratio: f64) -> String {
    format!("{:.0}%", ratio * 100.0)
}

fn token_input_pct(detail: &SessionDetailStore) -> f64 {
    let total = detail.total_tokens();
    if total == 0 {
        return 0.0;
    }
    (f64::from(detail.input_tokens) / f64::from(total)) * 100.0
}

/// Format the duration between two ISO timestamps.
fn format_duration_between(start: Option<&str>, end: Option<&str>) -> String {
    let (Some(start), Some(end)) = (start, end) else {
        return "—".to_string();
    };

    let start_secs = crate::state::sessions::parse_iso_to_unix(start).unwrap_or(0);
    let end_secs = crate::state::sessions::parse_iso_to_unix(end).unwrap_or(0);

    if start_secs == 0 || end_secs == 0 {
        return "—".to_string();
    }

    let delta = end_secs.saturating_sub(start_secs);

    if delta < 60 {
        format!("{delta}s")
    } else if delta < 3600 {
        format!("{}m {}s", delta / 60, delta % 60)
    } else if delta < 86400 {
        format!("{}h {}m", delta / 3600, (delta % 3600) / 60)
    } else {
        format!("{}d {}h", delta / 86400, (delta % 86400) / 3600)
    }
}
