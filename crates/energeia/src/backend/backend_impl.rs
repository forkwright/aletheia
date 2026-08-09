//! `DispatchBackend` trait implementations.

#[cfg(feature = "storage-fjall")]
use std::future::Future;
#[cfg(feature = "storage-fjall")]
use std::pin::Pin;
#[cfg(feature = "storage-fjall")]
use std::sync::Arc;
#[cfg(feature = "storage-fjall")]
use tokio_util::sync::CancellationToken;

#[cfg(feature = "storage-fjall")]
use crate::backend::DispatchBackend;
#[cfg(feature = "storage-fjall")]
use crate::error::{self, Result};
#[cfg(feature = "storage-fjall")]
use crate::metrics::cost::CostReport;
#[cfg(feature = "storage-fjall")]
use crate::metrics::health::HealthReport;
#[cfg(feature = "storage-fjall")]
use crate::metrics::status::StatusDashboard;
#[cfg(feature = "storage-fjall")]
use crate::prompt::PromptSpec;
#[cfg(feature = "storage-fjall")]
use crate::steward::{StewardBackend, StewardResult};
#[cfg(feature = "storage-fjall")]
use crate::types::{DispatchResult, DispatchSpec};

/// Production [`DispatchBackend`] using energeia's orchestrator and steward.
///
/// Wraps the existing [`Orchestrator`](crate::orchestrator::Orchestrator) for
/// dispatch, steward service for PR management, and fjall-backed metrics
/// for status/health/cost queries.
///
/// WHY(#4718): the steward pass needs external interactions (PR fetch, CI
/// status, merge execution) that this crate deliberately does not implement
/// -- see `steward::backend`'s module doc. `steward_backend` is injected via
/// [`Self::with_steward_backend`] rather than required by [`Self::new`] so
/// existing callers that construct an `EnergeiaBackend` without a steward
/// transport keep compiling; `steward_pass` fails closed with
/// `Error::NotConfigured` rather than silently returning empty results when
/// none was supplied.
#[cfg(feature = "storage-fjall")]
pub struct EnergeiaBackend {
    pub(crate) orchestrator: crate::orchestrator::Orchestrator,
    pub(crate) steward_config: crate::steward::StewardConfig,
    pub(crate) metrics: crate::metrics::MetricsService,
    pub(crate) steward_backend: Option<Arc<dyn StewardBackend>>,
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
            steward_backend: None,
        }
    }

    /// Attach an external cancellation token to the underlying orchestrator.
    ///
    /// Time: O(1). Space: O(1).
    #[must_use]
    pub fn with_cancel_token(mut self, cancel: CancellationToken) -> Self {
        self.orchestrator = self.orchestrator.with_cancel_token(cancel);
        self
    }

    /// Attach a [`StewardBackend`] implementation for PR fetch, CI status,
    /// and merge execution. Without one, [`DispatchBackend::steward_pass`]
    /// fails closed with `Error::NotConfigured`.
    #[must_use]
    pub fn with_steward_backend(mut self, backend: Arc<dyn StewardBackend>) -> Self {
        self.steward_backend = Some(backend);
        self
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
            let Some(backend) = self.steward_backend.as_deref() else {
                return error::NotConfiguredSnafu {
                    what: "steward backend (call EnergeiaBackend::with_steward_backend)",
                }
                .fail();
            };
            let mut config = self.steward_config.clone();
            config.project = project.to_owned();
            config.once = true;
            Ok(crate::steward::run_once_with_backend(&config, backend).await)
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

#[cfg(all(test, feature = "storage-fjall"))]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::future::Future as StdFuture;
    use std::pin::Pin as StdPin;

    use crate::error::Result as EnergeiaResult;
    use crate::http::mock::MockEngine;
    use crate::orchestrator::{Orchestrator, OrchestratorConfig};
    use crate::qa::QaGate;
    use crate::steward::StewardConfig;
    use crate::steward::types::{CheckRun, CiStatus, MergeMethod, PullRequest};
    use crate::types::{MechanicalIssue, QaResult, QaVerdict};

    use super::*;

    struct AlwaysPassQa;

    impl QaGate for AlwaysPassQa {
        fn evaluate<'a>(
            &'a self,
            prompt: &'a crate::qa::PromptSpec,
            pr_number: u64,
            _diff: &'a str,
        ) -> StdPin<Box<dyn StdFuture<Output = EnergeiaResult<QaResult>> + Send + 'a>> {
            Box::pin(async move {
                Ok(QaResult {
                    prompt_number: prompt.prompt_number,
                    pr_number,
                    verdict: QaVerdict::Pass,
                    criteria_results: vec![],
                    mechanical_issues: vec![],
                    reasons: vec![],
                    cost_usd: 0.0,
                    evaluated_at: jiff::Timestamp::now(),
                    semantic_evaluated: false,
                })
            })
        }

        fn mechanical_check(
            &self,
            _diff: &str,
            _prompt: &crate::qa::PromptSpec,
        ) -> Vec<MechanicalIssue> {
            vec![]
        }
    }

    struct EmptyStewardBackend;

    impl StewardBackend for EmptyStewardBackend {
        fn list_open_prs<'a>(
            &'a self,
        ) -> StdPin<Box<dyn StdFuture<Output = EnergeiaResult<Vec<PullRequest>>> + Send + 'a>>
        {
            Box::pin(async move { Ok(Vec::new()) })
        }

        fn check_runs<'a>(
            &'a self,
            _sha: &'a str,
        ) -> StdPin<Box<dyn StdFuture<Output = EnergeiaResult<Vec<CheckRun>>> + Send + 'a>> {
            Box::pin(async move { Ok(Vec::new()) })
        }

        fn changed_files<'a>(
            &'a self,
            _pr_number: u64,
        ) -> StdPin<Box<dyn StdFuture<Output = EnergeiaResult<Vec<String>>> + Send + 'a>> {
            Box::pin(async move { Ok(Vec::new()) })
        }

        fn diff<'a>(
            &'a self,
            _pr_number: u64,
        ) -> StdPin<Box<dyn StdFuture<Output = EnergeiaResult<String>> + Send + 'a>> {
            Box::pin(async move { Ok(String::new()) })
        }

        fn has_gate_trailer<'a>(
            &'a self,
            _pr_number: u64,
        ) -> StdPin<Box<dyn StdFuture<Output = EnergeiaResult<bool>> + Send + 'a>> {
            Box::pin(async move { Ok(false) })
        }

        fn declared_blast_radius<'a>(
            &'a self,
            _pr_number: u64,
        ) -> StdPin<Box<dyn StdFuture<Output = EnergeiaResult<Option<Vec<String>>>> + Send + 'a>>
        {
            Box::pin(async move { Ok(None) })
        }

        fn merge<'a>(
            &'a self,
            _pr_number: u64,
            _method: MergeMethod,
        ) -> StdPin<Box<dyn StdFuture<Output = EnergeiaResult<()>> + Send + 'a>> {
            Box::pin(async move { Ok(()) })
        }

        fn main_branch_ci_status<'a>(
            &'a self,
        ) -> StdPin<Box<dyn StdFuture<Output = EnergeiaResult<CiStatus>> + Send + 'a>> {
            Box::pin(async move { Ok(CiStatus::Green) })
        }
    }

    fn test_backend() -> EnergeiaBackend {
        let engine = Arc::new(MockEngine::new(vec![]));
        let qa = Arc::new(AlwaysPassQa);
        let orchestrator = Orchestrator::new(engine, qa, OrchestratorConfig::default());
        let dir = tempfile::TempDir::new().expect("create temp dir");
        let db = fjall::Database::builder(dir.path())
            .open()
            .expect("open test database");
        let store = Arc::new(crate::store::EnergeiaStore::new(&db).expect("open store"));
        let metrics = crate::metrics::MetricsService::new(store);
        // WHY: leak the TempDir so the database directory survives for the
        // lifetime of the returned backend -- test-only, bounded scope.
        std::mem::forget(dir);
        EnergeiaBackend::new(orchestrator, StewardConfig::new("acme/repo".to_owned()), metrics)
    }

    #[tokio::test]
    async fn steward_pass_fails_closed_without_a_configured_backend() {
        let backend = test_backend();

        let result = backend.steward_pass("acme/repo").await;

        let err = result.expect_err("steward_pass must fail without a steward backend");
        assert!(err.to_string().contains("not configured"));
    }

    #[tokio::test]
    async fn steward_pass_runs_through_the_configured_backend() {
        let backend = test_backend().with_steward_backend(Arc::new(EmptyStewardBackend));

        let result: StewardResult = backend
            .steward_pass("acme/repo")
            .await
            .expect("steward_pass should succeed with a configured backend");

        assert!(result.classified.is_empty());
        assert_eq!(result.main_ci_status, CiStatus::Green);
    }
}
