//! Collapsible thinking panel for assistant reasoning display.

use dioxus::prelude::*;

/// Props for the [`ThinkingPanel`] component.
#[derive(Props, Clone, PartialEq)]
pub(crate) struct ThinkingPanelProps {
    /// The thinking/reasoning content to display.
    pub content: String,
    /// Whether the thinking content is still being streamed.
    pub is_streaming: bool,
}

/// Collapsible panel that displays assistant thinking/reasoning content.
///
/// Expanded during streaming, auto-collapses after completion. Visually
/// distinct from message content with muted text, italic style, and a
/// left border using design system muted tokens.
#[component]
pub(crate) fn ThinkingPanel(props: ThinkingPanelProps) -> Element {
    let content = &props.content;
    let is_streaming = props.is_streaming;

    // WHY: Local signal tracks expand/collapse per-panel instance. During
    // streaming the panel is forced open; after finalization it collapses.
    let mut expanded = use_signal(|| is_streaming);

    // WHY: Sync the expanded state when streaming status changes. During
    // streaming, force expanded. On completion, auto-collapse.
    use_effect(move || {
        if is_streaming {
            expanded.set(true);
        } else {
            expanded.set(false);
        }
    });

    if content.is_empty() {
        return rsx! {};
    }

    let is_expanded = *expanded.read();
    let header_label = if is_streaming {
        "Thinking..."
    } else {
        "Thinking"
    };
    let chevron = if is_expanded { "\u{25BC}" } else { "\u{25B6}" };
    rsx! {
        div {
            // Header: clickable toggle
            div {
                style: "display: flex; align-items: center; gap: var(--space-2); cursor: pointer; user-select: none; color: var(--text-secondary); font-size: var(--text-xs); font-style: italic; margin-top: var(--space-2); transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick);",
                onclick: move |_| {
                    let current = *expanded.read();
                    expanded.set(!current);
                },
                span { "{chevron}" }
                span { "{header_label}" }
            }
            // Content: animated expand/collapse via max-height transition
            div {
                style: if is_expanded {
                    "border-left: 3px solid var(--border); padding: var(--space-2) var(--space-3); margin-top: var(--space-2); overflow: hidden; transition: max-height 0.3s ease, opacity 0.3s ease; max-height: 2000px; opacity: 1;"
                } else {
                    "border-left: 3px solid var(--border); padding: 0px var(--space-3); margin-top: var(--space-2); overflow: hidden; transition: max-height 0.3s ease, opacity 0.3s ease; max-height: 0px; opacity: 0;"
                },
                div {
                    style: "color: var(--text-secondary); font-style: italic; font-size: var(--text-sm); white-space: pre-line; word-wrap: break-word; overflow-wrap: break-word; line-height: var(--leading-normal);",
                    "{content}"
                }
            }
        }
    }
}

/// Testable state for the thinking panel, separate from Dioxus signals.
#[derive(Debug, Clone, Default)]
pub(crate) struct ThinkingPanelState {
    /// Whether the panel is expanded.
    pub expanded: bool,
    /// Whether the panel is actively streaming.
    pub is_streaming: bool,
}

impl ThinkingPanelState {
    /// Create a state that represents active streaming (expanded).
    #[cfg_attr(not(test), expect(dead_code, reason = "used in tests"))]
    #[must_use]
    pub(crate) fn streaming() -> Self {
        Self {
            expanded: true,
            is_streaming: true,
        }
    }

    /// Finalize the panel: stop streaming and collapse.
    #[cfg_attr(not(test), expect(dead_code, reason = "used in tests"))]
    pub(crate) fn finalize(&mut self) {
        self.is_streaming = false;
        self.expanded = false;
    }

    /// Toggle the expanded/collapsed state.
    #[cfg_attr(not(test), expect(dead_code, reason = "used in tests"))]
    pub(crate) fn toggle(&mut self) {
        self.expanded = !self.expanded;
    }

    /// Header label reflecting streaming state.
    #[cfg_attr(not(test), expect(dead_code, reason = "used in tests"))]
    #[must_use]
    pub(crate) fn header_label(&self) -> &'static str {
        if self.is_streaming {
            "Thinking..."
        } else {
            "Thinking"
        }
    }

    /// Chevron character reflecting expanded state.
    #[cfg_attr(not(test), expect(dead_code, reason = "used in tests"))]
    #[must_use]
    pub(crate) fn chevron(&self) -> &'static str {
        if self.expanded {
            "\u{25BC}"
        } else {
            "\u{25B6}"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_state_default_is_collapsed() {
        let state = ThinkingPanelState::default();
        assert!(!state.expanded);
        assert!(!state.is_streaming);
    }

    #[test]
    fn streaming_state_is_expanded() {
        let state = ThinkingPanelState::streaming();
        assert!(state.expanded);
        assert!(state.is_streaming);
    }

    #[test]
    fn finalize_collapses_panel() {
        let mut state = ThinkingPanelState::streaming();
        state.finalize();
        assert!(!state.expanded);
        assert!(!state.is_streaming);
    }

    #[test]
    fn toggle_flips_expanded() {
        let mut state = ThinkingPanelState::default();
        state.toggle();
        assert!(state.expanded);
        state.toggle();
        assert!(!state.expanded);
    }

    #[test]
    fn header_label_reflects_streaming() {
        let streaming = ThinkingPanelState::streaming();
        assert_eq!(streaming.header_label(), "Thinking...");

        let done = ThinkingPanelState::default();
        assert_eq!(done.header_label(), "Thinking");
    }

    #[test]
    fn chevron_reflects_expanded() {
        let mut state = ThinkingPanelState::default();
        assert_eq!(state.chevron(), "\u{25B6}");
        state.toggle();
        assert_eq!(state.chevron(), "\u{25BC}");
    }
}
