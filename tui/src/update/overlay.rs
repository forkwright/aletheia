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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_helpers::*;

    #[tokio::test]
    async fn open_overlay_help() {
        let mut app = test_app();
        handle_open_overlay(&mut app, OverlayKind::Help).await;
        assert!(matches!(app.overlay, Some(Overlay::Help)));
    }

    #[tokio::test]
    async fn open_overlay_agent_picker() {
        let mut app = test_app();
        handle_open_overlay(&mut app, OverlayKind::AgentPicker).await;
        assert!(matches!(app.overlay, Some(Overlay::AgentPicker { cursor: 0 })));
    }

    #[tokio::test]
    async fn open_overlay_system_status() {
        let mut app = test_app();
        handle_open_overlay(&mut app, OverlayKind::SystemStatus).await;
        assert!(matches!(app.overlay, Some(Overlay::SystemStatus)));
    }

    #[test]
    fn close_overlay_clears() {
        let mut app = test_app();
        app.overlay = Some(Overlay::Help);
        handle_close_overlay(&mut app);
        assert!(app.overlay.is_none());
    }

    #[test]
    fn close_overlay_settings_edit_mode_cancels_edit() {
        let mut app = test_app();
        let mut settings = crate::state::settings::SettingsOverlay::from_config(
            &serde_json::json!({"agents": {"defaults": {"maxToolIterations": 10}}}),
        );
        settings.editing = Some(crate::state::settings::EditState {
            buffer: "123".to_string(),
            cursor: 3,
        });
        app.overlay = Some(Overlay::Settings(settings));

        handle_close_overlay(&mut app);
        // Should cancel the edit, not close the overlay
        if let Some(Overlay::Settings(s)) = &app.overlay {
            assert!(s.editing.is_none());
        } else {
            panic!("overlay should still be Settings");
        }
    }

    #[test]
    fn overlay_up_agent_picker() {
        let mut app = test_app();
        app.agents.push(test_agent("a", "A"));
        app.agents.push(test_agent("b", "B"));
        app.overlay = Some(Overlay::AgentPicker { cursor: 1 });

        handle_overlay_up(&mut app);

        if let Some(Overlay::AgentPicker { cursor }) = &app.overlay {
            assert_eq!(*cursor, 0);
        }
    }

    #[test]
    fn overlay_up_saturates_at_zero() {
        let mut app = test_app();
        app.overlay = Some(Overlay::AgentPicker { cursor: 0 });

        handle_overlay_up(&mut app);

        if let Some(Overlay::AgentPicker { cursor }) = &app.overlay {
            assert_eq!(*cursor, 0);
        }
    }

    #[test]
    fn overlay_down_agent_picker() {
        let mut app = test_app();
        app.agents.push(test_agent("a", "A"));
        app.agents.push(test_agent("b", "B"));
        app.overlay = Some(Overlay::AgentPicker { cursor: 0 });

        handle_overlay_down(&mut app);

        if let Some(Overlay::AgentPicker { cursor }) = &app.overlay {
            assert_eq!(*cursor, 1);
        }
    }

    #[test]
    fn overlay_down_clamps_at_max() {
        let mut app = test_app();
        app.agents.push(test_agent("a", "A"));
        app.overlay = Some(Overlay::AgentPicker { cursor: 0 });

        handle_overlay_down(&mut app);

        if let Some(Overlay::AgentPicker { cursor }) = &app.overlay {
            assert_eq!(*cursor, 0);
        }
    }

    #[test]
    fn overlay_up_plan_approval() {
        let mut app = test_app();
        app.overlay = Some(Overlay::PlanApproval(crate::state::PlanApprovalOverlay {
            plan_id: "p1".into(),
            steps: vec![
                crate::state::PlanStepApproval {
                    id: 1,
                    label: "S1".to_string(),
                    role: "r".to_string(),
                    checked: true,
                },
                crate::state::PlanStepApproval {
                    id: 2,
                    label: "S2".to_string(),
                    role: "r".to_string(),
                    checked: true,
                },
            ],
            total_cost_cents: 100,
            cursor: 1,
        }));

        handle_overlay_up(&mut app);

        if let Some(Overlay::PlanApproval(plan)) = &app.overlay {
            assert_eq!(plan.cursor, 0);
        }
    }
}
