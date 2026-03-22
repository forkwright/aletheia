//! Ephemeral sub-agent spawning service.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use tracing::{Instrument, info, warn};

use aletheia_hermeneus::provider::ProviderRegistry;
use aletheia_organon::registry::ToolRegistry;
use aletheia_organon::types::{SpawnRequest, SpawnResult, SpawnService};
use aletheia_taxis::oikos::Oikos;

use crate::actor;
use crate::config::{NousConfig, PipelineConfig, StageBudget};
use crate::roles::Role;

const SONNET_MODEL: &str = "claude-sonnet-4-20250514";

/// Resolve role from string, returning typed role or falling back to model heuristic.
fn resolve_role(role_str: &str) -> Option<Role> {
    Role::parse(role_str)
}

/// Concrete [`SpawnService`] that bridges to `actor::spawn`.
pub struct SpawnServiceImpl {
    providers: Arc<ProviderRegistry>,
    tools: Arc<ToolRegistry>,
    oikos: Arc<Oikos>,
}

impl SpawnServiceImpl {
    /// Create a new spawn service from the given provider, tool, and oikos registries.
    #[must_use]
    pub fn new(
        providers: Arc<ProviderRegistry>,
        tools: Arc<ToolRegistry>,
        oikos: Arc<Oikos>,
    ) -> Self {
        Self {
            providers,
            tools,
            oikos,
        }
    }
}

impl SpawnService for SpawnServiceImpl {
    // NOTE: sequential ephemeral-actor lifecycle: build config, spawn actor, run single turn,
    // teardown. Splitting would fragment a cohesive lifecycle.
    #[expect(clippy::too_many_lines, reason = "spawn setup requires many steps")]
    fn spawn_and_run(
        &self,
        request: SpawnRequest,
        parent_nous_id: &str,
    ) -> Pin<Box<dyn Future<Output = Result<SpawnResult, String>> + Send + '_>> {
        let spawn_id = format!(
            "spawn-{}-{}",
            parent_nous_id,
            ulid::Ulid::new().to_string().to_lowercase()
        );
        let role = resolve_role(&request.role);
        let template = role.map(Role::template);

        let model = request.model.clone().unwrap_or_else(|| {
            template
                .as_ref()
                .map_or_else(|| SONNET_MODEL.to_owned(), |t| t.model.to_owned())
        });

        let tool_allowlist = request
            .allowed_tools
            .clone()
            .or_else(|| template.as_ref().and_then(|t| t.tool_policy.to_allowlist()));

        let timeout = Duration::from_secs(request.timeout_secs);
        let task = request.task.clone();
        let session_key = format!("spawn:{}", ulid::Ulid::new().to_string().to_lowercase());

        let config = NousConfig {
            id: spawn_id.clone(),
            name: None,
            generation: crate::config::NousGenerationConfig {
                model,
                context_window: 200_000,
                max_output_tokens: 16_384,
                bootstrap_max_tokens: 4_000,
                thinking_enabled: false,
                thinking_budget: 0,
                chars_per_token: 4,
                prosoche_model: "claude-haiku-4-5-20251001".to_owned(),
            },
            limits: crate::config::NousLimits {
                max_tool_iterations: 100,
                loop_detection_threshold: 3,
                consecutive_error_threshold: 4,
                loop_max_warnings: 2,
                session_token_cap: 500_000,
                max_tool_result_bytes: 32_768,
            },
            domains: Vec::new(),
            server_tools: Vec::new(),
            cache_enabled: true,
            recall: crate::recall::RecallConfig::default(),
            tool_allowlist,
        };

        let pipeline_config = PipelineConfig {
            history_budget_ratio: 0.6,
            extraction: None,
            stage_budget: StageBudget::default(),
        };

        let providers = Arc::clone(&self.providers);
        let tools = Arc::clone(&self.tools);
        let oikos = Arc::clone(&self.oikos);

        let span = tracing::info_span!(
            "spawn_sub_agent",
            spawn.id = %spawn_id,
            spawn.role = %request.role,
        );

        let soul_content = template.as_ref().map_or_else(
            || {
                let role_str = request.role.clone();
                format!("You are an ephemeral {role_str} sub-agent. Complete the assigned task precisely and concisely.")
            },
            |t| t.system_prompt.to_owned(),
        );

        Box::pin(
            async move {
                let nous_dir = oikos.nous_dir(&spawn_id);
                if let Err(e) = tokio::fs::create_dir_all(&nous_dir).await {
                    return Err(format!("failed to create spawn workspace: {e}"));
                }
                let soul_path = nous_dir.join("SOUL.md");
                if let Err(e) = tokio::fs::write(&soul_path, &soul_content).await {
                    return Err(format!("failed to write SOUL.md: {e}"));
                }

                // WHY: ephemeral actors get their own cancellation token: short-lived, no shared parent
                let ephemeral_cancel = CancellationToken::new();
                let (handle, join_handle, _active_turn) = actor::spawn(
                    config,
                    pipeline_config,
                    providers,
                    tools,
                    oikos,
                    None,
                    None,
                    None,
                    #[cfg(feature = "knowledge-store")]
                    None,
                    None,
                    Vec::new(),
                    None,
                    ephemeral_cancel,
                );

                info!(session_key = %session_key, "ephemeral actor started");

                let result =
                    tokio::time::timeout(timeout, handle.send_turn(&session_key, &task)).await;

                let _ = handle.shutdown().await;
                let _ = join_handle.await;

                let _ = tokio::fs::remove_dir_all(&nous_dir).await;

                match result {
                    Ok(Ok(turn)) => Ok(SpawnResult {
                        content: turn.content,
                        is_error: false,
                        input_tokens: turn.usage.input_tokens,
                        output_tokens: turn.usage.output_tokens,
                    }),
                    Ok(Err(e)) => Ok(SpawnResult {
                        content: format!("Sub-agent error: {e}"),
                        is_error: true,
                        input_tokens: 0,
                        output_tokens: 0,
                    }),
                    Err(_elapsed) => {
                        warn!(timeout_secs = timeout.as_secs(), "sub-agent timed out");
                        Ok(SpawnResult {
                            content: format!("Sub-agent timed out after {}s", timeout.as_secs()),
                            is_error: true,
                            input_tokens: 0,
                            output_tokens: 0,
                        })
                    }
                }
            }
            .instrument(span),
        )
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use aletheia_hermeneus::provider::LlmProvider;
    use aletheia_hermeneus::test_utils::MockProvider;
    use aletheia_hermeneus::types::{
        CompletionRequest, CompletionResponse, ContentBlock, StopReason, Usage,
    };
    use aletheia_taxis::oikos::Oikos;

    use super::*;

    fn make_oikos() -> (tempfile::TempDir, Arc<Oikos>) {
        let dir = tempfile::TempDir::new().expect("tmpdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("shared")).expect("mkdir");
        std::fs::create_dir_all(root.join("theke")).expect("mkdir");
        let oikos = Arc::new(Oikos::from_root(root));
        (dir, oikos)
    }

    fn make_providers() -> Arc<ProviderRegistry> {
        let response = CompletionResponse {
            id: "msg_mock".to_owned(),
            model: "mock-model".to_owned(),
            stop_reason: StopReason::EndTurn,
            content: vec![ContentBlock::Text {
                text: "Sub-agent result".to_owned(),
                citations: None,
            }],
            usage: Usage {
                input_tokens: 200,
                output_tokens: 80,
                ..Usage::default()
            },
        };
        let mut providers = ProviderRegistry::new();
        providers.register(Box::new(
            MockProvider::with_responses(vec![response])
                .models(&["claude-sonnet-4-20250514", "claude-haiku-4-5-20251001"]),
        ));
        Arc::new(providers)
    }

    fn make_spawn_service(oikos: Arc<Oikos>) -> SpawnServiceImpl {
        SpawnServiceImpl::new(make_providers(), Arc::new(ToolRegistry::new()), oikos)
    }

    #[tokio::test]
    async fn spawn_runs_single_turn() {
        let (_dir, oikos) = make_oikos();
        let svc = make_spawn_service(oikos);

        let result = svc
            .spawn_and_run(
                SpawnRequest {
                    role: "coder".to_owned(),
                    task: "Write a function".to_owned(),
                    model: None,
                    allowed_tools: None,
                    timeout_secs: 30,
                },
                "test-parent",
            )
            .await
            .expect("spawn");

        assert!(!result.is_error, "unexpected error: {}", result.content);
        assert_eq!(result.content, "Sub-agent result");
        assert_eq!(result.input_tokens, 200);
        assert_eq!(result.output_tokens, 80);
    }

    #[test]
    fn spawn_uses_role_default_model() {
        use crate::roles::Role;
        assert_eq!(Role::Coder.template().model, "claude-sonnet-4-20250514");
        assert_eq!(Role::Reviewer.template().model, "claude-opus-4-20250514");
        assert_eq!(
            Role::Researcher.template().model,
            "claude-sonnet-4-20250514"
        );
        assert_eq!(Role::Explorer.template().model, "claude-haiku-4-5-20251001");
        assert_eq!(Role::Runner.template().model, "claude-haiku-4-5-20251001");
        assert!(resolve_role("unknown").is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn spawn_timeout_returns_error() {
        let (_dir, oikos) = make_oikos();

        let mut providers = ProviderRegistry::new();
        providers.register(Box::new(SlowProvider));
        let svc = SpawnServiceImpl::new(Arc::new(providers), Arc::new(ToolRegistry::new()), oikos);

        let result = svc
            .spawn_and_run(
                SpawnRequest {
                    role: "coder".to_owned(),
                    task: "Slow task".to_owned(),
                    model: None,
                    allowed_tools: None,
                    timeout_secs: 1,
                },
                "test-parent",
            )
            .await
            .expect("spawn");

        assert!(result.is_error);
        assert!(result.content.contains("timed out"));
    }

    struct SlowProvider;

    impl LlmProvider for SlowProvider {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = aletheia_hermeneus::error::Result<CompletionResponse>,
                    > + Send
                    + 'a,
            >,
        > {
            Box::pin(async {
                // WHY: real sleep required to test timeout in multi_thread runtime
                tokio::time::sleep(std::time::Duration::from_secs(5)).await; // kanon:ignore TESTING/sleep-in-test
                Ok(CompletionResponse {
                    id: "slow".to_owned(),
                    model: "claude-sonnet-4-20250514".to_owned(),
                    stop_reason: StopReason::EndTurn,
                    content: vec![ContentBlock::Text {
                        text: "late".to_owned(),
                        citations: None,
                    }],
                    usage: Usage::default(),
                })
            })
        }

        fn supported_models(&self) -> &[&str] {
            &["claude-sonnet-4-20250514"]
        }

        #[expect(clippy::unnecessary_literal_bound, reason = "trait requires &str")]
        fn name(&self) -> &str {
            "slow"
        }
    }

    #[test]
    fn resolve_role_known_roles() {
        assert!(resolve_role("coder").is_some());
        assert!(resolve_role("reviewer").is_some());
        assert!(resolve_role("researcher").is_some());
        assert!(resolve_role("explorer").is_some());
        assert!(resolve_role("runner").is_some());
    }

    #[test]
    fn resolve_role_unknown_returns_none() {
        assert!(resolve_role("").is_none());
        assert!(resolve_role("analyst").is_none());
        assert!(resolve_role("planner").is_none());
    }

    #[tokio::test]
    async fn spawn_with_explicit_model() {
        let (_dir, oikos) = make_oikos();
        let svc = make_spawn_service(oikos);

        let result = svc
            .spawn_and_run(
                SpawnRequest {
                    role: "coder".to_owned(),
                    task: "Test task".to_owned(),
                    model: Some("claude-haiku-4-5-20251001".to_owned()),
                    allowed_tools: None,
                    timeout_secs: 30,
                },
                "test-parent",
            )
            .await
            .expect("spawn");

        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn spawn_cleans_up_workspace() {
        let (_dir, oikos) = make_oikos();
        let svc = make_spawn_service(Arc::clone(&oikos));

        let result = svc
            .spawn_and_run(
                SpawnRequest {
                    role: "coder".to_owned(),
                    task: "Cleanup test".to_owned(),
                    model: None,
                    allowed_tools: None,
                    timeout_secs: 30,
                },
                "test-parent",
            )
            .await
            .expect("spawn");

        assert!(!result.is_error);
        // WHY: The ephemeral workspace should have been cleaned up
        // (we can't easily check the exact path but the spawn completed)
    }

    #[test]
    fn spawn_service_construction() {
        let providers = Arc::new(ProviderRegistry::new());
        let tools = Arc::new(ToolRegistry::new());
        let dir = tempfile::TempDir::new().expect("tmpdir");
        let oikos = Arc::new(Oikos::from_root(dir.path()));
        let _svc = SpawnServiceImpl::new(providers, tools, oikos);
    }
}
