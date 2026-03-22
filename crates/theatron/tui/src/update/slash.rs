/// Slash-command autocomplete update handlers.
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

use crate::app::App;
use crate::command::COMMANDS;
use crate::state::SlashSuggestion;

pub(crate) fn handle_open(app: &mut App) {
    app.interaction.slash_complete.active = true;
    app.interaction.slash_complete.query.clear();
    app.interaction.slash_complete.cursor = 0;
    refresh_suggestions(app);
}

pub(crate) fn handle_close(app: &mut App) {
    app.interaction.slash_complete.active = false;
    app.interaction.slash_complete.query.clear();
    app.interaction.slash_complete.suggestions.clear();
    app.interaction.slash_complete.cursor = 0;
}

pub(crate) fn handle_input(app: &mut App, c: char) {
    app.interaction.slash_complete.query.push(c);
    app.interaction.slash_complete.cursor = 0;
    refresh_suggestions(app);
}

pub(crate) fn handle_backspace(app: &mut App) {
    if app.interaction.slash_complete.query.is_empty() {
        handle_close(app);
    } else {
        app.interaction.slash_complete.query.pop();
        app.interaction.slash_complete.cursor = 0;
        refresh_suggestions(app);
    }
}

pub(crate) fn handle_up(app: &mut App) {
    app.interaction.slash_complete.cursor = app.interaction.slash_complete.cursor.saturating_sub(1);
}

pub(crate) fn handle_down(app: &mut App) {
    let max = app
        .interaction
        .slash_complete
        .suggestions
        .len()
        .saturating_sub(1);
    app.interaction.slash_complete.cursor = (app.interaction.slash_complete.cursor + 1).min(max);
}

pub(crate) async fn handle_select(app: &mut App) {
    let execute_as = app
        .interaction
        .slash_complete
        .suggestions
        .get(app.interaction.slash_complete.cursor)
        .map(|s| s.execute_as.clone());

    handle_close(app);

    if let Some(cmd) = execute_as {
        app.interaction.command_palette.input = cmd;
        app.interaction.command_palette.cursor = app.interaction.command_palette.input.len();
        super::command::execute_command(app).await;
    }
}

const MAX_SLASH_SUGGESTIONS: usize = 8;

fn refresh_suggestions(app: &mut App) {
    let query = app.interaction.slash_complete.query.clone();
    let matcher = SkimMatcherV2::default();

    let mut scored: Vec<(i64, SlashSuggestion)> = COMMANDS
        .iter()
        .filter_map(|cmd| {
            if query.is_empty() {
                return Some((
                    0i64,
                    SlashSuggestion {
                        name: cmd.name.to_string(),
                        description: cmd.description.to_string(),
                        execute_as: cmd.name.to_string(),
                    },
                ));
            }
            let mut best: Option<i64> = None;
            if let Some(s) = matcher.fuzzy_match(cmd.name, &query) {
                best = Some(s);
            }
            for alias in cmd.aliases {
                if let Some(s) = matcher.fuzzy_match(alias, &query) {
                    best = best.map_or(Some(s), |p| Some(p.max(s)));
                }
            }
            if let Some(s) = matcher.fuzzy_match(cmd.description, &query) {
                best = best.map_or(Some(s), |p| Some(p.max(s)));
            }
            best.map(|score| {
                (
                    score,
                    SlashSuggestion {
                        name: cmd.name.to_string(),
                        description: cmd.description.to_string(),
                        execute_as: cmd.name.to_string(),
                    },
                )
            })
        })
        .collect();

    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.truncate(MAX_SLASH_SUGGESTIONS);
    app.interaction.slash_complete.suggestions = scored.into_iter().map(|(_, s)| s).collect();

    // Clamp cursor after suggestion list may have shrunk.
    let max = app
        .interaction
        .slash_complete
        .suggestions
        .len()
        .saturating_sub(1);
    if app.interaction.slash_complete.cursor > max {
        app.interaction.slash_complete.cursor = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_helpers::test_app;

    #[test]
    fn open_activates_and_populates() {
        let mut app = test_app();
        handle_open(&mut app);
        assert!(app.interaction.slash_complete.active);
        assert!(app.interaction.slash_complete.query.is_empty());
        assert!(!app.interaction.slash_complete.suggestions.is_empty());
    }

    #[test]
    fn close_clears_state() {
        let mut app = test_app();
        handle_open(&mut app);
        handle_close(&mut app);
        assert!(!app.interaction.slash_complete.active);
        assert!(app.interaction.slash_complete.suggestions.is_empty());
    }

    #[test]
    fn input_filters_suggestions() {
        let mut app = test_app();
        handle_open(&mut app);
        handle_input(&mut app, 'q');
        assert!(
            app.interaction
                .slash_complete
                .suggestions
                .iter()
                .any(|s| s.name == "quit")
        );
    }

    #[test]
    fn backspace_on_empty_query_closes() {
        let mut app = test_app();
        handle_open(&mut app);
        handle_backspace(&mut app);
        assert!(!app.interaction.slash_complete.active);
    }

    #[test]
    fn backspace_removes_last_char() {
        let mut app = test_app();
        handle_open(&mut app);
        handle_input(&mut app, 'q');
        handle_input(&mut app, 'u');
        handle_backspace(&mut app);
        assert_eq!(app.interaction.slash_complete.query, "q");
        assert!(app.interaction.slash_complete.active);
    }

    #[test]
    fn up_down_navigation() {
        let mut app = test_app();
        handle_open(&mut app);
        assert_eq!(app.interaction.slash_complete.cursor, 0);
        handle_down(&mut app);
        assert_eq!(app.interaction.slash_complete.cursor, 1);
        handle_up(&mut app);
        assert_eq!(app.interaction.slash_complete.cursor, 0);
        handle_up(&mut app);
        assert_eq!(app.interaction.slash_complete.cursor, 0);
    }

    #[test]
    fn suggestions_capped_at_max() {
        let mut app = test_app();
        handle_open(&mut app);
        assert!(app.interaction.slash_complete.suggestions.len() <= MAX_SLASH_SUGGESTIONS);
    }

    #[test]
    fn input_resets_cursor_to_zero() {
        let mut app = test_app();
        handle_open(&mut app);
        handle_down(&mut app);
        handle_down(&mut app);
        handle_input(&mut app, 'q');
        assert_eq!(app.interaction.slash_complete.cursor, 0);
    }
}
