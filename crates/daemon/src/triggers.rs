//! Event-driven activation: file watchers and webhook receiver.
//!
//! WHY: KAIROS daemon needs to react to external events (file changes,
//! webhook calls) in addition to cron-scheduled tasks. The trigger router
//! multiplexes these event sources into the task runner.

/// Routes external events (file changes, webhooks) to registered task handlers.
#[derive(Debug)]
pub struct TriggerRouter {
    _private: (),
}

impl TriggerRouter {
    /// Create a new trigger router.
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
