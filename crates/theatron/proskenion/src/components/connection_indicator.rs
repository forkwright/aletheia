//! Global connection indicator wrapper.
//!
//! Maps proskenion's SSE transport state plus backend health onto skeue's
//! canonical [`ConnectionIndicator`] (W2 extraction). Visual + token handling
//! lives in theatron; this module owns only the state reduction.

use dioxus::prelude::*;
use skeue::{ConnectionIndicator, IndicatorTone};

use crate::state::events::SseConnectionState;
use crate::state::ops::{HealthStatus, ServiceHealthStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Severity {
    Healthy,
    Degraded,
    Failed,
}

fn props_for(
    sse_state: &SseConnectionState,
    health: &ServiceHealthStore,
) -> (IndicatorTone, String, String) {
    let (sse_severity, sse_label, sse_tip) = sse_props(sse_state);
    let (health_severity, health_label, health_tip) = health_props(health);

    match sse_severity.max(health_severity) {
        Severity::Healthy => (
            IndicatorTone::Healthy,
            "Connected".to_string(),
            "Receiving live events from a healthy backend".to_string(),
        ),
        Severity::Degraded if health_severity >= sse_severity => {
            (IndicatorTone::Degraded, health_label, health_tip)
        }
        Severity::Degraded => (IndicatorTone::Degraded, sse_label, sse_tip),
        Severity::Failed if health_severity >= sse_severity => {
            (IndicatorTone::Failed, health_label, health_tip)
        }
        Severity::Failed => (IndicatorTone::Failed, sse_label, sse_tip),
    }
}

fn sse_props(state: &SseConnectionState) -> (Severity, String, String) {
    match state {
        SseConnectionState::Connected => (
            Severity::Healthy,
            "Connected".to_string(),
            "Receiving live events from the server".to_string(),
        ),
        SseConnectionState::Reconnecting { attempt } => (
            Severity::Degraded,
            format!("Reconnecting ({attempt})"),
            format!("Connection lost. Reconnection attempt {attempt} in progress."),
        ),
        SseConnectionState::Disconnected => (
            Severity::Failed,
            "Disconnected".to_string(),
            "Not connected to the event stream".to_string(),
        ),
    }
}

fn health_props(health: &ServiceHealthStore) -> (Severity, String, String) {
    match health.status {
        HealthStatus::Healthy => (
            Severity::Healthy,
            "Backend healthy".to_string(),
            "All backend health checks pass".to_string(),
        ),
        HealthStatus::Degraded => (
            Severity::Degraded,
            "Backend degraded".to_string(),
            health_tooltip("Backend degraded", health),
        ),
        HealthStatus::Unhealthy => (
            Severity::Failed,
            "Backend unhealthy".to_string(),
            health_tooltip("Backend unhealthy", health),
        ),
        HealthStatus::Unknown if health.error.is_some() => (
            Severity::Failed,
            "Health unavailable".to_string(),
            health
                .error
                .clone()
                .unwrap_or_else(|| "Health unavailable".to_string()),
        ),
        HealthStatus::Unknown => (
            Severity::Degraded,
            "Health unknown".to_string(),
            "Backend health has not loaded yet".to_string(),
        ),
    }
}

fn health_tooltip(prefix: &str, health: &ServiceHealthStore) -> String {
    let names: Vec<&str> = health
        .checks
        .iter()
        .filter(|check| HealthStatus::from_status(&check.status) != HealthStatus::Healthy)
        .map(|check| check.name.as_str())
        .collect();
    if names.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix}: {}", names.join(", "))
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
    let health = use_context::<Signal<ServiceHealthStore>>();
    let (tone, label, tooltip) = props_for(&sse_state.read(), &health.read());
    rsx! {
        ConnectionIndicator { tone, label, tooltip: Some(tooltip) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn health(status: HealthStatus) -> ServiceHealthStore {
        ServiceHealthStore {
            status,
            checks: Vec::new(),
            error: None,
        }
    }

    fn health_with_check(status: HealthStatus, check_status: &str) -> ServiceHealthStore {
        ServiceHealthStore {
            status,
            checks: vec![crate::state::ops::HealthCheckInfo {
                name: "providers".to_string(),
                status: check_status.to_string(),
                message: Some("provider missing".to_string()),
            }],
            error: None,
        }
    }

    #[test]
    fn connected_with_healthy_backend_maps_to_healthy() {
        let (tone, label, _) = props_for(
            &SseConnectionState::Connected,
            &health(HealthStatus::Healthy),
        );
        assert_eq!(tone, IndicatorTone::Healthy);
        assert_eq!(label, "Connected");
    }

    #[test]
    fn reconnecting_with_healthy_backend_maps_to_degraded_with_attempt() {
        let (tone, label, tooltip) = props_for(
            &SseConnectionState::Reconnecting { attempt: 3 },
            &health(HealthStatus::Healthy),
        );
        assert_eq!(tone, IndicatorTone::Degraded);
        assert_eq!(label, "Reconnecting (3)");
        assert!(tooltip.contains('3'));
    }

    #[test]
    fn disconnected_maps_to_failed_even_with_healthy_backend() {
        let (tone, label, _) = props_for(
            &SseConnectionState::Disconnected,
            &health(HealthStatus::Healthy),
        );
        assert_eq!(tone, IndicatorTone::Failed);
        assert_eq!(label, "Disconnected");
    }

    #[test]
    fn degraded_backend_downgrades_connected_sse() {
        let (tone, label, tooltip) = props_for(
            &SseConnectionState::Connected,
            &health_with_check(HealthStatus::Degraded, "warn"),
        );
        assert_eq!(tone, IndicatorTone::Degraded);
        assert_eq!(label, "Backend degraded");
        assert!(tooltip.contains("providers"));
    }

    #[test]
    fn unhealthy_backend_fails_connected_sse() {
        let (tone, label, tooltip) = props_for(
            &SseConnectionState::Connected,
            &health_with_check(HealthStatus::Unhealthy, "fail"),
        );
        assert_eq!(tone, IndicatorTone::Failed);
        assert_eq!(label, "Backend unhealthy");
        assert!(tooltip.contains("providers"));
    }

    #[test]
    fn unknown_backend_is_not_green() {
        let (tone, label, _) = props_for(
            &SseConnectionState::Connected,
            &health(HealthStatus::Unknown),
        );
        assert_eq!(tone, IndicatorTone::Degraded);
        assert_eq!(label, "Health unknown");
    }
}
