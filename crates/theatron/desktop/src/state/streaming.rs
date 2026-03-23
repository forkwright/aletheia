//! Per-message streaming state for the desktop chat view.

use theatron_core::id::TurnId;
use tokio_util::sync::CancellationToken;

/// Lifecycle phases of a single streaming turn.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum StreamPhase {
    /// No active stream.
    Idle,
    /// Stream initiated, awaiting first event.
    Streaming {
        /// Turn identifier for the active stream.
        message_id: TurnId,
    },
    /// User requested abort, waiting for confirmation.
    Aborting,
    /// Stream completed normally.
    Complete,
    /// Stream terminated with an error.
    Error(String),
}

impl Default for StreamPhase {
    fn default() -> Self {
        Self::Idle
    }
}

impl StreamPhase {
    /// Whether a stream is actively receiving deltas.
    #[must_use]
    pub(crate) fn is_active(&self) -> bool {
        matches!(self, Self::Streaming { .. })
    }

    /// Whether we are in any non-idle phase (streaming, aborting, etc).
    #[must_use]
    pub(crate) fn is_busy(&self) -> bool {
        matches!(self, Self::Streaming { .. } | Self::Aborting)
    }
}

/// Per-message streaming state that drives the chat view signals.
///
/// Holds the accumulated text and thinking buffers, active tool calls,
/// and the abort handle for the current stream.
#[derive(Debug, Clone)]
pub struct PerMessageStreamState {
    /// Current streaming phase.
    pub phase: StreamPhase,
    /// Accumulated response text (flushed from the debounce buffer).
    pub text: String,
    /// Accumulated thinking/reasoning text.
    pub thinking: String,
    /// Active tool calls during this turn.
    pub tool_calls: Vec<crate::state::events::ToolCallInfo>,
    /// Error message if the stream failed.
    pub error: Option<String>,
    /// Cancellation token for aborting the stream.
    pub(crate) cancel: CancellationToken,
}

impl Default for PerMessageStreamState {
    fn default() -> Self {
        Self {
            phase: StreamPhase::default(),
            text: String::new(),
            thinking: String::new(),
            tool_calls: Vec::new(),
            error: None,
            cancel: CancellationToken::new(),
        }
    }
}

impl PerMessageStreamState {
    /// Reset to idle state with a fresh cancellation token.
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    /// Request abort of the active stream.
    ///
    /// Cancels the token and transitions to the `Aborting` phase.
    pub(crate) fn abort(&mut self) {
        if self.phase.is_active() {
            self.cancel.cancel();
            self.phase = StreamPhase::Aborting;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_phase_is_idle() {
        let state = PerMessageStreamState::default();
        assert_eq!(state.phase, StreamPhase::Idle);
        assert!(!state.phase.is_active());
        assert!(!state.phase.is_busy());
    }

    #[test]
    fn streaming_phase_is_active_and_busy() {
        let phase = StreamPhase::Streaming {
            message_id: "t1".into(),
        };
        assert!(phase.is_active());
        assert!(phase.is_busy());
    }

    #[test]
    fn aborting_phase_is_busy_not_active() {
        let phase = StreamPhase::Aborting;
        assert!(!phase.is_active());
        assert!(phase.is_busy());
    }

    #[test]
    fn abort_cancels_token_and_transitions() {
        let mut state = PerMessageStreamState::default();
        state.phase = StreamPhase::Streaming {
            message_id: "t1".into(),
        };
        let token = state.cancel.clone();

        state.abort();
        assert_eq!(state.phase, StreamPhase::Aborting);
        assert!(token.is_cancelled());
    }

    #[test]
    fn abort_noop_when_idle() {
        let mut state = PerMessageStreamState::default();
        let token = state.cancel.clone();

        state.abort();
        assert_eq!(state.phase, StreamPhase::Idle);
        assert!(!token.is_cancelled());
    }

    #[test]
    fn reset_returns_to_default() {
        let mut state = PerMessageStreamState::default();
        state.text = "accumulated".into();
        state.thinking = "reasoning".into();
        state.phase = StreamPhase::Complete;

        state.reset();
        assert_eq!(state.phase, StreamPhase::Idle);
        assert!(state.text.is_empty());
        assert!(state.thinking.is_empty());
    }
}
