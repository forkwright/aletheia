//! Steward service: configurable polling loop for CI management.

use std::time::Duration;

use tokio_util::sync::CancellationToken;

use super::types::{CiStatus, StewardResult};

/// Configuration for the steward service polling loop.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct StewardConfig {
    /// Polling interval between steward passes.
    pub interval: Duration,
    /// Whether to run a single pass and exit.
    pub once: bool,
    /// Dry-run mode: classify without executing actions.
    pub dry_run: bool,
    /// GitHub project slug (owner/repo).
    pub project: String,
    /// Required CI check names (empty = all checks matter).
    pub required_checks: Vec<String>,
}

impl StewardConfig {
    #[must_use]
    pub fn new(project: String) -> Self {
        Self {
            interval: Duration::from_secs(300), // 5 minutes default
            once: false,
            dry_run: false,
            project,
            required_checks: Vec::new(),
        }
    }
}

/// Run the steward polling loop.
///
/// Each cycle: classify PRs, make merge decisions, execute actions.
/// Respects the cancellation token for graceful shutdown.
///
/// WHY: Separating the polling loop from the single-pass logic allows
/// both daemon mode (polling) and CLI mode (single pass).
///
/// # Cancel safety
///
/// Cancel-safe at loop boundaries. The `select!` uses `cancel.cancelled()`
/// which is cancel-safe. Dropping the future between iterations simply
/// delays the next poll without losing state.
pub async fn run(config: &StewardConfig, cancel: CancellationToken) -> Vec<StewardResult> {
    let results = Vec::new();

    loop {
        tracing::info!(
            project = %config.project,
            "steward pass starting"
        );

        // NOTE: Single-pass mode exits after one cycle.
        if config.once {
            tracing::info!("single-pass mode, exiting");
            break;
        }

        // NOTE: Sleep respecting cancellation.
        tokio::select! {
            biased;
            () = cancel.cancelled() => {
                tracing::info!("steward cancelled, shutting down");
                break;
            }
            () = tokio::time::sleep(config.interval) => {}
        }
    }

    results
}

/// Run a single steward pass (classify, decide, act).
///
/// This is the unit of work for both polling and single-pass modes.
/// Returns the classification and action results.
///
/// # Cancel safety
///
/// Not cancel-safe. This is a placeholder implementation; the real
/// implementation will perform side effects (fetching PRs, executing
/// merges) that are not idempotent. Do not use in `select!` branches.
pub async fn run_once(config: &StewardConfig) -> StewardResult {
    tracing::info!(
        project = %config.project,
        "steward single pass"
    );

    // NOTE: Placeholder -- real implementation will use a backend trait
    // to fetch PRs, CI status, and execute merges.
    StewardResult {
        classified: Vec::new(),
        merged: Vec::new(),
        needs_fix: Vec::new(),
        blocked: Vec::new(),
        main_ci_status: CiStatus::Unknown,
        main_fix_attempted: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steward_config_new_defaults() {
        let config = StewardConfig::new("acme/repo".to_string());
        assert_eq!(config.interval, Duration::from_secs(300));
        assert!(!config.once);
        assert!(!config.dry_run);
        assert_eq!(config.project, "acme/repo");
        assert!(config.required_checks.is_empty());
    }

    #[tokio::test]
    async fn run_once_returns_empty_result() {
        let config = StewardConfig::new("acme/repo".to_string());
        let result = run_once(&config).await;
        assert!(result.classified.is_empty());
        assert!(result.merged.is_empty());
        assert_eq!(result.main_ci_status, CiStatus::Unknown);
    }

    #[tokio::test]
    async fn run_single_pass_exits_immediately() {
        let config = StewardConfig {
            once: true,
            ..StewardConfig::new("acme/repo".to_string())
        };
        let cancel = CancellationToken::new();
        let results = run(&config, cancel).await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn run_cancellation_exits() {
        let config = StewardConfig {
            interval: Duration::from_secs(3600), // Long interval
            ..StewardConfig::new("acme/repo".to_string())
        };
        let cancel = CancellationToken::new();
        // Cancel immediately.
        cancel.cancel();
        let results = run(&config, cancel).await;
        assert!(results.is_empty());
    }
}
