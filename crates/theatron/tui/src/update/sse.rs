use std::collections::HashMap;

use crate::api::types::ActiveTurn;
use crate::app::App;
use crate::id::{NousId, SessionId};
use crate::msg::ErrorToast;
use crate::sanitize::sanitize_for_display;
use crate::state::{ActiveTool, AgentState, AgentStatus};

const RECONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

#[tracing::instrument(skip_all)]
// SAFETY: sanitized at ingestion: agent data from API is sanitized here on SSE reconnect.
pub(crate) async fn handle_sse_connected(app: &mut App) {
    let was_disconnected = !app.connection.sse_connected;
    app.connection.sse_connected = true;
    app.connection.sse_disconnected_at = None;
    app.connection.sse_last_event_at = Some(std::time::Instant::now());
    if was_disconnected {
        app.connection.sse_reconnect_count += 1;
    }

    if was_disconnected {
        tracing::info!("SSE reconnected — reloading agent state");
        if let Ok(agents) = app.client.agents().await {
            let unread: HashMap<NousId, u32> = app
                .dashboard
                .agents
                .iter()
                .map(|a| (a.id.clone(), a.unread_count))
                .collect();

            app.dashboard.agents = agents
                .into_iter()
                .map(|a| {
                    let count = unread.get(&a.id).copied().unwrap_or(0);
                    let name = sanitize_for_display(a.display_name()).into_owned();
                    let name_lower = name.to_lowercase();
                    AgentState {
                        id: a.id.clone(),
                        name,
                        name_lower,
                        emoji: a.emoji.map(|e| sanitize_for_display(&e).into_owned()),
                        status: AgentStatus::Idle,
                        active_tool: None,
                        sessions: Vec::new(),
                        model: a.model.map(|m| sanitize_for_display(&m).into_owned()),
                        compaction_stage: None,
                        unread_count: count,
                        tools: Vec::new(),
                    }
                })
                .collect();
        }
        // WHY: skip reload when a stream is pending or active: the optimistic
        // user message and streaming state must not be clobbered by a full
        // history fetch triggered by SSE reconnection.
        if app.connection.stream_rx.is_none() && app.connection.active_turn_id.is_none() {
            app.load_focused_session().await;
        }
    }
}

#[tracing::instrument(skip_all)]
pub(crate) fn handle_sse_disconnected(app: &mut App) {
    app.connection.sse_connected = false;
    if app.connection.sse_disconnected_at.is_none() {
        app.connection.sse_disconnected_at = Some(std::time::Instant::now());
    }
}

/// Called on each tick to detect prolonged disconnection and surface an error.
pub(crate) fn check_sse_reconnect_timeout(app: &mut App) {
    if app.connection.sse_connected {
        return;
    }
    if let Some(disconnected_at) = app.connection.sse_disconnected_at
        && disconnected_at.elapsed() >= RECONNECT_TIMEOUT
        && app
            .viewport
            .error_toast
            .as_ref()
            .map(|t| !t.message.starts_with("Server unreachable"))
            .unwrap_or(true)
    {
        app.viewport.error_toast = Some(ErrorToast::new(
            "Server unreachable after 5 minutes. Check: journalctl --user -eu aletheia".to_string(),
        ));
    }
}

#[tracing::instrument(skip_all, fields(turn_count = active_turns.len()))]
pub(crate) fn handle_sse_init(app: &mut App, active_turns: Vec<ActiveTurn>) {
    app.connection.sse_last_event_at = Some(std::time::Instant::now());
    for turn in active_turns {
        if let Some(agent) = app
            .dashboard
            .agents
            .iter_mut()
            .find(|a| a.id == turn.nous_id)
        {
            agent.status = AgentStatus::Working;
        }
    }
}

#[tracing::instrument(skip_all, fields(%nous_id))]
pub(crate) fn handle_sse_turn_before(app: &mut App, nous_id: NousId) {
    if let Some(agent) = app.dashboard.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.status = AgentStatus::Working;
        agent.active_tool = None;
    }
}

#[tracing::instrument(skip_all, fields(%nous_id, %session_id))]
pub(crate) async fn handle_sse_turn_after(app: &mut App, nous_id: NousId, session_id: SessionId) {
    app.connection.sse_last_event_at = Some(std::time::Instant::now());
    let is_focused = app.dashboard.focused_agent.as_ref() == Some(&nous_id);
    if let Some(agent) = app.dashboard.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.status = AgentStatus::Idle;
        agent.active_tool = None;
        if !is_focused {
            agent.unread_count += 1;
            // Ring terminal bell for new messages on inactive agents.
            if app.layout.bell_enabled {
                let _ = std::io::Write::write_all(&mut std::io::stderr(), b"\x07");
            }
        }
    }
    // WHY: stream_rx is set by send_message before active_turn_id is set by
    // StreamTurnStart. Without this guard, a turn:after arriving in that gap
    // triggers load_focused_session which replaces the optimistic user message.
    if is_focused
        && app.dashboard.focused_session_id.as_ref() == Some(&session_id)
        && app.connection.active_turn_id.is_none()
        && app.connection.stream_rx.is_none()
    {
        app.load_focused_session().await;
    }

    app.layout.tab_bar.mark_unread(&nous_id, &session_id);
}

#[tracing::instrument(skip_all, fields(%nous_id, %tool_name))]
// SAFETY: sanitized at ingestion: tool name from SSE event.
pub(crate) fn handle_sse_tool_called(app: &mut App, nous_id: NousId, tool_name: String) {
    if let Some(agent) = app.dashboard.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.active_tool = Some(ActiveTool {
            name: sanitize_for_display(&tool_name).into_owned(),
            started_at: std::time::Instant::now(),
        });
    }
}

#[tracing::instrument(skip_all, fields(%nous_id))]
pub(crate) fn handle_sse_tool_failed(app: &mut App, nous_id: NousId) {
    if let Some(agent) = app.dashboard.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.active_tool = None;
    }
}

#[tracing::instrument(skip_all, fields(%nous_id, %status))]
pub(crate) fn handle_sse_status_update(app: &mut App, nous_id: NousId, status: String) {
    if let Some(agent) = app.dashboard.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.status = match status.as_str() {
            "working" => AgentStatus::Working,
            "streaming" => AgentStatus::Streaming,
            "compacting" => AgentStatus::Compacting,
            _ => AgentStatus::Idle,
        };
    }
}

#[tracing::instrument(skip_all, fields(%nous_id))]
pub(crate) async fn handle_sse_session_created(app: &mut App, nous_id: NousId) {
    if let Some(agent) = app.dashboard.agents.iter_mut().find(|a| a.id == nous_id)
        && let Ok(sessions) = app.client.sessions(&nous_id).await
    {
        agent.sessions = sessions;
    }
}

#[tracing::instrument(skip_all, fields(%nous_id, %session_id))]
pub(crate) fn handle_sse_session_archived(app: &mut App, nous_id: NousId, session_id: SessionId) {
    if let Some(agent) = app.dashboard.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.sessions.retain(|s| s.id != session_id);
    }
}

#[tracing::instrument(skip_all, fields(%nous_id))]
pub(crate) fn handle_sse_distill_before(app: &mut App, nous_id: NousId) {
    if let Some(agent) = app.dashboard.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.status = AgentStatus::Compacting;
        agent.compaction_stage = Some("starting".to_string());
    }
}

#[tracing::instrument(skip_all, fields(%nous_id, %stage))]
// SAFETY: sanitized at ingestion: distill stage from SSE event.
pub(crate) fn handle_sse_distill_stage(app: &mut App, nous_id: NousId, stage: String) {
    if let Some(agent) = app.dashboard.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.compaction_stage = Some(sanitize_for_display(&stage).into_owned());
    }
}

#[tracing::instrument(skip_all, fields(%nous_id))]
pub(crate) async fn handle_sse_distill_after(app: &mut App, nous_id: NousId) {
    if let Some(agent) = app.dashboard.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.status = AgentStatus::Idle;
        agent.compaction_stage = None;
    }
    if app.dashboard.focused_agent.as_ref() == Some(&nous_id) {
        app.load_focused_session().await;
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;
    use crate::app::test_helpers::*;

    #[test]
    fn sse_disconnected_sets_flag() {
        let mut app = test_app();
        app.connection.sse_connected = true;
        handle_sse_disconnected(&mut app);
        assert!(!app.connection.sse_connected);
    }

    #[test]
    fn sse_disconnected_records_timestamp() {
        let mut app = test_app();
        app.connection.sse_connected = true;
        handle_sse_disconnected(&mut app);
        assert!(app.connection.sse_disconnected_at.is_some());
    }

    #[test]
    fn sse_disconnected_does_not_overwrite_existing_timestamp() {
        let mut app = test_app();
        // Use a past instant to simulate "disconnected 10s ago"
        let earlier = std::time::Instant::now();
        app.connection.sse_disconnected_at = Some(earlier);
        handle_sse_disconnected(&mut app);
        // Timestamp must remain at the original disconnect time, not reset
        assert_eq!(app.connection.sse_disconnected_at, Some(earlier));
    }

    #[test]
    fn check_timeout_no_error_when_connected() {
        let mut app = test_app();
        app.connection.sse_connected = true;
        check_sse_reconnect_timeout(&mut app);
        assert!(app.viewport.error_toast.is_none());
    }

    #[test]
    fn check_timeout_no_error_when_disconnected_briefly() {
        let mut app = test_app();
        app.connection.sse_connected = false;
        app.connection.sse_disconnected_at = Some(std::time::Instant::now());
        check_sse_reconnect_timeout(&mut app);
        assert!(app.viewport.error_toast.is_none());
    }

    #[test]
    fn check_timeout_no_error_when_no_disconnect_time() {
        let mut app = test_app();
        app.connection.sse_connected = false;
        app.connection.sse_disconnected_at = None;
        check_sse_reconnect_timeout(&mut app);
        assert!(app.viewport.error_toast.is_none());
    }

    #[test]
    fn sse_init_marks_active_agents() {
        let mut app = test_app();
        app.dashboard.agents.push(test_agent("syn", "Syn"));
        app.dashboard.agents.push(test_agent("cody", "Cody"));

        let active_turns = vec![ActiveTurn {
            nous_id: "syn".into(),
            session_id: "s1".into(),
            turn_id: "t1".into(),
        }];

        handle_sse_init(&mut app, active_turns);

        assert_eq!(app.dashboard.agents[0].status, AgentStatus::Working);
        assert_eq!(app.dashboard.agents[1].status, AgentStatus::Idle);
    }

    #[test]
    fn sse_turn_before_sets_working() {
        let mut app = test_app();
        app.dashboard.agents.push(test_agent("syn", "Syn"));

        handle_sse_turn_before(&mut app, "syn".into());

        assert_eq!(app.dashboard.agents[0].status, AgentStatus::Working);
        assert!(app.dashboard.agents[0].active_tool.is_none());
    }

    #[test]
    fn sse_tool_called_sets_tool() {
        let mut app = test_app();
        app.dashboard.agents.push(test_agent("syn", "Syn"));

        handle_sse_tool_called(&mut app, "syn".into(), "read_file".to_string());

        let tool = app.dashboard.agents[0]
            .active_tool
            .as_ref()
            .expect("active_tool should be set");
        assert_eq!(tool.name, "read_file");
        // started_at should be very recent
        assert!(tool.started_at.elapsed().as_secs() < 5);
    }

    #[test]
    fn sse_tool_failed_clears_tool() {
        let mut app = test_app();
        app.dashboard.agents.push(test_agent("syn", "Syn"));
        app.dashboard.agents[0].active_tool = Some(ActiveTool {
            name: "read_file".to_string(),
            started_at: std::time::Instant::now(),
        });

        handle_sse_tool_failed(&mut app, "syn".into());

        assert!(app.dashboard.agents[0].active_tool.is_none());
    }

    #[test]
    fn sse_status_update_working() {
        let mut app = test_app();
        app.dashboard.agents.push(test_agent("syn", "Syn"));

        handle_sse_status_update(&mut app, "syn".into(), "working".to_string());
        assert_eq!(app.dashboard.agents[0].status, AgentStatus::Working);
    }

    #[test]
    fn sse_status_update_streaming() {
        let mut app = test_app();
        app.dashboard.agents.push(test_agent("syn", "Syn"));

        handle_sse_status_update(&mut app, "syn".into(), "streaming".to_string());
        assert_eq!(app.dashboard.agents[0].status, AgentStatus::Streaming);
    }

    #[test]
    fn sse_status_update_compacting() {
        let mut app = test_app();
        app.dashboard.agents.push(test_agent("syn", "Syn"));

        handle_sse_status_update(&mut app, "syn".into(), "compacting".to_string());
        assert_eq!(app.dashboard.agents[0].status, AgentStatus::Compacting);
    }

    #[test]
    fn sse_status_update_unknown_defaults_to_idle() {
        let mut app = test_app();
        app.dashboard.agents.push(test_agent("syn", "Syn"));
        app.dashboard.agents[0].status = AgentStatus::Working;

        handle_sse_status_update(&mut app, "syn".into(), "unknown_status".to_string());
        assert_eq!(app.dashboard.agents[0].status, AgentStatus::Idle);
    }

    #[test]
    fn sse_session_archived_removes_session() {
        let mut app = test_app();
        app.dashboard.agents.push(test_agent("syn", "Syn"));
        app.dashboard.agents[0]
            .sessions
            .push(crate::api::types::Session {
                id: "s1".into(),
                nous_id: "syn".into(),
                key: "main".to_string(),
                status: None,
                message_count: 0,
                session_type: None,
                updated_at: None,
                display_name: None,
            });

        handle_sse_session_archived(&mut app, "syn".into(), "s1".into());

        assert!(app.dashboard.agents[0].sessions.is_empty());
    }

    #[test]
    fn sse_distill_before_sets_compacting() {
        let mut app = test_app();
        app.dashboard.agents.push(test_agent("syn", "Syn"));

        handle_sse_distill_before(&mut app, "syn".into());

        assert_eq!(app.dashboard.agents[0].status, AgentStatus::Compacting);
        assert_eq!(
            app.dashboard.agents[0].compaction_stage.as_deref(),
            Some("starting")
        );
    }

    #[test]
    fn sse_distill_stage_updates_stage() {
        let mut app = test_app();
        app.dashboard.agents.push(test_agent("syn", "Syn"));
        app.dashboard.agents[0].status = AgentStatus::Compacting;

        handle_sse_distill_stage(&mut app, "syn".into(), "extracting".to_string());

        assert_eq!(
            app.dashboard.agents[0].compaction_stage.as_deref(),
            Some("extracting")
        );
    }

    #[test]
    fn sse_nonexistent_agent_noop() {
        let mut app = test_app();
        // No agents: should not panic
        handle_sse_turn_before(&mut app, "nonexistent".into());
        handle_sse_tool_called(&mut app, "nonexistent".into(), "tool".to_string());
        handle_sse_tool_failed(&mut app, "nonexistent".into());
        handle_sse_distill_before(&mut app, "nonexistent".into());
    }

    #[tokio::test]
    async fn turn_after_preserves_optimistic_message_when_stream_pending() {
        let mut app = test_app();
        app.dashboard.agents.push(test_agent("syn", "Syn"));
        app.dashboard.focused_agent = Some("syn".into());
        app.dashboard.focused_session_id = Some("s1".into());

        app.dashboard.messages.push(crate::state::ChatMessage {
            role: "user".to_string(),
            text: "hello".to_string(),
            text_lower: "hello".to_string(),
            timestamp: None,
            model: None,
            is_streaming: false,
            tool_calls: Vec::new(),
        });

        // Simulate the gap: stream started (stream_rx set) but StreamTurnStart
        // has not yet arrived (active_turn_id is None).
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        app.connection.stream_rx = Some(rx);

        handle_sse_turn_after(&mut app, "syn".into(), "s1".into()).await;

        assert_eq!(
            app.dashboard.messages.len(),
            1,
            "optimistic message must survive"
        );
        assert_eq!(app.dashboard.messages[0].text, "hello");
    }

    #[tokio::test]
    async fn turn_after_preserves_messages_when_turn_active() {
        let mut app = test_app();
        app.dashboard.agents.push(test_agent("syn", "Syn"));
        app.dashboard.focused_agent = Some("syn".into());
        app.dashboard.focused_session_id = Some("s1".into());
        app.connection.active_turn_id = Some("t1".into());

        app.dashboard.messages.push(crate::state::ChatMessage {
            role: "user".to_string(),
            text: "hello".to_string(),
            text_lower: "hello".to_string(),
            timestamp: None,
            model: None,
            is_streaming: false,
            tool_calls: Vec::new(),
        });

        handle_sse_turn_after(&mut app, "syn".into(), "s1".into()).await;

        assert_eq!(
            app.dashboard.messages.len(),
            1,
            "messages must survive during active turn"
        );
        assert_eq!(app.dashboard.messages[0].text, "hello");
    }

    #[tokio::test]
    async fn connected_preserves_optimistic_message_when_stream_pending() {
        let mut app = test_app();
        app.dashboard.agents.push(test_agent("syn", "Syn"));
        app.dashboard.focused_agent = Some("syn".into());
        app.dashboard.focused_session_id = Some("s1".into());
        app.connection.sse_connected = false;

        app.dashboard.messages.push(crate::state::ChatMessage {
            role: "user".to_string(),
            text: "hello".to_string(),
            text_lower: "hello".to_string(),
            timestamp: None,
            model: None,
            is_streaming: false,
            tool_calls: Vec::new(),
        });

        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        app.connection.stream_rx = Some(rx);

        handle_sse_connected(&mut app).await;

        assert_eq!(
            app.dashboard.messages.len(),
            1,
            "optimistic message must survive reconnect"
        );
        assert_eq!(app.dashboard.messages[0].text, "hello");
    }
}
