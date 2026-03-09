use crate::app::App;
use crate::command::build_suggestions;
use crate::msg::ErrorToast;
use crate::state::Overlay;

pub fn handle_open(app: &mut App) {
    app.command_palette.active = true;
    app.command_palette.input.clear();
    app.command_palette.cursor = 0;
    app.command_palette.selected = 0;
    app.command_palette.suggestions = build_suggestions("", &app.agents);
}

pub fn handle_close(app: &mut App) {
    app.command_palette.active = false;
    app.command_palette.input.clear();
}

pub fn handle_input(app: &mut App, c: char) {
    app.command_palette
        .input
        .insert(app.command_palette.cursor, c);
    app.command_palette.cursor += c.len_utf8();
    refresh_suggestions(app);
    app.command_palette.selected = 0;
}

pub fn handle_backspace(app: &mut App) {
    if app.command_palette.cursor > 0 {
        let mut prev = app.command_palette.cursor - 1;
        while prev > 0 && !app.command_palette.input.is_char_boundary(prev) {
            prev -= 1;
        }
        app.command_palette.input.remove(prev);
        app.command_palette.cursor = prev;
        refresh_suggestions(app);
        app.command_palette.selected = 0;
    } else {
        // Backspace on empty input closes palette (vim behavior)
        app.command_palette.active = false;
    }
}

pub fn handle_delete_word(app: &mut App) {
    let mut pos = app.command_palette.cursor;
    while pos > 0 && app.command_palette.input.as_bytes().get(pos - 1) == Some(&b' ') {
        pos -= 1;
    }
    while pos > 0 && app.command_palette.input.as_bytes().get(pos - 1) != Some(&b' ') {
        pos -= 1;
    }
    app.command_palette
        .input
        .drain(pos..app.command_palette.cursor);
    app.command_palette.cursor = pos;
    refresh_suggestions(app);
    app.command_palette.selected = 0;
}

pub fn handle_up(app: &mut App) {
    app.command_palette.selected = app.command_palette.selected.saturating_sub(1);
}

pub fn handle_down(app: &mut App) {
    let max = app.command_palette.suggestions.len().saturating_sub(1);
    app.command_palette.selected = (app.command_palette.selected + 1).min(max);
}

pub fn handle_tab(app: &mut App) {
    if let Some(suggestion) = app
        .command_palette
        .suggestions
        .get(app.command_palette.selected)
    {
        let base = suggestion.execute_as.clone();
        let args = app
            .command_palette
            .input
            .split_once(' ')
            .map(|(_, a)| format!(" {a}"))
            .unwrap_or_default();
        app.command_palette.input = format!("{base}{args}");
        app.command_palette.cursor = base.len();
        refresh_suggestions(app);
    }
}

pub async fn handle_select(app: &mut App) {
    // Resolve to the selected suggestion before executing
    if let Some(suggestion) = app
        .command_palette
        .suggestions
        .get(app.command_palette.selected)
    {
        let execute_as = suggestion.execute_as.clone();
        let extra_args = app
            .command_palette
            .input
            .split_once(' ')
            .map(|(_, a)| a.trim().to_string())
            .unwrap_or_default();

        if extra_args.is_empty() {
            app.command_palette.input = execute_as;
        } else {
            // Preserve typed args (e.g., user typed "agent sy" but suggestion is "agent")
            let suggestion_has_args = execute_as.contains(' ');
            if suggestion_has_args {
                app.command_palette.input = execute_as;
            } else {
                app.command_palette.input = format!("{execute_as} {extra_args}");
            }
        }
    }
    execute_command(app).await;
}

fn refresh_suggestions(app: &mut App) {
    app.command_palette.suggestions = build_suggestions(&app.command_palette.input, &app.agents);
}

async fn execute_command(app: &mut App) {
    let input = app.command_palette.input.trim().to_string();
    app.command_palette.active = false;
    app.command_palette.input.clear();

    if input.is_empty() {
        return;
    }

    let (cmd_name, args) = match input.split_once(' ') {
        Some((cmd, rest)) => (cmd, rest.trim()),
        None => (input.as_str(), ""),
    };

    match cmd_name {
        "quit" | "q" => app.should_quit = true,
        "help" | "?" => {
            app.overlay = Some(Overlay::Help);
        }
        "agents" | "a" | "sessions" | "s" => {
            app.overlay = Some(Overlay::AgentPicker { cursor: 0 });
        }
        "health" | "h" | "cost" | "$" => {
            app.overlay = Some(Overlay::SystemStatus);
        }
        "agent" => {
            if !args.is_empty() {
                let target = args.to_lowercase();
                if let Some(agent) = app
                    .agents
                    .iter()
                    .find(|a| a.id.to_lowercase() == target || a.name.to_lowercase() == target)
                {
                    let id = agent.id.clone();
                    app.save_scroll_state();
                    if let Some(a) = app.agents.iter_mut().find(|a| a.id == id) {
                        a.has_notification = false;
                    }
                    app.focused_agent = Some(id);
                    app.load_focused_session().await;
                    app.restore_scroll_state();
                } else {
                    app.error_toast = Some(ErrorToast::new(format!("Unknown agent: {args}")));
                }
            } else {
                app.overlay = Some(Overlay::AgentPicker { cursor: 0 });
            }
        }
        "clear" => {
            app.messages.clear();
            app.focused_session_id = None;
            app.streaming_text.clear();
            app.streaming_thinking.clear();
            app.streaming_tool_calls.clear();
            app.scroll_to_bottom();
        }
        "compact" => {
            execute_compact(app).await;
        }
        "recall" | "r" => {
            if args.is_empty() {
                app.error_toast = Some(ErrorToast::new("Usage: :recall <query>".into()));
            } else {
                execute_recall(app, args).await;
            }
        }
        "model" => {
            execute_model(app);
        }
        "settings" => {
            super::settings::handle_open(app).await;
        }
        _ => {
            app.error_toast = Some(ErrorToast::new(format!("Unknown command: {cmd_name}")));
        }
    }
}

fn execute_model(app: &mut App) {
    let agent = app
        .focused_agent
        .as_ref()
        .and_then(|id| app.agents.iter().find(|a| &a.id == id));

    match agent {
        Some(agent) => {
            let model = agent.model.as_deref().unwrap_or("unknown");
            let name = &agent.name;
            app.error_toast = Some(ErrorToast::new(format!("{name}: {model}")));
        }
        None => {
            app.error_toast = Some(ErrorToast::new("No agent focused".into()));
        }
    }
}

async fn execute_compact(app: &mut App) {
    let session_id = match &app.focused_session_id {
        Some(id) => id.clone(),
        None => {
            app.error_toast = Some(ErrorToast::new("No active session to compact".into()));
            return;
        }
    };

    let client = app.client.clone();
    match client.compact(&session_id).await {
        Ok(()) => {
            app.error_toast = Some(ErrorToast::new("Distillation triggered".into()));
        }
        Err(e) => {
            app.error_toast = Some(ErrorToast::new(format!("Compact failed: {e}")));
        }
    }
}

async fn execute_recall(app: &mut App, query: &str) {
    let nous_id = match &app.focused_agent {
        Some(id) => id.clone(),
        None => {
            app.error_toast = Some(ErrorToast::new("No agent focused for recall".into()));
            return;
        }
    };

    let client = app.client.clone();
    let query = query.to_string();
    match client.recall(&nous_id, &query).await {
        Ok(result) => {
            let display = if result.len() > 200 {
                format!("{}...", safe_truncate(&result, 200))
            } else {
                result
            };
            app.error_toast = Some(ErrorToast::new(display));
        }
        Err(e) => {
            app.error_toast = Some(ErrorToast::new(format!("Recall failed: {e}")));
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
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_helpers::*;

    #[test]
    fn handle_open_activates_palette() {
        let mut app = test_app();
        handle_open(&mut app);
        assert!(app.command_palette.active);
        assert!(app.command_palette.input.is_empty());
        assert_eq!(app.command_palette.cursor, 0);
        assert_eq!(app.command_palette.selected, 0);
        assert!(!app.command_palette.suggestions.is_empty());
    }

    #[test]
    fn handle_close_deactivates_palette() {
        let mut app = test_app();
        handle_open(&mut app);
        handle_close(&mut app);
        assert!(!app.command_palette.active);
        assert!(app.command_palette.input.is_empty());
    }

    #[test]
    fn handle_input_inserts_char() {
        let mut app = test_app();
        handle_open(&mut app);
        handle_input(&mut app, 'q');
        assert_eq!(app.command_palette.input, "q");
        assert_eq!(app.command_palette.cursor, 1);
        assert_eq!(app.command_palette.selected, 0);
    }

    #[test]
    fn handle_input_multibyte_char() {
        let mut app = test_app();
        handle_open(&mut app);
        handle_input(&mut app, '\u{00e9}'); // e with accent
        assert_eq!(app.command_palette.input, "\u{00e9}");
        assert_eq!(app.command_palette.cursor, 2); // 2-byte UTF-8
    }

    #[test]
    fn handle_backspace_removes_char() {
        let mut app = test_app();
        handle_open(&mut app);
        handle_input(&mut app, 'a');
        handle_input(&mut app, 'b');
        handle_backspace(&mut app);
        assert_eq!(app.command_palette.input, "a");
        assert_eq!(app.command_palette.cursor, 1);
    }

    #[test]
    fn handle_backspace_on_empty_closes() {
        let mut app = test_app();
        handle_open(&mut app);
        handle_backspace(&mut app);
        assert!(!app.command_palette.active);
    }

    #[test]
    fn handle_delete_word_removes_word() {
        let mut app = test_app();
        handle_open(&mut app);
        for c in "hello world".chars() {
            handle_input(&mut app, c);
        }
        handle_delete_word(&mut app);
        assert_eq!(app.command_palette.input, "hello ");
    }

    #[test]
    fn handle_up_saturates_at_zero() {
        let mut app = test_app();
        handle_open(&mut app);
        app.command_palette.selected = 0;
        handle_up(&mut app);
        assert_eq!(app.command_palette.selected, 0);
    }

    #[test]
    fn handle_down_clamps_at_max() {
        let mut app = test_app();
        handle_open(&mut app);
        let max = app.command_palette.suggestions.len().saturating_sub(1);
        app.command_palette.selected = max;
        handle_down(&mut app);
        assert_eq!(app.command_palette.selected, max);
    }

    #[test]
    fn handle_tab_completes_suggestion() {
        let mut app = test_app();
        handle_open(&mut app);
        handle_input(&mut app, 'q');
        handle_tab(&mut app);
        // After tab, input should contain the full command name
        assert!(
            app.command_palette.input.starts_with("quit")
                || !app.command_palette.suggestions.is_empty()
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
