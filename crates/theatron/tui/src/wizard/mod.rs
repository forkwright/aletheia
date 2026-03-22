//! Setup wizard: first-run TUI for instance initialization.

mod state;
mod view;

pub use state::WizardAnswers;
pub(crate) use state::WizardState;

use std::io::IsTerminal as _;
use std::path::PathBuf;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use snafu::ResultExt as _;

use crate::error::{IoSnafu, WizardAbortedSnafu};
use crate::theme::Theme;
use crate::wizard::state::{FieldKind, TOTAL_STEPS};

/// Run the interactive TUI setup wizard.
///
/// Returns [`WizardAnswers`] when the user confirms on the final step.
/// Returns [`crate::error::Error::WizardAborted`] if the user presses Esc.
///
/// The caller is responsible for checking whether the terminal supports a TUI
/// (e.g., via [`std::io::IsTerminal`]) before calling this function.
pub(crate) fn run(
    root: Option<PathBuf>,
    api_key: Option<String>,
) -> crate::error::Result<WizardAnswers> {
    let theme = Theme::detect();
    let mut state = WizardState::new(root, api_key);
    let mut terminal = ratatui::init();

    let result = run_event_loop(&mut terminal, &mut state, &theme);

    ratatui::restore();
    result
}

/// Returns `true` when the current environment supports a TUI wizard.
///
/// Requires both stdin and stdout to be connected to a TTY.
pub fn is_tty() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

fn run_event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    state: &mut WizardState,
    theme: &Theme,
) -> crate::error::Result<WizardAnswers> {
    loop {
        terminal
            .draw(|frame| {
                let area = frame.area();
                view::render(state, frame, area, theme);
            })
            .context(IoSnafu {
                context: "wizard draw",
            })?;

        if state.should_quit {
            return WizardAbortedSnafu.fail();
        }

        if state.completed {
            return Ok(state.collect_answers());
        }

        let event = crossterm::event::read().context(IoSnafu {
            context: "wizard read event",
        })?;

        handle_event(state, event);
    }
}

fn handle_event(state: &mut WizardState, event: Event) {
    let Event::Key(KeyEvent {
        code, modifiers, ..
    }) = event
    else {
        return;
    };

    // Global quit: Ctrl+C or Ctrl+Q
    if modifiers == KeyModifiers::CONTROL && matches!(code, KeyCode::Char('c') | KeyCode::Char('q'))
    {
        state.should_quit = true;
        return;
    }

    let is_editing = state
        .current_step()
        .map(|s| s.editing.is_some())
        .unwrap_or(false);

    if is_editing {
        handle_edit_keys(state, code, modifiers);
    } else {
        handle_nav_keys(state, code, modifiers);
    }
}

fn handle_edit_keys(state: &mut WizardState, code: KeyCode, _modifiers: KeyModifiers) {
    let Some(step) = state.current_step_mut() else {
        return;
    };

    match code {
        KeyCode::Enter => step.commit_edit(),
        KeyCode::Esc => step.cancel_edit(),
        KeyCode::Backspace => {
            if let Some(ref mut edit) = step.editing {
                edit.delete_before();
            }
        }
        KeyCode::Left => {
            if let Some(ref mut edit) = step.editing {
                edit.move_left();
            }
        }
        KeyCode::Right => {
            if let Some(ref mut edit) = step.editing {
                edit.move_right();
            }
        }
        KeyCode::Home => {
            if let Some(ref mut edit) = step.editing {
                edit.move_home();
            }
        }
        KeyCode::End => {
            if let Some(ref mut edit) = step.editing {
                edit.move_end();
            }
        }
        KeyCode::Char('k') if _modifiers == KeyModifiers::CONTROL => {
            if let Some(ref mut edit) = step.editing {
                edit.delete_to_end();
            }
        }
        KeyCode::Char(c) => {
            if let Some(ref mut edit) = step.editing {
                edit.insert(c);
            }
        }
        _ => {}
    }
}

fn handle_nav_keys(state: &mut WizardState, code: KeyCode, modifiers: KeyModifiers) {
    // Ready step: Enter confirms
    if state.step == TOTAL_STEPS - 1 && code == KeyCode::Enter {
        state.completed = true;
        return;
    }

    let is_select = state
        .current_step()
        .and_then(|s| s.current_field())
        .map(|f| matches!(f.kind, FieldKind::Select { .. }))
        .unwrap_or(false);

    let is_readonly = state
        .current_step()
        .and_then(|s| s.current_field())
        .map(|f| matches!(f.kind, FieldKind::ReadOnly))
        .unwrap_or(false);

    match code {
        // Field navigation
        KeyCode::Up | KeyCode::BackTab => {
            if let Some(step) = state.current_step_mut() {
                step.nav_up();
            }
        }
        KeyCode::Down | KeyCode::Tab => {
            if let Some(step) = state.current_step_mut() {
                step.nav_down();
            }
        }

        // Select cycling
        KeyCode::Left if is_select => {
            if let Some(step) = state.current_step_mut() {
                step.cycle_select_prev();
            }
        }
        KeyCode::Right if is_select => {
            if let Some(step) = state.current_step_mut() {
                step.cycle_select_next();
            }
        }

        // Enter: begin edit (text) or cycle select
        KeyCode::Enter if is_select => {
            if let Some(step) = state.current_step_mut() {
                step.cycle_select_next();
            }
        }
        KeyCode::Enter if !is_readonly => {
            if let Some(step) = state.current_step_mut() {
                step.begin_edit();
            }
        }

        // Step navigation
        KeyCode::Char('n') | KeyCode::Char(']') => {
            state.next_step();
        }
        KeyCode::Char('b') | KeyCode::Char('[') => {
            state.back_step();
        }
        KeyCode::Right if modifiers == KeyModifiers::CONTROL => {
            state.next_step();
        }
        KeyCode::Left if modifiers == KeyModifiers::CONTROL => {
            state.back_step();
        }
        KeyCode::PageDown => {
            state.next_step();
        }
        KeyCode::PageUp => {
            state.back_step();
        }

        // Abort
        KeyCode::Esc => {
            state.should_quit = true;
        }

        _ => {}
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::wizard::state::WizardState;

    #[test]
    fn handle_event_ctrl_c_sets_quit() {
        let mut state = WizardState::new(None, None);
        handle_event(
            &mut state,
            Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        );
        assert!(state.should_quit);
    }

    #[test]
    fn handle_event_esc_sets_quit() {
        let mut state = WizardState::new(None, None);
        handle_event(
            &mut state,
            Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        );
        assert!(state.should_quit);
    }

    #[test]
    fn handle_event_n_advances_step() {
        let mut state = WizardState::new(None, None);
        handle_event(
            &mut state,
            Event::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)),
        );
        assert_eq!(state.step, 1);
    }

    #[test]
    fn handle_event_b_does_not_go_below_zero() {
        let mut state = WizardState::new(None, None);
        handle_event(
            &mut state,
            Event::Key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE)),
        );
        assert_eq!(state.step, 0);
    }

    #[test]
    fn handle_event_down_moves_cursor() {
        let mut state = WizardState::new(None, None);
        handle_event(
            &mut state,
            Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
        );
        assert_eq!(state.steps.first().unwrap().cursor, 1);
    }

    #[test]
    fn handle_event_enter_on_select_cycles() {
        let mut state = WizardState::new(None, None);
        // Provider field (index 0) is a Select
        let initial = state
            .steps
            .first()
            .unwrap()
            .fields
            .first()
            .unwrap()
            .value
            .clone();
        handle_event(
            &mut state,
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );
        let after = state
            .steps
            .first()
            .unwrap()
            .fields
            .first()
            .unwrap()
            .value
            .clone();
        assert_ne!(initial, after, "Enter on select should cycle");
    }

    #[test]
    fn handle_event_enter_on_ready_step_sets_completed() {
        let mut state = WizardState::new(None, None);
        // Advance to the last step
        for _ in 0..(TOTAL_STEPS - 1) {
            state.next_step();
        }
        assert_eq!(state.step, TOTAL_STEPS - 1);
        handle_event(
            &mut state,
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );
        assert!(state.completed);
    }

    #[test]
    fn handle_event_enter_on_text_field_begins_edit() {
        let mut state = WizardState::new(None, None);
        // Account step (1), Instance path field (0) is Text
        state.next_step();
        assert_eq!(state.step, 1);
        handle_event(
            &mut state,
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );
        assert!(state.steps.get(1).unwrap().editing.is_some());
    }

    #[test]
    fn handle_event_edit_commit_on_enter() {
        let mut state = WizardState::new(None, None);
        state.next_step(); // Account step
        // Begin edit
        handle_event(
            &mut state,
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );
        assert!(state.steps.get(1).unwrap().editing.is_some());
        // Type a character
        handle_event(
            &mut state,
            Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
        );
        // Commit
        handle_event(
            &mut state,
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );
        assert!(state.steps.get(1).unwrap().editing.is_none());
        let val = &state.steps.get(1).unwrap().fields.first().unwrap().value;
        assert!(val.ends_with('x'));
    }

    #[test]
    fn is_tty_returns_bool() {
        // Just assert it doesn't panic (actual value depends on test runner env)
        let _ = is_tty();
    }
}
