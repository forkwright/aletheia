//! Data retention policy execution.

use serde::{Deserialize, Serialize};

/// Configuration for retention policy execution.
#[derive(Debug, Clone, Default)]
pub struct RetentionConfig {
    pub enabled: bool,
}

/// Trait for components that can execute data retention cleanup.
///
/// Implemented in the aletheia binary where `SessionStore` is available.
/// The daemon crate defines the interface only.
pub trait RetentionExecutor: Send + Sync {
    /// Run retention and return a summary of what was cleaned.
    fn execute_retention(&self) -> crate::error::Result<RetentionSummary>;
}

/// Outcome of a retention execution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RetentionSummary {
    pub sessions_cleaned: u32,
    pub messages_cleaned: u32,
    pub bytes_freed: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockExecutor {
        summary: RetentionSummary,
    }

    impl RetentionExecutor for MockExecutor {
        fn execute_retention(&self) -> crate::error::Result<RetentionSummary> {
            Ok(self.summary.clone())
        }
    }

    struct FailingExecutor;

    impl RetentionExecutor for FailingExecutor {
        fn execute_retention(&self) -> crate::error::Result<RetentionSummary> {
            crate::error::TaskFailedSnafu {
                task_id: "retention",
                reason: "simulated failure",
            }
            .fail()
        }
    }

    #[test]
    fn mock_executor_returns_summary() {
        let executor = MockExecutor {
            summary: RetentionSummary {
                sessions_cleaned: 5,
                messages_cleaned: 100,
                bytes_freed: 1024,
            },
        };

        let result = executor.execute_retention().expect("should succeed");
        assert_eq!(result.sessions_cleaned, 5);
        assert_eq!(result.messages_cleaned, 100);
        assert_eq!(result.bytes_freed, 1024);
    }

    #[test]
    fn failing_executor_returns_error() {
        let executor = FailingExecutor;
        assert!(executor.execute_retention().is_err());
    }

    #[test]
    fn default_config_is_disabled() {
        let config = RetentionConfig::default();
        assert!(!config.enabled);
    }
}
