//! Energeia-specific errors.

use std::path::PathBuf;

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
    /// I/O error reading or listing prompt files.
    #[snafu(display("I/O error for '{}': {source}", path.display()))]
    Io {
        path: PathBuf,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// YAML frontmatter parse error in a prompt file.
    #[snafu(display("frontmatter parse error in '{}': {detail}", path.display()))]
    FrontmatterParse {
        path: PathBuf,
        detail: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Cycle detected in the prompt dependency graph.
    #[snafu(display("cycle in prompt DAG: {}", cycle.iter().map(|n| format!("#{n}")).collect::<Vec<_>>().join(" -> ")))]
    DagCycle {
        /// Prompt numbers forming the cycle.
        cycle: Vec<u32>,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Broken or missing dependency references in the prompt DAG.
    #[snafu(display("broken prompt dependencies: {detail}"))]
    DagMissingDeps {
        detail: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

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
    // NOTE: will wrap hermeneus::error::Error as `source` once
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

    /// State store read/write failure.
    #[snafu(display("store error: {message}"))]
    Store {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Record serialization or deserialization failure.
    #[snafu(display("serialization error: {message}"))]
    Serialization {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Requested record not found.
    #[snafu(display("not found: {what}"))]
    NotFound {
        what: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Invalid model identifier specified.
    #[snafu(display("invalid model: {model}"))]
    InvalidModel {
        model: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Feature not yet implemented.
    #[snafu(display("not implemented: {feature}"))]
    NotImplemented {
        feature: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Convenience alias for results with [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

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
    fn error_display_io() {
        use snafu::IntoError as _;
        let err = IoSnafu {
            path: PathBuf::from("/tmp/foo.md"),
        }
        .into_error(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no such file",
        ));
        assert!(err.to_string().contains("foo.md"));
    }

    #[test]
    fn error_display_frontmatter_parse() {
        let err = FrontmatterParseSnafu {
            path: PathBuf::from("/tmp/foo.md"),
            detail: "missing number field",
        }
        .build();
        let msg = err.to_string();
        assert!(msg.contains("foo.md"));
        assert!(msg.contains("missing number field"));
    }

    #[test]
    fn error_display_dag_cycle() {
        let err = DagCycleSnafu {
            cycle: vec![1u32, 2u32, 3u32],
        }
        .build();
        let msg = err.to_string();
        assert!(msg.contains("cycle"));
        assert!(msg.contains("#1"));
    }

    #[test]
    fn error_display_dag_missing_deps() {
        let err = DagMissingDepsSnafu {
            detail: "prompt 2 -> 99",
        }
        .build();
        assert!(err.to_string().contains("2 -> 99"));
    }

    #[test]
    fn error_is_send_sync() {
        const _: fn() = || {
            fn assert<T: Send + Sync>() {}
            assert::<Error>();
        };
    }
}
