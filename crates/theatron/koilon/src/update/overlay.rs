use tracing::Instrument;

use crate::app::App;
use crate::msg::{ErrorToast, OverlayKind, ToolApprovalAction};
use crate::sanitize::sanitize_for_display;
use crate::state::{ControlMutationStatus, Overlay, SessionPickerOverlay};

pub(crate) async fn handle_open_overlay(app: &mut App, kind: OverlayKind) {
    match kind {
        OverlayKind::Settings => {
            super::settings::handle_open(app).await;
        }
        other => {
            app.layout.overlay = Some(match other {
                OverlayKind::Help => Overlay::Help,
                OverlayKind::AgentPicker => Overlay::AgentPicker { cursor: 0 },
                OverlayKind::SessionPicker => Overlay::SessionPicker(SessionPickerOverlay {
                    cursor: 0,
                    show_archived: false,
                    new_session_status: app.dashboard.new_session_status.clone(),
                }),
                OverlayKind::SessionPickerAll => Overlay::SessionPicker(SessionPickerOverlay {
                    cursor: 0,
                    show_archived: true,
                    new_session_status: app.dashboard.new_session_status.clone(),
                }),
                OverlayKind::SystemStatus => Overlay::SystemStatus,
                OverlayKind::ContextBudget => Overlay::ContextBudget,
                OverlayKind::Settings => {
                    // kanon:ignore RUST/unreachable-in-match — Settings overlay is dispatched through a dedicated handler, not the generic overlay router
                    unreachable!()
                }
                OverlayKind::NotificationHistory => {
                    app.layout.notifications.mark_all_read();
                    Overlay::NotificationHistory { scroll: 0 }
                }
            });
        }
    }
}

pub(crate) fn handle_tool_approval_always_allow(app: &mut App) {
    start_tool_approval_action(app, ToolApprovalAction::AlwaysAllow);
}

pub(crate) fn handle_close_overlay(app: &mut App) {
    // NOTE: Esc in settings edit mode cancels the edit, not the overlay itself
    if let Some(Overlay::Settings(ref s)) = app.layout.overlay
        && s.editing.is_some()
    {
        super::settings::handle_edit_escape(app);
        return;
    }
    if let Some(Overlay::ToolApproval(ref approval)) = app.layout.overlay {
        if approval.status.is_pending() {
            return;
        }
        start_tool_approval_action(app, ToolApprovalAction::Deny);
        return;
    }
    // NOTE: DecisionCard close without submit = skip, no API call needed
    app.layout.overlay = None;
}

pub(crate) fn handle_overlay_up(app: &mut App) {
    match &mut app.layout.overlay {
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
        Some(Overlay::DecisionCard(card))
            if card.focused_field == crate::state::DecisionField::Options =>
        {
            card.cursor = card.cursor.saturating_sub(1);
        }
        Some(Overlay::DecisionCard(_)) => {}
        Some(Overlay::NotificationHistory { scroll }) => {
            *scroll = scroll.saturating_sub(1);
        }
        _ => {
            // NOTE: no overlay or non-navigable overlay, nothing to do
        }
    }
}

pub(crate) fn handle_overlay_down(app: &mut App) {
    match &mut app.layout.overlay {
        Some(Overlay::AgentPicker { cursor }) => {
            let max = app.dashboard.agents.len().saturating_sub(1);
            *cursor = (*cursor + 1).min(max);
        }
        Some(Overlay::SessionPicker(picker)) => {
            let show_archived = picker.show_archived;
            let cursor = picker.cursor;
            let count = visible_session_count(app, show_archived);
            let max = count.saturating_sub(1);
            if let Some(Overlay::SessionPicker(picker)) = &mut app.layout.overlay {
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
        Some(Overlay::DecisionCard(card))
            if card.focused_field == crate::state::DecisionField::Options =>
        {
            let max = card.options.len().saturating_sub(1);
            card.cursor = (card.cursor + 1).min(max);
        }
        Some(Overlay::DecisionCard(_)) => {}
        Some(Overlay::NotificationHistory { scroll }) => {
            *scroll += 1;
        }
        _ => {
            // NOTE: no overlay or non-navigable overlay, nothing to do
        }
    }
}

pub(crate) async fn handle_overlay_select(app: &mut App) {
    match &app.layout.overlay {
        Some(Overlay::AgentPicker { cursor }) => {
            if let Some(agent) = app.dashboard.agents.get_mut(*cursor) {
                agent.unread_count = 0;
                let id = agent.id.clone();
                app.dashboard.focused_agent = Some(id);
                app.layout.overlay = None;
                app.load_focused_session().await;
            }
        }
        Some(Overlay::SessionPicker(picker)) => {
            let cursor = picker.cursor;
            let show_archived = picker.show_archived;
            if let Some(session_id) = pick_session_id(app, cursor, show_archived) {
                app.save_scroll_state();
                app.dashboard.focused_session_id = Some(session_id.clone());
                app.layout.overlay = None;
                match app.client.history(&session_id).await {
                    Ok(history) => {
                        app.dashboard.messages = history
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
                                    tool_calls: Vec::new(),
                                    kind: crate::state::MessageKind::default(),
                                })
                            })
                            .collect();
                        app.scroll_to_bottom();
                    }
                    Err(e) => {
                        tracing::error!("failed to load history: {e}");
                        app.viewport.error_toast =
                            Some(crate::msg::ErrorToast::new(format!("Load failed: {e}")));
                    }
                }
            }
        }
        Some(Overlay::ToolApproval(approval)) => {
            if !approval.status.is_pending() {
                start_tool_approval_action(app, ToolApprovalAction::Approve);
            }
        }
        Some(Overlay::PlanApproval(_plan)) => {
            mark_plan_approval_failed(app);
        }
        Some(Overlay::ContextActions(ctx)) => {
            if let Some(action) = ctx.selected_action() {
                let kind = action.kind;
                app.layout.overlay = None;
                super::selection::handle_message_action(app, kind);
            }
        }
        Some(Overlay::Settings(_)) => {
            super::settings::handle_enter(app);
        }
        Some(Overlay::DecisionCard(_)) => {
            if let Some(Overlay::DecisionCard(card)) = app.layout.overlay.take() {
                let chosen = card.chosen_label().to_string();
                let decision = crate::state::SubmittedDecision {
                    question: card.question,
                    chosen_label: chosen,
                    notes: card.notes,
                    submitted_at: std::time::Instant::now(),
                };
                app.dashboard.submitted_decisions.push(decision);
            }
        }
        _ => {
            app.layout.overlay = None;
        }
    }
}

fn start_tool_approval_action(app: &mut App, action: ToolApprovalAction) {
    let Some(Overlay::ToolApproval(ref mut approval)) = app.layout.overlay else {
        return;
    };
    if approval.status.is_pending() {
        return;
    }

    let turn_id = approval.turn_id.clone();
    let tool_id = approval.tool_id.clone();
    let tool_name = approval.tool_name.clone();
    let action_id = tool_approval_action_id(action, &turn_id, &tool_id);
    approval.status = ControlMutationStatus::pending(action_id.clone());

    let client = app.client.clone();
    let span = tracing::info_span!(
        "tool_approval_action",
        %action_id,
        action = action.label(),
        %turn_id,
        %tool_id,
        %tool_name
    );
    app.background_tasks.spawn(
        async move {
            let result = match action {
                ToolApprovalAction::Deny => client.deny_tool(&turn_id, &tool_id).await,
                ToolApprovalAction::Approve
                | ToolApprovalAction::AlwaysAllow
                | ToolApprovalAction::AutoApprove => client.approve_tool(&turn_id, &tool_id).await,
            }
            .map_err(|e| e.to_string());

            crate::msg::Msg::ToolApprovalCompleted {
                action_id,
                turn_id,
                tool_id,
                tool_name: Some(tool_name),
                action,
                result,
            }
        }
        .instrument(span),
    );
}

pub(crate) fn start_auto_tool_approval(
    app: &mut App,
    turn_id: crate::id::TurnId,
    tool_id: crate::id::ToolId,
    tool_name: String,
) {
    let action = ToolApprovalAction::AutoApprove;
    let action_id = tool_approval_action_id(action, &turn_id, &tool_id);
    let client = app.client.clone();
    let span = tracing::info_span!(
        "auto_approve_tool",
        %action_id,
        %turn_id,
        %tool_id,
        %tool_name
    );
    app.background_tasks.spawn(
        async move {
            let result = client
                .approve_tool(&turn_id, &tool_id)
                .await
                .map_err(|e| e.to_string());
            crate::msg::Msg::ToolApprovalCompleted {
                action_id,
                turn_id,
                tool_id,
                tool_name: Some(tool_name),
                action,
                result,
            }
        }
        .instrument(span),
    );
}

pub(crate) fn handle_tool_approval_completed(
    app: &mut App,
    action_id: String,
    turn_id: crate::id::TurnId,
    tool_id: crate::id::ToolId,
    tool_name: Option<String>,
    action: ToolApprovalAction,
    result: Result<(), String>,
) {
    match result {
        Ok(()) => {
            if action == ToolApprovalAction::AlwaysAllow
                && let Some(tool_name) = tool_name
            {
                app.interaction.always_allowed_tools.insert(tool_name);
            }
            if current_tool_approval_matches(app, &turn_id, &tool_id, &action_id) {
                app.layout.overlay = None;
            }
        }
        Err(message) => {
            let message = sanitize_for_display(&message).into_owned();
            let status_message = format!("Tool {} failed: {message}", action.label());
            let feedback = format!("[{action_id}] {status_message}");
            if let Some(Overlay::ToolApproval(ref mut approval)) = app.layout.overlay
                && approval.turn_id == turn_id
                && approval.tool_id == tool_id
            {
                approval.status = ControlMutationStatus::failed(action_id.clone(), status_message);
            }
            app.viewport.error_toast = Some(ErrorToast::new(feedback));
        }
    }
}

fn current_tool_approval_matches(
    app: &App,
    turn_id: &crate::id::TurnId,
    tool_id: &crate::id::ToolId,
    action_id: &str,
) -> bool {
    matches!(
        &app.layout.overlay,
        Some(Overlay::ToolApproval(approval))
            if &approval.turn_id == turn_id
                && &approval.tool_id == tool_id
                && matches!(
                    &approval.status,
                    ControlMutationStatus::Pending { action_id: pending_id }
                        if pending_id == action_id
                )
    )
}

fn tool_approval_action_id(
    action: ToolApprovalAction,
    turn_id: &crate::id::TurnId,
    tool_id: &crate::id::ToolId,
) -> String {
    format!("tool:{}:{turn_id}:{tool_id}", action.action_key())
}

fn mark_plan_approval_failed(app: &mut App) {
    let action_id = "plan:approval:unavailable".to_string();
    let message = "Plan approval API not available - pending pylon support.".to_string();
    if let Some(Overlay::PlanApproval(ref mut plan)) = app.layout.overlay {
        if plan.status.is_pending() {
            return;
        }
        plan.status = ControlMutationStatus::failed(action_id.clone(), message.clone());
    }
    app.viewport.error_toast = Some(ErrorToast::new(format!("[{action_id}] {message}")));
}

pub(crate) fn visible_session_count(app: &App, show_archived: bool) -> usize {
    let agent_id = match &app.dashboard.focused_agent {
        Some(id) => id,
        None => return 0,
    };
    let agent = match app.dashboard.agents.iter().find(|a| &a.id == agent_id) {
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
    let agent_id = app.dashboard.focused_agent.as_ref()?;
    let agent = app.dashboard.agents.iter().find(|a| &a.id == agent_id)?;
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;

    async fn failing_server() -> (String, JoinHandle<()>) {
        let listener = match TcpListener::bind("127.0.0.1:0").await {
            Ok(listener) => listener,
            Err(e) => panic!("bind failing test server: {e}"),
        };
        let addr = match listener.local_addr() {
            Ok(addr) => addr,
            Err(e) => panic!("read failing test server address: {e}"),
        };
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _addr)) = listener.accept().await else {
                    break;
                };
                let _connection = tokio::spawn(async move {
                    let mut request = [0_u8; 1024];
                    if stream.read(&mut request).await.is_err() {
                        return;
                    }
                    let response = concat!(
                        "HTTP/1.1 500 Internal Server Error\r\n",
                        "content-type: text/plain\r\n",
                        "content-length: 19\r\n",
                        "connection: close\r\n",
                        "\r\n",
                        "backend unavailable"
                    );
                    if let Err(e) = stream.write_all(response.as_bytes()).await {
                        tracing::debug!("failed to write test response: {e}");
                    }
                });
            }
        });
        (format!("http://{addr}"), handle)
    }

    fn point_app_at(app: &mut App, url: &str) {
        app.config.url = url.to_string();
        app.client = match crate::api::client::ApiClient::new(url, None) {
            Ok(client) => client,
            Err(e) => panic!("test ApiClient::new failed: {e}"),
        };
    }

    async fn drain_one_background(app: &mut App) {
        let Some(result) = app.background_tasks.join_next().await else {
            panic!("expected one background task");
        };
        let msg = match result {
            Ok(msg) => msg,
            Err(e) => panic!("background task failed: {e}"),
        };
        app.update(msg).await;
    }

    fn tool_approval_overlay() -> Overlay {
        Overlay::ToolApproval(crate::state::ToolApprovalOverlay {
            turn_id: "t1".into(),
            tool_id: "tool1".into(),
            tool_name: "write_file".to_string(),
            input: serde_json::json!({"path": "/tmp/test"}),
            risk: "high".to_string(),
            reason: "writes files".to_string(),
            timeout_secs: 120,
            default_decision: "denied".to_string(),
            status: ControlMutationStatus::Idle,
        })
    }

    #[tokio::test]
    async fn open_overlay_help() {
        let mut app = test_app();
        handle_open_overlay(&mut app, OverlayKind::Help).await;
        assert!(matches!(app.layout.overlay, Some(Overlay::Help)));
    }

    #[tokio::test]
    async fn approve_failure_keeps_overlay_failed_with_action_id() {
        let (url, _server) = failing_server().await;
        let mut app = test_app();
        point_app_at(&mut app, &url);
        app.layout.overlay = Some(tool_approval_overlay());

        handle_overlay_select(&mut app).await;

        assert!(matches!(
            app.layout.overlay,
            Some(Overlay::ToolApproval(crate::state::ToolApprovalOverlay {
                status: ControlMutationStatus::Pending { .. },
                ..
            }))
        ));

        drain_one_background(&mut app).await;

        let Some(Overlay::ToolApproval(approval)) = &app.layout.overlay else {
            panic!("approval failure should keep overlay open");
        };
        assert!(matches!(
            &approval.status,
            ControlMutationStatus::Failed { action_id, .. }
                if action_id == "tool:approve:t1:tool1"
        ));
        assert!(
            app.viewport
                .error_toast
                .as_ref()
                .is_some_and(|toast| toast.message.contains("tool:approve:t1:tool1"))
        );
    }

    #[tokio::test]
    async fn deny_failure_keeps_overlay_failed_with_action_id() {
        let (url, _server) = failing_server().await;
        let mut app = test_app();
        point_app_at(&mut app, &url);
        app.layout.overlay = Some(tool_approval_overlay());

        handle_close_overlay(&mut app);

        assert!(matches!(
            app.layout.overlay,
            Some(Overlay::ToolApproval(crate::state::ToolApprovalOverlay {
                status: ControlMutationStatus::Pending { .. },
                ..
            }))
        ));

        drain_one_background(&mut app).await;

        let Some(Overlay::ToolApproval(approval)) = &app.layout.overlay else {
            panic!("deny failure should keep overlay open");
        };
        assert!(matches!(
            &approval.status,
            ControlMutationStatus::Failed { action_id, .. }
                if action_id == "tool:deny:t1:tool1"
        ));
        assert!(
            app.viewport
                .error_toast
                .as_ref()
                .is_some_and(|toast| toast.message.contains("tool:deny:t1:tool1"))
        );
    }

    #[tokio::test]
    async fn always_allow_failure_does_not_insert_local_allow() {
        let (url, _server) = failing_server().await;
        let mut app = test_app();
        point_app_at(&mut app, &url);
        app.layout.overlay = Some(tool_approval_overlay());

        handle_tool_approval_always_allow(&mut app);
        drain_one_background(&mut app).await;

        assert!(!app.interaction.always_allowed_tools.contains("write_file"));
        let Some(Overlay::ToolApproval(approval)) = &app.layout.overlay else {
            panic!("always-allow failure should keep overlay open");
        };
        assert!(matches!(
            &approval.status,
            ControlMutationStatus::Failed { action_id, .. }
                if action_id == "tool:always-allow:t1:tool1"
        ));
    }

    #[tokio::test]
    async fn plan_approval_failure_keeps_overlay_failed_with_action_id() {
        let mut app = test_app();
        app.layout.overlay = Some(Overlay::PlanApproval(crate::state::PlanApprovalOverlay {
            steps: vec![crate::state::PlanStepApproval {
                id: 1,
                label: "Step".to_string(),
                role: "planner".to_string(),
                checked: true,
            }],
            total_cost_cents: 100,
            cursor: 0,
            status: ControlMutationStatus::Idle,
        }));

        handle_overlay_select(&mut app).await;

        let Some(Overlay::PlanApproval(plan)) = &app.layout.overlay else {
            panic!("plan approval failure should keep overlay open");
        };
        assert!(matches!(
            &plan.status,
            ControlMutationStatus::Failed { action_id, .. }
                if action_id == "plan:approval:unavailable"
        ));
        assert!(
            app.viewport
                .error_toast
                .as_ref()
                .is_some_and(|toast| toast.message.contains("plan:approval:unavailable"))
        );
    }

    #[tokio::test]
    async fn open_overlay_agent_picker() {
        let mut app = test_app();
        handle_open_overlay(&mut app, OverlayKind::AgentPicker).await;
        assert!(matches!(
            app.layout.overlay,
            Some(Overlay::AgentPicker { cursor: 0 })
        ));
    }

    #[tokio::test]
    async fn open_overlay_system_status() {
        let mut app = test_app();
        handle_open_overlay(&mut app, OverlayKind::SystemStatus).await;
        assert!(matches!(app.layout.overlay, Some(Overlay::SystemStatus)));
    }

    #[test]
    fn close_overlay_clears() {
        let mut app = test_app();
        app.layout.overlay = Some(Overlay::Help);
        handle_close_overlay(&mut app);
        assert!(app.layout.overlay.is_none());
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
        app.layout.overlay = Some(Overlay::Settings(settings));

        handle_close_overlay(&mut app);
        // Should cancel the edit, not close the overlay
        let Some(Overlay::Settings(s)) = &app.layout.overlay else {
            unreachable!("overlay should still be Settings");
        };
        assert!(s.editing.is_none());
    }

    #[test]
    fn overlay_up_agent_picker() {
        let mut app = test_app();
        app.dashboard.agents.push(test_agent("a", "A"));
        app.dashboard.agents.push(test_agent("b", "B"));
        app.layout.overlay = Some(Overlay::AgentPicker { cursor: 1 });

        handle_overlay_up(&mut app);

        if let Some(Overlay::AgentPicker { cursor }) = &app.layout.overlay {
            assert_eq!(*cursor, 0);
        }
    }

    #[test]
    fn overlay_up_saturates_at_zero() {
        let mut app = test_app();
        app.layout.overlay = Some(Overlay::AgentPicker { cursor: 0 });

        handle_overlay_up(&mut app);

        if let Some(Overlay::AgentPicker { cursor }) = &app.layout.overlay {
            assert_eq!(*cursor, 0);
        }
    }

    #[test]
    fn overlay_down_agent_picker() {
        let mut app = test_app();
        app.dashboard.agents.push(test_agent("a", "A"));
        app.dashboard.agents.push(test_agent("b", "B"));
        app.layout.overlay = Some(Overlay::AgentPicker { cursor: 0 });

        handle_overlay_down(&mut app);

        if let Some(Overlay::AgentPicker { cursor }) = &app.layout.overlay {
            assert_eq!(*cursor, 1);
        }
    }

    #[test]
    fn overlay_down_clamps_at_max() {
        let mut app = test_app();
        app.dashboard.agents.push(test_agent("a", "A"));
        app.layout.overlay = Some(Overlay::AgentPicker { cursor: 0 });

        handle_overlay_down(&mut app);

        if let Some(Overlay::AgentPicker { cursor }) = &app.layout.overlay {
            assert_eq!(*cursor, 0);
        }
    }

    fn make_session(id: &str, key: &str) -> crate::api::types::Session {
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

    fn make_archived_session(id: &str, key: &str) -> crate::api::types::Session {
        crate::api::types::Session {
            status: Some("archived".to_string()),
            ..make_session(id, key)
        }
    }

    #[tokio::test]
    async fn open_overlay_session_picker() {
        let mut app = test_app();
        handle_open_overlay(&mut app, OverlayKind::SessionPicker).await;
        let Some(Overlay::SessionPicker(picker)) = &app.layout.overlay else {
            unreachable!("expected SessionPicker overlay");
        };
        assert_eq!(picker.cursor, 0);
        assert!(!picker.show_archived);
    }

    #[test]
    fn session_picker_up_saturates_at_zero() {
        let mut app = test_app();
        app.layout.overlay = Some(Overlay::SessionPicker(SessionPickerOverlay {
            cursor: 0,
            show_archived: false,
            new_session_status: ControlMutationStatus::Idle,
        }));
        handle_overlay_up(&mut app);
        if let Some(Overlay::SessionPicker(picker)) = &app.layout.overlay {
            assert_eq!(picker.cursor, 0);
        }
    }

    #[test]
    fn session_picker_up_decrements() {
        let mut app = test_app();
        app.layout.overlay = Some(Overlay::SessionPicker(SessionPickerOverlay {
            cursor: 2,
            show_archived: false,
            new_session_status: ControlMutationStatus::Idle,
        }));
        handle_overlay_up(&mut app);
        if let Some(Overlay::SessionPicker(picker)) = &app.layout.overlay {
            assert_eq!(picker.cursor, 1);
        }
    }

    #[test]
    fn session_picker_down_clamps_at_max() {
        let mut app = test_app();
        let mut agent = test_agent("syn", "Syn");
        agent.sessions.push(make_session("s1", "main"));
        agent.sessions.push(make_session("s2", "debug"));
        app.dashboard.agents.push(agent);
        app.dashboard.focused_agent = Some("syn".into());
        app.layout.overlay = Some(Overlay::SessionPicker(SessionPickerOverlay {
            cursor: 1,
            show_archived: false,
            new_session_status: ControlMutationStatus::Idle,
        }));
        handle_overlay_down(&mut app);
        if let Some(Overlay::SessionPicker(picker)) = &app.layout.overlay {
            assert_eq!(picker.cursor, 1);
        }
    }

    #[test]
    fn session_picker_down_increments() {
        let mut app = test_app();
        let mut agent = test_agent("syn", "Syn");
        agent.sessions.push(make_session("s1", "main"));
        agent.sessions.push(make_session("s2", "debug"));
        agent.sessions.push(make_session("s3", "test"));
        app.dashboard.agents.push(agent);
        app.dashboard.focused_agent = Some("syn".into());
        app.layout.overlay = Some(Overlay::SessionPicker(SessionPickerOverlay {
            cursor: 0,
            show_archived: false,
            new_session_status: ControlMutationStatus::Idle,
        }));
        handle_overlay_down(&mut app);
        if let Some(Overlay::SessionPicker(picker)) = &app.layout.overlay {
            assert_eq!(picker.cursor, 1);
        }
    }

    #[test]
    fn visible_session_count_filters_non_interactive() {
        let mut app = test_app();
        let mut agent = test_agent("syn", "Syn");
        agent.sessions.push(make_session("s1", "main"));
        agent.sessions.push(make_session("s2", "cron:daily"));
        agent.sessions.push(make_session("s3", "prosoche-wake"));
        agent.sessions.push(make_session("s4", "agent:sub"));
        agent.sessions.push(make_session("s5", "debug"));
        agent.sessions.push(make_session("s6", "daemon:prosoche"));
        app.dashboard.agents.push(agent);
        app.dashboard.focused_agent = Some("syn".into());

        assert_eq!(visible_session_count(&app, false), 2);
        assert_eq!(visible_session_count(&app, true), 6);
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
        agent.sessions.push(make_session("s1", "main"));
        agent.sessions.push(make_session("s2", "debug"));
        app.dashboard.agents.push(agent);
        app.dashboard.focused_agent = Some("syn".into());

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
        agent.sessions.push(make_session("s1", "cron:daily"));
        agent.sessions.push(make_session("s2", "main"));
        app.dashboard.agents.push(agent);
        app.dashboard.focused_agent = Some("syn".into());

        // cursor 0 should be "main" (only interactive session)
        assert_eq!(pick_session_id(&app, 0, false).as_deref(), Some("s2"));
        // with show_archived, cursor 0 is "cron:daily"
        assert_eq!(pick_session_id(&app, 0, true).as_deref(), Some("s1"));
    }

    #[test]
    fn visible_session_count_includes_archived_when_show_all() {
        let mut app = test_app();
        let mut agent = test_agent("syn", "Syn");
        agent.sessions.push(make_session("s1", "main"));
        agent.sessions.push(make_archived_session("s2", "old"));
        app.dashboard.agents.push(agent);
        app.dashboard.focused_agent = Some("syn".into());

        // Archived sessions are not interactive
        assert_eq!(visible_session_count(&app, false), 1);
        assert_eq!(visible_session_count(&app, true), 2);
    }

    #[test]
    fn overlay_up_plan_approval() {
        let mut app = test_app();
        app.layout.overlay = Some(Overlay::PlanApproval(crate::state::PlanApprovalOverlay {
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
            status: ControlMutationStatus::Idle,
        }));

        handle_overlay_up(&mut app);

        if let Some(Overlay::PlanApproval(plan)) = &app.layout.overlay {
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
        app.layout.overlay = Some(make_context_actions_overlay(3));
        handle_overlay_up(&mut app);
        if let Some(Overlay::ContextActions(ctx)) = &app.layout.overlay {
            assert_eq!(ctx.cursor, 0);
        }
    }

    #[test]
    fn context_actions_overlay_down_increments() {
        let mut app = test_app();
        app.layout.overlay = Some(make_context_actions_overlay(3));
        handle_overlay_down(&mut app);
        if let Some(Overlay::ContextActions(ctx)) = &app.layout.overlay {
            assert_eq!(ctx.cursor, 1);
        }
    }

    #[test]
    fn context_actions_overlay_down_clamps() {
        let mut app = test_app();
        app.layout.overlay = Some(make_context_actions_overlay(2));
        handle_overlay_down(&mut app);
        handle_overlay_down(&mut app);
        handle_overlay_down(&mut app);
        if let Some(Overlay::ContextActions(ctx)) = &app.layout.overlay {
            assert_eq!(ctx.cursor, 1);
        }
    }

    #[tokio::test]
    async fn context_actions_select_dispatches_action() {
        let mut app = test_app_with_messages(vec![("user", "hello")]);
        app.interaction.selected_message = Some(0);
        app.layout.overlay = Some(Overlay::ContextActions(
            crate::state::ContextActionsOverlay {
                actions: vec![crate::state::ContextAction {
                    label: "Quote in reply",
                    kind: crate::msg::MessageActionKind::QuoteInReply,
                }],
                cursor: 0,
            },
        ));
        handle_overlay_select(&mut app).await;
        assert!(app.layout.overlay.is_none());
        assert!(app.interaction.input.text.contains("> hello"));
    }

    #[test]
    fn context_actions_close_clears_overlay() {
        let mut app = test_app();
        app.layout.overlay = Some(make_context_actions_overlay(2));
        handle_close_overlay(&mut app);
        assert!(app.layout.overlay.is_none());
    }
}
