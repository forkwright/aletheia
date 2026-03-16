//! Actor message types, lifecycle state machine, and status snapshots.

use std::fmt;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};

use crate::error;
use crate::pipeline::TurnResult;
use crate::stream::TurnStreamEvent;

/// Messages sent to a [`NousActor`](crate::actor::NousActor) via its inbox.
#[non_exhaustive]
pub enum NousMessage {
    /// Process a user message in a session.
    Turn {
        session_key: String,
        /// Database session ID from the session store. When `Some`, the actor
        /// adopts this ID for the in-memory `SessionState` instead of generating
        /// a new one, preventing FK constraint failures in finalize and tools.
        session_id: Option<String>,
        content: String,
        /// Override the agent's configured model for this turn. Used by daemon
        /// tasks (e.g. prosoche heartbeat) to route through a cheaper model.
        model_override: Option<String>,
        /// Caller's tracing span — propagated into the pipeline task for request correlation.
        span: tracing::Span,
        reply: oneshot::Sender<error::Result<TurnResult>>,
    },
    /// Process a user message with real-time streaming events.
    StreamingTurn {
        session_key: String,
        /// Database session ID from the session store. See `Turn::session_id`.
        session_id: Option<String>,
        content: String,
        /// Override the agent's configured model for this turn.
        model_override: Option<String>,
        stream_tx: mpsc::Sender<TurnStreamEvent>,
        /// Caller's tracing span — propagated into the pipeline task for request correlation.
        span: tracing::Span,
        reply: oneshot::Sender<error::Result<TurnResult>>,
    },
    /// Query current lifecycle state.
    Status { reply: oneshot::Sender<NousStatus> },
    /// Liveness ping — actor replies immediately to prove it's alive.
    Ping { reply: oneshot::Sender<()> },
    /// Transition to dormant (sleep).
    Sleep,
    /// Wake from dormant.
    Wake,
    /// Graceful shutdown.
    Shutdown,
}

/// Lifecycle state machine for a nous actor.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NousLifecycle {
    /// Processing a turn or background task.
    Active,
    /// Waiting for messages, no active work.
    Idle,
    /// Paused, inbox buffered. Wakes on message or schedule.
    Dormant,
    /// Too many panics — only accepts Status and Ping queries.
    Degraded,
}

impl fmt::Display for NousLifecycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Idle => write!(f, "idle"),
            Self::Dormant => write!(f, "dormant"),
            Self::Degraded => write!(f, "degraded"),
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
    /// Number of panics caught by the panic boundary.
    pub panic_count: u32,
    /// How long the actor has been running.
    pub uptime: Duration,
}

/// Health snapshot returned by the manager's periodic health check.
#[derive(Debug, Clone)]
pub struct ActorHealth {
    /// Whether the actor responded to a ping in time.
    pub alive: bool,
    /// Number of panics caught since (re)start.
    pub panic_count: u32,
    /// Uptime since last (re)start.
    pub uptime: Duration,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_display() {
        assert_eq!(NousLifecycle::Active.to_string(), "active");
        assert_eq!(NousLifecycle::Idle.to_string(), "idle");
        assert_eq!(NousLifecycle::Dormant.to_string(), "dormant");
        assert_eq!(NousLifecycle::Degraded.to_string(), "degraded");
    }

    #[test]
    fn lifecycle_equality() {
        assert_eq!(NousLifecycle::Active, NousLifecycle::Active);
        assert_ne!(NousLifecycle::Active, NousLifecycle::Idle);
        assert_ne!(NousLifecycle::Idle, NousLifecycle::Dormant);
        assert_ne!(NousLifecycle::Dormant, NousLifecycle::Degraded);
    }

    #[test]
    fn status_construction() {
        let status = NousStatus {
            id: "syn".to_owned(),
            lifecycle: NousLifecycle::Idle,
            session_count: 3,
            active_session: None,
            panic_count: 0,
            uptime: Duration::from_secs(60),
        };
        assert_eq!(status.id, "syn");
        assert_eq!(status.lifecycle, NousLifecycle::Idle);
        assert_eq!(status.session_count, 3);
        assert!(status.active_session.is_none());
        assert_eq!(status.panic_count, 0);
    }

    #[test]
    fn status_with_active_session() {
        let status = NousStatus {
            id: "syn".to_owned(),
            lifecycle: NousLifecycle::Active,
            session_count: 1,
            active_session: Some("main".to_owned()),
            panic_count: 0,
            uptime: Duration::from_secs(0),
        };
        assert_eq!(status.lifecycle, NousLifecycle::Active);
        assert_eq!(status.active_session.as_deref(), Some("main"));
    }

    #[test]
    fn lifecycle_copy() {
        let a = NousLifecycle::Idle;
        let b = a;
        assert_eq!(a, b);
    }

    #[test]
    fn lifecycle_all_variants_display() {
        let variants = [
            NousLifecycle::Active,
            NousLifecycle::Idle,
            NousLifecycle::Dormant,
            NousLifecycle::Degraded,
        ];
        let displays: Vec<String> = variants.iter().map(ToString::to_string).collect();
        assert_eq!(displays.len(), 4);
        // Ensure no duplicates
        let mut deduped = displays.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(deduped.len(), 4);
    }

    #[test]
    fn actor_health_construction() {
        let health = ActorHealth {
            alive: true,
            panic_count: 2,
            uptime: Duration::from_secs(120),
        };
        assert!(health.alive);
        assert_eq!(health.panic_count, 2);
        assert_eq!(health.uptime.as_secs(), 120);
    }
}
