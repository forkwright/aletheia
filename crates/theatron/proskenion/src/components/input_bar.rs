//! Rich chat input bar with multiline textarea, history, and submit handling.

use dioxus::prelude::*;

use crate::state::commands::{CommandStore, CommandUiState};
use crate::state::input::InputState;

/// Props for the [`InputBar`] component.
#[derive(Props, Clone, PartialEq)]
pub(crate) struct InputBarProps {
    /// Signal holding the input state (text, history, submission).
    pub input: Signal<InputState>,
    /// Whether a stream is currently active (disables input).
    pub is_streaming: bool,
    /// Callback fired when the user submits a message.
    pub on_submit: EventHandler<String>,
    /// Callback fired when the user clicks the abort button.
    pub on_abort: EventHandler<()>,
}

/// Rich chat input bar with multiline textarea and history navigation.
///
/// - Submit: Enter (Ctrl+Enter also works)
/// - Newline: Shift+Enter
/// - History: Up/Down arrows when cursor is at start/end
/// - Disabled with "Streaming..." placeholder during active stream
#[component]
pub(crate) fn InputBar(props: InputBarProps) -> Element {
    let mut input = props.input;
    let is_streaming = props.is_streaming;
    let on_submit = props.on_submit;
    let on_abort = props.on_abort;
    let mut command_ui = use_context::<Signal<CommandUiState>>();
    let mut commands = use_context::<Signal<CommandStore>>();

    let can_submit = !is_streaming && !input.read().text.trim().is_empty();

    let mut do_submit = move || {
        let text = input.read().text.trim().to_string();
        if text.is_empty() || is_streaming {
            return;
        }
        input.write().push_history(text.clone());
        input.write().clear();
        on_submit.call(text);
    };

    let mut submit_selected_command = move || {
        let command = commands.read().selected().cloned();
        if let Some(command) = command {
            let text = format!("/{}", command.name);
            input.write().push_history(text.clone());
            input.write().clear();
            command_ui.write().palette_open = false;
            on_submit.call(text);
        } else if input.read().text.trim().starts_with('/') {
            let text = input.read().text.trim().to_string();
            if !text.is_empty() {
                input.write().push_history(text.clone());
                input.write().clear();
                on_submit.call(text);
            }
        } else {
            command_ui.write().palette_open = false;
        }
    };

    rsx! {
        div {
            class: "input-bar",
            textarea {
                class: "input-bar-textarea",
                placeholder: if is_streaming { "Streaming..." } else { "Type a message... (Enter to send, Shift+Enter for newline)" },
                disabled: is_streaming,
                rows: "1",
                value: "{input.read().text}",
                oninput: move |evt: Event<FormData>| {
                    let value = evt.value().clone();
                    input.write().text = value.clone();

                    if is_streaming {
                        return;
                    }

                    if let Some(prefix) = value.strip_prefix('/') {
                        commands.write().filter_by_prefix(prefix);
                        command_ui.write().palette_open = true;
                    } else if command_ui.read().palette_open {
                        commands.write().filter_by_prefix(&value);
                    }
                },
                onkeydown: move |evt: Event<KeyboardData>| {
                    let key = evt.key();
                    let modifiers = evt.modifiers();

                    if !is_streaming && command_ui.read().palette_open {
                        match key {
                            Key::Escape => {
                                evt.prevent_default();
                                command_ui.write().palette_open = false;
                                commands.write().filter_by_prefix("");
                                return;
                            }
                            Key::ArrowUp => {
                                evt.prevent_default();
                                commands.write().cursor_up();
                                return;
                            }
                            Key::ArrowDown => {
                                evt.prevent_default();
                                commands.write().cursor_down();
                                return;
                            }
                            Key::Enter if !modifiers.contains(Modifiers::SHIFT) => {
                                evt.prevent_default();
                                submit_selected_command();
                                return;
                            }
                            _ => {}
                        }
                    }

                    // Shift+Enter: newline (default textarea behavior, no prevention)
                    if key == Key::Enter && modifiers.contains(Modifiers::SHIFT) {
                        return;
                    }

                    // Enter (plain or Ctrl): submit
                    if key == Key::Enter {
                        evt.prevent_default();
                        do_submit();
                        return;
                    }

                    // Up arrow: navigate to previous history entry
                    if key == Key::ArrowUp && !is_streaming {
                        if input.write().history_prev() {
                            evt.prevent_default();
                        }
                        return;
                    }

                    // Down arrow: navigate to next history entry
                    if key == Key::ArrowDown && !is_streaming && input.write().history_next() {
                        evt.prevent_default();
                    }
                },
            }
            if is_streaming {
                button {
                    class: "btn-chat-action btn-abort",
                    onclick: move |_| on_abort.call(()),
                    "Abort"
                }
            } else {
                button {
                    class: "btn-chat-action btn-send",
                    disabled: !can_submit,
                    onclick: move |_| do_submit(),
                    "Send"
                }
            }
        }
    }
}

/// Compute the number of visible rows for the textarea, clamped to [1, 10].
#[cfg_attr(not(test), expect(dead_code, reason = "used in tests"))]
fn compute_rows(text: &str) -> usize {
    text.split('\n').count().clamp(1, 10)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::input::SubmissionState;

    #[test]
    fn compute_rows_single_line() {
        assert_eq!(compute_rows("hello"), 1);
    }

    #[test]
    fn compute_rows_multiline() {
        assert_eq!(compute_rows("line1\nline2\nline3"), 3);
    }

    #[test]
    fn compute_rows_trailing_newline() {
        assert_eq!(compute_rows("line1\n"), 2);
    }

    #[test]
    fn compute_rows_empty() {
        assert_eq!(compute_rows(""), 1);
    }

    #[test]
    fn compute_rows_clamped_at_ten() {
        let text = "a\n".repeat(20);
        assert_eq!(compute_rows(&text), 10);
    }

    #[test]
    fn submission_state_variants() {
        let idle = SubmissionState::Idle;
        let submitting = SubmissionState::Submitting;
        let error = SubmissionState::Error("fail".into());
        assert_eq!(idle, SubmissionState::Idle);
        assert_eq!(submitting, SubmissionState::Submitting);
        assert_eq!(error, SubmissionState::Error("fail".into()));
    }
}
