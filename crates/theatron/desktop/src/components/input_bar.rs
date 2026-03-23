//! Rich chat input bar with multiline textarea, history, and submit handling.

use dioxus::prelude::*;

use crate::state::input::InputState;

const INPUT_BAR_STYLE: &str = "\
    display: flex; \
    gap: 8px; \
    padding: 12px 16px; \
    background: #1a1a2e; \
    border-top: 1px solid #333; \
    align-items: flex-end;\
";

const TEXTAREA_STYLE: &str = "\
    flex: 1; \
    background: #0f0f1a; \
    border: 1px solid #333; \
    border-radius: 8px; \
    padding: 10px 14px; \
    color: #e0e0e0; \
    font-size: 14px; \
    font-family: inherit; \
    resize: none; \
    overflow-y: auto; \
    min-height: 40px; \
    max-height: 200px; \
    line-height: 1.4;\
";

const TEXTAREA_DISABLED_STYLE: &str = "\
    flex: 1; \
    background: #0a0a14; \
    border: 1px solid #2a2a3a; \
    border-radius: 8px; \
    padding: 10px 14px; \
    color: #555; \
    font-size: 14px; \
    font-family: inherit; \
    resize: none; \
    overflow-y: auto; \
    min-height: 40px; \
    max-height: 200px; \
    line-height: 1.4;\
";

const SEND_BTN_STYLE: &str = "\
    background: #4a4aff; \
    color: white; \
    border: none; \
    border-radius: 8px; \
    padding: 10px 20px; \
    font-size: 14px; \
    cursor: pointer; \
    white-space: nowrap;\
";

const SEND_BTN_DISABLED: &str = "\
    background: #333; \
    color: #666; \
    border: none; \
    border-radius: 8px; \
    padding: 10px 20px; \
    font-size: 14px; \
    cursor: not-allowed; \
    white-space: nowrap;\
";

const ABORT_BTN_STYLE: &str = "\
    background: #ef4444; \
    color: white; \
    border: none; \
    border-radius: 8px; \
    padding: 10px 20px; \
    font-size: 14px; \
    cursor: pointer; \
    white-space: nowrap;\
";

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
/// - Submit: Ctrl+Enter (Linux)
/// - Newline: Shift+Enter or Enter
/// - History: Up/Down arrows when cursor is at start/end
/// - Disabled with "Streaming..." placeholder during active stream
#[component]
pub(crate) fn InputBar(props: InputBarProps) -> Element {
    let mut input = props.input;
    let is_streaming = props.is_streaming;
    let on_submit = props.on_submit;
    let on_abort = props.on_abort;

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

    rsx! {
        div {
            style: "{INPUT_BAR_STYLE}",
            textarea {
                style: if is_streaming { "{TEXTAREA_DISABLED_STYLE}" } else { "{TEXTAREA_STYLE}" },
                placeholder: if is_streaming { "Streaming..." } else { "Type a message... (Ctrl+Enter to send)" },
                disabled: is_streaming,
                rows: "1",
                value: "{input.read().text}",
                oninput: move |evt: Event<FormData>| {
                    input.write().text = evt.value().clone();
                },
                onkeydown: move |evt: Event<KeyboardData>| {
                    let key = evt.key();
                    let modifiers = evt.modifiers();

                    // Ctrl+Enter: submit
                    if key == Key::Enter && modifiers.contains(Modifiers::CONTROL) {
                        evt.prevent_default();
                        do_submit();
                        return;
                    }

                    // Shift+Enter: newline (default textarea behavior, no prevention)
                    if key == Key::Enter && modifiers.contains(Modifiers::SHIFT) {
                        return;
                    }

                    // Plain Enter: also newline in a multiline textarea
                    if key == Key::Enter {
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
                    if key == Key::ArrowDown && !is_streaming {
                        if input.write().history_next() {
                            evt.prevent_default();
                        }
                    }
                },
            }
            if is_streaming {
                button {
                    style: "{ABORT_BTN_STYLE}",
                    onclick: move |_| on_abort.call(()),
                    "Abort"
                }
            } else {
                button {
                    style: if can_submit { "{SEND_BTN_STYLE}" } else { "{SEND_BTN_DISABLED}" },
                    disabled: !can_submit,
                    onclick: move |_| do_submit(),
                    "Send"
                }
            }
        }
    }
}

/// Compute textarea row count from content for auto-grow.
///
/// Returns the number of visual rows (clamped to 1..=10), accounting
/// for explicit newlines.
#[must_use]
pub(crate) fn compute_rows(text: &str) -> usize {
    let line_count = text.lines().count().max(1);
    // WHY: Add 1 for the trailing newline that .lines() drops.
    let extra = if text.ends_with('\n') { 1 } else { 0 };
    (line_count + extra).clamp(1, 10)
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
