use crate::api::types::{Agent, HistoryMessage, Session};
use crate::app::App;
use crate::msg::ErrorToast;
use crate::state::{AgentState, AgentStatus, ChatMessage};

pub(crate) fn handle_agents_loaded(app: &mut App, agents: Vec<Agent>) {
    app.agents = agents
        .into_iter()
        .map(|a| AgentState {
            id: a.id,
            name: a.name,
            emoji: a.emoji,
            status: AgentStatus::Idle,
            active_tool: None,
            tool_started_at: None,
            sessions: Vec::new(),
            compaction_stage: None,
            has_notification: false,
        })
        .collect();
}

pub(crate) fn handle_sessions_loaded(app: &mut App, nous_id: String, sessions: Vec<Session>) {
    if let Some(agent) = app.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.sessions = sessions;
    }
}

pub(crate) fn handle_history_loaded(app: &mut App, messages: Vec<HistoryMessage>) {
    app.messages = messages
        .into_iter()
        .filter_map(|m| {
            if m.role != "user" && m.role != "assistant" {
                return None;
            }
            let text = extract_text_content(&m.content)?;
            Some(ChatMessage {
                role: m.role,
                text,
                timestamp: m.created_at,
                model: m.model,
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

pub(crate) fn handle_show_error(app: &mut App, msg: String) {
    app.error_toast = Some(ErrorToast::new(msg));
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
