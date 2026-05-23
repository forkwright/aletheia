//! Reserved event-trigger boundary.
//!
//! WHY: KAIROS eventually needs to react to external events (file changes,
//! webhook calls) in addition to cron-scheduled tasks. The current runtime
//! does not start file watchers, a webhook listener, or task queue dispatch
//! from external events yet.

/// Reserved handle for future external trigger routing.
///
/// Today this type has no handler registry and does not dispatch file-watch or
/// webhook events into the task runner. It remains as a stable API marker for
/// the planned trigger subsystem.
#[derive(Debug)]
pub struct TriggerRouter {
    _private: (),
}

impl TriggerRouter {
    /// Create an unwired trigger-router marker.
    #[must_use]
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for TriggerRouter {
    fn default() -> Self {
        Self::new()
    }
}
