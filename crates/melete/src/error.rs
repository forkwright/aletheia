//! Melete-specific errors.

use snafu::Snafu;

/// Errors from distillation operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[expect(
    missing_docs,
    reason = "snafu variant fields are self-documenting via display format"
)]
#[non_exhaustive]
pub enum Error {
    /// LLM call failed during distillation.
    #[snafu(display("LLM call failed during distillation: {source}"))]
    LlmCall {
        source: hermeneus::error::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Distillation produced an empty summary.
    #[snafu(display("distillation produced empty summary"))]
    EmptySummary {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Session has no messages to distill.
    #[snafu(display("session has no messages to distill"))]
    NoMessages {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// LLM provider panicked during distillation. (#2216)
    #[snafu(display("LLM call panicked during distillation: {message}"))]
    LlmPanic {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// I/O error during consolidation lock operations.
    #[snafu(display("consolidation lock I/O: {context}"))]
    DreamLockIo {
        context: String,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Blocking-pool task running a consolidation lock operation panicked.
    #[snafu(display("consolidation lock task panicked: {source}"))]
    DreamLockJoin {
        source: tokio::task::JoinError,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Blocking-pool task running a consolidation store operation was
    /// cancelled before it produced a value.
    #[snafu(display("consolidation {context} task cancelled: {source}"))]
    DreamStoreCancelled {
        context: String,
        source: tokio::task::JoinError,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Consolidation lock is held by another active process.
    #[snafu(display("consolidation lock held by PID {pid}"))]
    DreamLockHeld {
        pid: u32,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Transcript source failed during auto-dream consolidation.
    #[snafu(display("transcript source error: {context}"))]
    DreamTranscriptSource {
        context: String,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Consolidation target failed during fact merge.
    #[snafu(display("consolidation target error: {context}"))]
    DreamConsolidationTarget {
        context: String,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Backward-path probe verification rejected a memory flush.
    #[snafu(display(
        "backward-path probe verification failed: {failure_count}/{total_probes} probes failed"
    ))]
    ProbeVerification {
        failure_count: usize,
        total_probes: usize,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

impl Error {
    /// Whether this is a typed front-door "provider not ready" refusal
    /// (`hermeneus::error::Error::ProviderNotReady`, surfaced through
    /// [`Error::LlmCall`]).
    ///
    /// WHY(#7261): the auto-dream loop (`crate::dream`) uses this to tell a
    /// Sleeping/Loading/Failed refusal — deployment lifecycle state that
    /// clears on its own — apart FROM a genuine, non-retryable distillation
    /// failure, so the consolidation lock can be left un-advanced and the
    /// affected sessions stay eligible for the next dream cycle instead of
    /// being skipped forever.
    #[must_use]
    pub fn is_provider_not_ready(&self) -> bool {
        matches!(
            self,
            Error::LlmCall {
                source: hermeneus::error::Error::ProviderNotReady { .. },
                ..
            }
        )
    }
}

/// Convenience alias for `Result` with melete's [`Error`] type.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use snafu::ResultExt as _;

    use super::*;

    /// Regression test for #7261: the classification must key off the
    /// specific `ProviderNotReady` variant, not merely "some `LlmCall` error".
    #[test]
    fn is_provider_not_ready_true_for_front_door_refusal() {
        let front_door_err: hermeneus::error::Error = hermeneus::error::ProviderNotReadySnafu {
            provider: "mock".to_owned(),
            state: hermeneus::front_door::FrontDoorState::Loading,
            retry_after_ms: 500_u64,
        }
        .build();
        let err: Result<()> = Err(front_door_err).context(LlmCallSnafu);
        assert!(err.unwrap_err().is_provider_not_ready());
    }

    #[test]
    fn is_provider_not_ready_false_for_other_llm_call_errors() {
        let other_err: hermeneus::error::Error = hermeneus::error::ApiRequestSnafu {
            message: "boom".to_owned(),
        }
        .build();
        let err: Result<()> = Err(other_err).context(LlmCallSnafu);
        assert!(!err.unwrap_err().is_provider_not_ready());
    }

    #[test]
    fn is_provider_not_ready_false_for_non_llm_call_errors() {
        let err: Result<()> = EmptySummarySnafu.fail();
        assert!(!err.unwrap_err().is_provider_not_ready());
    }
}
