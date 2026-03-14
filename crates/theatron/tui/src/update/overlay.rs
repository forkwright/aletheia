use tracing::Instrument;

use crate::app::App;
use crate::msg::OverlayKind;
use crate::state::{Overlay, SessionPickerOverlay};

pub(crate) async fn handle_open_overlay(app: &mut App, kind: OverlayKind) {
    match kind {
        OverlayKind::Settings => {
            super::settings::handle_open(app).await;
        }
        other => {
            app.overlay = Some(match other {
                OverlayKind::Help => Overlay::Help,
                OverlayKind::AgentPicker => Overlay::AgentPicker { cursor: 0 },
                OverlayKind::SessionPicker => Overlay::SessionPicker(SessionPickerOverlay {
                    cursor: 0,
                    show_archived: false,
                }),
                OverlayKind::SessionPickerAll => Overlay::SessionPicker(SessionPickerOverlay {
                    cursor: 0,
                    show_archived: true,
                }),
                OverlayKind::SystemStatus => Overlay::SystemStatus,
                OverlayKind::Settings => unreachable!(),
            });
        }
    }
}

pub(crate) fn handle_close_overlay(app: &mut App) {
    // NOTE: Esc in settings edit mode cancels the edit, not the overlay itself
    if let Some(Overlay::Settings(ref s)) = app.overlay
        && s.editing.is_some()
    {
        super::settings::handle_edit_escape(app);
        return;
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
        Some(Overlay::SessionPicker(picker)) => {
            picker.cursor = picker.cursor.saturating_sub(1);
        }
        Some(Overlay::PlanApproval(plan)) => {
            plan.cursor = plan.cursor.saturating_sub(1);
        }
        Some(Overlay::ContextActions(ctx)) => {
            ctx.cursor = ctx.cursor.saturating_sub(1);
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
        Some(Overlay::SessionPicker(picker)) => {
            let show_archived = picker.show_archived;
            let cursor = picker.cursor;
            let count = visible_session_count(app, show_archived);
            let max = count.saturating_sub(1);
            if let Some(Overlay::SessionPicker(picker)) = &mut app.overlay {
                picker.cursor = (cursor + 1).min(max);
            }
        }
        Some(Overlay::PlanApproval(plan)) => {
            let max = plan.steps.len().saturating_sub(1);
            plan.cursor = (plan.cursor + 1).min(max);
        }
        Some(Overlay::ContextActions(ctx)) => {
            let max = ctx.actions.len().saturating_sub(1);
            ctx.cursor = (ctx.cursor + 1).min(max);
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
        Some(Overlay::SessionPicker(picker)) => {
            let cursor = picker.cursor;
            let show_archived = picker.show_archived;
            if let Some(session_id) = pick_session_id(app, cursor, show_archived) {
                app.save_scroll_state();
                app.focused_session_id = Some(session_id.clone());
                app.overlay = None;
                match app.client.history(&session_id).await {
                    Ok(history) => {
                        app.messages = history
                            .into_iter()
                            .filter_map(|m| {
                                if m.role != "user" && m.role != "assistant" {
                                    return None;
                                }
                                let text = crate::update::extract_text_content(&m.content)?;
                                use crate::sanitize::sanitize_for_display;
                                let text = sanitize_for_display(&text).into_owned();
                                let text_lower = text.to_lowercase();
                                Some(crate::state::ChatMessage {
                                    role: sanitize_for_display(&m.role).into_owned(),
                                    text,
                                    text_lower,
                                    timestamp: m
                                        .created_at
                                        .map(|t| sanitize_for_display(&t).into_owned()),
                                    model: m.model.map(|m| sanitize_for_display(&m).into_owned()),
                                    is_streaming: false,
                                    tool_calls: Vec::new(),
                                })
                            })
                            .collect();
                        app.scroll_to_bottom();
                    }
                    Err(e) => {
                        tracing::error!("failed to load history: {e}");
                        app.error_toast =
                            Some(crate::msg::ErrorToast::new(format!("Load failed: {e}")));
                    }
                }
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
        Some(Overlay::ContextActions(ctx)) => {
            if let Some(action) = ctx.selected_action() {
                let kind = action.kind;
                app.overlay = None;
                super::selection::handle_message_action(app, kind);
            }
        }
        Some(Overlay::Settings(_)) => {
            super::settings::handle_enter(app);
        }
        _ => {
            app.overlay = None;
        }
    }
}

pub(crate) fn visible_session_count(app: &App, show_archived: bool) -> usize {
    let agent_id = match &app.focused_agent {
        Some(id) => id,
        None => return 0,
    };
    let agent = match app.agents.iter().find(|a| &a.id == agent_id) {
        Some(a) => a,
        None => return 0,
    };
    if show_archived {
        agent.sessions.len()
    } else {
        agent.sessions.iter().filter(|s| s.is_interactive()).count()
    }
}

pub(crate) fn pick_session_id_pub(
    app: &App,
    cursor: usize,
    show_archived: bool,
) -> Option<crate::id::SessionId> {
    pick_session_id(app, cursor, show_archived)
}

fn pick_session_id(app: &App, cursor: usize, show_archived: bool) -> Option<crate::id::SessionId> {
    let agent_id = app.focused_agent.as_ref()?;
    let agent = app.agents.iter().find(|a| &a.id == agent_id)?;
    let sessions: Vec<_> = if show_archived {
        agent.sessions.iter().collect()
    } else {
        agent
            .sessions
            .iter()
            .filter(|s| s.is_interactive())
            .collect()
    };
    sessions.get(cursor).map(|s| s.id.clone())
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
        assert!(matches!(
            app.overlay,
            Some(Overlay::AgentPicker { cursor: 0 })
        ));
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
        let Some(Overlay::Settings(s)) = &app.overlay else {
            unreachable!("overlay should still be Settings");
        };
        assert!(s.editing.is_none());
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

    fn test_session(id: &str, key: &str) -> crate::api::types::Session {
        crate::api::types::Session {
            id: id.into(),
            nous_id: "syn".into(),
            key: key.to_string(),
            status: None,
            message_count: 0,
            session_type: None,
            updated_at: None,
            display_name: None,
        }
    }

    fn test_session_archived(id: &str, key: &str) -> crate::api::types::Session {
        crate::api::types::Session {
            status: Some("archived".to_string()),
            ..test_session(id, key)
        }
    }

    #[tokio::test]
    async fn open_overlay_session_picker() {
        let mut app = test_app();
        handle_open_overlay(&mut app, OverlayKind::SessionPicker).await;
        let Some(Overlay::SessionPicker(picker)) = &app.overlay else {
            unreachable!("expected SessionPicker overlay");
        };
        assert_eq!(picker.cursor, 0);
        assert!(!picker.show_archived);
    }

    #[test]
    fn session_picker_up_saturates_at_zero() {
        let mut app = test_app();
        app.overlay = Some(Overlay::SessionPicker(SessionPickerOverlay {
            cursor: 0,
            show_archived: false,
        }));
        handle_overlay_up(&mut app);
        if let Some(Overlay::SessionPicker(picker)) = &app.overlay {
            assert_eq!(picker.cursor, 0);
        }
    }

    #[test]
    fn session_picker_up_decrements() {
        let mut app = test_app();
        app.overlay = Some(Overlay::SessionPicker(SessionPickerOverlay {
            cursor: 2,
            show_archived: false,
        }));
        handle_overlay_up(&mut app);
        if let Some(Overlay::SessionPicker(picker)) = &app.overlay {
            assert_eq!(picker.cursor, 1);
        }
    }

    #[test]
    fn session_picker_down_clamps_at_max() {
        let mut app = test_app();
        let mut agent = test_agent("syn", "Syn");
        agent.sessions.push(test_session("s1", "main"));
        agent.sessions.push(test_session("s2", "debug"));
        app.agents.push(agent);
        app.focused_agent = Some("syn".into());
        app.overlay = Some(Overlay::SessionPicker(SessionPickerOverlay {
            cursor: 1,
            show_archived: false,
        }));
        handle_overlay_down(&mut app);
        if let Some(Overlay::SessionPicker(picker)) = &app.overlay {
            assert_eq!(picker.cursor, 1);
        }
    }

    #[test]
    fn session_picker_down_increments() {
        let mut app = test_app();
        let mut agent = test_agent("syn", "Syn");
        agent.sessions.push(test_session("s1", "main"));
        agent.sessions.push(test_session("s2", "debug"));
        agent.sessions.push(test_session("s3", "test"));
        app.agents.push(agent);
        app.focused_agent = Some("syn".into());
        app.overlay = Some(Overlay::SessionPicker(SessionPickerOverlay {
            cursor: 0,
            show_archived: false,
        }));
        handle_overlay_down(&mut app);
        if let Some(Overlay::SessionPicker(picker)) = &app.overlay {
            assert_eq!(picker.cursor, 1);
        }
    }

    #[test]
    fn visible_session_count_filters_non_interactive() {
        let mut app = test_app();
        let mut agent = test_agent("syn", "Syn");
        agent.sessions.push(test_session("s1", "main"));
        agent.sessions.push(test_session("s2", "cron:daily"));
        agent.sessions.push(test_session("s3", "prosoche-wake"));
        agent.sessions.push(test_session("s4", "agent:sub"));
        agent.sessions.push(test_session("s5", "debug"));
        app.agents.push(agent);
        app.focused_agent = Some("syn".into());

        assert_eq!(visible_session_count(&app, false), 2);
        assert_eq!(visible_session_count(&app, true), 5);
    }

    #[test]
    fn visible_session_count_no_focused_agent() {
        let app = test_app();
        assert_eq!(visible_session_count(&app, false), 0);
    }

    #[test]
    fn pick_session_id_returns_correct_session() {
        let mut app = test_app();
        let mut agent = test_agent("syn", "Syn");
        agent.sessions.push(test_session("s1", "main"));
        agent.sessions.push(test_session("s2", "debug"));
        app.agents.push(agent);
        app.focused_agent = Some("syn".into());

        assert_eq!(pick_session_id(&app, 0, false).as_deref(), Some("s1"));
        assert_eq!(pick_session_id(&app, 1, false).as_deref(), Some("s2"));
        assert!(pick_session_id(&app, 5, false).is_none());
    }

    #[test]
    fn pick_session_id_no_focused_agent_returns_none() {
        let app = test_app();
        assert!(pick_session_id(&app, 0, false).is_none());
    }

    #[test]
    fn pick_session_id_skips_non_interactive_when_not_showing_archived() {
        let mut app = test_app();
        let mut agent = test_agent("syn", "Syn");
        agent.sessions.push(test_session("s1", "cron:daily"));
        agent.sessions.push(test_session("s2", "main"));
        app.agents.push(agent);
        app.focused_agent = Some("syn".into());

        // cursor 0 should be "main" (only interactive session)
        assert_eq!(pick_session_id(&app, 0, false).as_deref(), Some("s2"));
        // with show_archived, cursor 0 is "cron:daily"
        assert_eq!(pick_session_id(&app, 0, true).as_deref(), Some("s1"));
    }

    #[test]
    fn visible_session_count_includes_archived_when_show_all() {
        let mut app = test_app();
        let mut agent = test_agent("syn", "Syn");
        agent.sessions.push(test_session("s1", "main"));
        agent.sessions.push(test_session_archived("s2", "old"));
        app.agents.push(agent);
        app.focused_agent = Some("syn".into());

        // Archived sessions are not interactive
        assert_eq!(visible_session_count(&app, false), 1);
        assert_eq!(visible_session_count(&app, true), 2);
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

    // --- Context actions overlay tests ---

    fn make_context_actions_overlay(n: usize) -> Overlay {
        use crate::msg::MessageActionKind;
        let actions: Vec<_> = (0..n)
            .map(|_| crate::state::ContextAction {
                label: "Copy text",
                kind: MessageActionKind::Copy,
            })
            .collect();
        Overlay::ContextActions(crate::state::ContextActionsOverlay { actions, cursor: 0 })
    }

    #[test]
    fn context_actions_overlay_up_saturates() {
        let mut app = test_app();
        app.overlay = Some(make_context_actions_overlay(3));
        handle_overlay_up(&mut app);
        if let Some(Overlay::ContextActions(ctx)) = &app.overlay {
            assert_eq!(ctx.cursor, 0);
        }
    }

    #[test]
    fn context_actions_overlay_down_increments() {
        let mut app = test_app();
        app.overlay = Some(make_context_actions_overlay(3));
        handle_overlay_down(&mut app);
        if let Some(Overlay::ContextActions(ctx)) = &app.overlay {
            assert_eq!(ctx.cursor, 1);
        }
    }

    #[test]
    fn context_actions_overlay_down_clamps() {
        let mut app = test_app();
        app.overlay = Some(make_context_actions_overlay(2));
        handle_overlay_down(&mut app);
        handle_overlay_down(&mut app);
        handle_overlay_down(&mut app);
        if let Some(Overlay::ContextActions(ctx)) = &app.overlay {
            assert_eq!(ctx.cursor, 1);
        }
    }

    #[tokio::test]
    async fn context_actions_select_dispatches_action() {
        let mut app = test_app_with_messages(vec![("user", "hello")]);
        app.selected_message = Some(0);
        app.overlay = Some(Overlay::ContextActions(
            crate::state::ContextActionsOverlay {
                actions: vec![crate::state::ContextAction {
                    label: "Quote in reply",
                    kind: crate::msg::MessageActionKind::QuoteInReply,
                }],
                cursor: 0,
            },
        ));
        handle_overlay_select(&mut app).await;
        assert!(app.overlay.is_none());
        assert!(app.input.text.contains("> hello"));
    }

    #[test]
    fn context_actions_close_clears_overlay() {
        let mut app = test_app();
        app.overlay = Some(make_context_actions_overlay(2));
        handle_close_overlay(&mut app);
        assert!(app.overlay.is_none());
    }
}
