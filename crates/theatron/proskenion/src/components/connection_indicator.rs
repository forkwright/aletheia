//! SSE connection indicator wrapper.
//!
//! Maps proskenion's [`SseConnectionState`] onto skeue'
//! canonical [`ConnectionIndicator`] (W2 extraction). Visual + token
//! handling lives in theatron — this module only owns the
//! state-to-props mapping for the proskenion-specific connection type.

use dioxus::prelude::*;
use skeue::{ConnectionIndicator, IndicatorTone};

use crate::state::events::SseConnectionState;

fn props_for(state: &SseConnectionState) -> (IndicatorTone, String, String) {
    match state {
        SseConnectionState::Connected => (
            IndicatorTone::Healthy,
            "Connected".to_string(),
            "Receiving live events from the server".to_string(),
        ),
        SseConnectionState::Reconnecting { attempt } => (
            IndicatorTone::Degraded,
            format!("Reconnecting ({attempt})"),
            format!("Connection lost. Reconnection attempt {attempt} in progress."),
        ),
        SseConnectionState::Disconnected => (
            IndicatorTone::Failed,
            "Disconnected".to_string(),
            "Not connected to the event stream".to_string(),
        ),
    }
}

/// Render the SSE connection indicator.
///
/// Reads `Signal<SseConnectionState>` from context (provided in app
/// root) and forwards a [`(tone, label, tooltip)`](IndicatorTone) tuple
/// into the canonical [`ConnectionIndicator`].
#[component]
pub(crate) fn ConnectionIndicatorView() -> Element {
    let sse_state = use_context::<Signal<SseConnectionState>>();
    let (tone, label, tooltip) = props_for(&sse_state.read());
    rsx! {
        ConnectionIndicator { tone, label, tooltip: Some(tooltip) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connected_maps_to_healthy() {
        let (tone, label, _) = props_for(&SseConnectionState::Connected);
        assert_eq!(tone, IndicatorTone::Healthy);
        assert_eq!(label, "Connected");
    }

    #[test]
    fn reconnecting_maps_to_degraded_with_attempt() {
        let (tone, label, tooltip) =
            props_for(&SseConnectionState::Reconnecting { attempt: 3 });
        assert_eq!(tone, IndicatorTone::Degraded);
        assert_eq!(label, "Reconnecting (3)");
        assert!(tooltip.contains('3'));
    }

    #[test]
    fn disconnected_maps_to_failed() {
        let (tone, label, _) = props_for(&SseConnectionState::Disconnected);
        assert_eq!(tone, IndicatorTone::Failed);
        assert_eq!(label, "Disconnected");
    }
}
