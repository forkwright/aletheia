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

    /// Self-audit error.
    #[snafu(display("self-audit failed: {message}"))]
    SelfAudit {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Role contract loading failed.
    #[snafu(display("role contract error: {message}"))]
    RoleContract {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Competence store error.
    #[snafu(display("competence store error: {message}: {source}"))]
    CompetenceStore {
        message: String,
        source: rusqlite::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Uncertainty store error.
    #[snafu(display("uncertainty store error: {message}: {source}"))]
    UncertaintyStore {
        message: String,
        source: rusqlite::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Convenience alias for results with [`Error`].
pub type Result<T> = std::result::Result<T, Error>; // kanon:ignore RUST/pub-visibility

impl aletheia_koina::error_class::Classifiable for Error {
    fn class(&self) -> aletheia_koina::error_class::ErrorClass {
        use aletheia_koina::error_class::ErrorClass;
        match self {
            // Delegate to the inner error's own classification where possible.
            Error::Llm { source, .. } => source.class(),

            // Transient: callers should retry after brief backoff
            Error::Store { .. } => ErrorClass::Transient,
            Error::AskTimeout { .. } => ErrorClass::Transient,
            Error::PipelineTimeout { .. } => ErrorClass::Transient,
            Error::InboxFull { .. } => ErrorClass::Transient,
            Error::ActorSend { .. } => ErrorClass::Transient,
            Error::ActorRecv { .. } => ErrorClass::Transient,
            Error::RecallEmbedding { .. } => ErrorClass::Transient,
            Error::RecallSearch { .. } => ErrorClass::Transient,

            // Permanent: these will not succeed on retry
            Error::Config { .. } => ErrorClass::Permanent,
            Error::WorkspaceValidation { .. } => ErrorClass::Permanent,
            Error::ContextAssembly { .. } => ErrorClass::Permanent,
            Error::ContextAssemblyIo { .. } => ErrorClass::Permanent,
            Error::NousNotFound { .. } => ErrorClass::Permanent,
            Error::GuardRejected { .. } => ErrorClass::Permanent,
            Error::LoopDetected { .. } => ErrorClass::Permanent,
            Error::AskCycleDetected { .. } => ErrorClass::Permanent,
            Error::MutexPoisoned { .. } => ErrorClass::Permanent,
            Error::PipelinePanic { .. } => ErrorClass::Permanent,
            Error::CompetenceStore { .. } => ErrorClass::Permanent,
            Error::UncertaintyStore { .. } => ErrorClass::Permanent,

            // Unknown: incomplete information — escalate
            Error::DeliveryFailed { .. } => ErrorClass::Unknown,
            Error::ReplyNotFound { .. } => ErrorClass::Unknown,
            Error::ServiceDegraded { .. } => ErrorClass::Unknown,
            Error::PipelineStage { .. } => ErrorClass::Unknown,
            Error::Distillation { .. } => ErrorClass::Unknown,
            Error::SelfAudit { .. } => ErrorClass::Unknown,
        }
    }

    fn action(&self) -> aletheia_koina::error_class::ErrorAction {
        use aletheia_koina::error_class::{ErrorAction, ErrorClass};
        match self {
            // Delegate LLM errors to hermeneus — it carries retry-after hints.
            Error::Llm { source, .. } => source.action(),

            // Transient pipeline errors: retry with backoff
            Error::Store { .. } => ErrorAction::Retry {
                max_attempts: 3,
                backoff_base_ms: 200,
            },
            Error::AskTimeout { .. } => ErrorAction::Retry {
                max_attempts: 2,
                backoff_base_ms: 500,
            },
            Error::PipelineTimeout { .. } => ErrorAction::Retry {
                max_attempts: 2,
                backoff_base_ms: 1_000,
            },
            Error::InboxFull { .. } => ErrorAction::Retry {
                max_attempts: 3,
                backoff_base_ms: 500,
            },
            Error::ActorSend { .. } | Error::ActorRecv { .. } => ErrorAction::Retry {
                max_attempts: 2,
                backoff_base_ms: 200,
            },
            Error::RecallEmbedding { .. } | Error::RecallSearch { .. } => ErrorAction::Retry {
                max_attempts: 2,
                backoff_base_ms: 300,
            },

            // Permanent errors surfaced to the user
            Error::GuardRejected { reason, .. } => ErrorAction::Surface {
                user_message: format!("Request rejected: {reason}"),
            },
            Error::LoopDetected { pattern, .. } => ErrorAction::Surface {
                user_message: format!(
                    "Loop detected ({pattern}) — please rephrase your request."
                ),
            },
            Error::NousNotFound { nous_id, .. } => ErrorAction::Surface {
                user_message: format!("Agent '{nous_id}' not found."),
            },

            // Everything else: escalate for operator visibility
            _ => {
                // WHY: use class() to keep permanent vs unknown distinction explicit
                // in logs, but escalate both since operator must inspect
                match self.class() {
                    ErrorClass::Transient => {
                        // covered above — but avoid unreachable to stay non_exhaustive safe
                        ErrorAction::Retry {
                            max_attempts: 2,
                            backoff_base_ms: 500,
                        }
                    }
                    _ => ErrorAction::Escalate,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use snafu::IntoError as _;

    use super::*;

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
