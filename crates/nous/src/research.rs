//! Parallel research orchestrator: spawns domain researchers via the sub-agent system.

use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinSet;
use tracing::{Instrument, info, warn};

use aletheia_dianoia::research::{
    FindingStatus, ResearchConfig, ResearchDomain, ResearchFinding, ResearchOutput, domain_prompt,
    merge_research,
};
use aletheia_organon::types::SpawnService;

fn domain_sort_key(domain: ResearchDomain) -> u8 {
    match domain {
        ResearchDomain::Stack => 0,
        ResearchDomain::Features => 1,
        ResearchDomain::Architecture => 2,
        ResearchDomain::Pitfalls => 3,
        _ => 4,
    }
}

/// Spawn parallel researchers for each domain and merge results.
///
/// Each researcher runs as an ephemeral sub-agent via [`SpawnService`]. All
/// researchers run concurrently. Partial results are accepted if some researchers
/// fail or timeout.
///
/// # Errors
///
/// Returns `String` only if the spawn service itself is unavailable. Individual
/// researcher failures are captured as [`FindingStatus::Failed`] or
/// [`FindingStatus::TimedOut`] in the output.
pub async fn run_research(
    spawn_service: &Arc<dyn SpawnService>,
    parent_nous_id: &str,
    project_goal: &str,
    config: &ResearchConfig,
) -> Result<ResearchOutput, String> {
    if config.domains.is_empty() {
        return Ok(merge_research(Vec::new()));
    }

    let timeout = Duration::from_secs(config.timeout_secs);
    let mut set = JoinSet::new();

    for domain in &config.domains {
        let prompt = domain_prompt(*domain, project_goal);
        let svc = Arc::clone(spawn_service);
        let parent_id = parent_nous_id.to_owned();
        let domain_copy = *domain;
        let span = tracing::info_span!(
            "research_domain",
            domain = %domain_copy,
            parent = %parent_id,
        );

        set.spawn(
            async move {
                info!("spawning researcher");
                let request = aletheia_organon::types::SpawnRequest {
                    role: "researcher".to_owned(),
                    task: prompt,
                    model: None,
                    allowed_tools: None,
                    timeout_secs: timeout.as_secs(),
                };

                let result = svc.spawn_and_run(request, &parent_id).await;

                match result {
                    Ok(spawn_result) if !spawn_result.is_error => {
                        info!(
                            input_tokens = spawn_result.input_tokens,
                            output_tokens = spawn_result.output_tokens,
                            "researcher completed"
                        );
                        ResearchFinding {
                            domain: domain_copy,
                            content: spawn_result.content,
                            status: FindingStatus::Complete,
                        }
                    }
                    Ok(spawn_result) if spawn_result.content.contains("timed out") => {
                        warn!("researcher timed out");
                        ResearchFinding {
                            domain: domain_copy,
                            content: spawn_result.content,
                            status: FindingStatus::TimedOut,
                        }
                    }
                    Ok(spawn_result) => {
                        warn!(error = %spawn_result.content, "researcher failed");
                        ResearchFinding {
                            domain: domain_copy,
                            content: spawn_result.content,
                            status: FindingStatus::Failed,
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "researcher spawn failed");
                        ResearchFinding {
                            domain: domain_copy,
                            content: e,
                            status: FindingStatus::Failed,
                        }
                    }
                }
            }
            .instrument(span),
        );
    }

    let mut findings = Vec::with_capacity(config.domains.len());
    while let Some(result) = set.join_next().await {
        match result {
            Ok(finding) => findings.push(finding),
            Err(join_err) => {
                warn!(error = %join_err, "researcher task panicked");
            }
        }
    }

    // WHY: sort by domain ordinal so output is deterministic regardless of completion order
    findings.sort_by_key(|f| domain_sort_key(f.domain));

    let succeeded = findings
        .iter()
        .filter(|f| f.status == FindingStatus::Complete || f.status == FindingStatus::Partial)
        .count();
    let total = config.domains.len();
    info!(succeeded, total, "research phase complete");

    Ok(merge_research(findings))
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions on known-length collections"
)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;

    use aletheia_dianoia::research::ResearchLevel;
    use aletheia_organon::types::{SpawnRequest, SpawnResult};

    use super::*;

    struct MockSpawnService {
        response: String,
        is_error: bool,
    }

    impl MockSpawnService {
        fn success(response: &str) -> Self {
            Self {
                response: response.to_owned(),
                is_error: false,
            }
        }

        fn error(message: &str) -> Self {
            Self {
                response: message.to_owned(),
                is_error: true,
            }
        }
    }

    impl SpawnService for MockSpawnService {
        fn spawn_and_run(
            &self,
            _request: SpawnRequest,
            _parent_nous_id: &str,
        ) -> Pin<Box<dyn Future<Output = Result<SpawnResult, String>> + Send + '_>> {
            let result = SpawnResult {
                content: self.response.clone(),
                is_error: self.is_error,
                input_tokens: 100,
                output_tokens: 50,
            };
            Box::pin(async move { Ok(result) })
        }
    }

    struct TimeoutSpawnService;

    impl SpawnService for TimeoutSpawnService {
        fn spawn_and_run(
            &self,
            _request: SpawnRequest,
            _parent_nous_id: &str,
        ) -> Pin<Box<dyn Future<Output = Result<SpawnResult, String>> + Send + '_>> {
            Box::pin(async {
                Ok(SpawnResult {
                    content: "Sub-agent timed out after 5s".to_owned(),
                    is_error: true,
                    input_tokens: 0,
                    output_tokens: 0,
                })
            })
        }
    }

    struct FailingSpawnService;

    impl SpawnService for FailingSpawnService {
        fn spawn_and_run(
            &self,
            _request: SpawnRequest,
            _parent_nous_id: &str,
        ) -> Pin<Box<dyn Future<Output = Result<SpawnResult, String>> + Send + '_>> {
            Box::pin(async { Err("spawn service unavailable".to_owned()) })
        }
    }

    #[tokio::test]
    async fn four_researchers_spawn_concurrently() {
        let svc: Arc<dyn SpawnService> =
            Arc::new(MockSpawnService::success("research findings here"));
        let config = ResearchConfig::default();

        let output = run_research(&svc, "test-parent", "build a chat app", &config)
            .await
            .expect("research should succeed");

        assert_eq!(
            output.findings.len(),
            4,
            "should have one finding per domain"
        );
        for finding in &output.findings {
            assert_eq!(finding.status, FindingStatus::Complete);
        }
        // WHY: first finding keeps the content; later findings have it deduplicated
        assert!(
            output.findings[0]
                .content
                .contains("research findings here")
        );
    }

    #[tokio::test]
    async fn empty_domains_returns_empty_output() {
        let svc: Arc<dyn SpawnService> = Arc::new(MockSpawnService::success("unused"));
        let config = ResearchConfig {
            timeout_secs: 60,
            domains: Vec::new(),
        };

        let output = run_research(&svc, "test-parent", "test", &config)
            .await
            .expect("should succeed");

        assert!(output.findings.is_empty());
    }

    #[tokio::test]
    async fn partial_results_on_timeout() {
        let svc: Arc<dyn SpawnService> = Arc::new(TimeoutSpawnService);
        let config = ResearchConfig::default();

        let output = run_research(&svc, "test-parent", "test", &config)
            .await
            .expect("should succeed with partial results");

        assert_eq!(output.findings.len(), 4);
        for finding in &output.findings {
            assert_eq!(finding.status, FindingStatus::TimedOut);
        }
        assert!(output.markdown.contains("timed out"));
    }

    #[tokio::test]
    async fn failed_spawn_produces_failed_findings() {
        let svc: Arc<dyn SpawnService> = Arc::new(FailingSpawnService);
        let config = ResearchConfig::default();

        let output = run_research(&svc, "test-parent", "test", &config)
            .await
            .expect("should succeed with failed findings");

        for finding in &output.findings {
            assert_eq!(finding.status, FindingStatus::Failed);
        }
    }

    #[tokio::test]
    async fn findings_sorted_by_domain_ordinal() {
        let svc: Arc<dyn SpawnService> = Arc::new(MockSpawnService::success("data"));
        let config = ResearchConfig::default();

        let output = run_research(&svc, "test-parent", "test", &config)
            .await
            .expect("should succeed");

        let domains: Vec<_> = output.findings.iter().map(|f| f.domain).collect();
        assert_eq!(
            domains,
            vec![
                ResearchDomain::Stack,
                ResearchDomain::Features,
                ResearchDomain::Architecture,
                ResearchDomain::Pitfalls,
            ]
        );
    }

    #[tokio::test]
    async fn quick_level_spawns_pitfalls_only() {
        let svc: Arc<dyn SpawnService> = Arc::new(MockSpawnService::success("pitfall data"));
        let config = ResearchLevel::Quick.to_config(60);

        let output = run_research(&svc, "test-parent", "simple task", &config)
            .await
            .expect("should succeed");

        assert_eq!(output.findings.len(), 1);
        assert_eq!(output.findings[0].domain, ResearchDomain::Pitfalls);
    }

    #[tokio::test]
    async fn error_findings_preserve_error_content() {
        let svc: Arc<dyn SpawnService> = Arc::new(MockSpawnService::error("provider unavailable"));
        let config = ResearchLevel::Quick.to_config(30);

        let output = run_research(&svc, "test-parent", "test", &config)
            .await
            .expect("should succeed");

        assert_eq!(output.findings[0].status, FindingStatus::Failed);
        assert!(output.findings[0].content.contains("provider unavailable"));
    }

    #[tokio::test]
    async fn research_output_has_structured_markdown() {
        let svc: Arc<dyn SpawnService> =
            Arc::new(MockSpawnService::success("Domain-specific findings."));
        let config = ResearchConfig::default();

        let output = run_research(&svc, "test-parent", "build an API", &config)
            .await
            .expect("should succeed");

        assert!(output.markdown.contains("# Research Summary"));
        assert!(output.markdown.contains("## Stack"));
        assert!(output.markdown.contains("## Features"));
        assert!(output.markdown.contains("## Architecture"));
        assert!(output.markdown.contains("## Pitfalls"));
    }

    #[tokio::test]
    async fn config_timeout_passed_to_spawn_request() {
        use std::sync::atomic::{AtomicU64, Ordering};

        struct CapturingSpawnService {
            captured_timeout: AtomicU64,
        }

        impl SpawnService for CapturingSpawnService {
            fn spawn_and_run(
                &self,
                request: SpawnRequest,
                _parent_nous_id: &str,
            ) -> Pin<Box<dyn Future<Output = Result<SpawnResult, String>> + Send + '_>>
            {
                self.captured_timeout
                    .store(request.timeout_secs, Ordering::Relaxed);
                Box::pin(async {
                    Ok(SpawnResult {
                        content: "ok".to_owned(),
                        is_error: false,
                        input_tokens: 0,
                        output_tokens: 0,
                    })
                })
            }
        }

        let svc = Arc::new(CapturingSpawnService {
            captured_timeout: AtomicU64::new(0),
        });
        let config = ResearchConfig {
            timeout_secs: 180,
            domains: vec![ResearchDomain::Stack],
        };

        let _ = run_research(
            &(svc.clone() as Arc<dyn SpawnService>),
            "test-parent",
            "test",
            &config,
        )
        .await;

        assert_eq!(svc.captured_timeout.load(Ordering::Relaxed), 180);
    }
}
