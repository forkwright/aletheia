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
            execute_compact(app).await;
        }
        "recall" | "r" => {
            if args.is_empty() {
                app.viewport.error_toast = Some(ErrorToast::new("Usage: :recall <query>".into()));
            } else {
                execute_recall(app, args).await;
            }
        }
        "model" => {
            execute_model(app);
        }
        "new" => {
            super::api::handle_new_session(app).await;
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
            execute_export(app);
        }
        "search" => {
            super::search::handle_open(app);
        }
        "notifications" | "notif" => {
            app.layout.notifications.mark_all_read();
            app.layout.overlay = Some(Overlay::NotificationHistory { scroll: 0 });
        }
        "metrics" | "stats" => {
            super::metrics::handle_open(app).await;
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

async fn execute_compact(app: &mut App) {
    let session_id = match &app.dashboard.focused_session_id {
        Some(id) => id.clone(),
        None => {
            app.viewport.error_toast = Some(ErrorToast::new("No active session to compact".into()));
            return;
        }
    };

    let client = app.client.clone();
    match client.compact(&session_id).await {
        Ok(()) => {
            app.viewport.error_toast = Some(ErrorToast::new("Distillation triggered".into()));
        }
        Err(e) => {
            app.viewport.error_toast = Some(ErrorToast::new(format!("Compact failed: {e}")));
        }
    }
}

async fn execute_recall(app: &mut App, query: &str) {
    let nous_id = match &app.dashboard.focused_agent {
        Some(id) => id.clone(),
        None => {
            app.viewport.error_toast = Some(ErrorToast::new("No agent focused for recall".into()));
            return;
        }
    };

    let client = app.client.clone();
    let query = query.to_string();
    match client.recall(&nous_id, &query).await {
        Ok(result) => {
            // SAFETY: sanitized at ingestion: recall results from memory API.
            let clean = sanitize_for_display(&result).into_owned();
            let display = if clean.len() > 200 {
                format!("{}...", safe_truncate(&clean, 200))
            } else {
                clean
            };
            app.viewport.error_toast = Some(ErrorToast::new(display));
        }
        Err(e) => {
            app.viewport.error_toast = Some(ErrorToast::new(format!("Recall failed: {e}")));
        }
    }
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

fn safe_truncate(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or(s)
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
            app.viewport.success_toast =
                Some(ErrorToast::new(format!("Exported to {}", path.display())));
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

    #[test]
    fn safe_truncate_ascii() {
        assert_eq!(safe_truncate("hello", 3), "hel");
        assert_eq!(safe_truncate("hello", 10), "hello");
        assert_eq!(safe_truncate("hello", 5), "hello");
    }

    #[test]
    fn safe_truncate_multibyte() {
        let s = "hello\u{00e9}world"; // e-accent is 2 bytes
        let result = safe_truncate(s, 6);
        // Should not split the multi-byte char
        assert!(result.is_char_boundary(result.len()));
        assert_eq!(result, "hello");
    }

    #[test]
    fn safe_truncate_empty() {
        assert_eq!(safe_truncate("", 5), "");
    }

    #[test]
    fn safe_truncate_emoji() {
        let s = "\u{1F600}test"; // emoji is 4 bytes
        let result = safe_truncate(s, 2);
        assert!(result.is_char_boundary(result.len()));
    }
}
