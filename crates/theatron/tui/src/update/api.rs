use crate::api::types::{Agent, HistoryMessage, Session};
use crate::app::App;
use crate::id::NousId;
use crate::msg::ErrorToast;
use crate::sanitize::sanitize_for_display;
use crate::state::{AgentState, AgentStatus, ChatMessage};

#[tracing::instrument(skip_all, fields(count = agents.len()))]
// SAFETY: sanitized at ingestion: all Agent fields from API are sanitized here.
pub(crate) fn handle_agents_loaded(app: &mut App, agents: Vec<Agent>) {
    app.dashboard.agents = agents
        .into_iter()
        .map(|a| {
            let name = sanitize_for_display(a.display_name()).into_owned();
            let name_lower = name.to_lowercase();
            AgentState {
                id: a.id.clone(),
                name,
                name_lower,
                emoji: a.emoji.map(|e| sanitize_for_display(&e).into_owned()),
                status: AgentStatus::Idle,
                active_tool: None,
                sessions: sanitize_sessions(Vec::new()),
                model: a.model.map(|m| sanitize_for_display(&m).into_owned()),
                compaction_stage: None,
                unread_count: 0,
                tools: Vec::new(),
            }
        })
        .collect();
}

#[tracing::instrument(skip_all, fields(%nous_id, count = sessions.len()))]
// SAFETY: sanitized at ingestion: session keys and fields from API are sanitized here.
pub(crate) fn handle_sessions_loaded(app: &mut App, nous_id: NousId, sessions: Vec<Session>) {
    if let Some(agent) = app.dashboard.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.sessions = sanitize_sessions(sessions);
    }
}

#[tracing::instrument(skip_all, fields(count = messages.len()))]
// SAFETY: sanitized at ingestion: all message content from API is sanitized here.
pub(crate) fn handle_history_loaded(app: &mut App, messages: Vec<HistoryMessage>) {
    app.dashboard.messages = messages
        .into_iter()
        .filter_map(|m| {
            if m.role != "user" && m.role != "assistant" {
                return None;
            }
            let text = extract_text_content(&m.content)?;
            let text = sanitize_for_display(&text).into_owned();
            let text_lower = text.to_lowercase();
            Some(ChatMessage {
                role: sanitize_for_display(&m.role).into_owned(),
                text,
                text_lower,
                timestamp: m.created_at.map(|t| sanitize_for_display(&t).into_owned()),
                model: m.model.map(|m| sanitize_for_display(&m).into_owned()),
                tool_calls: Vec::new(),
            })
        })
        .collect();
    // Stale streaming markdown from the previous session must not bleed through
    // when history is replaced on session switch.
    app.viewport.render.markdown_cache.clear();
    app.rebuild_virtual_scroll();
    app.scroll_to_bottom();
}

#[tracing::instrument(skip_all, fields(daily_total_cents))]
pub(crate) fn handle_cost_loaded(app: &mut App, daily_total_cents: u32) {
    app.dashboard.daily_cost_cents = daily_total_cents;
}

#[tracing::instrument(skip_all)]
pub(crate) async fn handle_new_session(app: &mut App) {
    if let Some(ref agent_id) = app.dashboard.focused_agent.clone() {
        app.dashboard.messages.clear();
        app.viewport.render.virtual_scroll.clear();
        app.scroll_to_bottom();

        let session_key = format!("tui-{}", chrono_compact_now());
        let client = app.client.clone();
        let agent_id = agent_id.clone();
        let key = session_key.clone();
        match client.create_session(&agent_id, &key).await {
            Ok(session) => {
                app.dashboard.focused_session_id = Some(session.id.clone());
                if let Some(agent) = app.dashboard.agents.iter_mut().find(|a| a.id == agent_id) {
                    agent.sessions.push(session);
                }
            }
            Err(e) => {
                tracing::error!("failed to create session: {e}");
                app.viewport.error_toast =
                    Some(ErrorToast::new(format!("New session failed: {e}")));
            }
        }
    }
}

#[tracing::instrument(skip_all)]
pub(crate) async fn handle_session_picker_new(app: &mut App) {
    app.layout.overlay = None;
    handle_new_session(app).await;
}

#[tracing::instrument(skip_all)]
pub(crate) async fn handle_session_picker_archive(app: &mut App) {
    let (cursor, show_archived) = match &app.layout.overlay {
        Some(crate::state::Overlay::SessionPicker(picker)) => (picker.cursor, picker.show_archived),
        _ => return,
    };

    let session_id = match super::overlay::pick_session_id_pub(app, cursor, show_archived) {
        Some(id) => id,
        None => return,
    };

    let client = app.client.clone();
    match client.archive_session(&session_id).await {
        Ok(()) => {
            if let Some(ref agent_id) = app.dashboard.focused_agent
                && let Some(agent) = app.dashboard.agents.iter_mut().find(|a| &a.id == agent_id)
                && let Some(session) = agent.sessions.iter_mut().find(|s| s.id == session_id)
            {
                session.status = Some("archived".to_string());
            }
            if app.dashboard.focused_session_id.as_ref() == Some(&session_id) {
                app.dashboard.messages.clear();
                app.viewport.render.virtual_scroll.clear();
                app.dashboard.focused_session_id = None;
                app.scroll_to_bottom();
            }
            app.viewport.error_toast = Some(ErrorToast::new("Session archived".into()));
        }
        Err(e) => {
            app.viewport.error_toast = Some(ErrorToast::new(format!("Archive failed: {e}")));
        }
    }

    app.layout.overlay = None;
}

#[tracing::instrument(skip_all)]
// SAFETY: sanitized at ingestion: error messages may contain external data.
pub(crate) fn handle_show_error(app: &mut App, msg: String) {
    app.viewport.error_toast = Some(ErrorToast::new(sanitize_for_display(&msg).into_owned()));
}

#[tracing::instrument(skip_all)]
pub(crate) fn handle_show_success(app: &mut App, msg: String) {
    app.viewport.success_toast = Some(ErrorToast::new(sanitize_for_display(&msg).into_owned()));
}

#[tracing::instrument(skip_all)]
pub(crate) fn handle_dismiss_error(app: &mut App) {
    app.viewport.error_toast = None;
}

#[tracing::instrument(skip_all)]
pub(crate) fn handle_tick(app: &mut App) {
    app.viewport.tick_count = app.viewport.tick_count.wrapping_add(1);
    if app
        .viewport
        .error_toast
        .as_ref()
        .is_some_and(|t| t.is_expired())
    {
        app.viewport.error_toast = None;
    }
    if app
        .viewport
        .success_toast
        .as_ref()
        .is_some_and(|t| t.is_expired())
    {
        app.viewport.success_toast = None;
    }
    super::sse::check_sse_reconnect_timeout(app);
}

/// Sanitize session fields that may contain external data.
fn sanitize_sessions(sessions: Vec<Session>) -> Vec<Session> {
    sessions
        .into_iter()
        .map(|s| Session {
            id: s.id,
            nous_id: s.nous_id,
            key: sanitize_for_display(&s.key).into_owned(),
            status: s.status.map(|st| sanitize_for_display(&st).into_owned()),
            message_count: s.message_count,
            session_type: s
                .session_type
                .map(|t| sanitize_for_display(&t).into_owned()),
            updated_at: s.updated_at,
            display_name: s
                .display_name
                .map(|n| sanitize_for_display(&n).into_owned()),
        })
        .collect()
}

fn chrono_compact_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{:x}", secs)
}

pub(crate) fn extract_text_content(content: &Option<serde_json::Value>) -> Option<String> {
    let content = content.as_ref()?;

    if let Some(s) = content.as_str() {
        if s.is_empty() {
            return None;
        }
        if s.starts_with('[')
            && let Ok(parsed) = serde_json::from_str::<Vec<serde_json::Value>>(s)
        {
            return extract_texts_from_array(&parsed);
        }
        // WHY: tool_use inputs are sometimes stored as JSON object strings;
        // skip them rather than rendering raw JSON in the chat pane.
        if s.starts_with('{') && serde_json::from_str::<serde_json::Value>(s).is_ok() {
            return None;
        }
        return Some(s.to_string());
    }

    if let Some(arr) = content.as_array() {
        return extract_texts_from_array(arr);
    }

    None
}

fn extract_texts_from_array(arr: &[serde_json::Value]) -> Option<String> {
    let mut texts = Vec::new();

    for block in arr {
        if block.get("type").and_then(|t| t.as_str()) == Some("text")
            && let Some(t) = block.get("text").and_then(|t| t.as_str())
            && !t.is_empty()
        {
            texts.push(t.to_string());
        }
    }

    if texts.is_empty() {
        None
    } else {
        Some(texts.join("\n"))
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions use direct indexing for clarity"
)]
mod tests {
    use super::*;

    #[test]
    fn extract_text_content_plain_string() {
        let content = Some(serde_json::Value::String("hello".to_string()));
        assert_eq!(extract_text_content(&content).as_deref(), Some("hello"));
    }

    #[test]
    fn extract_text_content_empty_string() {
        let content = Some(serde_json::Value::String(String::new()));
        assert!(extract_text_content(&content).is_none());
    }

    #[test]
    fn extract_text_content_none() {
        assert!(extract_text_content(&None).is_none());
    }

    #[test]
    fn extract_text_content_array_with_text_blocks() {
        let content = Some(serde_json::json!([
            {"type": "text", "text": "hello"},
            {"type": "text", "text": "world"}
        ]));
        assert_eq!(
            extract_text_content(&content).as_deref(),
            Some("hello\nworld")
        );
    }

    #[test]
    fn extract_text_content_array_skips_non_text() {
        let content = Some(serde_json::json!([
            {"type": "tool_use", "name": "test"},
            {"type": "text", "text": "result"}
        ]));
        assert_eq!(extract_text_content(&content).as_deref(), Some("result"));
    }

    #[test]
    fn extract_text_content_string_containing_json_array() {
        let content = Some(serde_json::Value::String(
            r#"[{"type": "text", "text": "parsed"}]"#.to_string(),
        ));
        assert_eq!(extract_text_content(&content).as_deref(), Some("parsed"));
    }

    #[test]
    fn extract_text_content_empty_array() {
        let content = Some(serde_json::json!([]));
        assert!(extract_text_content(&content).is_none());
    }

    #[test]
    fn extract_text_content_json_object_string_skipped() {
        // Tool use inputs stored as JSON object strings must not render as raw JSON.
        let content = Some(serde_json::Value::String(
            r#"{"command":"head -30 /path"}"#.to_string(),
        ));
        assert!(extract_text_content(&content).is_none());
    }

    #[test]
    fn extract_text_content_non_json_brace_string_kept() {
        // Plain text that happens to start with '{' but is not valid JSON is kept.
        let content = Some(serde_json::Value::String("{not json}".to_string()));
        assert_eq!(
            extract_text_content(&content).as_deref(),
            Some("{not json}")
        );
    }

    #[test]
    fn extract_text_content_array_with_empty_texts() {
        let content = Some(serde_json::json!([
            {"type": "text", "text": ""},
            {"type": "text", "text": ""}
        ]));
        assert!(extract_text_content(&content).is_none());
    }

    #[test]
    fn handle_agents_loaded_populates() {
        use crate::app::test_helpers::*;
        let mut app = test_app();
        let agents = vec![Agent {
            id: "syn".into(),
            name: Some("Syn".to_string()),
            model: Some("claude-opus-4-6".to_string()),
            emoji: Some("\u{1F9E0}".to_string()),
        }];
        handle_agents_loaded(&mut app, agents);
        assert_eq!(app.dashboard.agents.len(), 1);
        assert_eq!(app.dashboard.agents[0].name, "Syn");
        assert_eq!(app.dashboard.agents[0].status, AgentStatus::Idle);
    }

    #[test]
    fn handle_sessions_loaded_for_agent() {
        use crate::app::test_helpers::*;
        let mut app = test_app();
        app.dashboard.agents.push(test_agent("syn", "Syn"));
        let sessions = vec![Session {
            id: "s1".into(),
            nous_id: "syn".into(),
            key: "main".to_string(),
            status: None,
            message_count: 5,
            session_type: None,
            updated_at: None,
            display_name: None,
        }];
        handle_sessions_loaded(&mut app, "syn".into(), sessions);
        assert_eq!(app.dashboard.agents[0].sessions.len(), 1);
    }

    #[test]
    fn handle_sessions_loaded_unknown_agent_noop() {
        use crate::app::test_helpers::*;
        let mut app = test_app();
        let sessions = vec![Session {
            id: "s1".into(),
            nous_id: "unknown".into(),
            key: "main".to_string(),
            status: None,
            message_count: 0,
            session_type: None,
            updated_at: None,
            display_name: None,
        }];
        handle_sessions_loaded(&mut app, "unknown".into(), sessions);
        // No agents, should not panic
    }

    #[test]
    fn handle_cost_loaded_updates() {
        use crate::app::test_helpers::*;
        let mut app = test_app();
        handle_cost_loaded(&mut app, 1234);
        assert_eq!(app.dashboard.daily_cost_cents, 1234);
    }

    #[test]
    fn handle_show_error_sets_toast() {
        use crate::app::test_helpers::*;
        let mut app = test_app();
        handle_show_error(&mut app, "test error".to_string());
        assert!(app.viewport.error_toast.is_some());
        assert_eq!(
            app.viewport.error_toast.as_ref().unwrap().message,
            "test error"
        );
    }

    #[test]
    fn handle_show_success_sets_success_toast_not_error_toast() {
        use crate::app::test_helpers::*;
        let mut app = test_app();
        handle_show_success(&mut app, "all good".to_string());
        assert!(app.viewport.success_toast.is_some());
        assert!(app.viewport.error_toast.is_none());
        assert_eq!(
            app.viewport.success_toast.as_ref().unwrap().message,
            "all good"
        );
    }

    #[test]
    fn handle_dismiss_error_clears_toast() {
        use crate::app::test_helpers::*;
        let mut app = test_app();
        app.viewport.error_toast = Some(ErrorToast::new("error".to_string()));
        handle_dismiss_error(&mut app);
        assert!(app.viewport.error_toast.is_none());
    }

    #[test]
    fn handle_tick_increments_counter() {
        use crate::app::test_helpers::*;
        let mut app = test_app();
        handle_tick(&mut app);
        assert_eq!(app.viewport.tick_count, 1);
        handle_tick(&mut app);
        assert_eq!(app.viewport.tick_count, 2);
    }

    #[test]
    fn handle_tick_wraps_at_max() {
        use crate::app::test_helpers::*;
        let mut app = test_app();
        app.viewport.tick_count = u64::MAX;
        handle_tick(&mut app);
        assert_eq!(app.viewport.tick_count, 0);
    }

    #[test]
    fn handle_history_loaded_clears_markdown_cache() {
        use crate::app::test_helpers::*;
        let mut app = test_app();
        // Pre-populate stale cache from a previous streaming session.
        app.viewport.render.markdown_cache.text = "stale from previous session".to_string();
        app.viewport.render.markdown_cache.lines = vec![ratatui::text::Line::raw("stale")];

        handle_history_loaded(&mut app, vec![]);

        assert!(
            app.viewport.render.markdown_cache.text.is_empty(),
            "history load must clear stale markdown text cache"
        );
        assert!(
            app.viewport.render.markdown_cache.lines.is_empty(),
            "history load must clear stale markdown line cache"
        );
    }

    #[test]
    fn handle_history_loaded_filters_roles() {
        use crate::app::test_helpers::*;
        let mut app = test_app();
        let messages = vec![
            HistoryMessage {
                role: "user".to_string(),
                content: Some(serde_json::Value::String("hello".to_string())),
                created_at: None,
                model: None,
                tool_name: None,
            },
            HistoryMessage {
                role: "system".to_string(),
                content: Some(serde_json::Value::String("system prompt".to_string())),
                created_at: None,
                model: None,
                tool_name: None,
            },
            HistoryMessage {
                role: "assistant".to_string(),
                content: Some(serde_json::Value::String("response".to_string())),
                created_at: None,
                model: None,
                tool_name: None,
            },
        ];
        handle_history_loaded(&mut app, messages);
        assert_eq!(app.dashboard.messages.len(), 2);
        assert_eq!(app.dashboard.messages[0].role, "user");
        assert_eq!(app.dashboard.messages[1].role, "assistant");
    }
}
