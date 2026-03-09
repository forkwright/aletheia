use crate::api::types::{Agent, HistoryMessage, Session};
use crate::app::App;
use crate::id::NousId;
use crate::msg::ErrorToast;
use crate::sanitize::sanitize_for_display;
use crate::state::{AgentState, AgentStatus, ChatMessage};

// SAFETY: sanitized at ingestion — all Agent fields from API are sanitized here.
pub(crate) fn handle_agents_loaded(app: &mut App, agents: Vec<Agent>) {
    app.agents = agents
        .into_iter()
        .map(|a| AgentState {
            id: a.id.clone(),
            name: sanitize_for_display(a.display_name()).into_owned(),
            emoji: a.emoji.map(|e| sanitize_for_display(&e).into_owned()),
            status: AgentStatus::Idle,
            active_tool: None,
            tool_started_at: None,
            sessions: sanitize_sessions(Vec::new()),
            model: a.model.map(|m| sanitize_for_display(&m).into_owned()),
            compaction_stage: None,
            has_notification: false,
        })
        .collect();
}

// SAFETY: sanitized at ingestion — session keys and fields from API are sanitized here.
pub(crate) fn handle_sessions_loaded(app: &mut App, nous_id: NousId, sessions: Vec<Session>) {
    if let Some(agent) = app.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.sessions = sanitize_sessions(sessions);
    }
}

// SAFETY: sanitized at ingestion — all message content from API is sanitized here.
pub(crate) fn handle_history_loaded(app: &mut App, messages: Vec<HistoryMessage>) {
    app.messages = messages
        .into_iter()
        .filter_map(|m| {
            if m.role != "user" && m.role != "assistant" {
                return None;
            }
            let text = extract_text_content(&m.content)?;
            Some(ChatMessage {
                role: sanitize_for_display(&m.role).into_owned(),
                text: sanitize_for_display(&text).into_owned(),
                timestamp: m.created_at.map(|t| sanitize_for_display(&t).into_owned()),
                model: m.model.map(|m| sanitize_for_display(&m).into_owned()),
                is_streaming: false,
                tool_calls: Vec::new(),
            })
        })
        .collect();
    app.scroll_to_bottom();
}

pub(crate) fn handle_cost_loaded(app: &mut App, daily_total_cents: u32) {
    app.daily_cost_cents = daily_total_cents;
}

pub(crate) async fn handle_new_session(app: &mut App) {
    if let Some(ref agent_id) = app.focused_agent.clone() {
        app.messages.clear();
        app.scroll_to_bottom();

        let session_key = format!("tui-{}", chrono_compact_now());
        let client = app.client.clone();
        let agent_id = agent_id.clone();
        let key = session_key.clone();
        match client.create_session(&agent_id, &key).await {
            Ok(session) => {
                app.focused_session_id = Some(session.id.clone());
                if let Some(agent) = app.agents.iter_mut().find(|a| a.id == agent_id) {
                    agent.sessions.push(session);
                }
            }
            Err(e) => {
                tracing::error!("failed to create session: {e}");
                app.error_toast = Some(ErrorToast::new(format!("New session failed: {e}")));
            }
        }
    }
}

// SAFETY: sanitized at ingestion — error messages may contain external data.
pub(crate) fn handle_show_error(app: &mut App, msg: String) {
    app.error_toast = Some(ErrorToast::new(sanitize_for_display(&msg).into_owned()));
}

pub(crate) fn handle_dismiss_error(app: &mut App) {
    app.error_toast = None;
}

pub(crate) fn handle_tick(app: &mut App) {
    app.tick_count = app.tick_count.wrapping_add(1);
    if app.error_toast.as_ref().is_some_and(|t| t.is_expired()) {
        app.error_toast = None;
    }
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
        if s.starts_with('[') {
            if let Ok(parsed) = serde_json::from_str::<Vec<serde_json::Value>>(s) {
                return extract_texts_from_array(&parsed);
            }
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
        if block.get("type").and_then(|t| t.as_str()) == Some("text") {
            if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                if !t.is_empty() {
                    texts.push(t.to_string());
                }
            }
        }
    }

    if texts.is_empty() {
        None
    } else {
        Some(texts.join("\n"))
    }
}

#[cfg(test)]
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
        assert_eq!(app.agents.len(), 1);
        assert_eq!(app.agents[0].name, "Syn");
        assert_eq!(app.agents[0].status, AgentStatus::Idle);
    }

    #[test]
    fn handle_sessions_loaded_for_agent() {
        use crate::app::test_helpers::*;
        let mut app = test_app();
        app.agents.push(test_agent("syn", "Syn"));
        let sessions = vec![Session {
            id: "s1".into(),
            nous_id: "syn".into(),
            key: "main".to_string(),
            status: None,
            message_count: 5,
            session_type: None,
            updated_at: None,
        }];
        handle_sessions_loaded(&mut app, "syn".into(), sessions);
        assert_eq!(app.agents[0].sessions.len(), 1);
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
        }];
        handle_sessions_loaded(&mut app, "unknown".into(), sessions);
        // No agents, should not panic
    }

    #[test]
    fn handle_cost_loaded_updates() {
        use crate::app::test_helpers::*;
        let mut app = test_app();
        handle_cost_loaded(&mut app, 1234);
        assert_eq!(app.daily_cost_cents, 1234);
    }

    #[test]
    fn handle_show_error_sets_toast() {
        use crate::app::test_helpers::*;
        let mut app = test_app();
        handle_show_error(&mut app, "test error".to_string());
        assert!(app.error_toast.is_some());
        assert_eq!(app.error_toast.as_ref().unwrap().message, "test error");
    }

    #[test]
    fn handle_dismiss_error_clears_toast() {
        use crate::app::test_helpers::*;
        let mut app = test_app();
        app.error_toast = Some(ErrorToast::new("error".to_string()));
        handle_dismiss_error(&mut app);
        assert!(app.error_toast.is_none());
    }

    #[test]
    fn handle_tick_increments_counter() {
        use crate::app::test_helpers::*;
        let mut app = test_app();
        handle_tick(&mut app);
        assert_eq!(app.tick_count, 1);
        handle_tick(&mut app);
        assert_eq!(app.tick_count, 2);
    }

    #[test]
    fn handle_tick_wraps_at_max() {
        use crate::app::test_helpers::*;
        let mut app = test_app();
        app.tick_count = u64::MAX;
        handle_tick(&mut app);
        assert_eq!(app.tick_count, 0);
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
        assert_eq!(app.messages.len(), 2);
        assert_eq!(app.messages[0].role, "user");
        assert_eq!(app.messages[1].role, "assistant");
    }
}
