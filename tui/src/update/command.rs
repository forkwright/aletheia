use crate::app::App;
use crate::msg::ErrorToast;
use crate::state::Overlay;

pub fn handle_open(app: &mut App) {
    app.command_palette.active = true;
    app.command_palette.input.clear();
    app.command_palette.cursor = 0;
    app.command_palette.selected = 0;
    app.command_palette.suggestions = crate::command::filter_commands("");
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
    app.command_palette.suggestions =
        crate::command::filter_commands(&app.command_palette.input);
    app.command_palette.selected = 0;
}

pub fn handle_backspace(app: &mut App) {
    if app.command_palette.cursor > 0 {
        app.command_palette.cursor -= 1;
        app.command_palette.input.remove(app.command_palette.cursor);
        app.command_palette.suggestions =
            crate::command::filter_commands(&app.command_palette.input);
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
    app.command_palette.suggestions =
        crate::command::filter_commands(&app.command_palette.input);
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
    if let Some(scored) = app
        .command_palette
        .suggestions
        .get(app.command_palette.selected)
    {
        let cmd = &crate::command::COMMANDS[scored.index];
        let args = app
            .command_palette
            .input
            .split_once(' ')
            .map(|(_, a)| format!(" {a}"))
            .unwrap_or_default();
        app.command_palette.input = format!("{}{}", cmd.name, args);
        app.command_palette.cursor = cmd.name.len();
        app.command_palette.suggestions =
            crate::command::filter_commands(&app.command_palette.input);
    }
}

pub async fn handle_select(app: &mut App) {
    execute_command(app).await;
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
            app.error_toast =
                Some(ErrorToast::new("Compact not yet available via TUI".into()));
        }
        "recall" | "r" => {
            if args.is_empty() {
                app.error_toast =
                    Some(ErrorToast::new("Usage: :recall <query>".into()));
            } else {
                app.error_toast =
                    Some(ErrorToast::new(format!("Recall not yet available: {args}")));
            }
        }
        "model" => {
            app.error_toast =
                Some(ErrorToast::new("Model info not yet available".into()));
        }
        _ => {
            app.error_toast =
                Some(ErrorToast::new(format!("Unknown command: {cmd_name}")));
        }
    }
}
