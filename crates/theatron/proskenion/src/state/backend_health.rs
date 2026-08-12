//! Backend subsystem health state (#5315).
//!
//! Sourced from `GET /api/v1/system/status` (#5313) via
//! [`crate::services::backend_health`]. Kept distinct from
//! [`super::events::SseConnectionState`]: whether the SSE event stream is up
//! and whether the backend's own subsystems are healthy are separate
//! failure domains, merged explicitly by
//! [`crate::components::connection_indicator`] rather than conflated into
//! one enum.

/// Backend subsystem health, as last observed by the polling coroutine.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub(crate) enum BackendHealthState {
    /// Not yet polled since connecting.
    ///
    /// WHY: distinct from `Healthy` so the global indicator never renders
    /// "all clear" before the first poll actually completes -- avoid
    /// treating missing health telemetry as healthy (#5315 acceptance
    /// criteria).
    #[default]
    Unknown,
    /// Every subsystem reports healthy.
    Healthy,
    /// One or more subsystems degraded; none failed.
    Degraded {
        /// Names of the non-healthy subsystems.
        failing: Vec<String>,
    },
    /// One or more subsystems failed.
    Failed {
        /// Names of the failing subsystems.
        failing: Vec<String>,
    },
    /// Transport-level failure: the status endpoint could not be reached or
    /// its response could not be parsed.
    Unreachable,
    /// The connected bearer token lacks the role required to read backend
    /// health.
    ///
    /// INVARIANT: kept as its own case rather than folded into
    /// `Unreachable` -- operator decision (#5315): a permissions problem
    /// the operator can fix must never render identically to a
    /// connectivity problem they cannot.
    Unauthorized,
}

impl BackendHealthState {
    /// Human-readable label for the indicator/tooltip.
    #[must_use]
    pub(crate) fn label(&self) -> String {
        match self {
            Self::Unknown => "Checking backend health…".to_string(),
            Self::Healthy => "Backend healthy".to_string(),
            Self::Degraded { failing } => format!("Backend degraded ({})", failing.join(", ")),
            Self::Failed { failing } => format!("Backend unhealthy ({})", failing.join(", ")),
            Self::Unreachable => "Backend health unreachable".to_string(),
            Self::Unauthorized => {
                "Unauthorized — this token cannot read backend health".to_string()
            }
        }
    }

    /// Severity rank used to merge with SSE transport state by worst-of.
    ///
    /// WHY: `Unauthorized` ranks below `Failed`/`Unreachable` -- the
    /// operator's own rationale (#5315) is that a fixable permission gap is
    /// less severe than an outright outage, even though both are distinct
    /// from `Degraded`.
    #[must_use]
    pub(crate) fn severity(&self) -> u8 {
        match self {
            Self::Unknown | Self::Healthy => 0,
            Self::Degraded { .. } => 1,
            Self::Unauthorized => 2,
            Self::Failed { .. } => 3,
            Self::Unreachable => 4,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_orders_unauthorized_below_failed_and_unreachable() {
        let unauthorized = BackendHealthState::Unauthorized.severity();
        let failed = BackendHealthState::Failed { failing: vec![] }.severity();
        let unreachable = BackendHealthState::Unreachable.severity();
        let degraded = BackendHealthState::Degraded { failing: vec![] }.severity();

        assert!(degraded < unauthorized);
        assert!(unauthorized < failed);
        assert!(failed < unreachable);
    }

    #[test]
    fn unknown_and_healthy_share_zero_severity() {
        assert_eq!(
            BackendHealthState::Unknown.severity(),
            BackendHealthState::Healthy.severity()
        );
    }

    #[test]
    fn unauthorized_is_distinct_from_unreachable() {
        assert_ne!(
            BackendHealthState::Unauthorized,
            BackendHealthState::Unreachable
        );
        assert_ne!(
            BackendHealthState::Unauthorized.label(),
            BackendHealthState::Unreachable.label()
        );
    }

    #[test]
    fn default_is_unknown() {
        assert_eq!(BackendHealthState::default(), BackendHealthState::Unknown);
    }

    #[test]
    fn degraded_label_lists_failing_subsystems() {
        let state = BackendHealthState::Degraded {
            failing: vec!["embeddings".to_string()],
        };
        assert!(state.label().contains("embeddings"));
    }

    #[test]
    fn failed_label_lists_failing_subsystems() {
        let state = BackendHealthState::Failed {
            failing: vec!["session_store".to_string(), "embeddings".to_string()],
        };
        assert!(state.label().contains("session_store"));
        assert!(state.label().contains("embeddings"));
    }
}
