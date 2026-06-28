use std::collections::HashMap;

use crate::api::types::ActiveTurn;
use crate::app::App;
use crate::id::{NousId, SessionId};
use crate::msg::ErrorToast;
use crate::sanitize::sanitize_for_display;
use crate::state::{ActiveTool, AgentState, AgentStatus, ChatMessage};

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
                        status: AgentStatus::from(a.status),
                        active_tool: None,
                        sessions: Vec::new(),
                        model: a.model.map(|m| sanitize_for_display(&m).into_owned()),
                        compaction_stage: None,
                        distill_completed_at: None,
                        unread_count: count,
                        tools: Vec::new(),
                    }
                })
                .collect();
        }
        // WHY: skip reload when a stream is pending, active, or just completed:
        // the optimistic user message and streaming state must not be clobbered
        // by a full history fetch triggered by SSE reconnection.
        if !app.connection.is_stream_busy() {
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

/// Called on each tick to detect prolonged disconnection and force reconnection.
///
/// WHY: A stalled SSE connection could persist indefinitely, leaving the agent
/// appearing active but actually stuck. After `RECONNECT_TIMEOUT` we tear down
/// the old connection and establish a new one rather than only showing a banner.
pub(crate) fn check_sse_reconnect_timeout(app: &mut App) {
    if app.connection.sse_connected {
        return;
    }
    let Some(disconnected_at) = app.connection.sse_disconnected_at else {
        return;
    };
    let stall_duration = disconnected_at.elapsed();
    if stall_duration < RECONNECT_TIMEOUT {
        return;
    }
    // WHY: Fire once per disconnect cycle -- clearing the timestamp keeps this
    // branch from re-triggering every tick until a fresh SseDisconnected event
    // sets a new timestamp.
    app.connection.sse_disconnected_at = None;

    let stall_secs = stall_duration.as_secs();
    tracing::warn!(
        stall_secs,
        "SSE stalled for {stall_secs}s — forcing reconnect"
    );

    app.restore_sse(Some(crate::api::sse::SseConnection::connect(
        app.client.streaming_client().clone(),
        &app.config.url,
    )));

    app.viewport.error_toast = Some(ErrorToast::new(format!(
        "Connection stalled for {stall_secs}s — reconnecting..."
    )));
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
            if app.layout.bell_enabled {
                // kanon:ignore RUST/no-silent-result-swallow — best-effort terminal bell; failure is benign
                let _ = std::io::Write::write_all(&mut std::io::stderr(), b"\x07");
            }
        }
    }
    // WHY: stream_rx is set by send_message before active_turn_id is set by
    // StreamTurnStart. Without this guard, a turn:after arriving in that gap
    // triggers load_focused_session which replaces the optimistic user message.
    // is_stream_busy also catches the Done phase window after TurnComplete.
    if is_focused
        && app.dashboard.focused_session_id.as_ref() == Some(&session_id)
        && !app.connection.is_stream_busy()
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
pub(crate) fn handle_sse_status_update(
    app: &mut App,
    nous_id: NousId,
    status: koina::agent::AgentLifecycle,
) {
    if let Some(agent) = app.dashboard.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.status = AgentStatus::from(status);
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

#[tracing::instrument(skip_all, fields(dropped))]
pub(crate) async fn handle_sse_stream_lagged(app: &mut App, dropped: u64) {
    app.connection.sse_last_event_at = Some(std::time::Instant::now());
    app.viewport.error_toast = Some(ErrorToast::new(format!(
        "Stream lagged; {dropped} events dropped - resyncing..."
    )));
    // WHY: the server explicitly told us we missed events; refresh the focused
    // session so the UI does not stay stale relative to the recovered stream.
    if !app.connection.is_stream_busy() {
        app.load_focused_session().await;
    }
}

#[tracing::instrument(skip_all, fields(%nous_id))]
pub(crate) async fn handle_sse_distill_after(app: &mut App, nous_id: NousId) {
    if let Some(agent) = app.dashboard.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.status = AgentStatus::Idle;
        agent.compaction_stage = Some("done".to_string());
        agent.distill_completed_at = Some(std::time::Instant::now());
    }
    // WHY: skip reload when a stream is busy to avoid clobbering streaming state.
    // The history will be refreshed on the next idle reload opportunity.
    if app.dashboard.focused_agent.as_ref() == Some(&nous_id) && !app.connection.is_stream_busy() {
        app.load_focused_session().await;
        app.dashboard.messages.push(ChatMessage {
            role: "system".to_string(),
            text: "— conversation summarized —".to_string(),
            text_lower: "— conversation summarized —".to_string(),
            timestamp: None,
            model: None,
            tool_calls: Vec::new(),
            kind: crate::state::MessageKind::default(),
        });
    }
}

/// Auto-dismiss distillation stage indicator after 3 seconds.
pub(crate) fn check_distill_auto_dismiss(app: &mut App) {
    const DISMISS_DELAY: std::time::Duration = std::time::Duration::from_secs(3);
    for agent in &mut app.dashboard.agents {
        if agent
            .distill_completed_at
            .is_some_and(|t| t.elapsed() >= DISMISS_DELAY)
        {
            agent.compaction_stage = None;
            agent.distill_completed_at = None;
        }
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions may panic on failure")]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions use direct indexing for clarity"
)]
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

    #[tokio::test]
    async fn check_timeout_forces_reconnect_after_threshold() {
        let mut app = test_app();
        app.connection.sse_connected = false;
        // Set disconnected_at to well past the threshold
        app.connection.sse_disconnected_at =
            Some(std::time::Instant::now() - RECONNECT_TIMEOUT - std::time::Duration::from_secs(1));
        check_sse_reconnect_timeout(&mut app);
        // Should show a reconnecting toast
        assert!(app.viewport.error_toast.is_some());
        let msg = &app.viewport.error_toast.as_ref().unwrap().message;
        assert!(
            msg.contains("reconnecting"),
            "toast should mention reconnecting, got: {msg}"
        );
        // Disconnect timestamp should be cleared to prevent re-triggering
        assert!(app.connection.sse_disconnected_at.is_none());
        // SSE connection re-established: verify via take_sse which extracts the
        // private field through the public API.
        assert!(
            app.take_sse().is_some(),
            "SSE connection should be re-established"
        );
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
    fn sse_status_update_active() {
        let mut app = test_app();
        app.dashboard.agents.push(test_agent("syn", "Syn"));

        handle_sse_status_update(&mut app, "syn".into(), koina::agent::AgentLifecycle::Active);
        assert_eq!(app.dashboard.agents[0].status, AgentStatus::Working);
    }

    #[test]
    fn sse_status_update_degraded() {
        let mut app = test_app();
        app.dashboard.agents.push(test_agent("syn", "Syn"));

        handle_sse_status_update(
            &mut app,
            "syn".into(),
            koina::agent::AgentLifecycle::Degraded,
        );
        assert_eq!(app.dashboard.agents[0].status, AgentStatus::Degraded);
    }

    #[test]
    fn sse_status_update_disabled() {
        let mut app = test_app();
        app.dashboard.agents.push(test_agent("syn", "Syn"));

        handle_sse_status_update(
            &mut app,
            "syn".into(),
            koina::agent::AgentLifecycle::Disabled,
        );
        assert_eq!(app.dashboard.agents[0].status, AgentStatus::Disabled);
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
            tool_calls: Vec::new(),
            kind: crate::state::MessageKind::default(),
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
            tool_calls: Vec::new(),
            kind: crate::state::MessageKind::default(),
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
            tool_calls: Vec::new(),
            kind: crate::state::MessageKind::default(),
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
