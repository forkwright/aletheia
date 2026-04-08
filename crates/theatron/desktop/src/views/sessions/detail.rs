//! Session detail panel: stats, distillation history, message preview.

use dioxus::prelude::*;
use theatron_core::id::SessionId;

use crate::state::fetch::FetchState;
use crate::state::sessions::{
    SessionDetailStore, format_relative_time, session_display_status, status_color,
};

const DETAIL_CONTAINER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    height: 100%; \
    overflow-y: auto; \
    padding: 16px; \
    gap: 16px;\
";

const HEADER_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    gap: 12px;\
";

const TITLE_STYLE: &str = "\
    font-size: 18px; \
    font-weight: bold; \
    color: #e0e0e0;\
";

const STATUS_BADGE_STYLE: &str = "\
    display: inline-block; \
    padding: 2px 8px; \
    border-radius: 4px; \
    font-size: 11px;\
";

const AGENT_BADGE_STYLE: &str = "\
    display: inline-block; \
    padding: 2px 8px; \
    border-radius: 4px; \
    font-size: 11px; \
    background: #2a2a4a; \
    color: #7a7aff;\
";

const STATS_GRID_STYLE: &str = "\
    display: grid; \
    grid-template-columns: repeat(auto-fit, minmax(160px, 1fr)); \
    gap: 12px;\
";

const STAT_CARD_STYLE: &str = "\
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 8px; \
    padding: 12px;\
";

const STAT_LABEL_STYLE: &str = "\
    font-size: 11px; \
    color: #888; \
    margin-bottom: 4px;\
";

const STAT_VALUE_STYLE: &str = "\
    font-size: 20px; \
    font-weight: bold; \
    color: #e0e0e0;\
";

const STAT_SUB_STYLE: &str = "\
    font-size: 11px; \
    color: #666; \
    margin-top: 2px;\
";

const TOKEN_BAR_BG_STYLE: &str = "\
    width: 100%; \
    height: 6px; \
    background: #222; \
    border-radius: 3px; \
    margin-top: 8px; \
    overflow: hidden;\
";

const SECTION_TITLE_STYLE: &str = "\
    font-size: 14px; \
    font-weight: bold; \
    color: #e0e0e0; \
    margin: 0;\
";

const DISTILL_ROW_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 12px; \
    padding: 8px 12px; \
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 6px; \
    font-size: 12px;\
";

const DISTILL_TRIGGER_STYLE: &str = "\
    display: inline-block; \
    padding: 1px 6px; \
    border-radius: 3px; \
    font-size: 10px; \
    background: #2a2a3a; \
    color: #aaa;\
";

const MSG_PREVIEW_STYLE: &str = "\
    display: flex; \
    gap: 8px; \
    padding: 6px 0; \
    border-bottom: 1px solid #1a1a2e; \
    font-size: 13px;\
";

const ROLE_BADGE_USER: &str = "\
    flex-shrink: 0; \
    width: 60px; \
    font-size: 11px; \
    color: #4a9aff; \
    font-weight: bold;\
";

const ROLE_BADGE_ASSISTANT: &str = "\
    flex-shrink: 0; \
    width: 60px; \
    font-size: 11px; \
    color: #22c55e; \
    font-weight: bold;\
";

const OPEN_CHAT_BTN: &str = "\
    background: #4a4aff; \
    color: white; \
    border: none; \
    border-radius: 6px; \
    padding: 8px 16px; \
    font-size: 13px; \
    cursor: pointer;\
";

const ARCHIVE_BTN: &str = "\
    background: #2a2a3a; \
    color: #e0e0e0; \
    border: 1px solid #444; \
    border-radius: 6px; \
    padding: 8px 16px; \
    font-size: 13px; \
    cursor: pointer;\
";

const EMPTY_DETAIL_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    align-items: center; \
    justify-content: center; \
    flex: 1; \
    gap: 12px; \
    color: #555;\
";

/// Empty state shown when no session is selected.
#[component]
pub(crate) fn SessionDetailEmpty() -> Element {
    rsx! {
        div {
            style: "{EMPTY_DETAIL_STYLE}",
            div { style: "font-size: 48px;", "[S]" }
            div { style: "font-size: 16px;", "Select a session" }
            div { style: "font-size: 13px;",
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
                    style: "{EMPTY_DETAIL_STYLE} color: #ef4444;",
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
                            "archived" => "background: #2a2a3a;",
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
                                            style: "display: flex; gap: 8px; align-items: center; margin-top: 4px;",
                                            span { style: "{AGENT_BADGE_STYLE}", "{session.nous_id}" }
                                            span {
                                                style: "{STATUS_BADGE_STYLE} {status_bg} color: {s_color};",
                                                "{status}"
                                            }
                                            if let Some(ref updated) = session.updated_at {
                                                span {
                                                    style: "font-size: 11px; color: #666;",
                                                    "Last active: {format_relative_time(updated)}"
                                                }
                                            }
                                        }
                                    }
                                    div {
                                        style: "display: flex; gap: 8px;",
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
                                                    style: "height: 100%; background: #4a9aff; border-radius: 3px; width: {token_input_pct(detail)}%;",
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
                                                style: "font-size: 13px; color: #e0e0e0; word-break: break-all;",
                                                "{model}"
                                            }
                                        }
                                    }
                                }
                                // Distillation history
                                if !detail.distillation_events.is_empty() {
                                    div {
                                        style: "display: flex; flex-direction: column; gap: 8px;",
                                        h3 { style: "{SECTION_TITLE_STYLE}", "Distillation History" }
                                        for (i , event) in detail.distillation_events.iter().enumerate() {
                                            div {
                                                key: "{i}",
                                                style: "{DISTILL_ROW_STYLE}",
                                                span { style: "color: #888;",
                                                    "{format_relative_time(&event.timestamp)}"
                                                }
                                                span { style: "{DISTILL_TRIGGER_STYLE}", "{event.trigger}" }
                                                span { style: "color: #e0e0e0;",
                                                    "{format_tokens(event.tokens_before)} -> {format_tokens(event.tokens_after)}"
                                                }
                                                span { style: "color: #22c55e;",
                                                    "-{format_tokens(event.tokens_saved())} ({format_pct(1.0 - event.compression_ratio())})"
                                                }
                                            }
                                        }
                                    }
                                }
                                // Message preview
                                if !detail.message_previews.is_empty() {
                                    div {
                                        style: "display: flex; flex-direction: column; gap: 4px;",
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
                                                    style: "color: #ccc; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;",
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
