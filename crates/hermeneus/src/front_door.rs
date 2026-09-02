//! Stateful front door for on-demand localhosted/embedded providers (#7152).
//!
//! An admission cap ([`crate::concurrency::AdmissionPolicy::Fixed`]) bounds
//! how many requests a localhosted provider serves *once it is up*. It says
//! nothing about the provider's own lifecycle: an on-demand model server
//! (idle-stop, cold-start on first request) spends real time asleep or
//! loading before it can answer at all. Left alone, a transport failure
//! during that window is just another `is_retryable() == true` transient
//! error — [`nous`](../../nous/index.html)'s registry-backed fallback chain
//! (`crates/nous/src/execute/model_fallback.rs`) walks past it to the next
//! configured route, which may be a cloud model. That is exactly the
//! silent-cloud-fallback the launch contract forbids for the local path.
//!
//! [`FrontDoorTracker`] gives each front-door-enabled provider instance a
//! small state machine — sleeping, loading, ready, overloaded, failed — and
//! [`crate::error::Error::ProviderNotReady`] gives transport failures
//! observed while not `Ready` a typed, non-retryable shape instead of the
//! generic transient `ApiRequest` classification. Non-retryable here means
//! specifically what it means for [`crate::error::Error::ProviderSaturated`]:
//! `record_attempt` in the fallback chain treats a non-retryable error as
//! terminal and does not advance to the next route.
//!
//! Overload is intentionally NOT modeled here as its own refusal error —
//! [`Self::note_saturated`] only updates the observable state; the caller
//! still sees [`crate::error::Error::ProviderSaturated`] from the admission
//! layer, which already satisfies "typed, non-retryable" for that case.

use std::sync::Mutex; // kanon:ignore RUST/std-mutex-in-async — lock held only during brief state reads/writes, never across .await

/// Consecutive transport failures before [`FrontDoorState::Sleeping`] or
/// [`FrontDoorState::Loading`] escalates to [`FrontDoorState::Failed`].
///
/// WHY: a single dropped connection or slow cold start is expected and
/// self-resolving; a fourth consecutive failure without ever completing a
/// request means something needs operator attention, not another silent
/// retry-later hint.
pub const FRONT_DOOR_FAILURE_THRESHOLD: u32 = 3;

/// Retry hint for a [`FrontDoorState::Sleeping`] refusal, in milliseconds.
///
/// WHY: unlike [`crate::error::Error::ProviderSaturated`]'s
/// latency-EWMA-derived hint, there is no in-process signal for how long an
/// on-demand backend takes to answer its first connection — the wake
/// trigger and boot time belong to the deployment's own service lifecycle
/// (out of hermeneus's knowledge). A conservative fixed default asks the
/// caller to come back rather than hammering a socket nothing is behind yet.
pub const SLEEPING_RETRY_HINT_MS: u64 = 5_000;

/// Retry hint for a [`FrontDoorState::Loading`] refusal, in milliseconds.
///
/// Shorter than [`SLEEPING_RETRY_HINT_MS`]: a request already reached the
/// backend (the transport failure was a timeout, not a refused connection),
/// so the cold start is already in progress.
pub const LOADING_RETRY_HINT_MS: u64 = 3_000;

/// Retry hint for a [`FrontDoorState::Failed`] refusal, in milliseconds.
///
/// Long enough that a caller polling on this hint does not itself become a
/// retry storm against a backend that has already failed
/// [`FRONT_DOOR_FAILURE_THRESHOLD`] times in a row.
pub const FAILED_RETRY_HINT_MS: u64 = 30_000;

/// The kind of transport failure a front-door-tracked provider observed.
///
/// Derived directly from `reqwest::Error::is_connect()` /
/// `reqwest::Error::is_timeout()` at the call site — see
/// `crates/hermeneus/src/openai/error.rs::map_request_error`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TransportFailureKind {
    /// The connection itself was refused/unreachable — nothing is currently
    /// accepting connections on the endpoint.
    ConnectRefused,
    /// The connection was accepted but no response arrived in time — the
    /// backend is up enough to accept work but has not answered yet.
    Timeout,
}

/// Front-door lifecycle state for an on-demand localhosted/embedded
/// provider (#7152).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FrontDoorState {
    /// Nothing has answered yet and the provider has never been observed
    /// `Ready`, or the last few connection attempts were refused outright.
    /// The default starting state.
    Sleeping,
    /// A request reached the backend (or a request previously succeeded)
    /// but the most recent attempt timed out waiting for a response —
    /// consistent with a cold start already in progress.
    Loading,
    /// The provider answered a request successfully; front-door refusals
    /// are not emitted while in this state.
    Ready,
    /// The admission layer refused a request because the fixed cap was
    /// full ([`crate::error::Error::ProviderSaturated`]). Observational —
    /// see the module docs for why this state has no separate refusal
    /// error of its own.
    Overloaded,
    /// [`FRONT_DOOR_FAILURE_THRESHOLD`] consecutive transport failures
    /// without an intervening success. Needs operator attention; the front
    /// door does not attempt to recover this state on its own.
    Failed,
}

impl FrontDoorState {
    /// Retry hint in milliseconds for a [`crate::error::Error::ProviderNotReady`]
    /// refusal carrying this state.
    ///
    /// Defined for every variant so the type stays total; callers only ever
    /// construct the error for [`Self::Sleeping`], [`Self::Loading`], or
    /// [`Self::Failed`] (see the module docs), so [`Self::Ready`] and
    /// [`Self::Overloaded`] never reach this in practice.
    #[must_use]
    pub fn retry_hint_ms(self) -> u64 {
        // kanon:ignore RUST/pub-visibility
        match self {
            Self::Sleeping => SLEEPING_RETRY_HINT_MS,
            Self::Loading => LOADING_RETRY_HINT_MS,
            Self::Failed => FAILED_RETRY_HINT_MS,
            Self::Ready | Self::Overloaded => 0,
        }
    }

    /// Operator-facing description of this state for a given provider name,
    /// used as the [`crate::error::Error::ProviderNotReady`] display and
    /// `ErrorAction::Surface` message.
    #[must_use]
    pub fn refusal_message(self, provider: &str) -> String {
        // kanon:ignore RUST/pub-visibility
        match self {
            Self::Sleeping => format!(
                "provider '{provider}' has not answered a request yet (idle-stop or \
                 not yet started); retry to trigger on-demand activation"
            ),
            Self::Loading => format!(
                "provider '{provider}' is starting up (a request reached it but has \
                 not completed); retry shortly"
            ),
            Self::Failed => format!(
                "provider '{provider}' has failed {FRONT_DOOR_FAILURE_THRESHOLD} \
                 consecutive requests and needs attention; not retrying automatically"
            ),
            Self::Ready | Self::Overloaded => {
                format!("provider '{provider}' is not ready")
            }
        }
    }
}

struct FrontDoorInner {
    state: FrontDoorState,
    consecutive_failures: u32,
}

/// Per-provider front-door state machine (#7152).
///
/// Constructed once per front-door-enabled provider instance and shared
/// (via `Arc`) between the provider's logical-request call sites, mirroring
/// how [`crate::health::ProviderHealthTracker`] and
/// [`crate::concurrency::AdaptiveConcurrencyLimiter`] are held.
///
/// Thread-safe via `std::sync::Mutex`: every operation is a short state
/// read/write, never held across `.await`.
pub struct FrontDoorTracker {
    inner: Mutex<FrontDoorInner>,
}

impl Default for FrontDoorTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl FrontDoorTracker {
    /// Create a tracker starting in [`FrontDoorState::Sleeping`] — the
    /// correct default for an on-demand provider nothing has probed yet.
    #[must_use]
    pub fn new() -> Self {
        // kanon:ignore RUST/pub-visibility
        Self {
            inner: Mutex::new(FrontDoorInner {
                state: FrontDoorState::Sleeping,
                consecutive_failures: 0,
            }),
        }
    }

    fn lock_inner(&self) -> std::sync::MutexGuard<'_, FrontDoorInner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Current front-door state (snapshot).
    #[must_use]
    pub fn state(&self) -> FrontDoorState {
        // kanon:ignore RUST/pub-visibility
        self.lock_inner().state
    }

    /// Record a successful logical request: the provider is
    /// [`FrontDoorState::Ready`], and the consecutive-failure count resets.
    pub fn note_success(&self) {
        // kanon:ignore RUST/pub-visibility
        let mut inner = self.lock_inner();
        inner.state = FrontDoorState::Ready;
        inner.consecutive_failures = 0;
    }

    /// Record that the admission layer refused a request because the fixed
    /// cap was full ([`crate::error::Error::ProviderSaturated`]).
    ///
    /// Observational only — does not touch the consecutive-failure count,
    /// since a full admission queue is evidence the provider is up (a
    /// request is already running), not evidence it is failing.
    pub fn note_saturated(&self) {
        // kanon:ignore RUST/pub-visibility
        self.lock_inner().state = FrontDoorState::Overloaded;
    }

    /// Record a transport failure and return the resulting state.
    ///
    /// [`TransportFailureKind::ConnectRefused`] moves to
    /// [`FrontDoorState::Sleeping`] (nothing is accepting connections yet);
    /// [`TransportFailureKind::Timeout`] moves to [`FrontDoorState::Loading`]
    /// (a request reached the backend but did not complete). Either kind
    /// escalates to [`FrontDoorState::Failed`] after
    /// [`FRONT_DOOR_FAILURE_THRESHOLD`] consecutive failures.
    pub fn note_transport_failure(&self, kind: TransportFailureKind) -> FrontDoorState {
        // kanon:ignore RUST/pub-visibility
        let mut inner = self.lock_inner();
        inner.consecutive_failures = inner.consecutive_failures.saturating_add(1);
        inner.state = if inner.consecutive_failures >= FRONT_DOOR_FAILURE_THRESHOLD {
            FrontDoorState::Failed
        } else {
            match kind {
                TransportFailureKind::ConnectRefused => FrontDoorState::Sleeping,
                TransportFailureKind::Timeout => FrontDoorState::Loading,
            }
        };
        inner.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_sleeping() {
        let tracker = FrontDoorTracker::new();
        assert_eq!(tracker.state(), FrontDoorState::Sleeping);
    }

    #[test]
    fn connect_refused_stays_sleeping() {
        let tracker = FrontDoorTracker::new();
        let state = tracker.note_transport_failure(TransportFailureKind::ConnectRefused);
        assert_eq!(state, FrontDoorState::Sleeping);
        assert_eq!(tracker.state(), FrontDoorState::Sleeping);
    }

    #[test]
    fn timeout_moves_to_loading() {
        let tracker = FrontDoorTracker::new();
        let state = tracker.note_transport_failure(TransportFailureKind::Timeout);
        assert_eq!(state, FrontDoorState::Loading);
    }

    #[test]
    fn success_moves_to_ready_and_resets_failures() {
        let tracker = FrontDoorTracker::new();
        tracker.note_transport_failure(TransportFailureKind::Timeout);
        tracker.note_success();
        assert_eq!(tracker.state(), FrontDoorState::Ready);

        // WHY: the reset must be real, not cosmetic — two more failures
        // after a success should not immediately reach Failed.
        tracker.note_transport_failure(TransportFailureKind::Timeout);
        tracker.note_transport_failure(TransportFailureKind::Timeout);
        assert_eq!(tracker.state(), FrontDoorState::Loading);
    }

    #[test]
    fn threshold_consecutive_failures_escalate_to_failed() {
        let tracker = FrontDoorTracker::new();
        for _ in 0..FRONT_DOOR_FAILURE_THRESHOLD - 1 {
            let state = tracker.note_transport_failure(TransportFailureKind::Timeout);
            assert_ne!(state, FrontDoorState::Failed);
        }
        let state = tracker.note_transport_failure(TransportFailureKind::Timeout);
        assert_eq!(state, FrontDoorState::Failed);
    }

    #[test]
    fn mixed_failure_kinds_accumulate_toward_the_same_threshold() {
        let tracker = FrontDoorTracker::new();
        tracker.note_transport_failure(TransportFailureKind::ConnectRefused);
        tracker.note_transport_failure(TransportFailureKind::Timeout);
        let state = tracker.note_transport_failure(TransportFailureKind::ConnectRefused);
        assert_eq!(state, FrontDoorState::Failed);
    }

    #[test]
    fn failed_state_requires_a_success_to_recover() {
        let tracker = FrontDoorTracker::new();
        for _ in 0..FRONT_DOOR_FAILURE_THRESHOLD {
            tracker.note_transport_failure(TransportFailureKind::Timeout);
        }
        assert_eq!(tracker.state(), FrontDoorState::Failed);

        // WHY: a failure recorded while already Failed does not un-fail it
        // (it is already the worst state) — only note_success recovers.
        tracker.note_transport_failure(TransportFailureKind::Timeout);
        assert_eq!(tracker.state(), FrontDoorState::Failed);

        tracker.note_success();
        assert_eq!(tracker.state(), FrontDoorState::Ready);
    }

    #[test]
    fn saturation_marks_overloaded_without_touching_failure_count() {
        let tracker = FrontDoorTracker::new();
        tracker.note_transport_failure(TransportFailureKind::Timeout);
        tracker.note_saturated();
        assert_eq!(tracker.state(), FrontDoorState::Overloaded);

        // WHY: saturation must not have reset or advanced the failure
        // counter — one more timeout should still just be Loading, not Failed.
        let state = tracker.note_transport_failure(TransportFailureKind::Timeout);
        assert_eq!(state, FrontDoorState::Loading);
    }

    #[test]
    fn retry_hints_are_distinct_and_ordered_by_urgency() {
        assert!(FrontDoorState::Loading.retry_hint_ms() < FrontDoorState::Sleeping.retry_hint_ms());
        assert!(FrontDoorState::Sleeping.retry_hint_ms() < FrontDoorState::Failed.retry_hint_ms());
    }

    #[test]
    fn refusal_messages_name_the_provider() {
        for state in [
            FrontDoorState::Sleeping,
            FrontDoorState::Loading,
            FrontDoorState::Failed,
        ] {
            let message = state.refusal_message("menos-agent");
            assert!(message.contains("menos-agent"), "{message}");
        }
    }
}
