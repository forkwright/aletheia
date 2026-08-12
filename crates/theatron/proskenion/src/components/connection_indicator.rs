//! Global status indicator wrapper.
//!
//! Merges [`SseConnectionState`] (event-stream transport) and
//! [`BackendHealthState`] (backend subsystem health, #5315) onto skeue's
//! canonical [`ConnectionIndicator`] (W2 extraction) by worst severity, so a
//! degraded/unhealthy backend cannot hide behind a green "Connected" SSE
//! reading. Visual + token handling lives in theatron -- this module only
//! owns the state-to-props mapping for proskenion's connection types.

use std::cmp::Ordering;

use dioxus::prelude::*;
use skeue::{ConnectionIndicator, IndicatorTone};

use crate::state::backend_health::BackendHealthState;
use crate::state::events::SseConnectionState;

/// Ranking over skeue's 3-tone palette, used to compare severities across
/// the SSE and backend-health domains before merging.
///
/// NOTE: `IndicatorTone` is `#[non_exhaustive]` in skeue, so a wildcard arm
/// is required even though every current variant is named; a future tone
/// added upstream lands here as "worst" until this match is updated.
fn tone_rank(tone: IndicatorTone) -> u8 {
    match tone {
        IndicatorTone::Healthy => 0,
        IndicatorTone::Degraded => 1,
        IndicatorTone::Failed => 2,
        _ => 2,
    }
}

fn sse_props(state: &SseConnectionState) -> (IndicatorTone, String, String) {
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

/// Map backend subsystem health onto skeue's 3-tone palette.
///
/// The tone is derived from [`BackendHealthState::severity`] (SSOT for
/// backend ordering) rather than re-deriving it here: `Unauthorized` and
/// `Unreachable` both bucket into `Failed`/`Degraded`-tone (skeue has no
/// fourth or fifth color); they stay distinguishable by label/tooltip text
/// and, load-bearing, by the underlying [`BackendHealthState`] type --
/// callers can never conflate them in code, only in this 3-color rendering.
fn backend_props(state: &BackendHealthState) -> (IndicatorTone, String, String) {
    let tone = match state.severity() {
        0 => IndicatorTone::Healthy,
        1 | 2 => IndicatorTone::Degraded,
        _ => IndicatorTone::Failed,
    };
    let label = state.label();
    (tone, label.clone(), label)
}

/// Merge SSE transport state and backend subsystem health into one
/// indicator by worst severity (#5315 acceptance: "a degraded/unhealthy
/// backend cannot produce a green-only global indicator").
///
/// Ties (both non-healthy at the same tone) compose both labels/tooltips so
/// neither signal is silently dropped.
fn merge_global_status(
    sse: &SseConnectionState,
    backend: &BackendHealthState,
) -> (IndicatorTone, String, String) {
    let (sse_tone, sse_label, sse_tooltip) = sse_props(sse);
    let (backend_tone, backend_label, backend_tooltip) = backend_props(backend);

    match tone_rank(backend_tone).cmp(&tone_rank(sse_tone)) {
        Ordering::Greater => (backend_tone, backend_label, backend_tooltip),
        Ordering::Less => (sse_tone, sse_label, sse_tooltip),
        Ordering::Equal if backend_tone == IndicatorTone::Healthy => {
            (sse_tone, sse_label, sse_tooltip)
        }
        Ordering::Equal => (
            backend_tone,
            format!("{sse_label} · {backend_label}"),
            format!("{sse_tooltip} {backend_tooltip}"),
        ),
    }
}

/// Render the global status indicator.
///
/// Reads `Signal<SseConnectionState>` and `Signal<BackendHealthState>` from
/// context (provided in the app root; see
/// `crate::services::sse_coroutine::start_sse_coroutine` and
/// `crate::services::backend_health::start_backend_health_coroutine`) and
/// forwards a merged [`(tone, label, tooltip)`](IndicatorTone) tuple into
/// the canonical [`ConnectionIndicator`].
#[component]
pub(crate) fn ConnectionIndicatorView() -> Element {
    let sse_state = use_context::<Signal<SseConnectionState>>();
    let backend_health = use_context::<Signal<BackendHealthState>>();
    let (tone, label, tooltip) = merge_global_status(&sse_state.read(), &backend_health.read());
    rsx! {
        ConnectionIndicator { tone, label, tooltip: Some(tooltip) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connected_maps_to_healthy() {
        let (tone, label, _) = sse_props(&SseConnectionState::Connected);
        assert_eq!(tone, IndicatorTone::Healthy);
        assert_eq!(label, "Connected");
    }

    #[test]
    fn reconnecting_maps_to_degraded_with_attempt() {
        let (tone, label, tooltip) = sse_props(&SseConnectionState::Reconnecting { attempt: 3 });
        assert_eq!(tone, IndicatorTone::Degraded);
        assert_eq!(label, "Reconnecting (3)");
        assert!(tooltip.contains('3'));
    }

    #[test]
    fn disconnected_maps_to_failed() {
        let (tone, label, _) = sse_props(&SseConnectionState::Disconnected);
        assert_eq!(tone, IndicatorTone::Failed);
        assert_eq!(label, "Disconnected");
    }

    #[test]
    fn backend_unauthorized_and_unreachable_share_tone_but_differ_in_text() {
        let (unauthorized_tone, unauthorized_label, _) =
            backend_props(&BackendHealthState::Unauthorized);
        let (unreachable_tone, unreachable_label, _) =
            backend_props(&BackendHealthState::Unreachable);
        assert_eq!(unauthorized_tone, IndicatorTone::Failed);
        assert_eq!(unreachable_tone, IndicatorTone::Failed);
        assert_ne!(unauthorized_label, unreachable_label);
    }

    #[test]
    fn merge_prefers_worse_backend_health_over_healthy_sse() {
        let (tone, label, _) = merge_global_status(
            &SseConnectionState::Connected,
            &BackendHealthState::Failed {
                failing: vec!["session_store".to_string()],
            },
        );
        assert_eq!(tone, IndicatorTone::Failed);
        assert!(label.contains("session_store"));
    }

    #[test]
    fn merge_keeps_unauthorized_distinct_from_disconnected_label() {
        let (tone, unauthorized_label, _) = merge_global_status(
            &SseConnectionState::Connected,
            &BackendHealthState::Unauthorized,
        );
        let (_, disconnected_label, _) = merge_global_status(
            &SseConnectionState::Disconnected,
            &BackendHealthState::Unknown,
        );
        assert_eq!(tone, IndicatorTone::Degraded);
        assert_ne!(unauthorized_label, disconnected_label);
    }

    #[test]
    fn merge_prefers_worse_sse_state_over_healthy_backend() {
        let (tone, label, _) = merge_global_status(
            &SseConnectionState::Disconnected,
            &BackendHealthState::Healthy,
        );
        assert_eq!(tone, IndicatorTone::Failed);
        assert_eq!(label, "Disconnected");
    }

    #[test]
    fn merge_composes_labels_on_equal_nonhealthy_tone() {
        let (tone, label, tooltip) = merge_global_status(
            &SseConnectionState::Reconnecting { attempt: 1 },
            &BackendHealthState::Degraded {
                failing: vec!["embeddings".to_string()],
            },
        );
        assert_eq!(tone, IndicatorTone::Degraded);
        assert!(label.contains("Reconnecting"));
        assert!(label.contains("embeddings"));
        assert!(tooltip.contains("Reconnecting"));
        assert!(tooltip.contains("embeddings"));
    }

    #[test]
    fn merge_stays_healthy_when_both_sides_healthy() {
        let (tone, label, _) =
            merge_global_status(&SseConnectionState::Connected, &BackendHealthState::Healthy);
        assert_eq!(tone, IndicatorTone::Healthy);
        assert_eq!(label, "Connected");
    }

    #[test]
    fn merge_does_not_report_healthy_before_first_backend_poll() {
        // WHY: `Unknown` (not-yet-checked) must never render as a clean
        // pass -- acceptance criterion "avoid treating missing health
        // telemetry as healthy" (#5315). It shares severity 0 with Healthy
        // so it doesn't spuriously drag a connected SSE stream down, but it
        // must not claim "Backend healthy" either.
        let (_, label, tooltip) =
            merge_global_status(&SseConnectionState::Connected, &BackendHealthState::Unknown);
        assert_eq!(label, "Connected");
        assert!(!tooltip.to_lowercase().contains("backend healthy"));
    }
}
