//! Nous-specific errors.

use snafu::Snafu;

/// Errors from nous operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (source, location, message, file, session_id, etc.) are self-documenting via display format"
)]
pub enum Error {
    /// Session store error.
    #[snafu(display("session store error: {source}"))]
    Store {
        source: aletheia_mneme::error::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// LLM provider error.
    #[snafu(display("LLM error: {source}"))]
    Llm {
        source: aletheia_hermeneus::error::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Context assembly failed.
    #[snafu(display("context assembly failed: {message}"))]
    ContextAssembly {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Context assembly failed reading a required workspace file.
    ///
    /// Preserves the original [`std::io::Error`] source so callers can inspect
    /// the OS-level failure (permission denied, missing file, etc.) without it
    /// being erased into a string message.
    #[snafu(display("context assembly failed: required file '{file}' unreadable: {source}"))]
    ContextAssemblyIo {
        file: String,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Workspace validation failed on actor startup.
    #[snafu(display("workspace validation failed for '{nous_id}': {message}"))]
    WorkspaceValidation {
        nous_id: String,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Pipeline stage failed.
    #[snafu(display("pipeline stage '{stage}' failed: {message}"))]
    PipelineStage {
        stage: String,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Guard rejected the request.
    #[snafu(display("guard rejected: {reason}"))]
    GuardRejected {
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Loop detected in tool execution.
    #[snafu(display("loop detected after {iterations} iterations: {pattern}"))]
    LoopDetected {
        iterations: u32,
        pattern: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Session configuration error.
    #[snafu(display("session config error: {message}"))]
    Config {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Actor inbox send failed (actor shut down).
    #[snafu(display("actor send failed: {message}"))]
    ActorSend {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Actor reply receive failed (actor dropped reply channel).
    #[snafu(display("actor recv failed: {message}"))]
    ActorRecv {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Recall stage embedding failed.
    #[snafu(display("recall embedding failed: {message}"))]
    RecallEmbedding {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Recall stage search failed.
    #[snafu(display("recall search failed: {message}"))]
    RecallSearch {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Target nous not found in the router.
    #[snafu(display("nous not found: {nous_id}"))]
    NousNotFound {
        nous_id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Cross-nous message delivery failed (channel closed).
    #[snafu(display("delivery to '{nous_id}' failed: channel closed"))]
    DeliveryFailed {
        nous_id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Cross-nous ask timed out waiting for reply.
    #[snafu(display("ask to '{nous_id}' timed out after {timeout_secs}s"))]
    AskTimeout {
        nous_id: String,
        timeout_secs: u64,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Reply channel not found (already timed out or consumed).
    #[snafu(display("reply channel not found for message {message_id}"))]
    ReplyNotFound {
        message_id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Cycle detected in ask chain (would deadlock).
    #[snafu(display("ask cycle detected: {chain}"))]
    AskCycleDetected {
        chain: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Distillation failed.
    #[snafu(display("distillation failed: {source}"))]
    Distillation {
        source: aletheia_melete::error::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A mutex or rwlock was poisoned by a prior panic.
    #[snafu(display("mutex poisoned: {what}"))]
    MutexPoisoned {
        what: &'static str,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A pipeline stage exceeded its time budget.
    #[snafu(display("pipeline stage '{stage}' timed out after {timeout_secs}s"))]
    PipelineTimeout {
        stage: String,
        timeout_secs: u32,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Actor inbox is full and the send timed out.
    #[snafu(display("actor '{nous_id}' inbox full after {timeout_secs}s"))]
    InboxFull {
        nous_id: String,
        timeout_secs: u64,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Actor is in degraded state after repeated panics.
    #[snafu(display("actor '{nous_id}' is degraded after {panic_count} panics"))]
    ServiceDegraded {
        nous_id: String,
        panic_count: u32,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Pipeline stage panicked (caught by the panic boundary).
    #[snafu(display("pipeline panic in actor '{nous_id}': {message}"))]
    PipelinePanic {
        nous_id: String,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Chiron audit error.
    #[snafu(display("chiron audit failed: {message}"))]
    ChironAudit {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Convenience alias for results with [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;
    use snafu::IntoError as _;

    #[test]
    fn error_display_context_assembly() {
        let err = ContextAssemblySnafu {
            message: "SOUL.md missing",
        }
        .build();
        let msg = err.to_string();
        assert!(msg.contains("context assembly failed"));
        assert!(msg.contains("SOUL.md missing"));
    }

    #[test]
    fn error_display_context_assembly_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "permission denied");
        let err = ContextAssemblyIoSnafu { file: "SOUL.md" }.into_error(io_err);
        let msg = err.to_string();
        assert!(msg.contains("SOUL.md"));
        assert!(msg.contains("permission denied"));
        assert!(msg.contains("context assembly failed"));
    }

    #[test]
    fn error_display_workspace_validation() {
        let err = WorkspaceValidationSnafu {
            nous_id: "syn",
            message: "directory not found",
        }
        .build();
        let msg = err.to_string();
        assert!(msg.contains("syn"));
        assert!(msg.contains("directory not found"));
    }

    #[test]
    fn error_display_guard_rejected() {
        let err = GuardRejectedSnafu {
            reason: "rate limited",
        }
        .build();
        assert!(err.to_string().contains("rate limited"));
    }

    #[test]
    fn error_display_loop_detected() {
        let err = LoopDetectedSnafu {
            iterations: 5u32,
            pattern: "exec:abc123",
        }
        .build();
        let msg = err.to_string();
        assert!(msg.contains("5 iterations"));
        assert!(msg.contains("exec:abc123"));
    }

    #[test]
    fn error_display_actor_send() {
        let err = ActorSendSnafu {
            message: "actor 'syn' inbox closed",
        }
        .build();
        assert!(err.to_string().contains("inbox closed"));
    }

    #[test]
    fn error_display_actor_recv() {
        let err = ActorRecvSnafu {
            message: "actor 'syn' dropped reply",
        }
        .build();
        assert!(err.to_string().contains("dropped reply"));
    }

    #[test]
    fn error_display_nous_not_found() {
        let err = NousNotFoundSnafu { nous_id: "ghost" }.build();
        assert!(err.to_string().contains("ghost"));
    }

    #[test]
    fn error_display_delivery_failed() {
        let err = DeliveryFailedSnafu { nous_id: "target" }.build();
        assert!(err.to_string().contains("target"));
        assert!(err.to_string().contains("channel closed"));
    }

    #[test]
    fn error_display_ask_timeout() {
        let err = AskTimeoutSnafu {
            nous_id: "target",
            timeout_secs: 30u64,
        }
        .build();
        let msg = err.to_string();
        assert!(msg.contains("target"));
        assert!(msg.contains("30s"));
    }

    #[test]
    fn error_display_reply_not_found() {
        let err = ReplyNotFoundSnafu {
            message_id: "msg-123",
        }
        .build();
        assert!(err.to_string().contains("msg-123"));
    }

    #[test]
    fn error_display_pipeline_stage() {
        let err = PipelineStageSnafu {
            stage: "execute",
            message: "no provider",
        }
        .build();
        let msg = err.to_string();
        assert!(msg.contains("execute"));
        assert!(msg.contains("no provider"));
    }

    #[test]
    fn error_display_config() {
        let err = ConfigSnafu {
            message: "invalid model",
        }
        .build();
        assert!(err.to_string().contains("invalid model"));
    }

    #[test]
    fn error_display_recall_embedding() {
        let err = RecallEmbeddingSnafu {
            message: "embedding service down",
        }
        .build();
        assert!(err.to_string().contains("embedding service down"));
    }

    #[test]
    fn error_display_mutex_poisoned() {
        let err = MutexPoisonedSnafu {
            what: "session store",
        }
        .build();
        assert!(err.to_string().contains("session store"));
    }

    #[test]
    fn error_display_pipeline_timeout() {
        let err = PipelineTimeoutSnafu {
            stage: "execute",
            timeout_secs: 300u32,
        }
        .build();
        let msg = err.to_string();
        assert!(msg.contains("execute"));
        assert!(msg.contains("300s"));
    }

    #[test]
    fn error_display_inbox_full() {
        let err = InboxFullSnafu {
            nous_id: "syn",
            timeout_secs: 30u64,
        }
        .build();
        let msg = err.to_string();
        assert!(msg.contains("syn"));
        assert!(msg.contains("inbox full"));
    }

    #[test]
    fn error_display_service_degraded() {
        let err = ServiceDegradedSnafu {
            nous_id: "syn",
            panic_count: 5u32,
        }
        .build();
        let msg = err.to_string();
        assert!(msg.contains("degraded"));
        assert!(msg.contains("5 panics"));
    }

    #[test]
    fn error_display_ask_cycle_detected() {
        let err = AskCycleDetectedSnafu {
            chain: "a -> b -> a",
        }
        .build();
        let msg = err.to_string();
        assert!(msg.contains("cycle detected"));
        assert!(msg.contains("a -> b -> a"));
    }

    #[test]
    fn error_display_pipeline_panic() {
        let err = PipelinePanicSnafu {
            nous_id: "syn",
            message: "null pointer",
        }
        .build();
        let msg = err.to_string();
        assert!(msg.contains("panic"));
        assert!(msg.contains("null pointer"));
    }

    #[test]
    fn error_is_send_sync() {
        static_assertions::assert_impl_all!(Error: Send, Sync);
    }
}
