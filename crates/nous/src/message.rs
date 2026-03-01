//! Actor message types, lifecycle state machine, and status snapshots.

use std::fmt;

use tokio::sync::oneshot;

use crate::error;
use crate::pipeline::TurnResult;

/// Messages sent to a [`NousActor`](crate::actor::NousActor) via its inbox.
pub enum NousMessage {
    /// Process a user message in a session.
    Turn {
        session_key: String,
        content: String,
        reply: oneshot::Sender<error::Result<TurnResult>>,
    },
    /// Query current lifecycle state.
    Status {
        reply: oneshot::Sender<NousStatus>,
    },
    /// Transition to dormant (sleep).
    Sleep,
    /// Wake from dormant.
    Wake,
    /// Graceful shutdown.
    Shutdown,
}

/// Lifecycle state machine for a nous actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NousLifecycle {
    /// Processing a turn or background task.
    Active,
    /// Waiting for messages, no active work.
    Idle,
    /// Paused, inbox buffered. Wakes on message or schedule.
    Dormant,
}

impl fmt::Display for NousLifecycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Idle => write!(f, "idle"),
            Self::Dormant => write!(f, "dormant"),
        }
    }
}

/// Snapshot of a nous actor's current state.
#[derive(Debug, Clone)]
pub struct NousStatus {
    /// Agent identifier.
    pub id: String,
    /// Current lifecycle state.
    pub lifecycle: NousLifecycle,
    /// Number of sessions tracked.
    pub session_count: usize,
    /// Currently active session key, if any.
    pub active_session: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_display() {
        assert_eq!(NousLifecycle::Active.to_string(), "active");
        assert_eq!(NousLifecycle::Idle.to_string(), "idle");
        assert_eq!(NousLifecycle::Dormant.to_string(), "dormant");
    }

    #[test]
    fn lifecycle_equality() {
        assert_eq!(NousLifecycle::Active, NousLifecycle::Active);
        assert_ne!(NousLifecycle::Active, NousLifecycle::Idle);
        assert_ne!(NousLifecycle::Idle, NousLifecycle::Dormant);
    }

    #[test]
    fn status_construction() {
        let status = NousStatus {
            id: "syn".to_owned(),
            lifecycle: NousLifecycle::Idle,
            session_count: 3,
            active_session: None,
        };
        assert_eq!(status.id, "syn");
        assert_eq!(status.lifecycle, NousLifecycle::Idle);
        assert_eq!(status.session_count, 3);
        assert!(status.active_session.is_none());
    }
}
