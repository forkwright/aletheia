//! Connection indicator component state.
//!
//! Provides the data model for a small UI indicator showing SSE stream
//! health. In Dioxus, this becomes a component reading a
//! `Signal<SseConnectionState>` and rendering a colored dot with label.
//!
//! ```text
//! ● Connected          (green)
//! ● Reconnecting (2)   (yellow)
//! ● Disconnected       (red)
//! ```

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
#[non_exhaustive]
pub enum IndicatorColor {
    Green,
    Yellow,
    Red,
}

impl IndicatorColor {
    /// CSS-compatible color string for rendering.
    #[must_use]
    pub fn css(self) -> &'static str {
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
    pub fn from_state(state: &SseConnectionState) -> Self {
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
