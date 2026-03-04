//! User-facing error types for pipeline failures.

use std::fmt;

/// Presentation errors for end users.
///
/// The full technical error is logged via tracing at the call site.
/// This type carries only what a user should see.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum UserFacingError {
    /// The LLM provider is currently unavailable.
    ProviderUnavailable {
        provider: String,
        suggestion: String,
    },
    /// The conversation exceeded the model's context window.
    ContextOverflow { limit_tokens: u64 },
    /// A tool execution failed.
    ToolExecutionFailed {
        tool_name: String,
        message: String,
    },
    /// The session has expired or is invalid.
    SessionExpired { session_id: String },
    /// Rate limited by the provider.
    RateLimited { retry_after_secs: Option<u64> },
}

impl fmt::Display for UserFacingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProviderUnavailable {
                provider,
                suggestion,
            } => {
                write!(
                    f,
                    "The AI provider ({provider}) is temporarily unavailable. {suggestion}"
                )
            }
            Self::ContextOverflow { limit_tokens } => {
                write!(
                    f,
                    "This conversation has exceeded the maximum context length \
                     ({limit_tokens} tokens). Please start a new session."
                )
            }
            Self::ToolExecutionFailed { tool_name, message } => {
                write!(f, "The tool '{tool_name}' encountered an error: {message}")
            }
            Self::SessionExpired { session_id } => {
                write!(
                    f,
                    "Session '{session_id}' has expired. Please start a new conversation."
                )
            }
            Self::RateLimited {
                retry_after_secs: Some(secs),
            } => {
                write!(
                    f,
                    "The AI provider is currently busy. Please try again in {secs} seconds."
                )
            }
            Self::RateLimited {
                retry_after_secs: None,
            } => {
                write!(
                    f,
                    "The AI provider is currently busy. Please try again in a moment."
                )
            }
        }
    }
}

/// Convert a pipeline error into a user-facing error, if applicable.
///
/// Returns `None` for internal errors that should not be shown to users.
pub fn to_user_facing(error: &crate::error::Error) -> Option<UserFacingError> {
    use aletheia_hermeneus::error::Error as HError;
    use crate::error::Error;

    match error {
        Error::Llm { source, .. } => match source {
            HError::AuthFailed { .. } => Some(UserFacingError::ProviderUnavailable {
                provider: "AI provider".to_owned(),
                suggestion: "This may be a configuration issue. Please check the API key."
                    .to_owned(),
            }),
            HError::RateLimited {
                retry_after_ms, ..
            } => Some(UserFacingError::RateLimited {
                retry_after_secs: Some(retry_after_ms / 1000),
            }),
            HError::ApiError { status, .. } if *status >= 500 => {
                Some(UserFacingError::ProviderUnavailable {
                    provider: "AI provider".to_owned(),
                    suggestion: "Please try again in a moment.".to_owned(),
                })
            }
            HError::ApiRequest { .. } => Some(UserFacingError::ProviderUnavailable {
                provider: "AI provider".to_owned(),
                suggestion: "There may be a network issue. Please try again.".to_owned(),
            }),
            _ => None,
        },
        Error::PipelineStage { message, .. } if message.contains("unavailable") => {
            Some(UserFacingError::ProviderUnavailable {
                provider: "AI provider".to_owned(),
                suggestion: "The provider is recovering from errors. Please try again shortly."
                    .to_owned(),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_provider_unavailable() {
        let e = UserFacingError::ProviderUnavailable {
            provider: "anthropic".to_owned(),
            suggestion: "Try again.".to_owned(),
        };
        let s = e.to_string();
        assert!(s.contains("anthropic"));
        assert!(s.contains("Try again."));
    }

    #[test]
    fn display_context_overflow() {
        let e = UserFacingError::ContextOverflow {
            limit_tokens: 200_000,
        };
        assert!(e.to_string().contains("200000"));
    }

    #[test]
    fn display_tool_execution_failed() {
        let e = UserFacingError::ToolExecutionFailed {
            tool_name: "search".to_owned(),
            message: "timeout".to_owned(),
        };
        let s = e.to_string();
        assert!(s.contains("search"));
        assert!(s.contains("timeout"));
    }

    #[test]
    fn display_session_expired() {
        let e = UserFacingError::SessionExpired {
            session_id: "abc123".to_owned(),
        };
        assert!(e.to_string().contains("abc123"));
    }

    #[test]
    fn display_rate_limited_with_secs() {
        let e = UserFacingError::RateLimited {
            retry_after_secs: Some(30),
        };
        assert!(e.to_string().contains("30 seconds"));
    }

    #[test]
    fn display_rate_limited_without_secs() {
        let e = UserFacingError::RateLimited {
            retry_after_secs: None,
        };
        assert!(e.to_string().contains("in a moment"));
    }

    #[test]
    fn convert_llm_auth_error() {
        let err = crate::error::Error::Llm {
            source: aletheia_hermeneus::error::AuthFailedSnafu {
                message: "bad key",
            }
            .build(),
            location: snafu::Location::new("test", 1, 1),
        };
        let uf = to_user_facing(&err).expect("should convert");
        assert!(matches!(uf, UserFacingError::ProviderUnavailable { .. }));
    }

    #[test]
    fn convert_llm_rate_limit() {
        let err = crate::error::Error::Llm {
            source: aletheia_hermeneus::error::RateLimitedSnafu {
                retry_after_ms: 5000_u64,
            }
            .build(),
            location: snafu::Location::new("test", 1, 1),
        };
        let uf = to_user_facing(&err).expect("should convert");
        match uf {
            UserFacingError::RateLimited {
                retry_after_secs: Some(5),
            } => {}
            other => panic!("expected RateLimited(5), got {other:?}"),
        }
    }

    #[test]
    fn convert_llm_server_error() {
        let err = crate::error::Error::Llm {
            source: aletheia_hermeneus::error::ApiSnafu {
                status: 502_u16,
                message: "bad gateway",
            }
            .build(),
            location: snafu::Location::new("test", 1, 1),
        };
        let uf = to_user_facing(&err).expect("should convert");
        assert!(matches!(uf, UserFacingError::ProviderUnavailable { .. }));
    }

    #[test]
    fn convert_llm_network_error() {
        let err = crate::error::Error::Llm {
            source: aletheia_hermeneus::error::ApiRequestSnafu {
                message: "connection refused",
            }
            .build(),
            location: snafu::Location::new("test", 1, 1),
        };
        let uf = to_user_facing(&err).expect("should convert");
        assert!(matches!(uf, UserFacingError::ProviderUnavailable { .. }));
    }

    #[test]
    fn convert_pipeline_unavailable() {
        let err = crate::error::Error::PipelineStage {
            stage: "execute".to_owned(),
            message: "provider 'anthropic' is currently unavailable".to_owned(),
            location: snafu::Location::new("test", 1, 1),
        };
        let uf = to_user_facing(&err).expect("should convert");
        assert!(matches!(uf, UserFacingError::ProviderUnavailable { .. }));
    }

    #[test]
    fn convert_internal_error_returns_none() {
        let err = crate::error::Error::ActorSend {
            message: "actor shut down".to_owned(),
            location: snafu::Location::new("test", 1, 1),
        };
        assert!(to_user_facing(&err).is_none());
    }

    #[test]
    fn user_facing_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<UserFacingError>();
    }
}
