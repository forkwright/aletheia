use crate::app::App;
use crate::msg::OverlayKind;
use crate::state::Overlay;

pub(crate) fn handle_open_overlay(app: &mut App, kind: OverlayKind) {
    app.overlay = Some(match kind {
        OverlayKind::Help => Overlay::Help,
        OverlayKind::AgentPicker => Overlay::AgentPicker { cursor: 0 },
        OverlayKind::SystemStatus => Overlay::SystemStatus,
    });
}

pub(crate) fn handle_close_overlay(app: &mut App) {
    if let Some(Overlay::ToolApproval(ref approval)) = app.overlay {
        let turn_id = approval.turn_id.clone();
        let tool_id = approval.tool_id.clone();
        let client = app.client.clone();
        tokio::spawn(async move {
            if let Err(e) = client.deny_tool(&turn_id, &tool_id).await {
                tracing::error!("failed to deny tool: {e}");
            }
        });
    }
    if let Some(Overlay::PlanApproval(ref plan)) = app.overlay {
        let plan_id = plan.plan_id.clone();
        let client = app.client.clone();
        tokio::spawn(async move {
            if let Err(e) = client.cancel_plan(&plan_id).await {
                tracing::error!("failed to cancel plan: {e}");
            }
        });
    }
    app.overlay = None;
}

pub(crate) fn handle_overlay_up(app: &mut App) {
    match &mut app.overlay {
        Some(Overlay::AgentPicker { cursor }) => {
            *cursor = cursor.saturating_sub(1);
        }
        Some(Overlay::PlanApproval(plan)) => {
            plan.cursor = plan.cursor.saturating_sub(1);
        }
        _ => {}
    }
}

pub(crate) fn handle_overlay_down(app: &mut App) {
    match &mut app.overlay {
        Some(Overlay::AgentPicker { cursor }) => {
            let max = app.agents.len().saturating_sub(1);
            *cursor = (*cursor + 1).min(max);
        }
        Some(Overlay::PlanApproval(plan)) => {
            let max = plan.steps.len().saturating_sub(1);
            plan.cursor = (plan.cursor + 1).min(max);
        }
        _ => {}
    }
}

pub(crate) async fn handle_overlay_select(app: &mut App) {
    match &app.overlay {
        Some(Overlay::AgentPicker { cursor }) => {
            if let Some(agent) = app.agents.get_mut(*cursor) {
                agent.has_notification = false;
                let id = agent.id.clone();
                app.focused_agent = Some(id);
                app.overlay = None;
                app.load_focused_session().await;
            }
        }
        Some(Overlay::ToolApproval(approval)) => {
            let turn_id = approval.turn_id.clone();
            let tool_id = approval.tool_id.clone();
            let client = app.client.clone();
            tokio::spawn(async move {
                if let Err(e) = client.approve_tool(&turn_id, &tool_id).await {
                    tracing::error!("failed to approve tool: {e}");
                }
            });
            app.overlay = None;
        }
        Some(Overlay::PlanApproval(plan)) => {
            let plan_id = plan.plan_id.clone();
            let client = app.client.clone();
            tokio::spawn(async move {
                if let Err(e) = client.approve_plan(&plan_id).await {
                    tracing::error!("failed to approve plan: {e}");
                }
            });
            app.overlay = None;
        }
        _ => {
            app.overlay = None;
        }
    }
}
