//! `DispatchBackend` trait implementations.

#[cfg(feature = "storage-fjall")]
use std::future::Future;
#[cfg(feature = "storage-fjall")]
use std::pin::Pin;

#[cfg(feature = "storage-fjall")]
use crate::backend::DispatchBackend;
#[cfg(feature = "storage-fjall")]
use crate::error::Result;
#[cfg(feature = "storage-fjall")]
use crate::metrics::cost::CostReport;
#[cfg(feature = "storage-fjall")]
use crate::metrics::health::HealthReport;
#[cfg(feature = "storage-fjall")]
use crate::metrics::status::StatusDashboard;
#[cfg(feature = "storage-fjall")]
use crate::prompt::PromptSpec;
#[cfg(feature = "storage-fjall")]
use crate::steward::StewardResult;
#[cfg(feature = "storage-fjall")]
use crate::types::{DispatchResult, DispatchSpec};

/// Production [`DispatchBackend`] using energeia's orchestrator and steward.
///
/// Wraps the existing [`Orchestrator`](crate::orchestrator::Orchestrator) for
/// dispatch, steward service for PR management, and fjall-backed metrics
/// for status/health/cost queries.
#[cfg(feature = "storage-fjall")]
pub struct EnergeiaBackend {
    pub(crate) orchestrator: crate::orchestrator::Orchestrator,
    pub(crate) steward_config: crate::steward::StewardConfig,
    pub(crate) metrics: crate::metrics::MetricsService,
}

#[cfg(feature = "storage-fjall")]
impl EnergeiaBackend {
    /// Create a new energeia backend from pre-configured components.
    #[must_use]
    pub fn new(
        orchestrator: crate::orchestrator::Orchestrator,
        steward_config: crate::steward::StewardConfig,
        metrics: crate::metrics::MetricsService,
    ) -> Self {
        Self {
            orchestrator,
            steward_config,
            metrics,
        }
    }
}

#[cfg(feature = "storage-fjall")]
impl DispatchBackend for EnergeiaBackend {
    fn dispatch<'a>(
        &'a self,
        spec: &'a DispatchSpec,
        prompts: &'a [PromptSpec],
    ) -> Pin<Box<dyn Future<Output = Result<DispatchResult>> + Send + 'a>> {
        Box::pin(async move { self.orchestrator.dispatch(spec.clone(), prompts).await })
    }

    fn steward_pass<'a>(
        &'a self,
        project: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<StewardResult>> + Send + 'a>> {
        Box::pin(async move {
            let mut config = self.steward_config.clone();
            config.project = project.to_owned();
            config.once = true;
            Ok(crate::steward::run_once(&config).await)
        })
    }

    fn status<'a>(&'a self) -> Pin<Box<dyn Future<Output = Result<StatusDashboard>> + Send + 'a>> {
        Box::pin(async move { self.metrics.status_dashboard() })
    }

    fn health<'a>(
        &'a self,
        window_days: u32,
    ) -> Pin<Box<dyn Future<Output = Result<HealthReport>> + Send + 'a>> {
        Box::pin(async move { self.metrics.health_report(window_days) })
    }

    fn report<'a>(
        &'a self,
        days: u32,
    ) -> Pin<Box<dyn Future<Output = Result<CostReport>> + Send + 'a>> {
        Box::pin(async move { self.metrics.cost_report(days) })
    }
}
