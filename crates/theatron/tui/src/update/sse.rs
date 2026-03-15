use std::collections::HashMap;

use crate::api::types::ActiveTurn;
use crate::app::App;
use crate::id::{NousId, SessionId};
use crate::msg::ErrorToast;
use crate::sanitize::sanitize_for_display;
use crate::state::{ActiveTool, AgentState, AgentStatus};

const RECONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

#[tracing::instrument(skip_all)]
// SAFETY: sanitized at ingestion — agent data from API is sanitized here on SSE reconnect.
pub(crate) async fn handle_sse_connected(app: &mut App) {
    let was_disconnected = !app.sse_connected;
    app.sse_connected = true;
    app.sse_disconnected_at = None;
    app.sse_last_event_at = Some(std::time::Instant::now());
    if was_disconnected {
        app.sse_reconnect_count += 1;
    }

    if was_disconnected {
        tracing::info!("SSE reconnected — reloading agent state");
        if let Ok(agents) = app.client.agents().await {
            let unread: HashMap<NousId, u32> = app
                .agents
                .iter()
                .map(|a| (a.id.clone(), a.unread_count))
                .collect();

            app.agents = agents
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
                    }
                })
                .collect();
        }
        app.load_focused_session().await;
    }
}

#[tracing::instrument(skip_all)]
pub(crate) fn handle_sse_disconnected(app: &mut App) {
    app.sse_connected = false;
    if app.sse_disconnected_at.is_none() {
        app.sse_disconnected_at = Some(std::time::Instant::now());
    }
}

/// Called on each tick to detect prolonged disconnection and surface an error.
pub(crate) fn check_sse_reconnect_timeout(app: &mut App) {
    if app.sse_connected {
        return;
    }
    if let Some(disconnected_at) = app.sse_disconnected_at
        && disconnected_at.elapsed() >= RECONNECT_TIMEOUT
        && app
            .error_toast
            .as_ref()
            .map(|t| !t.message.starts_with("Server unreachable"))
            .unwrap_or(true)
    {
        app.error_toast = Some(ErrorToast::new(
            "Server unreachable after 5 minutes. Check: journalctl --user -eu aletheia".to_string(),
        ));
    }
}

#[tracing::instrument(skip_all, fields(turn_count = active_turns.len()))]
pub(crate) fn handle_sse_init(app: &mut App, active_turns: Vec<ActiveTurn>) {
    app.sse_last_event_at = Some(std::time::Instant::now());
    for turn in active_turns {
        if let Some(agent) = app.agents.iter_mut().find(|a| a.id == turn.nous_id) {
            agent.status = AgentStatus::Working;
        }
    }
}

#[tracing::instrument(skip_all, fields(%nous_id))]
pub(crate) fn handle_sse_turn_before(app: &mut App, nous_id: NousId) {
    if let Some(agent) = app.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.status = AgentStatus::Working;
        agent.active_tool = None;
    }
}

#[tracing::instrument(skip_all, fields(%nous_id, %session_id))]
pub(crate) async fn handle_sse_turn_after(app: &mut App, nous_id: NousId, session_id: SessionId) {
    app.sse_last_event_at = Some(std::time::Instant::now());
    let is_focused = app.focused_agent.as_ref() == Some(&nous_id);
    if let Some(agent) = app.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.status = AgentStatus::Idle;
        agent.active_tool = None;
        if !is_focused {
            agent.unread_count += 1;
            // Ring terminal bell for new messages on inactive agents.
            if app.bell_enabled {
                let _ = std::io::Write::write_all(&mut std::io::stderr(), b"\x07");
            }
        }
    }
    if is_focused
        && app.focused_session_id.as_ref() == Some(&session_id)
        && app.active_turn_id.is_none()
    {
        app.load_focused_session().await;
    }

    app.tab_bar.mark_unread(&nous_id, &session_id);
}

#[tracing::instrument(skip_all, fields(%nous_id, %tool_name))]
// SAFETY: sanitized at ingestion — tool name from SSE event.
pub(crate) fn handle_sse_tool_called(app: &mut App, nous_id: NousId, tool_name: String) {
    if let Some(agent) = app.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.active_tool = Some(ActiveTool {
            name: sanitize_for_display(&tool_name).into_owned(),
            started_at: std::time::Instant::now(),
        });
    }
}

#[tracing::instrument(skip_all, fields(%nous_id))]
pub(crate) fn handle_sse_tool_failed(app: &mut App, nous_id: NousId) {
    if let Some(agent) = app.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.active_tool = None;
    }
}

#[tracing::instrument(skip_all, fields(%nous_id, %status))]
pub(crate) fn handle_sse_status_update(app: &mut App, nous_id: NousId, status: String) {
    if let Some(agent) = app.agents.iter_mut().find(|a| a.id == nous_id) {
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
    if let Some(agent) = app.agents.iter_mut().find(|a| a.id == nous_id)
        && let Ok(sessions) = app.client.sessions(&nous_id).await
    {
        agent.sessions = sessions;
    }
}

#[tracing::instrument(skip_all, fields(%nous_id, %session_id))]
pub(crate) fn handle_sse_session_archived(app: &mut App, nous_id: NousId, session_id: SessionId) {
    if let Some(agent) = app.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.sessions.retain(|s| s.id != session_id);
    }
}

#[tracing::instrument(skip_all, fields(%nous_id))]
pub(crate) fn handle_sse_distill_before(app: &mut App, nous_id: NousId) {
    if let Some(agent) = app.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.status = AgentStatus::Compacting;
        agent.compaction_stage = Some("starting".to_string());
    }
}

#[tracing::instrument(skip_all, fields(%nous_id, %stage))]
// SAFETY: sanitized at ingestion — distill stage from SSE event.
pub(crate) fn handle_sse_distill_stage(app: &mut App, nous_id: NousId, stage: String) {
    if let Some(agent) = app.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.compaction_stage = Some(sanitize_for_display(&stage).into_owned());
    }
}

#[tracing::instrument(skip_all, fields(%nous_id))]
pub(crate) async fn handle_sse_distill_after(app: &mut App, nous_id: NousId) {
    if let Some(agent) = app.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.status = AgentStatus::Idle;
        agent.compaction_stage = None;
    }
    if app.focused_agent.as_ref() == Some(&nous_id) {
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
        app.sse_connected = true;
        handle_sse_disconnected(&mut app);
        assert!(!app.sse_connected);
    }

    #[test]
    fn sse_disconnected_records_timestamp() {
        let mut app = test_app();
        app.sse_connected = true;
        handle_sse_disconnected(&mut app);
        assert!(app.sse_disconnected_at.is_some());
    }

    #[test]
    fn sse_disconnected_does_not_overwrite_existing_timestamp() {
        let mut app = test_app();
        // Use a past instant to simulate "disconnected 10s ago"
        let earlier = std::time::Instant::now();
        app.sse_disconnected_at = Some(earlier);
        handle_sse_disconnected(&mut app);
        // Timestamp must remain at the original disconnect time, not reset
        assert_eq!(app.sse_disconnected_at, Some(earlier));
    }

    #[test]
    fn check_timeout_no_error_when_connected() {
        let mut app = test_app();
        app.sse_connected = true;
        check_sse_reconnect_timeout(&mut app);
        assert!(app.error_toast.is_none());
    }

    #[test]
    fn check_timeout_no_error_when_disconnected_briefly() {
        let mut app = test_app();
        app.sse_connected = false;
        app.sse_disconnected_at = Some(std::time::Instant::now());
        check_sse_reconnect_timeout(&mut app);
        assert!(app.error_toast.is_none());
    }

    #[test]
    fn check_timeout_no_error_when_no_disconnect_time() {
        let mut app = test_app();
        app.sse_connected = false;
        app.sse_disconnected_at = None;
        check_sse_reconnect_timeout(&mut app);
        assert!(app.error_toast.is_none());
    }

    #[test]
    fn sse_init_marks_active_agents() {
        let mut app = test_app();
        app.agents.push(test_agent("syn", "Syn"));
        app.agents.push(test_agent("cody", "Cody"));

        let active_turns = vec![ActiveTurn {
            nous_id: "syn".into(),
            session_id: "s1".into(),
            turn_id: "t1".into(),
        }];

        handle_sse_init(&mut app, active_turns);

        assert_eq!(app.agents[0].status, AgentStatus::Working);
        assert_eq!(app.agents[1].status, AgentStatus::Idle);
    }

    #[test]
    fn sse_turn_before_sets_working() {
        let mut app = test_app();
        app.agents.push(test_agent("syn", "Syn"));

        handle_sse_turn_before(&mut app, "syn".into());

        assert_eq!(app.agents[0].status, AgentStatus::Working);
        assert!(app.agents[0].active_tool.is_none());
    }

    #[test]
    fn sse_tool_called_sets_tool() {
        let mut app = test_app();
        app.agents.push(test_agent("syn", "Syn"));

        handle_sse_tool_called(&mut app, "syn".into(), "read_file".to_string());

        let tool = app.agents[0]
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
        app.agents.push(test_agent("syn", "Syn"));
        app.agents[0].active_tool = Some(ActiveTool {
            name: "read_file".to_string(),
            started_at: std::time::Instant::now(),
        });

        handle_sse_tool_failed(&mut app, "syn".into());

        assert!(app.agents[0].active_tool.is_none());
    }

    #[test]
    fn sse_status_update_working() {
        let mut app = test_app();
        app.agents.push(test_agent("syn", "Syn"));

        handle_sse_status_update(&mut app, "syn".into(), "working".to_string());
        assert_eq!(app.agents[0].status, AgentStatus::Working);
    }

    #[test]
    fn sse_status_update_streaming() {
        let mut app = test_app();
        app.agents.push(test_agent("syn", "Syn"));

        handle_sse_status_update(&mut app, "syn".into(), "streaming".to_string());
        assert_eq!(app.agents[0].status, AgentStatus::Streaming);
    }

    #[test]
    fn sse_status_update_compacting() {
        let mut app = test_app();
        app.agents.push(test_agent("syn", "Syn"));

        handle_sse_status_update(&mut app, "syn".into(), "compacting".to_string());
        assert_eq!(app.agents[0].status, AgentStatus::Compacting);
    }

    #[test]
    fn sse_status_update_unknown_defaults_to_idle() {
        let mut app = test_app();
        app.agents.push(test_agent("syn", "Syn"));
        app.agents[0].status = AgentStatus::Working;

        handle_sse_status_update(&mut app, "syn".into(), "unknown_status".to_string());
        assert_eq!(app.agents[0].status, AgentStatus::Idle);
    }

    #[test]
    fn sse_session_archived_removes_session() {
        let mut app = test_app();
        app.agents.push(test_agent("syn", "Syn"));
        app.agents[0].sessions.push(crate::api::types::Session {
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

        assert!(app.agents[0].sessions.is_empty());
    }

    #[test]
    fn sse_distill_before_sets_compacting() {
        let mut app = test_app();
        app.agents.push(test_agent("syn", "Syn"));

        handle_sse_distill_before(&mut app, "syn".into());

        assert_eq!(app.agents[0].status, AgentStatus::Compacting);
        assert_eq!(app.agents[0].compaction_stage.as_deref(), Some("starting"));
    }

    #[test]
    fn sse_distill_stage_updates_stage() {
        let mut app = test_app();
        app.agents.push(test_agent("syn", "Syn"));
        app.agents[0].status = AgentStatus::Compacting;

        handle_sse_distill_stage(&mut app, "syn".into(), "extracting".to_string());

        assert_eq!(
            app.agents[0].compaction_stage.as_deref(),
            Some("extracting")
        );
    }

    #[test]
    fn sse_nonexistent_agent_noop() {
        let mut app = test_app();
        // No agents — should not panic
        handle_sse_turn_before(&mut app, "nonexistent".into());
        handle_sse_tool_called(&mut app, "nonexistent".into(), "tool".to_string());
        handle_sse_tool_failed(&mut app, "nonexistent".into());
        handle_sse_distill_before(&mut app, "nonexistent".into());
    }
}
