//! Connection indicator component.
//!
//! Small UI indicator showing SSE stream health. Reads
//! `Signal<SseConnectionState>` from context and renders a colored dot
//! with label.
//!
//! ```text
//! ● Connected          (green)
//! ● Reconnecting (2)   (yellow)
//! ● Disconnected       (red)
//! ```

use dioxus::prelude::*;

use crate::state::events::SseConnectionState;

/// Visual properties for the connection indicator, derived from
/// [`SseConnectionState`]. Components read this to render without
/// matching on the enum directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionIndicator {
    /// Semantic color for the indicator dot.
    pub color: IndicatorColor,
    /// Short human-readable label.
    pub label: String,
    /// Tooltip or extended description.
    pub tooltip: String,
}

/// Semantic color for the connection indicator.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndicatorColor {
    /// Healthy connection.
    Green,
    /// Degraded or reconnecting.
    Yellow,
    /// Disconnected or failed.
    Red,
}

impl IndicatorColor {
    /// CSS-compatible color string for rendering.
    #[must_use]
    pub(crate) fn css(self) -> &'static str {
        match self {
            Self::Green => "#22c55e",
            Self::Yellow => "#eab308",
            Self::Red => "#ef4444",
        }
    }
}

impl ConnectionIndicator {
    /// Derive indicator state from the SSE connection state.
    #[must_use]
    pub(crate) fn from_state(state: &SseConnectionState) -> Self {
        match state {
            SseConnectionState::Connected => Self {
                color: IndicatorColor::Green,
                label: "Connected".to_string(),
                tooltip: "Receiving live events from the server".to_string(),
            },
            SseConnectionState::Reconnecting { attempt } => Self {
                color: IndicatorColor::Yellow,
                label: format!("Reconnecting ({attempt})"),
                tooltip: format!("Connection lost. Reconnection attempt {attempt} in progress."),
            },
            SseConnectionState::Disconnected => Self {
                color: IndicatorColor::Red,
                label: "Disconnected".to_string(),
                tooltip: "Not connected to the event stream".to_string(),
            },
        }
    }
}

const INDICATOR_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 6px; \
    padding: 6px 12px; \
    font-size: 12px; \
    opacity: 0.85;\
";

/// Render the SSE connection indicator.
///
/// Reads `Signal<SseConnectionState>` from context (provided in app root).
#[component]
pub(crate) fn ConnectionIndicatorView() -> Element {
    let sse_state = use_context::<Signal<SseConnectionState>>();
    let indicator = ConnectionIndicator::from_state(&sse_state.read());
    let color = indicator.color.css();

    rsx! {
        div {
            style: "{INDICATOR_STYLE}",
            title: "{indicator.tooltip}",
            span {
                style: "color: {color}; font-size: 10px;",
                "●"
            }
            span { "{indicator.label}" }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connected_indicator() {
        let ind = ConnectionIndicator::from_state(&SseConnectionState::Connected);
        assert_eq!(ind.color, IndicatorColor::Green);
        assert_eq!(ind.label, "Connected");
    }

    #[test]
    fn reconnecting_indicator() {
        let ind = ConnectionIndicator::from_state(&SseConnectionState::Reconnecting { attempt: 3 });
        assert_eq!(ind.color, IndicatorColor::Yellow);
        assert_eq!(ind.label, "Reconnecting (3)");
        assert!(ind.tooltip.contains('3'));
    }

    #[test]
    fn disconnected_indicator() {
        let ind = ConnectionIndicator::from_state(&SseConnectionState::Disconnected);
        assert_eq!(ind.color, IndicatorColor::Red);
        assert_eq!(ind.label, "Disconnected");
    }

    #[test]
    fn indicator_color_css() {
        assert_eq!(IndicatorColor::Green.css(), "#22c55e");
        assert_eq!(IndicatorColor::Yellow.css(), "#eab308");
        assert_eq!(IndicatorColor::Red.css(), "#ef4444");
    }
}
