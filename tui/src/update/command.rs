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
        app.command_palette.cursor -= 1;
        app.command_palette.input.remove(app.command_palette.cursor);
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
    app.command_palette.input.drain(pos..app.command_palette.cursor);
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
    app.command_palette.suggestions =
        build_suggestions(&app.command_palette.input, &app.agents);
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
                if let Some(agent) = app.agents.iter().find(|a| {
                    a.id.to_lowercase() == target || a.name.to_lowercase() == target
                }) {
                    let id = agent.id.clone();
                    app.save_scroll_state();
                    if let Some(a) = app.agents.iter_mut().find(|a| a.id == id) {
                        a.has_notification = false;
                    }
                    app.focused_agent = Some(id);
                    app.load_focused_session().await;
                    app.restore_scroll_state();
                } else {
                    app.error_toast =
                        Some(ErrorToast::new(format!("Unknown agent: {args}")));
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
                app.error_toast =
                    Some(ErrorToast::new("Usage: :recall <query>".into()));
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
            app.error_toast =
                Some(ErrorToast::new(format!("Unknown command: {cmd_name}")));
        }
    }
}

fn execute_model(app: &mut App) {
    let agent = app.focused_agent.as_ref().and_then(|id| {
        app.agents.iter().find(|a| &a.id == id)
    });

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
                format!("{}...", &result[..200])
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
