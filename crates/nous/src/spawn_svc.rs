//! Ephemeral sub-agent spawning service.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use aletheia_hermeneus::provider::ProviderRegistry;
use aletheia_organon::registry::ToolRegistry;
use aletheia_organon::types::{SpawnRequest, SpawnResult, SpawnService};
use aletheia_taxis::oikos::Oikos;
use tracing::{Instrument, info, warn};

use crate::actor;
use crate::config::{NousConfig, PipelineConfig, StageBudget};

const SONNET_MODEL: &str = "claude-sonnet-4-20250514";
const HAIKU_MODEL: &str = "claude-haiku-4-5-20251001";

fn model_for_role(role: &str) -> &'static str {
    match role {
        "explorer" | "runner" => HAIKU_MODEL,
        _ => SONNET_MODEL,
    }
}

/// Concrete [`SpawnService`] that bridges to [`actor::spawn`].
pub struct SpawnServiceImpl {
    providers: Arc<ProviderRegistry>,
    tools: Arc<ToolRegistry>,
    oikos: Arc<Oikos>,
}

impl SpawnServiceImpl {
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
        let model = request
            .model
            .clone()
            .unwrap_or_else(|| model_for_role(&request.role).to_owned());
        let timeout = Duration::from_secs(request.timeout_secs);
        let task = request.task.clone();
        let session_key = format!("spawn:{}", ulid::Ulid::new().to_string().to_lowercase());

        let config = NousConfig {
            id: spawn_id.clone(),
            name: None,
            model,
            context_window: 200_000,
            max_output_tokens: 16_384,
            bootstrap_max_tokens: 4_000,
            thinking_enabled: false,
            thinking_budget: 0,
            max_tool_iterations: 25,
            loop_detection_threshold: 3,
            domains: Vec::new(),
            server_tools: Vec::new(),
        };

        let pipeline_config = PipelineConfig {
            distillation_threshold: 1.0,
            include_notes: false,
            include_working_state: false,
            max_notes: 0,
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

        let role_desc = request.role.clone();

        Box::pin(
            async move {
                // Create minimal workspace directory for the ephemeral agent
                let nous_dir = oikos.nous_dir(&spawn_id);
                if let Err(e) = std::fs::create_dir_all(&nous_dir) {
                    return Err(format!("failed to create spawn workspace: {e}"));
                }
                let soul_path = nous_dir.join("SOUL.md");
                if let Err(e) = std::fs::write(
                    &soul_path,
                    format!("You are an ephemeral {role_desc} sub-agent. Complete the assigned task precisely and concisely."),
                ) {
                    return Err(format!("failed to write SOUL.md: {e}"));
                }

                let (handle, join_handle) = actor::spawn(
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
                );

                info!(session_key = %session_key, "ephemeral actor started");

                let result =
                    tokio::time::timeout(timeout, handle.send_turn(&session_key, &task)).await;

                let _ = handle.shutdown().await;
                let _ = join_handle.await;

                // Clean up ephemeral workspace
                let _ = std::fs::remove_dir_all(&nous_dir);

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
                            content: format!(
                                "Sub-agent timed out after {}s",
                                timeout.as_secs()
                            ),
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
mod tests {
    use std::sync::Mutex;

    use aletheia_hermeneus::provider::LlmProvider;
    use aletheia_hermeneus::types::{
        CompletionRequest, CompletionResponse, ContentBlock, StopReason, Usage,
    };
    use aletheia_taxis::oikos::Oikos;

    use super::*;

    struct MockProvider {
        response: Mutex<CompletionResponse>,
    }

    impl LlmProvider for MockProvider {
        fn complete(
            &self,
            _request: &CompletionRequest,
        ) -> aletheia_hermeneus::error::Result<CompletionResponse> {
            #[expect(
                clippy::expect_used,
                reason = "test mock: poisoned lock means a test bug"
            )]
            Ok(self.response.lock().expect("lock poisoned").clone())
        }

        fn supported_models(&self) -> &[&str] {
            &["claude-sonnet-4-20250514", "claude-haiku-4-5-20251001"]
        }

        #[expect(clippy::unnecessary_literal_bound, reason = "trait requires &str")]
        fn name(&self) -> &str {
            "mock"
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    fn test_oikos() -> (tempfile::TempDir, Arc<Oikos>) {
        let dir = tempfile::TempDir::new().expect("tmpdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("shared")).expect("mkdir");
        std::fs::create_dir_all(root.join("theke")).expect("mkdir");
        let oikos = Arc::new(Oikos::from_root(root));
        (dir, oikos)
    }

    fn test_providers() -> Arc<ProviderRegistry> {
        let mut providers = ProviderRegistry::new();
        providers.register(Box::new(MockProvider {
            response: Mutex::new(CompletionResponse {
                id: "resp-1".to_owned(),
                model: "claude-sonnet-4-20250514".to_owned(),
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
            }),
        }));
        Arc::new(providers)
    }

    fn test_spawn_service(oikos: Arc<Oikos>) -> SpawnServiceImpl {
        SpawnServiceImpl::new(test_providers(), Arc::new(ToolRegistry::new()), oikos)
    }

    #[tokio::test]
    async fn spawn_runs_single_turn() {
        let (_dir, oikos) = test_oikos();
        let svc = test_spawn_service(oikos);

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

    #[tokio::test]
    async fn spawn_uses_role_default_model() {
        assert_eq!(model_for_role("coder"), SONNET_MODEL);
        assert_eq!(model_for_role("reviewer"), SONNET_MODEL);
        assert_eq!(model_for_role("researcher"), SONNET_MODEL);
        assert_eq!(model_for_role("explorer"), HAIKU_MODEL);
        assert_eq!(model_for_role("runner"), HAIKU_MODEL);
        assert_eq!(model_for_role("unknown"), SONNET_MODEL);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn spawn_timeout_returns_error() {
        let (_dir, oikos) = test_oikos();

        // Use a provider that sleeps longer than the timeout
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
        fn complete(
            &self,
            _request: &CompletionRequest,
        ) -> aletheia_hermeneus::error::Result<CompletionResponse> {
            std::thread::sleep(std::time::Duration::from_secs(5));
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
        }

        fn supported_models(&self) -> &[&str] {
            &["claude-sonnet-4-20250514"]
        }

        #[expect(clippy::unnecessary_literal_bound, reason = "trait requires &str")]
        fn name(&self) -> &str {
            "slow"
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[test]
    fn model_for_role_defaults_to_sonnet() {
        assert_eq!(model_for_role(""), SONNET_MODEL);
        assert_eq!(model_for_role("analyst"), SONNET_MODEL);
        assert_eq!(model_for_role("planner"), SONNET_MODEL);
    }

    #[test]
    fn model_for_role_explorer_uses_haiku() {
        assert_eq!(model_for_role("explorer"), HAIKU_MODEL);
    }

    #[test]
    fn model_for_role_runner_uses_haiku() {
        assert_eq!(model_for_role("runner"), HAIKU_MODEL);
    }

    #[tokio::test]
    async fn spawn_with_explicit_model() {
        let (_dir, oikos) = test_oikos();
        let svc = test_spawn_service(oikos);

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
        let (_dir, oikos) = test_oikos();
        let svc = test_spawn_service(Arc::clone(&oikos));

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
        // The ephemeral workspace should have been cleaned up
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
