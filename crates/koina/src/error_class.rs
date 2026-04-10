//! Error classification for intelligent retry and escalation decisions.
//!
//! Every error type in the Aletheia pipeline can implement [`Classifiable`] to
//! declare whether it is safe to retry, must be escalated to the operator, or
//! should be surfaced to the user.  The pipeline uses these classifications
//! instead of per-crate guesswork, producing consistent retry behaviour across
//! hermeneus, nous, and graphe.
//!
//! # Design
//!
//! Two orthogonal dimensions:
//! - [`ErrorClass`] — *what* the error is (transient / permanent / unknown)
//! - [`ErrorAction`] — *what the caller should do* (retry / escalate / surface / ignore)
//!
//! `ErrorClass` drives the default action but callers may inspect both.  The
//! `Classifiable` trait binds them together on each concrete error type.

/// Classification of an error for retry and escalation decisions.
///
/// Errors are either transient (safe to retry), permanent (do not retry), or
/// unknown (cannot determine — escalate to the operator for visibility).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorClass {
    /// Transient failure — safe to retry with backoff.
    ///
    /// Examples: network timeout, 429 rate limit, provider 5xx, temporary
    /// resource unavailability.
    Transient,

    /// Permanent failure — retrying will not help.
    ///
    /// Examples: invalid input, authentication failure, missing resource,
    /// database corruption, unsupported model.
    Permanent,

    /// Classification cannot be determined — escalate to operator.
    ///
    /// Used when the error source is opaque or the variant is not yet mapped.
    Unknown,
}

/// What the caller should do with this error.
///
/// Returned by [`Classifiable::action`].  The pipeline executes the action;
/// individual error types declare it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorAction {
    /// Retry the operation with exponential backoff.
    ///
    /// `max_attempts` is the total number of attempts (including the first).
    /// `backoff_base_ms` is the initial delay before the first retry.
    Retry {
        /// Total number of attempts (1 = try once, no retries).
        max_attempts: u32,
        /// Base delay in milliseconds for exponential backoff.
        backoff_base_ms: u64,
    },

    /// Escalate to the operator or parent agent.
    ///
    /// The error is serious enough that automated retry would mask it.
    Escalate,

    /// Surface a human-readable message to the user.
    ///
    /// Used for errors the user can act on (e.g. budget exhausted, auth
    /// required).  `user_message` is safe to display directly.
    Surface {
        /// Human-readable message for the end user.
        user_message: String,
    },

    /// Log and discard — no further action required.
    ///
    /// Used for benign, fully-handled failures that do not need operator
    /// visibility (e.g. optional feature not available).
    Ignore,
}

/// A classifiable error: knows its own class and the action the caller should take.
///
/// Implement this on each concrete error type that flows through the pipeline.
/// The pipeline uses `class()` and `action()` to decide retry vs escalate vs
/// surface, replacing per-site `match` arms on individual error variants.
///
/// # Example
///
/// ```
/// use koina::error_class::{Classifiable, ErrorAction, ErrorClass};
///
/// struct MyError;
///
/// impl Classifiable for MyError {
///     fn class(&self) -> ErrorClass {
///         ErrorClass::Transient
///     }
///
///     fn action(&self) -> ErrorAction {
///         ErrorAction::Retry {
///             max_attempts: 3,
///             backoff_base_ms: 500,
///         }
///     }
/// }
/// ```
pub trait Classifiable {
    /// The fundamental nature of this error.
    fn class(&self) -> ErrorClass;

    /// What the caller should do with this error.
    fn action(&self) -> ErrorAction;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AlwaysTransient;

    impl Classifiable for AlwaysTransient {
        fn class(&self) -> ErrorClass {
            ErrorClass::Transient
        }

        fn action(&self) -> ErrorAction {
            ErrorAction::Retry {
                max_attempts: 3,
                backoff_base_ms: 100,
            }
        }
    }

    struct AlwaysPermanent;

    impl Classifiable for AlwaysPermanent {
        fn class(&self) -> ErrorClass {
            ErrorClass::Permanent
        }

        fn action(&self) -> ErrorAction {
            ErrorAction::Escalate
        }
    }

    #[test]
    fn transient_class_and_retry_action() {
        let err = AlwaysTransient;
        assert_eq!(err.class(), ErrorClass::Transient);
        assert_eq!(
            err.action(),
            ErrorAction::Retry {
                max_attempts: 3,
                backoff_base_ms: 100,
            }
        );
    }

    #[test]
    fn permanent_class_and_escalate_action() {
        let err = AlwaysPermanent;
        assert_eq!(err.class(), ErrorClass::Permanent);
        assert_eq!(err.action(), ErrorAction::Escalate);
    }

    #[test]
    fn error_class_variants_are_eq() {
        assert_eq!(ErrorClass::Transient, ErrorClass::Transient);
        assert_ne!(ErrorClass::Transient, ErrorClass::Permanent);
        assert_ne!(ErrorClass::Permanent, ErrorClass::Unknown);
    }

    #[test]
    fn error_action_ignore_variant() {
        assert_eq!(ErrorAction::Ignore, ErrorAction::Ignore);
    }

    #[test]
    fn error_action_surface_variant() {
        let action = ErrorAction::Surface {
            user_message: "please re-authenticate".to_owned(),
        };
        assert_eq!(
            action,
            ErrorAction::Surface {
                user_message: "please re-authenticate".to_owned(),
            }
        );
    }
}
