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
                style: "display: flex; align-items: center; gap: 6px; cursor: pointer; user-select: none; color: #888; font-size: 12px; font-style: italic; margin-top: 8px;",
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
                    "border-left: 3px solid #333; padding: 8px 12px; margin-top: 8px; overflow: hidden; transition: max-height 0.3s ease, opacity 0.3s ease; max-height: 2000px; opacity: 1;"
                } else {
                    "border-left: 3px solid #333; padding: 0px 12px; margin-top: 8px; overflow: hidden; transition: max-height 0.3s ease, opacity 0.3s ease; max-height: 0px; opacity: 0;"
                },
                div {
                    style: "color: #888; font-style: italic; font-size: 13px; white-space: pre-wrap; word-wrap: break-word; line-height: 1.4;",
                    "{content}"
                }
            }
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
