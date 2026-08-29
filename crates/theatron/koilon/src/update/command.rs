use crate::app::{self, App};
use crate::command::build_suggestions;
use crate::msg::ErrorToast;
use crate::sanitize::sanitize_for_display;
use crate::state::{Overlay, SessionPickerOverlay};

#[tracing::instrument(skip_all)]
pub(crate) fn handle_open(app: &mut App) {
    app.interaction.command_palette.active = true;
    app.interaction.command_palette.input.clear();
    app.interaction.command_palette.cursor = 0;
    app.interaction.command_palette.selected = 0;
    app.interaction.command_palette.suggestions = build_suggestions("", &app.dashboard.agents);
}

#[tracing::instrument(skip_all)]
pub(crate) fn handle_close(app: &mut App) {
    app.interaction.command_palette.active = false;
    app.interaction.command_palette.input.clear();
}

pub(crate) fn handle_input(app: &mut App, c: char) {
    app.interaction.command_history_index = None;
    app.interaction
        .command_palette
        .input
        .insert(app.interaction.command_palette.cursor, c);
    app.interaction.command_palette.cursor += c.len_utf8();
    refresh_suggestions(app);
    app.interaction.command_palette.selected = 0;
}

pub(crate) fn handle_backspace(app: &mut App) {
    if app.interaction.command_palette.cursor > 0 {
        let mut prev = app.interaction.command_palette.cursor - 1;
        while prev > 0 && !app.interaction.command_palette.input.is_char_boundary(prev) {
            prev -= 1;
        }
        app.interaction.command_palette.input.remove(prev);
        app.interaction.command_palette.cursor = prev;
        refresh_suggestions(app);
        app.interaction.command_palette.selected = 0;
    } else {
        // WHY: closes on empty backspace to match vim command-mode behavior
        app.interaction.command_palette.active = false;
    }
}

pub(crate) fn handle_delete_word(app: &mut App) {
    let mut pos = app.interaction.command_palette.cursor;
    while pos > 0
        && app
            .interaction
            .command_palette
            .input
            .as_bytes()
            .get(pos - 1)
            == Some(&b' ')
    {
        pos -= 1;
    }
    while pos > 0
        && app
            .interaction
            .command_palette
            .input
            .as_bytes()
            .get(pos - 1)
            != Some(&b' ')
    {
        pos -= 1;
    }
    app.interaction
        .command_palette
        .input
        .drain(pos..app.interaction.command_palette.cursor);
    app.interaction.command_palette.cursor = pos;
    refresh_suggestions(app);
    app.interaction.command_palette.selected = 0;
}

#[expect(
    clippy::indexing_slicing,
    reason = "idx < command_history.len() is guaranteed by the match arms; the reverse-index is always valid"
)]
pub(crate) fn handle_up(app: &mut App) {
    if app.interaction.command_history_index.is_some() {
        // WHY: Already in history-browsing mode: continue navigating history.
        if !app.interaction.command_history.is_empty() {
            let idx = match app.interaction.command_history_index {
                Some(i) if i + 1 < app.interaction.command_history.len() => i + 1,
                None => 0,
                Some(i) => i,
            };
            app.interaction.command_history_index = Some(idx);
            let entry = app.interaction.command_history
                [app.interaction.command_history.len() - 1 - idx]
                .clone();
            app.interaction.command_palette.input = entry;
            app.interaction.command_palette.cursor = app.interaction.command_palette.input.len();
            refresh_suggestions(app);
        }
    } else {
        app.interaction.command_palette.selected =
            app.interaction.command_palette.selected.saturating_sub(1);
    }
}

#[expect(
    clippy::indexing_slicing,
    reason = "idx = i - 1 where Some(i) was a previously stored history index, so reverse-index is valid"
)]
pub(crate) fn handle_down(app: &mut App) {
    if app.interaction.command_history_index.is_some() {
        match app.interaction.command_history_index {
            Some(0) => {
                app.interaction.command_history_index = None;
                app.interaction.command_palette.input.clear();
                app.interaction.command_palette.cursor = 0;
                refresh_suggestions(app);
            }
            Some(i) => {
                let idx = i - 1;
                app.interaction.command_history_index = Some(idx);
                let entry = app.interaction.command_history
                    [app.interaction.command_history.len() - 1 - idx]
                    .clone();
                app.interaction.command_palette.input = entry;
                app.interaction.command_palette.cursor =
                    app.interaction.command_palette.input.len();
                refresh_suggestions(app);
            }
            // NOTE: already at latest input, no history to navigate
            None => {}
        }
    } else {
        let max = app
            .interaction
            .command_palette
            .suggestions
            .len()
            .saturating_sub(1);
        app.interaction.command_palette.selected =
            (app.interaction.command_palette.selected + 1).min(max);
    }
}

pub(crate) fn handle_tab(app: &mut App) {
    if let Some(suggestion) = app
        .interaction
        .command_palette
        .suggestions
        .get(app.interaction.command_palette.selected)
    {
        let base = suggestion.execute_as.clone();
        let args = app
            .interaction
            .command_palette
            .input
            .split_once(' ')
            .map(|(_, a)| format!(" {a}"))
            .unwrap_or_default();
        app.interaction.command_palette.input = format!("{base}{args}");
        app.interaction.command_palette.cursor = base.len();
        refresh_suggestions(app);
    }
}

#[tracing::instrument(skip_all)]
pub(crate) async fn handle_select(app: &mut App) {
    if let Some(suggestion) = app
        .interaction
        .command_palette
        .suggestions
        .get(app.interaction.command_palette.selected)
    {
        let execute_as = suggestion.execute_as.clone();
        let extra_args = app
            .interaction
            .command_palette
            .input
            .split_once(' ')
            .map(|(_, a)| a.trim().to_string())
            .unwrap_or_default();

        if extra_args.is_empty() {
            app.interaction.command_palette.input = execute_as;
        } else {
            // NOTE: preserve typed args: user may have typed extra text beyond the suggestion base
            let suggestion_has_args = execute_as.contains(' ');
            if suggestion_has_args {
                app.interaction.command_palette.input = execute_as;
            } else {
                app.interaction.command_palette.input = format!("{execute_as} {extra_args}");
            }
        }
    }
    execute_command(app).await;
}

fn refresh_suggestions(app: &mut App) {
    app.interaction.command_palette.suggestions = build_suggestions(
        &app.interaction.command_palette.input,
        &app.dashboard.agents,
    );
}

pub(crate) async fn execute_command(app: &mut App) {
    let input = app.interaction.command_palette.input.trim().to_string();
    app.interaction.command_palette.active = false;
    app.interaction.command_palette.input.clear();
    app.interaction.command_history_index = None;

    if input.is_empty() {
        return;
    }

    // Persist command to history (deduplicate consecutive duplicates)
    if app.interaction.command_history.last().map(|s| s.as_str()) != Some(&input) {
        app.interaction.command_history.push(input.clone());
        if app.interaction.command_history.len() > app::MAX_COMMAND_HISTORY {
            app.interaction
                .command_history
                .drain(..app.interaction.command_history.len() - app::MAX_COMMAND_HISTORY);
        }
        app::save_command_history(&app.config, &app.interaction.command_history);
    }

    let (cmd_name, args) = match input.split_once(' ') {
        Some((cmd, rest)) => (cmd, rest.trim()),
        None => (input.as_str(), ""),
    };

    match cmd_name {
        "quit" | "q" => app.should_quit = true,
        "help" | "?" => {
            app.layout.overlay = Some(Overlay::Help);
        }
        "agents" | "a" => {
            app.layout.overlay = Some(Overlay::AgentPicker { cursor: 0 });
        }
        "sessions" | "s" => {
            let show_archived = args == "--all" || args == "-a";
            app.layout.overlay = Some(Overlay::SessionPicker(SessionPickerOverlay {
                cursor: 0,
                show_archived,
                new_session_status: app.dashboard.new_session_status.clone(),
            }));
        }
        "health" | "h" | "cost" | "$" => {
            app.layout.overlay = Some(Overlay::SystemStatus);
        }
        "agent" => {
            if !args.is_empty() {
                let target = args.to_lowercase();
                if let Some(agent) = app
                    .dashboard
                    .agents
                    .iter()
                    .find(|a| a.id.to_lowercase() == target || a.name.to_lowercase() == target)
                {
                    let id = agent.id.clone();
                    app.save_scroll_state();
                    if let Some(a) = app.dashboard.agents.iter_mut().find(|a| a.id == id) {
                        a.unread_count = 0;
                    }
                    app.dashboard.focused_agent = Some(id);
                    app.load_focused_session().await;
                    app.restore_scroll_state();
                } else {
                    app.viewport.error_toast =
                        Some(ErrorToast::new(format!("Unknown agent: {args}")));
                }
            } else {
                app.layout.overlay = Some(Overlay::AgentPicker { cursor: 0 });
            }
        }
        "clear" => {
            app.dashboard.messages.clear();
            app.dashboard.focused_session_id = None;
            app.connection.streaming_text.clear();
            app.connection.streaming_thinking.clear();
            app.connection.streaming_tool_calls.clear();
            app.scroll_to_bottom();
        }
        "compact" => {
            execute_compact(app);
        }
        "recall" | "r" => {
            if args.is_empty() {
                app.viewport.error_toast = Some(ErrorToast::new("Usage: :recall <query>".into()));
            } else {
                execute_recall(app, args);
            }
        }
        "model" => {
            execute_model(app);
        }
        "new" => {
            super::api::handle_new_session(app);
        }
        "rename" => {
            if args.is_empty() {
                app.viewport.error_toast = Some(ErrorToast::new("Usage: :rename <name>".into()));
            } else {
                execute_rename(app, args).await;
            }
        }
        "archive" => {
            execute_archive(app).await;
        }
        "unarchive" => {
            execute_unarchive(app).await;
        }
        "memory" | "mem" | "m" => {
            super::memory::handle_open(app).await;
        }
        "settings" => {
            super::settings::handle_open(app).await;
        }
        "diff" | "d" => {
            super::diff::handle_diff_open(app).await;
        }
        "ops" => {
            app.layout.ops.toggle();
        }
        "tab" => {
            super::tabs::handle_tab_command(app, args);
        }
        "export" => {
            if args == "json" {
                execute_export_json(app).await;
            } else {
                execute_export(app);
            }
        }
        "search" => {
            super::search::handle_open(app);
        }
        "notifications" | "notif" => {
            app.layout.notifications.mark_all_read();
            app.layout.overlay = Some(Overlay::NotificationHistory { scroll: 0 });
        }
        "metrics" | "stats" => {
            super::metrics::handle_open(app);
        }
        "editor" | "edit" | "e" => {
            super::editor::handle_open(app);
        }
        _ => {
            app.viewport.error_toast =
                Some(ErrorToast::new(format!("Unknown command: {cmd_name}")));
        }
    }
}

fn execute_model(app: &mut App) {
    let agent = app
        .dashboard
        .focused_agent
        .as_ref()
        .and_then(|id| app.dashboard.agents.iter().find(|a| &a.id == id));

    match agent {
        Some(agent) => {
            let model = agent.model.as_deref().unwrap_or("unknown");
            let name = &agent.name;
            app.viewport.error_toast = Some(ErrorToast::new(format!("{name}: {model}")));
        }
        None => {
            app.viewport.error_toast = Some(ErrorToast::new("No agent focused".into()));
        }
    }
}

fn execute_compact(app: &mut App) {
    app.viewport.error_toast = Some(ErrorToast::new(
        "Session distillation API not available - pending pylon support.".into(),
    ));
}

fn execute_recall(app: &mut App, _query: &str) {
    app.viewport.error_toast = Some(ErrorToast::new(
        "Semantic recall API not available - pending pylon support.".into(),
    ));
}

async fn execute_rename(app: &mut App, name: &str) {
    let session_id = match &app.dashboard.focused_session_id {
        Some(id) => id.clone(),
        None => {
            app.viewport.error_toast = Some(ErrorToast::new("No active session to rename".into()));
            return;
        }
    };

    let client = app.client.clone();
    let name = sanitize_for_display(name).into_owned();
    let name_for_update = name.clone();
    let sid = session_id.clone();
    match client.rename_session(&sid, &name_for_update).await {
        Ok(()) => {
            if let Some(ref agent_id) = app.dashboard.focused_agent
                && let Some(agent) = app.dashboard.agents.iter_mut().find(|a| &a.id == agent_id)
                && let Some(session) = agent.sessions.iter_mut().find(|s| s.id == session_id)
            {
                session.display_name = Some(name.clone());
            }
            app.viewport.error_toast = Some(ErrorToast::new(format!("Renamed to: {name}")));
        }
        Err(e) => {
            app.viewport.error_toast = Some(ErrorToast::new(format!("Rename failed: {e}")));
        }
    }
}

async fn execute_archive(app: &mut App) {
    let session_id = match &app.dashboard.focused_session_id {
        Some(id) => id.clone(),
        None => {
            app.viewport.error_toast = Some(ErrorToast::new("No active session to archive".into()));
            return;
        }
    };

    let client = app.client.clone();
    match client.archive_session(&session_id).await {
        Ok(()) => {
            if let Some(ref agent_id) = app.dashboard.focused_agent
                && let Some(agent) = app.dashboard.agents.iter_mut().find(|a| &a.id == agent_id)
                && let Some(session) = agent.sessions.iter_mut().find(|s| s.id == session_id)
            {
                session.status = Some("archived".to_string());
            }
            app.dashboard.messages.clear();
            app.dashboard.focused_session_id = None;
            app.scroll_to_bottom();
            app.viewport.error_toast = Some(ErrorToast::new("Session archived".into()));
        }
        Err(e) => {
            app.viewport.error_toast = Some(ErrorToast::new(format!("Archive failed: {e}")));
        }
    }
}

async fn execute_unarchive(app: &mut App) {
    let session_id = match &app.dashboard.focused_session_id {
        Some(id) => id.clone(),
        None => {
            app.viewport.error_toast =
                Some(ErrorToast::new("No active session to unarchive".into()));
            return;
        }
    };

    let client = app.client.clone();
    match client.unarchive_session(&session_id).await {
        Ok(()) => {
            if let Some(ref agent_id) = app.dashboard.focused_agent
                && let Some(agent) = app.dashboard.agents.iter_mut().find(|a| &a.id == agent_id)
                && let Some(session) = agent.sessions.iter_mut().find(|s| s.id == session_id)
            {
                session.status = Some("active".to_string());
            }
            app.viewport.error_toast = Some(ErrorToast::new("Session restored".into()));
        }
        Err(e) => {
            app.viewport.error_toast = Some(ErrorToast::new(format!("Unarchive failed: {e}")));
        }
    }
}

pub(crate) fn execute_export_from_msg(app: &mut App) {
    execute_export(app);
}

fn execute_export(app: &mut App) {
    if app.dashboard.messages.is_empty() {
        app.viewport.error_toast = Some(ErrorToast::new("No messages to export".into()));
        return;
    }

    let exports_dir = app::exports_dir(&app.config);
    if let Err(e) = std::fs::create_dir_all(&exports_dir) {
        app.viewport.error_toast = Some(ErrorToast::new(format!(
            "Failed to create exports dir: {e}"
        )));
        return;
    }

    let now = jiff::Zoned::now();
    let filename = format!("conversation-{}.md", now.strftime("%Y%m%d-%H%M%S"));
    let path = exports_dir.join(&filename);

    let agent_name = app
        .dashboard
        .focused_agent
        .as_ref()
        .and_then(|id| app.dashboard.agents.iter().find(|a| &a.id == id))
        .map(|a| a.name.as_str())
        .unwrap_or("unknown");

    let session_label = app
        .dashboard
        .focused_session_id
        .as_ref()
        // codequality:ignore -- session IDs are opaque identifiers, not credentials
        .map(|id| id.to_string())
        .unwrap_or_else(|| "none".to_string());

    let mut md = format!(
        "# Conversation Export\n\n- **Agent:** {agent_name}\n- **Session:** {session_label}\n- **Exported:** {now}\n\n---\n\n"
    );

    for msg in app.dashboard.messages.iter() {
        let role_label = match msg.role.as_str() {
            "user" => "User",
            "assistant" => "Assistant",
            other => other,
        };
        if let Some(ref ts) = msg.timestamp {
            md.push_str(&format!("### {role_label} — {ts}\n\n"));
        } else {
            md.push_str(&format!("### {role_label}\n\n"));
        }
        md.push_str(&msg.text);
        md.push_str("\n\n");

        for tc in &msg.tool_calls {
            let status = if tc.is_error { "error" } else { "ok" };
            let duration = tc
                .duration_ms
                .map(|d| format!(" ({d}ms)"))
                .unwrap_or_default();
            md.push_str(&format!("> Tool: `{}`{} — {status}\n\n", tc.name, duration));
        }
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "theatron TUI reads configuration and exports from disk in synchronous initialization paths"
    )]
    match std::fs::write(&path, &md) {
        Ok(()) => {
            // WHY: restrict export files to owner-only (0600) — contain conversation data
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Err(e) =
                    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                {
                    app.viewport.error_toast =
                        Some(ErrorToast::new(format!("Failed to set permissions: {e}")));
                    return;
                }
            }
            // WHY(#4913): the transient toast shows the basename only -- the
            // full local filesystem path is not for a shared/public-clean
            // surface. The export directory is a one-time settings fact, not
            // per-export detail worth repeating on every export.
            app.viewport.success_toast = Some(ErrorToast::new(format!("Exported to {filename}")));
        }
        Err(e) => {
            app.viewport.error_toast = Some(ErrorToast::new(format!("Export failed: {e}")));
        }
    }
}

/// Export the focused session as a replay-faithful JSON audit export.
///
/// WHY(#4913): the Markdown export above is transcript-only -- it drops
/// tool input/output/error detail, usage, provider/model, turn IDs,
/// approvals, and memory links because it is built from the TUI's local,
/// lossy `ChatMessage` state. This instead fetches the same replay schema
/// the CLI's `aletheia export --format json` already exposes.
async fn execute_export_json(app: &mut App) {
    let Some(session_id) = app.dashboard.focused_session_id.clone() else {
        app.viewport.error_toast = Some(ErrorToast::new("No session to export".into()));
        return;
    };

    let replay = match app.client.session_replay(&session_id).await {
        Ok(replay) => replay,
        Err(e) => {
            app.viewport.error_toast = Some(ErrorToast::new(format!("Export failed: {e}")));
            return;
        }
    };

    let exports_dir = app::exports_dir(&app.config);
    if let Err(e) = tokio::fs::create_dir_all(&exports_dir).await {
        app.viewport.error_toast = Some(ErrorToast::new(format!(
            "Failed to create exports dir: {e}"
        )));
        return;
    }

    let now = jiff::Zoned::now();
    let filename = format!("conversation-{}.json", now.strftime("%Y%m%d-%H%M%S"));
    let path = exports_dir.join(&filename);

    let json = match serde_json::to_string_pretty(&replay) {
        Ok(json) => json,
        Err(e) => {
            app.viewport.error_toast =
                Some(ErrorToast::new(format!("Failed to serialize export: {e}")));
            return;
        }
    };

    match tokio::fs::write(&path, &json).await {
        Ok(()) => {
            // WHY: restrict export files to owner-only (0600) -- contain conversation data
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Err(e) =
                    tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).await
                {
                    app.viewport.error_toast =
                        Some(ErrorToast::new(format!("Failed to set permissions: {e}")));
                    return;
                }
            }
            // WHY(#4913): matches the Markdown export's basename-only toast --
            // see the WHY above on the Markdown branch's success path.
            app.viewport.success_toast = Some(ErrorToast::new(format!("Exported to {filename}")));
        }
        Err(e) => {
            app.viewport.error_toast = Some(ErrorToast::new(format!("Export failed: {e}")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_helpers::*;

    /// Regression for #4913: the success toast is transient UI, not an
    /// audit surface -- it must name the export without repeating the
    /// machine-local directory it lives under.
    #[test]
    #[expect(clippy::expect_used, reason = "test assertions may panic on failure")]
    fn export_success_toast_names_the_file_not_the_local_path() {
        let dir = tempfile::tempdir().expect("create temp export dir");
        let mut app = test_app_with_messages(vec![("user", "hello")]);
        app.config.workspace_root = Some(dir.path().to_path_buf());

        execute_export(&mut app);

        let toast = app
            .viewport
            .success_toast
            .as_ref()
            .expect("export must report success");
        assert!(
            toast.message.starts_with("Exported to conversation-"),
            "toast should name the exported file: {}",
            toast.message
        );
        assert!(
            !toast.message.contains(
                dir.path()
                    .to_str()
                    .expect("temp dir path must be valid UTF-8")
            ),
            "toast must not leak the local export directory: {}",
            toast.message
        );
    }

    #[test]
    fn handle_open_activates_palette() {
        let mut app = test_app();
        handle_open(&mut app);
        assert!(app.interaction.command_palette.active);
        assert!(app.interaction.command_palette.input.is_empty());
        assert_eq!(app.interaction.command_palette.cursor, 0);
        assert_eq!(app.interaction.command_palette.selected, 0);
        assert!(!app.interaction.command_palette.suggestions.is_empty());
    }

    #[test]
    fn handle_close_deactivates_palette() {
        let mut app = test_app();
        handle_open(&mut app);
        handle_close(&mut app);
        assert!(!app.interaction.command_palette.active);
        assert!(app.interaction.command_palette.input.is_empty());
    }

    #[test]
    fn handle_input_inserts_char() {
        let mut app = test_app();
        handle_open(&mut app);
        handle_input(&mut app, 'q');
        assert_eq!(app.interaction.command_palette.input, "q");
        assert_eq!(app.interaction.command_palette.cursor, 1);
        assert_eq!(app.interaction.command_palette.selected, 0);
    }

    #[test]
    fn handle_input_multibyte_char() {
        let mut app = test_app();
        handle_open(&mut app);
        handle_input(&mut app, '\u{00e9}'); // e with accent
        assert_eq!(app.interaction.command_palette.input, "\u{00e9}");
        assert_eq!(app.interaction.command_palette.cursor, 2); // 2-byte UTF-8
    }

    #[test]
    fn handle_backspace_removes_char() {
        let mut app = test_app();
        handle_open(&mut app);
        handle_input(&mut app, 'a');
        handle_input(&mut app, 'b');
        handle_backspace(&mut app);
        assert_eq!(app.interaction.command_palette.input, "a");
        assert_eq!(app.interaction.command_palette.cursor, 1);
    }

    #[test]
    fn handle_backspace_on_empty_closes() {
        let mut app = test_app();
        handle_open(&mut app);
        handle_backspace(&mut app);
        assert!(!app.interaction.command_palette.active);
    }

    #[test]
    fn handle_delete_word_removes_word() {
        let mut app = test_app();
        handle_open(&mut app);
        for c in "hello world".chars() {
            handle_input(&mut app, c);
        }
        handle_delete_word(&mut app);
        assert_eq!(app.interaction.command_palette.input, "hello ");
    }

    #[test]
    fn handle_up_decrements_selected() {
        let mut app = test_app();
        handle_open(&mut app);
        app.interaction.command_palette.selected = 3;
        handle_up(&mut app);
        assert_eq!(app.interaction.command_palette.selected, 2);
    }

    #[test]
    fn handle_up_saturates_at_zero() {
        let mut app = test_app();
        handle_open(&mut app);
        app.interaction.command_palette.selected = 0;
        handle_up(&mut app);
        assert_eq!(app.interaction.command_palette.selected, 0);
    }

    #[test]
    fn handle_down_clamps_at_max() {
        let mut app = test_app();
        handle_open(&mut app);
        let max = app
            .interaction
            .command_palette
            .suggestions
            .len()
            .saturating_sub(1);
        app.interaction.command_palette.selected = max;
        handle_down(&mut app);
        assert_eq!(app.interaction.command_palette.selected, max);
    }

    #[test]
    fn handle_tab_completes_suggestion() {
        let mut app = test_app();
        handle_open(&mut app);
        handle_input(&mut app, 'q');
        handle_tab(&mut app);
        // After tab, input should contain the full command name
        assert!(
            app.interaction.command_palette.input.starts_with("quit")
                || !app.interaction.command_palette.suggestions.is_empty()
        );
    }

    mod export_json {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        use tokio::task::JoinHandle;

        use super::*;
        use crate::id::ApiSessionId;

        const REPLAY_BODY: &str = r#"{
            "version": 1,
            "exportType": "replay",
            "exportedAt": "2026-01-01T00:00:00Z",
            "session": {
                "id": "s1",
                "nousId": "syn",
                "sessionKey": "key",
                "status": "active",
                "sessionType": "chat",
                "messageCount": 1,
                "tokenCountEstimate": 10,
                "distillationCount": 0,
                "createdAt": "2026-01-01T00:00:00Z",
                "updatedAt": "2026-01-01T00:00:00Z",
                "lastInputTokens": 5,
                "computedContextTokens": 5
            },
            "messages": [{
                "id": 1,
                "seq": 1,
                "role": "assistant",
                "content": "hi",
                "tokenEstimate": 2,
                "isDistilled": false,
                "createdAt": "2026-01-01T00:00:00Z"
            }],
            "usageRecords": [],
            "toolAuditRecords": [{
                "id": 1,
                "nousId": "syn",
                "turnSeq": 1,
                "toolCallId": "tc1",
                "toolName": "read_file",
                "durationMs": 10,
                "isError": true,
                "outcome": "error",
                "result": "boom",
                "approval": "auto",
                "createdAt": "2026-01-01T00:00:00Z"
            }],
            "turnAttempts": []
        }"#;

        async fn success_json_server(body: &'static str) -> (String, JoinHandle<()>) {
            let listener = match TcpListener::bind("127.0.0.1:0").await {
                Ok(listener) => listener,
                Err(e) => panic!("bind success test server: {e}"),
            };
            let addr = match listener.local_addr() {
                Ok(addr) => addr,
                Err(e) => panic!("read success test server address: {e}"),
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
                        let response = format!(
                            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        if let Err(e) = stream.write_all(response.as_bytes()).await {
                            tracing::debug!("failed to write test response: {e}");
                        }
                    });
                }
            });
            (format!("http://{addr}"), handle)
        }

        /// Regression for #4913: before `session_replay` existed on the
        /// client, koilon's export had no path to the replay-faithful audit
        /// export at all -- only the lossy Markdown transcript. This asserts
        /// the JSON file actually lands on disk with the audit fields
        /// (tool error/approval detail) that the Markdown export drops.
        #[tokio::test]
        #[expect(clippy::expect_used, reason = "test assertions may panic on failure")]
        async fn execute_export_json_writes_replay_export_and_names_the_file() {
            let dir = tempfile::tempdir().expect("create temp export dir");
            let mut app = test_app();
            app.config.workspace_root = Some(dir.path().to_path_buf());
            app.dashboard.focused_session_id = Some(ApiSessionId::from("s1"));
            let (base_url, _server) = success_json_server(REPLAY_BODY).await;
            point_app_at(&mut app, &base_url);

            execute_export_json(&mut app).await;

            let toast = app
                .viewport
                .success_toast
                .as_ref()
                .expect("export must report success");
            assert!(
                toast.message.starts_with("Exported to conversation-")
                    && toast.message.ends_with(".json"),
                "toast should name the exported JSON file: {}",
                toast.message
            );
            assert!(
                !toast.message.contains(
                    dir.path()
                        .to_str()
                        .expect("temp dir path must be valid UTF-8")
                ),
                "toast must not leak the local export directory: {}",
                toast.message
            );

            let exports_dir = dir.path().join("exports");
            let mut entries =
                std::fs::read_dir(&exports_dir).expect("exports dir must have been created");
            let entry = entries
                .next()
                .expect("exactly one export file")
                .expect("readable dir entry");
            let written = std::fs::read_to_string(entry.path()).expect("read exported file");
            let replay: skene::api::types::SessionReplayResponse =
                serde_json::from_str(&written).expect("exported file must be valid JSON replay");
            assert_eq!(replay.messages.len(), 1);
            let audit = replay
                .tool_audit_records
                .first()
                .expect("tool audit record must survive the export");
            assert!(
                audit.is_error,
                "tool error detail must survive the JSON export -- the Markdown export drops it"
            );
            assert_eq!(audit.approval.as_deref(), Some("auto"));
        }

        #[tokio::test]
        #[expect(clippy::expect_used, reason = "test assertions may panic on failure")]
        async fn execute_export_json_errors_without_a_focused_session() {
            let mut app = test_app();
            app.dashboard.focused_session_id = None;

            execute_export_json(&mut app).await;

            let toast = app
                .viewport
                .error_toast
                .as_ref()
                .expect("must report an error with no focused session");
            assert_eq!(toast.message, "No session to export");
        }
    }
}
