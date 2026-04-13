//! Rich chat input bar with multiline textarea, history, and submit handling.

use dioxus::prelude::*;

use crate::state::input::InputState;

const INPUT_BAR_STYLE: &str = "\
    display: flex; \
    gap: var(--space-2); \
    padding: var(--space-3) var(--space-4); \
    background: var(--bg-surface); \
    border-top: 1px solid var(--border); \
    align-items: flex-end; \
    box-shadow: 0 -1px 3px rgb(18 17 15 / 0.08);\
";

const TEXTAREA_STYLE: &str = "\
    flex: 1; \
    background: var(--input-bg); \
    border: 1px solid var(--input-border); \
    border-radius: var(--radius-md); \
    padding: var(--space-3) var(--space-4); \
    color: var(--text-primary); \
    font-size: var(--text-base); \
    font-family: var(--font-body); \
    resize: none; \
    overflow-y: auto; \
    min-height: 40px; \
    max-height: 200px; \
    line-height: var(--leading-normal);\
";

const TEXTAREA_DISABLED_STYLE: &str = "\
    flex: 1; \
    background: var(--bg-surface-dim); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-3) var(--space-4); \
    color: var(--text-muted); \
    font-size: var(--text-base); \
    font-family: var(--font-body); \
    resize: none; \
    overflow-y: auto; \
    min-height: 40px; \
    max-height: 200px; \
    line-height: var(--leading-normal);\
";

const SEND_BTN_STYLE: &str = "\
    background: var(--accent); \
    color: var(--text-inverse); \
    border: none; \
    border-radius: var(--radius-md); \
    padding: var(--space-2) var(--space-4); \
    font-size: var(--text-sm); \
    font-weight: var(--weight-semibold); \
    cursor: pointer; \
    white-space: nowrap; \
    transition: background-color 150ms ease, transform 120ms ease; \
    flex-shrink: 0; \
    min-width: 70px;\
";

const SEND_BTN_DISABLED: &str = "\
    background: var(--border); \
    color: var(--text-muted); \
    border: none; \
    border-radius: var(--radius-md); \
    padding: var(--space-3) var(--space-5); \
    font-size: var(--text-base); \
    cursor: not-allowed; \
    white-space: nowrap;\
";

const ABORT_BTN_STYLE: &str = "\
    background: var(--status-error); \
    color: var(--text-inverse); \
    border: none; \
    border-radius: var(--radius-md); \
    padding: var(--space-2) var(--space-4); \
    font-size: var(--text-sm); \
    font-weight: var(--weight-semibold); \
    cursor: pointer; \
    white-space: nowrap; \
    transition: background-color 150ms ease, transform 120ms ease; \
    flex-shrink: 0; \
    min-width: 70px;\
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

/// Compute the number of visible rows for the textarea, clamped to [1, 10].
#[cfg_attr(not(test), expect(dead_code, reason = "used in tests"))]
fn compute_rows(text: &str) -> usize {
    let count = text.split('\n').count();
    count.max(1).min(10)
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
