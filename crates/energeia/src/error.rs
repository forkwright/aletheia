//! Energeia-specific errors.

use snafu::Snafu;

/// Errors from dispatch orchestration operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields are self-documenting via display format"
)]
pub enum Error {
    /// Dispatch was aborted via cancellation.
    #[snafu(display("dispatch aborted"))]
    Aborted {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Budget limit exceeded during dispatch.
    #[snafu(display("budget exceeded: {reason}"))]
    BudgetExceeded {
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Session spawn failed for a specific prompt.
    #[snafu(display("spawn failed for prompt {prompt_number}: {detail}"))]
    SpawnFailed {
        prompt_number: u32,
        detail: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Session resume failed.
    #[snafu(display("resume failed for session '{session_id}': {detail}"))]
    ResumeFailed {
        session_id: String,
        detail: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// QA evaluation failed.
    #[snafu(display("QA evaluation failed: {detail}"))]
    QaFailed {
        detail: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// LLM provider error during dispatch operations.
    // NOTE: will wrap aletheia_hermeneus::error::Error as `source` once
    // hermeneus compiles on main. For now, carries the error as a string.
    #[snafu(display("LLM error: {detail}"))]
    Llm {
        detail: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Preflight validation failed before dispatch could start.
    #[snafu(display("preflight failed: {reason}"))]
    Preflight {
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Blast radius overlap detected between concurrent sessions.
    #[snafu(display("blast radius overlap: {detail}"))]
    BlastRadiusOverlap {
        detail: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Task join error from a spawned session task.
    #[snafu(display("task join failed: {detail}"))]
    TaskJoin {
        detail: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Engine-level transport or protocol error.
    #[snafu(display("engine error: {detail}"))]
    Engine {
        detail: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Convenience alias for results with [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_aborted() {
        let err = AbortedSnafu.build();
        assert!(err.to_string().contains("aborted"));
    }

    #[test]
    fn error_display_budget_exceeded() {
        let err = BudgetExceededSnafu {
            reason: "cost limit $5.00",
        }
        .build();
        let msg = err.to_string();
        assert!(msg.contains("budget exceeded"));
        assert!(msg.contains("$5.00"));
    }

    #[test]
    fn error_display_spawn_failed() {
        let err = SpawnFailedSnafu {
            prompt_number: 3u32,
            detail: "auth failure",
        }
        .build();
        let msg = err.to_string();
        assert!(msg.contains("prompt 3"));
        assert!(msg.contains("auth failure"));
    }

    #[test]
    fn error_display_resume_failed() {
        let err = ResumeFailedSnafu {
            session_id: "sess-abc",
            detail: "session expired",
        }
        .build();
        let msg = err.to_string();
        assert!(msg.contains("sess-abc"));
        assert!(msg.contains("session expired"));
    }

    #[test]
    fn error_display_qa_failed() {
        let err = QaFailedSnafu {
            detail: "diff too large",
        }
        .build();
        assert!(err.to_string().contains("diff too large"));
    }

    #[test]
    fn error_display_preflight() {
        let err = PreflightSnafu {
            reason: "missing project config",
        }
        .build();
        assert!(err.to_string().contains("missing project config"));
    }

    #[test]
    fn error_display_blast_radius_overlap() {
        let err = BlastRadiusOverlapSnafu {
            detail: "prompts 1 and 3 share src/lib.rs",
        }
        .build();
        assert!(err.to_string().contains("src/lib.rs"));
    }

    #[test]
    fn error_display_task_join() {
        let err = TaskJoinSnafu {
            detail: "task panicked",
        }
        .build();
        assert!(err.to_string().contains("task panicked"));
    }

    #[test]
    fn error_display_engine() {
        let err = EngineSnafu {
            detail: "connection refused",
        }
        .build();
        assert!(err.to_string().contains("connection refused"));
    }

    #[test]
    fn error_is_send_sync() {
        static_assertions::assert_impl_all!(Error: Send, Sync);
    }
}
