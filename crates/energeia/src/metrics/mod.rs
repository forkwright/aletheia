//! Metrics and reporting for energeia dispatch orchestration.
//!
//! # Modules
//!
//! - [`health`] — 7 pipeline health signals (corrective rate, stuck rate, etc.)
//! - [`cost`] — cost and velocity reporting with per-project and per-day breakdown
//! - [`status`] — real-time dashboard: active dispatches, queue depth, recent outcomes
//! - [`prometheus`] — Prometheus metric registration and recording functions
//!
//! # Usage
//!
//! ```rust,ignore
//! use std::sync::Arc;
//! use aletheia_energeia::metrics::MetricsService;
//! use aletheia_energeia::store::EnergeiaStore;
//!
//! let service = MetricsService::new(Arc::clone(&store));
//!
//! let health = service.health_report(30)?;   // last 30 days
//! let cost   = service.cost_report(7)?;      // last 7 days
//! let status = service.status_dashboard()?;  // current snapshot
//!
//! // Initialize prometheus metrics at startup:
//! aletheia_energeia::metrics::prometheus::init();
//! ```

pub mod cost;
pub mod health;
pub mod prometheus;
pub mod status;

#[cfg(feature = "storage-fjall")]
use std::sync::Arc;

#[cfg(feature = "storage-fjall")]
use crate::error::Result;
#[cfg(feature = "storage-fjall")]
use crate::store::EnergeiaStore;

pub use cost::{CostReport, DailyVelocity, ProjectCost};
pub use health::{HealthMetric, HealthReport, HealthStatus};
pub use status::{ProjectSummary, RecentOutcome, StatusDashboard};

// ---------------------------------------------------------------------------
// MetricsService
// ---------------------------------------------------------------------------

/// Entry point for energeia metrics reporting.
///
/// Wraps an `Arc<EnergeiaStore>` and provides read-only access to health
/// metrics, cost reports, and the status dashboard. Constructed once and
/// shared across threads.
///
/// All methods are read-only and non-blocking beyond the underlying fjall
/// prefix scans.
#[cfg(feature = "storage-fjall")]
pub struct MetricsService {
    store: Arc<EnergeiaStore>,
}

#[cfg(feature = "storage-fjall")]
impl MetricsService {
    /// Create a new `MetricsService` backed by the given store.
    #[must_use]
    pub fn new(store: Arc<EnergeiaStore>) -> Self {
        Self { store }
    }

    /// Compute the 7 pipeline health metrics.
    ///
    /// `window_days` controls the historical window. Pass `0` for all data,
    /// `7` for the last week, `30` for the last month.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` if any store read fails.
    pub fn health_report(&self, window_days: u32) -> Result<HealthReport> {
        health::compute_health_report(&self.store, window_days)
    }

    /// Compute cost and velocity metrics.
    ///
    /// `window_days` controls the historical window. Pass `0` for all data.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` if any store read fails.
    pub fn cost_report(&self, window_days: u32) -> Result<CostReport> {
        cost::compute_cost_report(&self.store, window_days)
    }

    /// Build a real-time status dashboard snapshot.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` if any store read fails.
    pub fn status_dashboard(&self) -> Result<StatusDashboard> {
        status::compute_status_dashboard(&self.store)
    }
}

#[cfg(feature = "storage-fjall")]
impl std::fmt::Debug for MetricsService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetricsService").finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[cfg(feature = "storage-fjall")]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (TempDir, MetricsService) {
        let dir = TempDir::new().unwrap();
        let db = fjall::Database::builder(dir.path()).open().unwrap();
        let store = Arc::new(EnergeiaStore::new(&db).unwrap());
        (dir, MetricsService::new(store))
    }

    #[test]
    fn empty_store_health_report() {
        let (_dir, svc) = setup();
        let report = svc.health_report(30).unwrap();
        assert_eq!(report.metrics.len(), 7);
        // All metrics should be Unavailable with no data.
        for m in &report.metrics {
            assert_eq!(
                m.status,
                HealthStatus::Unavailable,
                "{} should be Unavailable with empty store",
                m.name
            );
        }
    }

    #[test]
    fn empty_store_cost_report() {
        let (_dir, svc) = setup();
        let report = svc.cost_report(30).unwrap();
        assert_eq!(report.total_dispatches, 0);
        assert_eq!(report.total_cost_usd, 0.0);
    }

    #[test]
    fn empty_store_status_dashboard() {
        let (_dir, svc) = setup();
        let dashboard = svc.status_dashboard().unwrap();
        assert_eq!(dashboard.active_dispatches, 0);
        assert_eq!(dashboard.queue_depth, 0);
    }

    #[test]
    fn service_is_send_sync() {
        const _: fn() = || {
            fn assert<T: Send + Sync>() {}
            assert::<MetricsService>();
        };
    }

    #[test]
    fn debug_format_does_not_panic() {
        let (_dir, svc) = setup();
        let _ = format!("{svc:?}");
    }
}
