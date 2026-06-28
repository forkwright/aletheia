//! Idempotent turn-finalization support.
//!
//! WHY: finalized turns need durable lifecycle evidence in addition to message
//! history. The primitives here let downstream record deterministic
//! idempotency tokens so pending attempts are detectable and completed turns
//! can short-circuit retries (#4691).

use std::fmt;

/// Stage of the turn-finalization pipeline reached for a given turn.
///
/// INVARIANT: variants are ordered from least to most persisted state. A
/// later stage implies all earlier stages have been reached for the same
/// `FinalizeToken`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum FinalizeStage {
    /// No finalize side effects recorded yet.
    Started,
    /// Session row ensured and user message appended.
    AfterUser,
    /// Tool-call and tool-result messages appended.
    AfterTools,
    /// Assistant message appended.
    AfterAssistant,
    /// Usage record committed.
    AfterUsage,
}

impl fmt::Display for FinalizeStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Started => f.write_str("started"),
            Self::AfterUser => f.write_str("after-user"),
            Self::AfterTools => f.write_str("after-tools"),
            Self::AfterAssistant => f.write_str("after-assistant"),
            Self::AfterUsage => f.write_str("after-usage"),
        }
    }
}

/// Deterministic idempotency token for one turn-finalization attempt.
///
/// WHY: `session_id` alone is not enough. The same session can have many
/// turns, and the same turn can be retried after a partial write. Pairing
/// `session_id` with `turn_id` and a monotonic stage gives a unique,
/// recoverable key for every intermediate state.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FinalizeToken {
    /// Owning session identifier.
    // kanon:ignore RUST/primitive-for-domain-id WHY: SessionId lives in koina::id which is not a dependency of mneme; plain String until koina is wired as a dep
    pub session_id: String,
    /// Turn identifier that is being finalized.
    // kanon:ignore RUST/primitive-for-domain-id WHY: TurnId lives in koina::id which is not a dependency of mneme; plain String until koina is wired as a dep
    pub turn_id: String,
    /// Last stage known to have been reached.
    pub stage: FinalizeStage,
}

impl FinalizeToken {
    /// Create a token at [`FinalizeStage::Started`] for a turn.
    #[must_use]
    pub fn new(session_id: impl Into<String>, turn_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            turn_id: turn_id.into(),
            stage: FinalizeStage::Started,
        }
    }

    /// Advance this token to a later stage.
    ///
    /// NOTE: passing an earlier stage leaves the stage unchanged, which keeps
    /// the token safe to call idempotently when a retry observes stale state.
    #[must_use]
    pub fn with_stage(mut self, stage: FinalizeStage) -> Self {
        if stage > self.stage {
            self.stage = stage;
        }
        self
    }

    /// Return a stable, deterministic key for this token.
    ///
    /// WHY: store layers that do not support multi-row transactions can still
    /// write this key atomically with the first side effect and use it as a
    /// dedup guard on retry.
    #[must_use]
    pub fn stable_key(&self) -> String {
        format!(
            "finalize:{session_id}:{turn_id}:{stage}",
            session_id = self.session_id,
            turn_id = self.turn_id,
            stage = self.stage
        )
    }
}

/// Diagnostic payload emitted when a partial finalization is detected or repaired.
#[derive(Clone, PartialEq, Eq)]
pub struct PartialFinalizeDiagnostic {
    /// Token that was found in a non-terminal state.
    pub token: FinalizeToken,
    /// Whether the partial turn was completed or rolled back.
    pub action: PartialFinalizeAction,
}

impl PartialFinalizeDiagnostic {
    /// Create a diagnostic payload for a detected partial finalization.
    #[must_use]
    pub fn new(token: FinalizeToken, action: PartialFinalizeAction) -> Self {
        Self { token, action }
    }
}

impl fmt::Debug for PartialFinalizeDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PartialFinalizeDiagnostic")
            .field("token_stage", &self.token.stage)
            .field("action", &self.action)
            .finish_non_exhaustive()
    }
}

/// Recovery action taken for a partial finalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PartialFinalizeAction {
    /// Missing stages were completed deterministically.
    Completed,
    /// Persisted stages were rolled back to the last consistent point.
    RolledBack,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_key_includes_session_turn_and_stage() {
        let token = FinalizeToken::new("ses-1", "turn-7").with_stage(FinalizeStage::AfterTools);
        assert_eq!(token.stable_key(), "finalize:ses-1:turn-7:after-tools");
    }

    #[test]
    fn stage_only_advances_forward() {
        let token = FinalizeToken::new("ses-1", "turn-7")
            .with_stage(FinalizeStage::AfterAssistant)
            .with_stage(FinalizeStage::AfterTools);
        assert_eq!(token.stage, FinalizeStage::AfterAssistant);
    }

    #[test]
    fn terminal_stage_orders_after_intermediate() {
        // INVARIANT: AfterUsage > AfterAssistant > AfterTools > AfterUser > Started.
        assert!(FinalizeStage::AfterUsage > FinalizeStage::AfterAssistant);
        assert!(FinalizeStage::AfterAssistant > FinalizeStage::AfterTools);
        assert!(FinalizeStage::AfterTools > FinalizeStage::AfterUser);
        assert!(FinalizeStage::AfterUser > FinalizeStage::Started);
    }

    #[test]
    fn diagnostic_roundtrip() {
        let token = FinalizeToken::new("ses-2", "turn-3").with_stage(FinalizeStage::AfterUser);
        let diag = PartialFinalizeDiagnostic {
            token,
            action: PartialFinalizeAction::Completed,
        };
        assert_eq!(diag.token.session_id, "ses-2");
        assert_eq!(diag.action, PartialFinalizeAction::Completed);
    }
}
