// WHY: Health-check stage verifies the target LLM backend is reachable before
// spawning dispatch sessions. Running this before execution means unreachable-
// backend failures surface immediately with a clear [health_check] error rather
// than only after sessions time out. The configured HTTP probe performs one
// GET to /v1/models (OpenAI-compatible) with a 5s timeout. When no HTTP
// endpoint is configured, the stage asks the engine for its own lightweight
// readiness probe (for example, the Claude CLI transport runs `claude --version`).

use std::time::Instant;

use snafu::ResultExt as _;

use crate::error::{EngineSnafu, PreflightSnafu};
use crate::pipeline::PipelineStage;
use crate::pipeline::context::PipelineContext;
use crate::pipeline::error::{PipelineError, StageSnafu};

// ---------------------------------------------------------------------------
// HealthCheckStage
// ---------------------------------------------------------------------------

/// Health-check stage: probe the target LLM backend before execution.
///
/// Runs between preparation and execution. If `ctx.health_endpoint` is `None`,
/// the stage asks the engine for a transport-specific readiness probe. Engines
/// that cannot provide a lightweight probe may return `Ok(None)` and the stage
/// then skips gracefully.
///
/// Probe rules:
/// - Timeout: `ctx.health_probe_timeout` (default 5 s).
/// - Transient failure (timeout, connection refused, 5xx): retry once against
///   `ctx.fallback_health_endpoint` if set; otherwise fail with a
///   `[health_check]` `PipelineError`.
/// - Permanent failure (4xx): fail immediately, no retry.
/// - Success: record latency in `ctx.health_probe_latency_ms`.
pub(crate) struct HealthCheckStage;

impl PipelineStage for HealthCheckStage {
    fn name(&self) -> &'static str {
        "health_check"
    }

    async fn run(&self, ctx: &mut PipelineContext) -> Result<(), PipelineError> {
        let t0 = std::time::Instant::now();

        let timeout = ctx.health_probe_timeout;
        let Some(ref endpoint) = ctx.health_endpoint.clone() else {
            match ctx
                .engine
                .probe_health(timeout)
                .await
                .context(StageSnafu { stage: self.name() })?
            {
                Some(latency_ms) => {
                    tracing::info!(latency_ms, "health_check: engine probe succeeded");
                    ctx.health_probe_latency_ms = Some(latency_ms);
                }
                None => {
                    tracing::debug!(
                        "health_check: no HTTP endpoint configured and engine has no probe"
                    );
                }
            }
            ctx.record_stage_latency(self.name(), t0.elapsed());
            return Ok(());
        };

        let client = build_client(timeout).context(StageSnafu { stage: self.name() })?;

        let result = match probe(&client, endpoint).await {
            ProbeOutcome::Ok { latency_ms } => {
                tracing::info!(latency_ms, endpoint, "health_check: backend reachable");
                ctx.health_probe_latency_ms = Some(latency_ms);
                Ok(())
            }
            ProbeOutcome::PermanentFailure { status } => PreflightSnafu {
                reason: format!(
                    "backend health probe returned {status} (permanent failure): {endpoint}"
                ),
            }
            .fail()
            .context(StageSnafu { stage: self.name() }),
            ProbeOutcome::TransientFailure { detail } => {
                tracing::warn!(
                    %detail,
                    endpoint,
                    "health_check: primary probe failed (transient), trying fallback"
                );

                // Try fallback if configured.
                let Some(ref fallback) = ctx.fallback_health_endpoint.clone() else {
                    let err = EngineSnafu {
                        detail: format!(
                            "backend health probe failed (transient) and no fallback configured: {detail}"
                        ),
                    }
                    .fail()
                    .context(StageSnafu { stage: self.name() });
                    ctx.record_stage_latency(self.name(), t0.elapsed());
                    return err;
                };

                match probe(&client, fallback).await {
                    ProbeOutcome::Ok { latency_ms } => {
                        tracing::info!(
                            latency_ms,
                            fallback_endpoint = %fallback,
                            "health_check: fallback backend reachable"
                        );
                        ctx.health_probe_latency_ms = Some(latency_ms);
                        Ok(())
                    }
                    ProbeOutcome::PermanentFailure { status } => {
                        PreflightSnafu {
                            reason: format!(
                                "fallback health probe returned {status} (permanent failure): {fallback}"
                            ),
                        }
                        .fail()
                        .context(StageSnafu { stage: self.name() })
                    }
                    ProbeOutcome::TransientFailure { detail: fb_detail } => {
                        EngineSnafu {
                            detail: format!(
                                "both primary and fallback health probes failed: primary={detail}; fallback={fb_detail}"
                            ),
                        }
                        .fail()
                        .context(StageSnafu { stage: self.name() })
                    }
                }
            }
        };

        ctx.record_stage_latency(self.name(), t0.elapsed());
        result
    }
}

// ---------------------------------------------------------------------------
// Internal probe helpers
// ---------------------------------------------------------------------------

/// Outcome of a single HTTP probe.
enum ProbeOutcome {
    /// Backend responded with 2xx; latency recorded.
    Ok { latency_ms: u64 },
    /// Backend responded with 4xx — do not retry.
    PermanentFailure { status: u16 },
    /// Connection error, timeout, or 5xx — may be transient.
    TransientFailure { detail: String },
}

/// Build a `reqwest::Client` with the given request timeout.
fn build_client(timeout: std::time::Duration) -> crate::error::Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| {
            EngineSnafu {
                detail: format!("failed to build HTTP client for health probe: {e}"),
            }
            .build()
        })
}

/// Send a GET request to `url` and classify the outcome.
async fn probe(client: &reqwest::Client, url: &str) -> ProbeOutcome {
    let t0 = Instant::now();
    match client.get(url).send().await {
        Ok(resp) => {
            let latency_ms = u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX);
            let status = resp.status().as_u16();
            if resp.status().is_success() {
                ProbeOutcome::Ok { latency_ms }
            } else if resp.status().is_client_error() {
                // 4xx: permanent failure (auth, not-found, etc.)
                ProbeOutcome::PermanentFailure { status }
            } else {
                // 5xx or unexpected: treat as transient
                ProbeOutcome::TransientFailure {
                    detail: format!("HTTP {status}"),
                }
            }
        }
        Err(e) => {
            // Connection refused, timeout, DNS failure — all transient.
            ProbeOutcome::TransientFailure {
                detail: e.to_string(),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::engine::{AgentOptions, DispatchEngine, SessionHandle, SessionSpec};
    use crate::error::Result;
    use crate::http::mock::MockEngine;
    use crate::orchestrator::OrchestratorConfig;
    use crate::pipeline::PipelineStage as _;
    use crate::pipeline::context::PipelineContext;
    use crate::prompt::PromptSpec;
    use crate::qa::QaGate;
    use crate::types::{DispatchSpec, MechanicalIssue, QaResult, QaVerdict};

    use super::HealthCheckStage;

    // ---------------------------------------------------------------------------
    // Test helpers
    // ---------------------------------------------------------------------------

    struct AlwaysPassQa;

    impl QaGate for AlwaysPassQa {
        fn evaluate<'a>(
            &'a self,
            prompt: &'a crate::qa::PromptSpec,
            pr_number: u64,
            _diff: &'a str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = crate::error::Result<QaResult>> + Send + 'a>,
        > {
            use jiff::Timestamp;
            Box::pin(async move {
                Ok(QaResult {
                    prompt_number: prompt.prompt_number,
                    pr_number,
                    verdict: QaVerdict::Pass,
                    criteria_results: vec![],
                    mechanical_issues: vec![],
                    reasons: vec![],
                    cost_usd: 0.0,
                    evaluated_at: Timestamp::now(),
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

    fn make_context() -> PipelineContext {
        let engine = Arc::new(MockEngine::new(vec![]));
        let qa = Arc::new(AlwaysPassQa);
        let spec = DispatchSpec::new("acme".to_owned(), vec![1]);
        let prompts = vec![PromptSpec {
            number: 1,
            description: "test".to_owned(),
            depends_on: vec![],
            context_policy: crate::dag::ContextPolicy::Fresh,
            output_format: None,
            worktree: crate::prompt::WorktreePolicy::default(),
            acceptance_criteria: vec![],
            blast_radius: vec![],
            body: "do the thing".to_owned(),

            prompt_components: None,
        }];
        PipelineContext::new(
            spec,
            prompts,
            engine,
            qa,
            OrchestratorConfig::default(),
            #[cfg(feature = "storage-fjall")]
            None,
        )
    }

    struct ProbeOnlyEngine {
        calls: AtomicUsize,
        latency_ms: u64,
    }

    impl ProbeOnlyEngine {
        fn new(latency_ms: u64) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                latency_ms,
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl DispatchEngine for ProbeOnlyEngine {
        fn probe_health<'a>(
            &'a self,
            _timeout: std::time::Duration,
        ) -> Pin<Box<dyn Future<Output = Result<Option<u64>>> + Send + 'a>> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Ok(Some(self.latency_ms))
            })
        }

        fn spawn_session<'a>(
            &'a self,
            _spec: &'a SessionSpec,
            _options: &'a AgentOptions,
        ) -> Pin<Box<dyn Future<Output = Result<Box<dyn SessionHandle>>> + Send + 'a>> {
            Box::pin(async {
                crate::error::EngineSnafu {
                    detail: "test probe engine cannot spawn sessions",
                }
                .fail()
            })
        }

        fn resume_session<'a>(
            &'a self,
            _session_id: &'a str,
            _prompt: &'a str,
            _options: &'a AgentOptions,
        ) -> Pin<Box<dyn Future<Output = Result<Box<dyn SessionHandle>>> + Send + 'a>> {
            Box::pin(async {
                crate::error::EngineSnafu {
                    detail: "test probe engine cannot resume sessions",
                }
                .fail()
            })
        }
    }

    fn make_context_with_engine(engine: Arc<dyn DispatchEngine>) -> PipelineContext {
        let qa = Arc::new(AlwaysPassQa);
        let spec = DispatchSpec::new("acme".to_owned(), vec![1]);
        let prompts = vec![PromptSpec {
            number: 1,
            description: "test".to_owned(),
            depends_on: vec![],
            context_policy: crate::dag::ContextPolicy::Fresh,
            output_format: None,
            worktree: crate::prompt::WorktreePolicy::default(),
            acceptance_criteria: vec![],
            blast_radius: vec![],
            body: "do the thing".to_owned(),

            prompt_components: None,
        }];
        PipelineContext::new(
            spec,
            prompts,
            engine,
            qa,
            OrchestratorConfig::default(),
            #[cfg(feature = "storage-fjall")]
            None,
        )
    }

    // ---------------------------------------------------------------------------
    // Shared TLS provider initialiser
    // ---------------------------------------------------------------------------

    fn init_tls() {
        // WHY: reqwest uses rustls-no-provider; tests must install a crypto
        // provider before the first HTTPS request. Ignoring the error is safe —
        // it means the provider was already installed (e.g. by a parallel test).
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    // ---------------------------------------------------------------------------
    // Happy-path probe succeeds
    // ---------------------------------------------------------------------------

    #[tokio::test]
    async fn probe_success_records_latency() {
        init_tls();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "object": "list",
                "data": [{"id": "qwen3", "object": "model"}]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let mut ctx = make_context();
        ctx.health_endpoint = Some(format!("{}/v1/models", server.uri()));

        HealthCheckStage
            .run(&mut ctx)
            .await
            .expect("probe should succeed");

        assert!(
            ctx.health_probe_latency_ms.is_some(),
            "latency should be recorded on success"
        );
    }

    // ---------------------------------------------------------------------------
    // 5xx retries once, falls over to fallback
    // ---------------------------------------------------------------------------

    #[tokio::test]
    async fn transient_5xx_retries_fallback() {
        init_tls();
        // Primary returns 503 (transient).
        let primary = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(503))
            .expect(1)
            .mount(&primary)
            .await;

        // Fallback returns 200.
        let fallback = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "object": "list",
                "data": []
            })))
            .expect(1)
            .mount(&fallback)
            .await;

        let mut ctx = make_context();
        ctx.health_endpoint = Some(format!("{}/v1/models", primary.uri()));
        ctx.fallback_health_endpoint = Some(format!("{}/v1/models", fallback.uri()));

        HealthCheckStage
            .run(&mut ctx)
            .await
            .expect("fallback probe should succeed");

        assert!(ctx.health_probe_latency_ms.is_some());
    }

    // ---------------------------------------------------------------------------
    // Timeout fails if no fallback configured
    // ---------------------------------------------------------------------------

    #[tokio::test]
    async fn timeout_fails_without_fallback() {
        init_tls();
        // Bind a port that accepts TCP connections but never responds.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        // Keep the listener alive so connect() succeeds and the timeout is what
        // triggers failure, not immediate connection refused.
        let _listener = listener;

        let mut ctx = make_context();
        ctx.health_endpoint = Some(format!("http://{}:{}/v1/models", addr.ip(), addr.port()));
        ctx.health_probe_timeout = std::time::Duration::from_millis(200);

        let err = HealthCheckStage
            .run(&mut ctx)
            .await
            .expect_err("should fail on timeout with no fallback");

        assert_eq!(err.stage(), "health_check");
        assert!(
            err.to_string().contains("health_check"),
            "stage name in error: {err}"
        );
    }

    // ---------------------------------------------------------------------------
    // Timeout succeeds with fallback
    // ---------------------------------------------------------------------------

    #[tokio::test]
    async fn timeout_succeeds_with_fallback() {
        init_tls();
        // Primary: listener that stalls (timeout triggers).
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let _listener = listener;

        // Fallback: real mock server that returns 200.
        let fallback = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "object": "list",
                "data": []
            })))
            .expect(1)
            .mount(&fallback)
            .await;

        let mut ctx = make_context();
        ctx.health_endpoint = Some(format!("http://{}:{}/v1/models", addr.ip(), addr.port()));
        ctx.fallback_health_endpoint = Some(format!("{}/v1/models", fallback.uri()));
        ctx.health_probe_timeout = std::time::Duration::from_millis(200);

        HealthCheckStage
            .run(&mut ctx)
            .await
            .expect("fallback should succeed after primary timeout");

        assert!(ctx.health_probe_latency_ms.is_some());
    }

    // ---------------------------------------------------------------------------
    // 4xx fails immediately (no retry)
    // ---------------------------------------------------------------------------

    #[tokio::test]
    async fn permanent_4xx_fails_immediately() {
        init_tls();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(401))
            // Expect exactly 1 call — no fallback should be attempted.
            .expect(1)
            .mount(&server)
            .await;

        let mut ctx = make_context();
        ctx.health_endpoint = Some(format!("{}/v1/models", server.uri()));

        let err = HealthCheckStage
            .run(&mut ctx)
            .await
            .expect_err("should fail on 401");

        assert_eq!(err.stage(), "health_check");
        assert!(
            err.to_string().contains("health_check"),
            "stage name in error: {err}"
        );
        assert!(
            err.to_string().contains("401"),
            "status code in error: {err}"
        );

        // Verify wiremock received exactly 1 request (no retry).
        server.verify().await;
    }

    // ---------------------------------------------------------------------------
    // Skips gracefully when no endpoint configured
    // ---------------------------------------------------------------------------

    #[tokio::test]
    async fn skips_when_no_endpoint_configured() {
        let mut ctx = make_context();
        // health_endpoint is None by default — stage should pass without touching network.
        HealthCheckStage
            .run(&mut ctx)
            .await
            .expect("stage should pass when no endpoint is configured");

        assert!(
            ctx.health_probe_latency_ms.is_none(),
            "no latency should be recorded when probe was skipped"
        );
    }

    #[tokio::test]
    async fn no_endpoint_uses_engine_health_probe_when_available() {
        let engine = Arc::new(ProbeOnlyEngine::new(17));
        let mut ctx = make_context_with_engine(engine.clone());

        HealthCheckStage
            .run(&mut ctx)
            .await
            .expect("engine probe should succeed");

        assert_eq!(ctx.health_probe_latency_ms, Some(17));
        assert_eq!(engine.calls(), 1);
    }
}
