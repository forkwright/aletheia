use tracing::Instrument;

use crate::app::App;
use crate::msg::OverlayKind;
use crate::state::Overlay;

pub(crate) async fn handle_open_overlay(app: &mut App, kind: OverlayKind) {
    match kind {
        OverlayKind::Settings => {
            super::settings::handle_open(app).await;
        }
        other => {
            app.overlay = Some(match other {
                OverlayKind::Help => Overlay::Help,
                OverlayKind::AgentPicker => Overlay::AgentPicker { cursor: 0 },
                OverlayKind::SystemStatus => Overlay::SystemStatus,
                OverlayKind::Settings => unreachable!(),
            });
        }
    }
}

pub(crate) fn handle_close_overlay(app: &mut App) {
    // Settings edit mode: Esc cancels the edit, not the overlay
    if let Some(Overlay::Settings(ref s)) = app.overlay {
        if s.editing.is_some() {
            super::settings::handle_edit_escape(app);
            return;
        }
    }
    if let Some(Overlay::ToolApproval(ref approval)) = app.overlay {
        let turn_id = approval.turn_id.clone();
        let tool_id = approval.tool_id.clone();
        let client = app.client.clone();
        let span = tracing::info_span!("deny_tool");
        tokio::spawn(
            async move {
                if let Err(e) = client.deny_tool(&turn_id, &tool_id).await {
                    tracing::error!("failed to deny tool: {e}");
                }
            }
            .instrument(span),
        );
    }
    if let Some(Overlay::PlanApproval(ref plan)) = app.overlay {
        let plan_id = plan.plan_id.clone();
        let client = app.client.clone();
        let span = tracing::info_span!("cancel_plan");
        tokio::spawn(
            async move {
                if let Err(e) = client.cancel_plan(&plan_id).await {
                    tracing::error!("failed to cancel plan: {e}");
                }
            }
            .instrument(span),
        );
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
        Some(Overlay::Settings(_)) => {
            super::settings::handle_up(app);
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
        Some(Overlay::Settings(_)) => {
            super::settings::handle_down(app);
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
            let span = tracing::info_span!("approve_tool");
            tokio::spawn(
                async move {
                    if let Err(e) = client.approve_tool(&turn_id, &tool_id).await {
                        tracing::error!("failed to approve tool: {e}");
                    }
                }
                .instrument(span),
            );
            app.overlay = None;
        }
        Some(Overlay::PlanApproval(plan)) => {
            let plan_id = plan.plan_id.clone();
            let client = app.client.clone();
            let span = tracing::info_span!("approve_plan");
            tokio::spawn(
                async move {
                    if let Err(e) = client.approve_plan(&plan_id).await {
                        tracing::error!("failed to approve plan: {e}");
                    }
                }
                .instrument(span),
            );
            app.overlay = None;
        }
        Some(Overlay::Settings(_)) => {
            super::settings::handle_enter(app);
        }
        _ => {
            app.overlay = None;
        }
    }
}
