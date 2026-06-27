//! Actor message types, lifecycle state machine, and status snapshots.

use std::fmt;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::config::{NousConfig, PipelineConfig};
use crate::error;
use crate::pipeline::TurnResult;
use crate::stream::TurnStreamEvent;

/// Messages sent to a [`NousActor`](crate::actor::NousActor) via its inbox.
#[non_exhaustive]
pub(crate) enum NousMessage {
    /// Process a user message in a session.
    Turn {
        session_key: String, // kanon:ignore RUST/plain-string-secret
        /// Database session ID from the session store. When `Some`, the actor
        /// adopts this ID for the in-memory `SessionState` instead of generating
        /// a new one, preventing FK constraint failures in finalize and tools.
        session_id: Option<String>,
        /// Inbound request correlation id, when supplied by the caller.
        request_id: Option<String>,
        content: String,
        /// Caller's tracing span: propagated into the pipeline task for request correlation.
        span: tracing::Span,
        /// Request-scoped turn cancellation token.
        turn_cancel: CancellationToken,
        reply: oneshot::Sender<error::Result<TurnResult>>,
    },
    /// Process a user message with real-time streaming events.
    StreamingTurn {
        session_key: String, // kanon:ignore RUST/plain-string-secret
        /// Database session ID from the session store. See `Turn::session_id`.
        session_id: Option<String>,
        /// Canonical turn ULID supplied by the gateway for idempotent streams.
        turn_id: Option<koina::ulid::Ulid>,
        /// Inbound request correlation id, when supplied by the caller.
        request_id: Option<String>,
        content: String,
        stream_tx: mpsc::Sender<TurnStreamEvent>,
        /// Operator approval gate for reversibility-class tool calls (#3958, ADR-005).
        /// `None` falls back to the default policy: deny Mandatory, allow Required.
        approval_gate: Option<crate::approval::ApprovalGate>,
        /// Caller's tracing span: propagated into the pipeline task for request correlation.
        span: tracing::Span,
        /// Request-scoped turn cancellation token.
        turn_cancel: CancellationToken,
        reply: oneshot::Sender<error::Result<TurnResult>>,
    },
    /// Query current lifecycle state.
    Status { reply: oneshot::Sender<NousStatus> },
    /// Liveness ping: actor replies immediately to prove it's alive.
    Ping { reply: oneshot::Sender<()> },
    /// Transition to dormant (sleep).
    Sleep,
    /// Wake from dormant.
    Wake,
    /// Reset degraded state: clears panic counter and restores lifecycle to Idle.
    Recover { reply: oneshot::Sender<bool> },
    /// Apply hot-reloadable runtime configuration to this actor.
    ReloadConfig {
        config: Box<NousConfig>,
        pipeline_config: Box<PipelineConfig>,
        tool_config: Box<taxis::config::ToolLimitsConfig>,
        reply: oneshot::Sender<()>,
    },
    /// Graceful shutdown.
    Shutdown,
}

/// Lifecycle state machine for a nous actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NousLifecycle {
    /// Processing a turn or background task.
    Active,
    /// Waiting for messages, no active work.
    Idle,
    /// Paused, inbox buffered. Wakes on message or schedule.
    Dormant,
    /// Too many panics: only accepts Status and Ping queries.
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
    // kanon:ignore RUST/primitive-for-domain-id — existing String-based ID; migrating to newtype requires cross-crate API changes
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
    /// Total background task failures (panics and join errors) since start.
    pub background_failure_total_count: u32,
    /// Background task failures within `degraded_window_secs`.
    pub background_failure_recent_count: u32,
    /// Most recent background failure message, if any.
    pub background_failure_latest_message: Option<String>,
    /// Kind of the most recent background failure (`panic`, `error`, etc.).
    pub background_failure_latest_kind: Option<String>,
    /// True when recent background failures reached `degraded_panic_threshold`.
    ///
    /// WHY(#5147): background failures are reported explicitly in status/health
    /// but do NOT transition the pipeline lifecycle to `Degraded`.
    pub background_health_degraded: bool,
}

/// Snapshot of the manager's health-poller supervisor state.
#[derive(Debug, Clone)]
pub struct ManagerPollerSnapshot {
    /// Whether the supervisor task is currently running.
    pub running: bool,
    /// Number of times the supervisor has had to respawn the inner poller.
    pub restart_count: u64,
    /// Message from the most recent inner-poller panic, if any.
    pub last_error: Option<String>,
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
    /// Total background task failures (panics and join errors) since start.
    pub background_failure_total_count: u32,
    /// Background task failures within `degraded_window_secs`.
    pub background_failure_recent_count: u32,
    /// Most recent background failure message, if any.
    pub background_failure_latest_message: Option<String>,
    /// Kind of the most recent background failure (`panic`, `error`, etc.).
    pub background_failure_latest_kind: Option<String>,
    /// True when recent background failures reached `degraded_panic_threshold`.
    pub background_health_degraded: bool,
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
            uptime: Duration::from_mins(1),
            background_failure_total_count: 0,
            background_failure_recent_count: 0,
            background_failure_latest_message: None,
            background_failure_latest_kind: None,
            background_health_degraded: false,
        };
        assert_eq!(status.id, "syn");
        assert_eq!(status.lifecycle, NousLifecycle::Idle);
        assert_eq!(status.session_count, 3);
        assert!(status.active_session.is_none());
        assert_eq!(status.panic_count, 0);
        assert_eq!(status.background_failure_total_count, 0);
        assert!(!status.background_health_degraded);
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
            background_failure_total_count: 0,
            background_failure_recent_count: 0,
            background_failure_latest_message: None,
            background_failure_latest_kind: None,
            background_health_degraded: false,
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
        // WHY: Ensure no duplicates
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
            uptime: Duration::from_mins(2),
            background_failure_total_count: 1,
            background_failure_recent_count: 1,
            background_failure_latest_message: Some("boom".to_owned()),
            background_failure_latest_kind: Some("panic".to_owned()),
            background_health_degraded: true,
        };
        assert!(health.alive);
        assert_eq!(health.panic_count, 2);
        assert_eq!(health.uptime.as_secs(), 120);
        assert_eq!(health.background_failure_total_count, 1);
        assert_eq!(health.background_failure_recent_count, 1);
        assert_eq!(
            health.background_failure_latest_message.as_deref(),
            Some("boom")
        );
        assert_eq!(
            health.background_failure_latest_kind.as_deref(),
            Some("panic")
        );
        assert!(health.background_health_degraded);
    }
}
