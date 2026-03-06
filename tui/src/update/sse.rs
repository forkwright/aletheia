use std::collections::HashMap;

use crate::api::types::ActiveTurn;
use crate::app::App;
use crate::id::{NousId, SessionId};
use crate::state::{AgentState, AgentStatus};

pub(crate) async fn handle_sse_connected(app: &mut App) {
    let was_disconnected = !app.sse_connected;
    app.sse_connected = true;

    if was_disconnected {
        tracing::info!("SSE reconnected — reloading agent state");
        if let Ok(agents) = app.client.agents().await {
            let notifications: HashMap<NousId, bool> = app
                .agents
                .iter()
                .map(|a| (a.id.clone(), a.has_notification))
                .collect();

            app.agents = agents
                .into_iter()
                .map(|a| {
                    let notif = notifications.get(&a.id).copied().unwrap_or(false);
                    AgentState {
                        id: a.id,
                        name: a.name,
                        emoji: a.emoji,
                        status: AgentStatus::Idle,
                        active_tool: None,
                        tool_started_at: None,
                        sessions: Vec::new(),
                        model: a.model,
                        compaction_stage: None,
                        has_notification: notif,
                    }
                })
                .collect();
        }
        app.load_focused_session().await;
    }
}

pub(crate) fn handle_sse_disconnected(app: &mut App) {
    app.sse_connected = false;
}

pub(crate) fn handle_sse_init(app: &mut App, active_turns: Vec<ActiveTurn>) {
    for turn in active_turns {
        if let Some(agent) = app.agents.iter_mut().find(|a| a.id == turn.nous_id) {
            agent.status = AgentStatus::Working;
        }
    }
}

pub(crate) fn handle_sse_turn_before(app: &mut App, nous_id: NousId) {
    if let Some(agent) = app.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.status = AgentStatus::Working;
        agent.active_tool = None;
    }
}

pub(crate) async fn handle_sse_turn_after(app: &mut App, nous_id: NousId, session_id: SessionId) {
    let is_focused = app.focused_agent.as_ref() == Some(&nous_id);
    if let Some(agent) = app.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.status = AgentStatus::Idle;
        agent.active_tool = None;
        agent.tool_started_at = None;
        if !is_focused {
            agent.has_notification = true;
        }
    }
    if is_focused
        && app.focused_session_id.as_ref() == Some(&session_id)
        && app.active_turn_id.is_none()
    {
        app.load_focused_session().await;
    }
}

pub(crate) fn handle_sse_tool_called(app: &mut App, nous_id: NousId, tool_name: String) {
    if let Some(agent) = app.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.active_tool = Some(tool_name);
        agent.tool_started_at = Some(std::time::Instant::now());
    }
}

pub(crate) fn handle_sse_tool_failed(app: &mut App, nous_id: NousId) {
    if let Some(agent) = app.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.active_tool = None;
        agent.tool_started_at = None;
    }
}

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

pub(crate) async fn handle_sse_session_created(app: &mut App, nous_id: NousId) {
    if let Some(agent) = app.agents.iter_mut().find(|a| a.id == nous_id) {
        if let Ok(sessions) = app.client.sessions(&nous_id).await {
            agent.sessions = sessions;
        }
    }
}

pub(crate) fn handle_sse_session_archived(app: &mut App, nous_id: NousId, session_id: SessionId) {
    if let Some(agent) = app.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.sessions.retain(|s| s.id != session_id);
    }
}

pub(crate) fn handle_sse_distill_before(app: &mut App, nous_id: NousId) {
    if let Some(agent) = app.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.status = AgentStatus::Compacting;
        agent.compaction_stage = Some("starting".to_string());
    }
}

pub(crate) fn handle_sse_distill_stage(app: &mut App, nous_id: NousId, stage: String) {
    if let Some(agent) = app.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.compaction_stage = Some(stage);
    }
}

pub(crate) async fn handle_sse_distill_after(app: &mut App, nous_id: NousId) {
    if let Some(agent) = app.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.status = AgentStatus::Idle;
        agent.compaction_stage = None;
    }
    if app.focused_agent.as_ref() == Some(&nous_id) {
        app.load_focused_session().await;
    }
}
