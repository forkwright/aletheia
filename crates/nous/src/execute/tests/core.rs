// kanon:ignore RUST/file-too-long — core execute test suite; kept single-file for shared test helpers
#![expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len"
)]
//! Core execute loop tests.
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use hermeneus::error as llm_error;
use hermeneus::provider::{DeploymentTarget, LlmProvider};

use crate::hooks::registry::HookRegistry;
use crate::hooks::{AfterToolContext, HookResult, TurnHook};

use super::*;

struct FallbackSequenceProvider {
    responses: Mutex<Vec<hermeneus::error::Result<CompletionResponse>>>,
    models: Mutex<Vec<String>>,
    supported_models: &'static [&'static str],
    provider_name: &'static str,
}

struct ArcProvider(Arc<FallbackSequenceProvider>);

struct ArcMockProvider(Arc<MockProvider>);

struct DeploymentTargetProvider {
    inner: MockProvider,
    target: DeploymentTarget,
}

impl FallbackSequenceProvider {
    fn new(
        provider_name: &'static str,
        supported_models: &'static [&'static str],
        responses: Vec<hermeneus::error::Result<CompletionResponse>>,
    ) -> Self {
        Self {
            responses: Mutex::new(responses),
            models: Mutex::new(Vec::new()),
            supported_models,
            provider_name,
        }
    }

    fn called_models(&self) -> Vec<String> {
        self.models.lock().expect("models lock").clone()
    }
}

impl DeploymentTargetProvider {
    fn new(inner: MockProvider, target: DeploymentTarget) -> Self {
        Self { inner, target }
    }
}

impl LlmProvider for FallbackSequenceProvider {
    fn complete<'a>(
        &'a self,
        request: &'a hermeneus::types::CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = hermeneus::error::Result<CompletionResponse>> + Send + 'a>>
    {
        self.models
            .lock()
            .expect("models lock")
            .push(request.model.clone());
        let result = self.responses.lock().expect("responses lock").remove(0);
        Box::pin(async move { result })
    }

    fn supported_models(&self) -> &[&str] {
        self.supported_models
    }

    fn name(&self) -> &str {
        self.provider_name
    }
}

impl LlmProvider for DeploymentTargetProvider {
    fn complete<'a>(
        &'a self,
        request: &'a hermeneus::types::CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = hermeneus::error::Result<CompletionResponse>> + Send + 'a>>
    {
        self.inner.complete(request)
    }

    fn supported_models(&self) -> &[&str] {
        self.inner.supported_models()
    }

    fn name(&self) -> &str {
        self.inner.name()
    }

    fn deployment_target(&self) -> DeploymentTarget {
        self.target
    }
}

impl LlmProvider for ArcProvider {
    fn complete<'a>(
        &'a self,
        request: &'a hermeneus::types::CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = hermeneus::error::Result<CompletionResponse>> + Send + 'a>>
    {
        self.0.complete(request)
    }

    fn supported_models(&self) -> &[&str] {
        self.0.supported_models()
    }

    fn name(&self) -> &str {
        self.0.name()
    }
}

impl LlmProvider for ArcMockProvider {
    fn complete<'a>(
        &'a self,
        request: &'a hermeneus::types::CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = hermeneus::error::Result<CompletionResponse>> + Send + 'a>>
    {
        self.0.complete(request)
    }

    fn supported_models(&self) -> &[&str] {
        self.0.supported_models()
    }

    fn name(&self) -> &str {
        self.0.name()
    }

    fn capabilities(&self) -> hermeneus::provider::ProviderCapabilities {
        // WHY(#5253): forwarded explicitly — the trait default (every
        // capability) would otherwise silently override `MockProvider`'s
        // `.without_tool_loop()` for every test that wraps it in `Arc` to
        // share a handle for post-call assertions.
        self.0.capabilities()
    }
}

fn make_multi_tool_response(tool_uses: Vec<(&str, &str, serde_json::Value)>) -> CompletionResponse {
    CompletionResponse {
        id: "resp-tools".to_owned(),
        model: "test-model".to_owned(),
        stop_reason: StopReason::ToolUse,
        content: tool_uses
            .into_iter()
            .map(|(name, id, input)| ContentBlock::ToolUse {
                id: id.to_owned(),
                name: name.to_owned(),
                input,
            })
            .collect(),
        usage: Usage {
            input_tokens: 80,
            output_tokens: 30,
            ..Usage::default()
        },
        cost_usd: None,
        duration_ms: None,
    }
}

fn make_exec_and_read_registry() -> ToolRegistry {
    let mut tools = ToolRegistry::new();
    tools
        .register(make_tool_def("exec"), Box::new(EchoExecutor))
        .expect("register exec");
    tools
        .register(make_tool_def("read_file"), Box::new(EchoExecutor))
        .expect("register read_file");
    tools
}

fn tool_result_ids_from_second_request(mock: &MockProvider) -> Vec<String> {
    let requests = mock.captured_requests();
    assert_eq!(requests.len(), 2, "tool loop should make two LLM requests");
    let second = requests.get(1).expect("second request should exist");
    let last_message = second.messages.last().expect("second request has messages");
    let hermeneus::types::Content::Blocks(blocks) = &last_message.content else {
        panic!("second request should end with tool result blocks");
    };
    blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::ToolResult { tool_use_id, .. } => Some(tool_use_id.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn simple_text_response() {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::with_responses(vec![make_text_response("Hello there!")])
            .models(&["test-model"]),
    ));

    let tools = ToolRegistry::new();
    let result = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &test_config(),
        &providers,
        &tools,
        &test_tool_ctx(),
        None,
    )
    .await
    .expect("execute");

    assert_eq!(
        result.content, "Hello there!",
        "response content should match mock text"
    );
    assert!(
        result.tool_calls.is_empty(),
        "text-only response should produce no tool calls"
    );
    assert_eq!(
        result.usage.llm_calls, 1,
        "single text response should use exactly one LLM call"
    );
    assert_eq!(
        result.usage.input_tokens, 100,
        "input token count should match mock response"
    );
    assert_eq!(
        result.usage.output_tokens, 50,
        "output token count should match mock response"
    );
    assert_eq!(
        result.stop_reason, "end_turn",
        "response should stop with end_turn reason"
    );
    assert!(
        result.signals.contains(&InteractionSignal::Conversation),
        "text-only response should produce Conversation signal"
    );
}

#[tokio::test]
async fn primary_success_records_observed_model_used() {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::with_responses(vec![make_text_response_for_model(
            "primary answer",
            "primary-model",
        )])
        .models(&["primary-model"]),
    ));

    let mut config = test_config();
    config.generation.model = "primary-model".to_owned();
    let session = SessionState::new("test-session".to_owned(), "main".to_owned(), &config);

    let result = execute(
        &test_pipeline_ctx(),
        &session,
        &config,
        &providers,
        &ToolRegistry::new(),
        &test_tool_ctx(),
        None,
    )
    .await
    .expect("execute");

    assert_eq!(result.content, "primary answer");
    assert_eq!(
        result.model_used, "primary-model",
        "primary success should report the observed response model"
    );
}

#[tokio::test]
async fn explicit_provider_route_wins_when_multiple_providers_claim_model() {
    let anthropic = Arc::new(
        MockProvider::with_responses(vec![make_text_response_for_model(
            "cloud answer",
            "shared-model",
        )])
        .named("anthropic-cloud")
        .models(&["shared-model"]),
    );
    let local = Arc::new(
        MockProvider::with_responses(vec![make_text_response_for_model(
            "local answer",
            "shared-model",
        )])
        .named("local-proxy")
        .models(&["shared-model"]),
    );
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(ArcMockProvider(Arc::clone(&anthropic))));
    providers.register(Box::new(ArcMockProvider(Arc::clone(&local))));

    let mut config = test_config();
    config.generation.model = "shared-model".to_owned();
    config.generation.provider = Some("local-proxy".to_owned());
    let session = SessionState::new("test-session".to_owned(), "main".to_owned(), &config);

    let result = execute(
        &test_pipeline_ctx(),
        &session,
        &config,
        &providers,
        &ToolRegistry::new(),
        &test_tool_ctx(),
        None,
    )
    .await
    .expect("execute with explicit provider route");

    assert_eq!(result.content, "local answer");
    assert_eq!(result.model_used, "shared-model");
    assert_eq!(result.provider_used.as_deref(), Some("local-proxy"));
    assert!(
        anthropic.captured_requests().is_empty(),
        "registration-order provider must not receive explicitly routed request"
    );
    assert_eq!(
        local.captured_requests().len(),
        1,
        "explicit provider should receive the request"
    );
}

#[tokio::test]
async fn configured_fallback_models_are_used_for_retryable_primary_failure() {
    let primary = Arc::new(FallbackSequenceProvider::new(
        "primary",
        &["test-model"],
        vec![Err(llm_error::RateLimitedSnafu {
            retry_after_ms: 100_u64,
        }
        .build())],
    ));
    let secondary = Arc::new(FallbackSequenceProvider::new(
        "secondary",
        &["fallback-model"],
        vec![Ok(make_text_response_for_model(
            "fallback answer",
            "fallback-model",
        ))],
    ));
    let tertiary = Arc::new(FallbackSequenceProvider::new(
        "tertiary",
        &["unused-fallback"],
        vec![Ok(make_text_response("unused"))],
    ));
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(ArcProvider(Arc::clone(&primary))));
    providers.register(Box::new(ArcProvider(Arc::clone(&secondary))));
    providers.register(Box::new(ArcProvider(Arc::clone(&tertiary))));

    let mut config = test_config();
    config.generation.fallback_models =
        vec!["fallback-model".to_owned(), "unused-fallback".to_owned()];
    config.generation.retries_before_fallback = 1;

    let result = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &config,
        &providers,
        &ToolRegistry::new(),
        &test_tool_ctx(),
        None,
    )
    .await
    .expect("execute");

    assert_eq!(result.content, "fallback answer");
    assert_eq!(
        result.model_used, "fallback-model",
        "fallback success should report the model that served the turn"
    );
    assert_eq!(
        result.usage.llm_calls, 2,
        "usage must count both the failed primary attempt and the successful \
         fallback attempt (#5372), not just the final success"
    );
    assert_eq!(primary.called_models(), ["test-model"]);
    assert_eq!(secondary.called_models(), ["fallback-model"]);
    assert!(
        tertiary.called_models().is_empty(),
        "fallback chain should stop after first success"
    );
}

#[tokio::test]
async fn configured_fallback_used_when_primary_provider_marked_down() {
    // WHY(#5260): when the primary provider is already marked Down in the
    // registry, the execute stage must still use a configured fallback model
    // instead of treating the resulting ApiRequest as a permanent failure.
    let primary = Arc::new(FallbackSequenceProvider::new(
        "primary",
        &["test-model"],
        Vec::new(),
    ));
    let secondary = Arc::new(FallbackSequenceProvider::new(
        "secondary",
        &["fallback-model"],
        vec![Ok(make_text_response("fallback answer"))],
    ));
    let mut providers = ProviderRegistry::new();
    providers.register_with_config(
        Box::new(ArcProvider(Arc::clone(&primary))),
        HealthConfig {
            consecutive_failure_threshold: 1,
            ..HealthConfig::default()
        },
    );
    providers.register(Box::new(ArcProvider(Arc::clone(&secondary))));

    let err = llm_error::ApiRequestSnafu {
        message: "forced transient error".to_owned(),
    }
    .build();
    providers.record_error("primary", &err);
    providers.record_error("primary", &err);
    assert!(
        matches!(
            providers.provider_health("primary"),
            Some(ProviderHealth::Down { .. })
        ),
        "primary provider should be Down"
    );

    let mut config = test_config();
    config.generation.fallback_models = vec!["fallback-model".to_owned()];
    config.generation.retries_before_fallback = 1;

    let result = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &config,
        &providers,
        &ToolRegistry::new(),
        &test_tool_ctx(),
        None,
    )
    .await
    .expect("execute should fall back when primary provider is Down");

    assert_eq!(result.content, "fallback answer");
    assert!(
        primary.called_models().is_empty(),
        "primary provider should not be called when already Down"
    );
    assert_eq!(secondary.called_models(), ["fallback-model"]);
}

#[tokio::test]
async fn configured_fallback_reports_aggregate_when_all_models_fail() {
    let primary = Arc::new(FallbackSequenceProvider::new(
        "primary",
        &["test-model"],
        vec![Err(llm_error::RateLimitedSnafu {
            retry_after_ms: 100_u64,
        }
        .build())],
    ));
    let secondary = Arc::new(FallbackSequenceProvider::new(
        "secondary",
        &["fallback-model"],
        vec![Err(llm_error::ApiSnafu {
            status: 503_u16,
            message: "fallback unavailable".to_owned(),
            context: llm_error::ApiErrorContext::empty(),
        }
        .build())],
    ));
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(ArcProvider(Arc::clone(&primary))));
    providers.register(Box::new(ArcProvider(Arc::clone(&secondary))));

    let mut config = test_config();
    config.generation.fallback_models = vec!["fallback-model".to_owned()];
    config.generation.retries_before_fallback = 1;

    let err = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &config,
        &providers,
        &ToolRegistry::new(),
        &test_tool_ctx(),
        None,
    )
    .await
    .expect_err("all fallback models should fail");

    let msg = err.to_string();
    assert!(
        msg.contains("all model routes in fallback chain failed")
            && msg.contains("test-model")
            && msg.contains("fallback-model"),
        "error should aggregate failed models, got: {msg}"
    );
}

#[tokio::test]
async fn single_provider_config_does_not_attempt_fallback() {
    let provider = Arc::new(FallbackSequenceProvider::new(
        "primary",
        &["test-model"],
        vec![Err(llm_error::RateLimitedSnafu {
            retry_after_ms: 100_u64,
        }
        .build())],
    ));
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(ArcProvider(Arc::clone(&provider))));

    let err = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &test_config(),
        &providers,
        &ToolRegistry::new(),
        &test_tool_ctx(),
        None,
    )
    .await
    .expect_err("primary failure should not try fallback without config");

    assert!(err.to_string().contains("rate limited"));
    assert_eq!(
        provider.called_models(),
        ["test-model"],
        "single-provider config should attempt only the primary model"
    );
}

// WHY(#5253, #5254): capability-aware routing/fallback tests. `MockProvider`
// stands in for a seat-bridged CLI provider (cc/codex/kimi) via
// `.without_tool_loop()`, so these exercise the negotiation mechanism itself
// rather than needing a real subprocess provider under a feature flag.

#[tokio::test]
async fn tool_bearing_turn_never_dispatches_to_incapable_provider_without_fallback() {
    let primary = Arc::new(
        MockProvider::new("should never be seen")
            .named("primary")
            .models(&["test-model"])
            .without_tool_loop(),
    );
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(ArcMockProvider(Arc::clone(&primary))));

    let err = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &test_config(),
        &providers,
        &make_exec_and_read_registry(),
        &test_tool_ctx(),
        None,
    )
    .await
    .expect_err("a tool-bearing turn must not route to a capability-incapable provider");

    let msg = err.to_string();
    assert!(
        msg.contains("capability") || msg.contains("tool-loop") || msg.contains("tool loop"),
        "error should name the capability mismatch, got: {msg}"
    );
    assert!(
        primary.captured_requests().is_empty(),
        "negotiation must reject the turn before the provider is ever dispatched to"
    );
}

#[tokio::test]
async fn tool_bearing_fallback_chain_skips_incapable_route_for_capable_one() {
    let primary = Arc::new(
        MockProvider::new("must never be dispatched to")
            .named("primary")
            .models(&["test-model"])
            .without_tool_loop(),
    );
    let fallback = Arc::new(
        MockProvider::new("fallback answer")
            .named("secondary")
            .models(&["fallback-model"]),
    );
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(ArcMockProvider(Arc::clone(&primary))));
    providers.register(Box::new(ArcMockProvider(Arc::clone(&fallback))));

    let mut config = test_config();
    config.generation.fallback_models = vec!["fallback-model".to_owned()];
    config.generation.retries_before_fallback = 3;

    let result = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &config,
        &providers,
        &make_exec_and_read_registry(),
        &test_tool_ctx(),
        None,
    )
    .await
    .expect("fallback chain should skip the incapable primary and use the capable fallback");

    assert_eq!(result.content, "fallback answer");
    assert!(
        primary.captured_requests().is_empty(),
        "an incapable route must never be dispatched to, not even once"
    );
    assert_eq!(fallback.captured_requests().len(), 1);
}

#[tokio::test]
async fn tool_bearing_fallback_chain_reports_capability_mismatch_when_every_route_incapable() {
    let primary = Arc::new(
        MockProvider::new("must never be dispatched to")
            .named("primary")
            .models(&["test-model"])
            .without_tool_loop(),
    );
    let fallback = Arc::new(
        MockProvider::new("must never be dispatched to either")
            .named("secondary")
            .models(&["fallback-model"])
            .without_tool_loop(),
    );
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(ArcMockProvider(Arc::clone(&primary))));
    providers.register(Box::new(ArcMockProvider(Arc::clone(&fallback))));

    let mut config = test_config();
    config.generation.fallback_models = vec!["fallback-model".to_owned()];
    config.generation.retries_before_fallback = 2;

    let err = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &config,
        &providers,
        &make_exec_and_read_registry(),
        &test_tool_ctx(),
        None,
    )
    .await
    .expect_err("a chain with no capable route must fail rather than dispatch anywhere");

    let msg = err.to_string();
    assert!(
        msg.contains("capability") || msg.contains("tool-loop") || msg.contains("tool loop"),
        "error should name the capability mismatch rather than reading as a generic \
         connection failure, got: {msg}"
    );
    assert!(primary.captured_requests().is_empty());
    assert!(fallback.captured_requests().is_empty());
}

#[tokio::test]
async fn non_tool_turn_still_routes_normally_to_a_capability_incapable_provider() {
    // WHY(#5253): the capability check must be a no-op for a turn that never
    // needed the missing capability — a seat-bridged-shaped provider serves
    // a tool-free turn exactly as it did before this mechanism existed.
    let primary = Arc::new(
        MockProvider::new("plain answer")
            .named("primary")
            .models(&["test-model"])
            .without_tool_loop(),
    );
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(ArcMockProvider(Arc::clone(&primary))));

    let result = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &test_config(),
        &providers,
        &ToolRegistry::new(),
        &test_tool_ctx(),
        None,
    )
    .await
    .expect("a non-tool-bearing turn must still route to a capability-incapable provider");

    assert_eq!(result.content, "plain answer");
    assert_eq!(primary.captured_requests().len(), 1);
}

#[tokio::test]
async fn cache_turns_disabled_without_a_cache_breakpoint() {
    // WHY(#3781, #5224): cache_turns must stay false when cache_enabled is
    // true but no message in the turn carries a cache breakpoint — enabling
    // it unconditionally would mark uncacheable turns for prompt-cache
    // pricing that never benefits from a hit.
    let mock = Arc::new(
        MockProvider::with_responses(vec![make_text_response("hi")]).models(&["test-model"]),
    );
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(ArcMockProvider(Arc::clone(&mock))));

    let ctx = test_pipeline_ctx();
    assert!(
        !ctx.messages.iter().any(|m| m.cache_breakpoint),
        "test fixture must not carry a cache breakpoint"
    );

    execute(
        &ctx,
        &test_session(),
        &test_config(),
        &providers,
        &ToolRegistry::new(),
        &test_tool_ctx(),
        None,
    )
    .await
    .expect("execute");

    let requests = mock.captured_requests();
    assert_eq!(requests.len(), 1);
    assert!(
        !requests.first().expect("one request").cache_turns,
        "cache_turns must be false with no cache-breakpoint message, even though cache_enabled=true"
    );
}

#[tokio::test]
async fn cache_turns_enabled_with_a_cache_breakpoint() {
    // WHY(#3781, #5224): the mirror of the test above — once a message
    // carries a cache breakpoint, cache_turns must turn on so the cached
    // prefix is actually read at cache pricing on the next turn.
    let mock = Arc::new(
        MockProvider::with_responses(vec![make_text_response("hi")]).models(&["test-model"]),
    );
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(ArcMockProvider(Arc::clone(&mock))));

    let mut ctx = test_pipeline_ctx();
    ctx.messages
        .push(PipelineMessage::text("user", "summary", 1).with_cache_breakpoint(true));

    execute(
        &ctx,
        &test_session(),
        &test_config(),
        &providers,
        &ToolRegistry::new(),
        &test_tool_ctx(),
        None,
    )
    .await
    .expect("execute");

    let requests = mock.captured_requests();
    assert_eq!(requests.len(), 1);
    assert!(
        requests.first().expect("one request").cache_turns,
        "cache_turns must be true once a message carries a cache breakpoint"
    );
}

#[tokio::test]
async fn single_tool_iteration() {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::with_responses(vec![
            make_tool_response("exec", "toolu_1", serde_json::json!({"input": "test"})),
            make_text_response("Done!"),
        ])
        .models(&["test-model"]),
    ));

    let tools = make_registry_with("exec", Box::new(EchoExecutor));

    let result = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &test_config(),
        &providers,
        &tools,
        &test_tool_ctx(),
        None,
    )
    .await
    .expect("execute");

    assert_eq!(
        result.content, "Done!",
        "final response content should match mock text"
    );
    assert_eq!(
        result.tool_calls.len(),
        1,
        "should have recorded exactly one tool call"
    );
    assert_eq!(
        result.tool_calls[0].name, "exec",
        "tool call name should match registered tool"
    );
    let result_text = result.tool_calls[0].result.as_deref().unwrap_or("");
    assert!(
        result_text.starts_with("executed: exec"),
        "tool result should start with echo executor output: {result_text}"
    );
    assert!(
        result_text.contains("[receipt:"),
        "tool result should contain receipt: {result_text}"
    );
    assert!(
        !result.tool_calls[0].is_error,
        "tool call should not be marked as an error"
    );
    assert_eq!(
        result.usage.llm_calls, 2,
        "one tool iteration requires two LLM calls"
    );
    assert_eq!(
        result.stop_reason, "end_turn",
        "final response should stop with end_turn reason"
    );
}

#[tokio::test]
async fn unadvertised_lazy_tool_is_denied_before_execution() {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::with_responses(vec![
            make_tool_response("lazy_exec", "toolu_1", serde_json::json!({"input": "test"})),
            make_text_response("Done!"),
        ])
        .models(&["test-model"]),
    ));

    let executions = Arc::new(AtomicUsize::new(0));
    let mut def = make_tool_def("lazy_exec");
    def.auto_activate = false;
    let mut tools = ToolRegistry::new();
    tools
        .register(
            def,
            Box::new(CountingExecutor::new(Arc::clone(&executions))),
        )
        .expect("register");

    let result = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &test_config(),
        &providers,
        &tools,
        &test_tool_ctx(),
        None,
    )
    .await
    .expect("execute");

    assert_eq!(result.content, "Done!");
    assert_eq!(
        executions.load(Ordering::SeqCst),
        0,
        "unadvertised lazy tool executor must not run"
    );
    assert_eq!(result.tool_calls.len(), 1);
    assert!(result.tool_calls[0].is_error);
    assert!(
        result.tool_calls[0]
            .result
            .as_deref()
            .unwrap_or_default()
            .contains("not active"),
        "lazy denial should be recorded in tool history"
    );
}

#[tokio::test]
async fn deny_all_tool_policy_blocks_tool_dispatch() {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::with_responses(vec![
            make_tool_response("exec", "toolu_1", serde_json::json!({"input": "test"})),
            make_text_response("Done!"),
        ])
        .models(&["test-model"]),
    ));

    let tools = make_registry_with("exec", Box::new(EchoExecutor));
    let mut config = test_config();
    config.tool_groups = organon::types::ToolGroupPolicy::DenyAll;

    let result = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &config,
        &providers,
        &tools,
        &test_tool_ctx(),
        None,
    )
    .await
    .expect("execute");

    assert_eq!(result.content, "Done!");
    assert_eq!(result.tool_calls.len(), 1);
    assert!(result.tool_calls[0].is_error);
    assert!(
        result.tool_calls[0]
            .result
            .as_deref()
            .unwrap_or_default()
            .contains("allowed tool groups"),
        "deny-all policy should be recorded as a dispatch denial"
    );
}

#[tokio::test]
async fn denied_first_allowed_second_preserves_tool_outcome_order() {
    let mock = Arc::new(
        MockProvider::with_responses(vec![
            make_multi_tool_response(vec![
                (
                    "read_file",
                    "toolu_denied",
                    serde_json::json!({"path": "notes.md"}),
                ),
                (
                    "exec",
                    "toolu_allowed",
                    serde_json::json!({"input": "date"}),
                ),
            ]),
            make_text_response("Done!"),
        ])
        .models(&["test-model"]),
    );
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(ArcMockProvider(Arc::clone(&mock))));

    let tools = make_exec_and_read_registry();
    let mut config = test_config();
    config.tool_allowlist = Some(vec!["exec".to_owned()]);

    let result = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &config,
        &providers,
        &tools,
        &test_tool_ctx(),
        None,
    )
    .await
    .expect("execute");

    let tool_call_ids: Vec<_> = result
        .tool_calls
        .iter()
        .map(|call| call.id.as_str())
        .collect();
    assert_eq!(
        tool_call_ids,
        vec!["toolu_denied", "toolu_allowed"],
        "TurnResult tool_calls should preserve provider tool_use order"
    );
    assert!(result.tool_calls[0].is_error);
    assert_eq!(
        result.tool_calls[0].approval.as_deref(),
        Some("denied_by_role")
    );
    assert!(!result.tool_calls[1].is_error);

    assert_eq!(
        tool_result_ids_from_second_request(&mock),
        vec!["toolu_denied", "toolu_allowed"],
        "LLM-facing tool_result blocks should preserve provider tool_use order"
    );
}

#[tokio::test]
async fn allowed_first_denied_second_preserves_tool_outcome_order() {
    let mock = Arc::new(
        MockProvider::with_responses(vec![
            make_multi_tool_response(vec![
                (
                    "exec",
                    "toolu_allowed",
                    serde_json::json!({"input": "date"}),
                ),
                (
                    "read_file",
                    "toolu_denied",
                    serde_json::json!({"path": "notes.md"}),
                ),
            ]),
            make_text_response("Done!"),
        ])
        .models(&["test-model"]),
    );
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(ArcMockProvider(Arc::clone(&mock))));

    let tools = make_exec_and_read_registry();
    let mut config = test_config();
    config.tool_allowlist = Some(vec!["exec".to_owned()]);

    let result = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &config,
        &providers,
        &tools,
        &test_tool_ctx(),
        None,
    )
    .await
    .expect("execute");

    let tool_call_ids: Vec<_> = result
        .tool_calls
        .iter()
        .map(|call| call.id.as_str())
        .collect();
    assert_eq!(
        tool_call_ids,
        vec!["toolu_allowed", "toolu_denied"],
        "TurnResult tool_calls should preserve provider tool_use order"
    );
    assert!(!result.tool_calls[0].is_error);
    assert!(result.tool_calls[1].is_error);
    assert_eq!(
        result.tool_calls[1].approval.as_deref(),
        Some("denied_by_role")
    );

    assert_eq!(
        tool_result_ids_from_second_request(&mock),
        vec!["toolu_allowed", "toolu_denied"],
        "LLM-facing tool_result blocks should preserve provider tool_use order"
    );
}

/// Records `(tool_use_id, tool_name, tool_input)` for every `after_tool` firing.
///
/// WHY(#5017): used to assert hook provenance is matched by tool-use ID, not
/// tool name — the regression class where two same-named tool calls in one
/// turn could otherwise be attributed to each other's input.
struct RecordingAfterToolHook {
    calls: Arc<Mutex<Vec<(String, String, serde_json::Value)>>>,
}

impl TurnHook for RecordingAfterToolHook {
    fn name(&self) -> &'static str {
        "recording_after_tool"
    }

    fn after_tool<'a>(
        &'a self,
        context: &'a AfterToolContext<'_>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + Send + 'a>> {
        let calls = Arc::clone(&self.calls);
        let record = (
            context.tool_use_id.to_owned(),
            context.tool_name.to_owned(),
            context.tool_input.clone(),
        );
        Box::pin(async move {
            calls.lock().expect("calls lock").push(record);
            HookResult::Continue
        })
    }
}

#[tokio::test]
async fn after_tool_hook_matches_input_by_id_when_same_tool_called_twice() {
    let mock = Arc::new(
        MockProvider::with_responses(vec![
            make_multi_tool_response(vec![
                ("exec", "toolu_a", serde_json::json!({"input": "first"})),
                ("exec", "toolu_b", serde_json::json!({"input": "second"})),
            ]),
            make_text_response("Done!"),
        ])
        .models(&["test-model"]),
    );
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(ArcMockProvider(Arc::clone(&mock))));

    let tools = make_registry_with("exec", Box::new(EchoExecutor));

    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut hooks = HookRegistry::new();
    hooks.register(
        0,
        Box::new(RecordingAfterToolHook {
            calls: Arc::clone(&calls),
        }),
    );

    execute(
        &test_pipeline_ctx(),
        &test_session(),
        &test_config(),
        &providers,
        &tools,
        &test_tool_ctx(),
        Some(&hooks),
    )
    .await
    .expect("execute");

    let recorded = calls.lock().expect("calls lock").clone();
    assert_eq!(
        recorded,
        vec![
            (
                "toolu_a".to_owned(),
                "exec".to_owned(),
                serde_json::json!({"input": "first"}),
            ),
            (
                "toolu_b".to_owned(),
                "exec".to_owned(),
                serde_json::json!({"input": "second"}),
            ),
        ],
        "after_tool hook should receive each call's own input keyed by tool-use id, \
         not a name-based lookup that could hand both calls the same input"
    );
}

#[tokio::test]
async fn empty_tool_def_groups_are_blocked_before_dispatch() {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::with_responses(vec![
            make_tool_response("legacy", "toolu_1", serde_json::json!({})),
            make_text_response("Done!"),
        ])
        .models(&["test-model"]),
    ));

    let mut def = make_tool_def("legacy");
    def.groups = Vec::new();
    let mut tools = ToolRegistry::new();
    tools
        .register(def, Box::new(EchoExecutor))
        .expect("register");
    let mut config = test_config();
    config.tool_groups =
        organon::types::ToolGroupPolicy::groups(vec![organon::types::ToolGroupId::Read]);

    let result = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &config,
        &providers,
        &tools,
        &test_tool_ctx(),
        None,
    )
    .await
    .expect("execute");

    assert_eq!(result.content, "Done!");
    assert_eq!(result.tool_calls.len(), 1);
    assert!(result.tool_calls[0].is_error);
    assert!(
        result.tool_calls[0]
            .result
            .as_deref()
            .unwrap_or_default()
            .contains("allowed tool groups"),
        "group policy denial should be recorded in tool history"
    );
}

#[tokio::test]
async fn multi_tool_iteration() {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::with_responses(vec![
            make_tool_response("exec", "toolu_1", serde_json::json!({"input": "first"})),
            make_tool_response("exec", "toolu_2", serde_json::json!({"input": "second"})),
            make_text_response("All done!"),
        ])
        .models(&["test-model"]),
    ));

    let tools = make_registry_with("exec", Box::new(EchoExecutor));

    let result = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &test_config(),
        &providers,
        &tools,
        &test_tool_ctx(),
        None,
    )
    .await
    .expect("execute");

    assert_eq!(
        result.content, "All done!",
        "final response content should match mock text"
    );
    assert_eq!(
        result.tool_calls.len(),
        2,
        "should have recorded two tool calls across iterations"
    );
    assert_eq!(
        result.usage.llm_calls, 3,
        "two tool iterations require three LLM calls"
    );
}

#[tokio::test]
async fn loop_detection_triggers() {
    let mut providers = ProviderRegistry::new();
    let response = make_tool_response("exec", "toolu_1", serde_json::json!({"input": "same"}));
    providers.register(Box::new(
        MockProvider::with_responses(vec![response.clone(), response.clone(), response])
            .models(&["test-model"]),
    ));

    let tools = make_registry_with("exec", Box::new(EchoExecutor));
    let mut config = test_config();
    config.limits.loop_detection_threshold = 3;

    let err = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &config,
        &providers,
        &tools,
        &test_tool_ctx(),
        None,
    )
    .await
    .expect_err("should detect loop");

    assert!(
        err.to_string().contains("loop detected"),
        "error message should indicate loop was detected"
    );
}

#[tokio::test]
async fn max_iterations_respected() {
    // WHY(#5369): exhausting the tool-iteration cap without the model ever
    // reaching a final response is now a hard error (`ToolIterationsExhausted`),
    // not a silently-successful `TurnResult` — see `max_iterations_reports_stop_reason`
    // for the paired assertion on the error's contents.
    let mut providers = ProviderRegistry::new();
    let responses: Vec<CompletionResponse> = (0..10)
        .map(|i| make_tool_response("exec", &format!("toolu_{i}"), serde_json::json!({"i": i})))
        .collect();
    providers.register(Box::new(
        MockProvider::with_responses(responses).models(&["test-model"]),
    ));

    let tools = make_registry_with("exec", Box::new(EchoExecutor));
    let mut config = test_config();
    config.limits.max_tool_iterations = 3;
    config.limits.loop_detection_threshold = 100;

    let err = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &config,
        &providers,
        &tools,
        &test_tool_ctx(),
        None,
    )
    .await
    .expect_err("exhausting max_tool_iterations without a final response must error");

    assert!(
        err.to_string().contains("tool iteration limit"),
        "error should identify the tool-iteration cap: {err}"
    );
}

#[tokio::test]
async fn max_iterations_reports_stop_reason() {
    let mut providers = ProviderRegistry::new();
    let responses: Vec<CompletionResponse> = (0..10)
        .map(|i| make_tool_response("exec", &format!("toolu_{i}"), serde_json::json!({"i": i})))
        .collect();
    providers.register(Box::new(
        MockProvider::with_responses(responses).models(&["test-model"]),
    ));

    let tools = make_registry_with("exec", Box::new(EchoExecutor));
    let mut config = test_config();
    config.limits.max_tool_iterations = 3;
    config.limits.loop_detection_threshold = 100;

    let err = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &config,
        &providers,
        &tools,
        &test_tool_ctx(),
        None,
    )
    .await
    .expect_err("exhausting max_tool_iterations without a final response must error");

    assert!(
        err.to_string().contains("limit (3)"),
        "error should report the configured max_tool_iterations value: {err}"
    );
}

#[tokio::test]
async fn tool_error_captured() {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::with_responses(vec![
            make_tool_response("exec", "toolu_1", serde_json::json!({"input": "test"})),
            make_text_response("Recovered"),
        ])
        .models(&["test-model"]),
    ));

    let tools = make_registry_with("exec", Box::new(ErrorExecutor));

    let result = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &test_config(),
        &providers,
        &tools,
        &test_tool_ctx(),
        None,
    )
    .await
    .expect("execute should succeed despite tool error");

    assert_eq!(
        result.tool_calls.len(),
        1,
        "should have recorded one tool call even when it errored"
    );
    assert!(
        result.tool_calls[0].is_error,
        "tool call should be marked as an error"
    );
    let result_text = result.tool_calls[0].result.as_deref().unwrap_or("");
    assert!(
        result_text.starts_with("tool failed"),
        "tool result should start with error message: {result_text}"
    );
    assert!(
        result_text.contains("[receipt:"),
        "tool result should contain receipt: {result_text}"
    );
    assert_eq!(
        result.content, "Recovered",
        "final response content should reflect recovery after tool error"
    );
}

#[test]
fn signal_classification_conversation() {
    let signals = classify_signals(&[], "Hello", false, false);
    assert_eq!(
        signals,
        vec![InteractionSignal::Conversation],
        "no tool calls and plain text should produce only Conversation signal"
    );
}

#[test]
fn signal_classification_code() {
    let calls = vec![ToolCall {
        id: "1".to_owned(),
        name: "write".to_owned(),
        input: serde_json::json!({}),
        result: Some("ok".to_owned()),
        is_error: false,
        duration_ms: 10,
        approval: None,
        receipt: None,
        outcome_detail: None,
    }];
    let signals = classify_signals(&calls, "", false, false);
    assert!(
        signals.contains(&InteractionSignal::ToolExecution),
        "write tool call should produce ToolExecution signal"
    );
    assert!(
        signals.contains(&InteractionSignal::CodeGeneration),
        "write tool call should produce CodeGeneration signal"
    );
}

#[test]
fn signal_classification_research() {
    let calls = vec![ToolCall {
        id: "1".to_owned(),
        name: "web_search".to_owned(),
        input: serde_json::json!({}),
        result: Some("results".to_owned()),
        is_error: false,
        duration_ms: 10,
        approval: None,
        receipt: None,
        outcome_detail: None,
    }];
    let signals = classify_signals(&calls, "", false, false);
    assert!(
        signals.contains(&InteractionSignal::ToolExecution),
        "web_search tool call should produce ToolExecution signal"
    );
    assert!(
        signals.contains(&InteractionSignal::Research),
        "web_search tool call should produce Research signal"
    );
}

#[test]
fn signal_classification_error_recovery() {
    let calls = vec![ToolCall {
        id: "1".to_owned(),
        name: "exec".to_owned(),
        input: serde_json::json!({}),
        result: Some("failed".to_owned()),
        is_error: true,
        duration_ms: 10,
        approval: None,
        receipt: None,
        outcome_detail: None,
    }];
    let signals = classify_signals(&calls, "", false, false);
    assert!(
        signals.contains(&InteractionSignal::ToolExecution),
        "error tool call should produce ToolExecution signal"
    );
    assert!(
        signals.contains(&InteractionSignal::ErrorRecovery),
        "failed tool call should produce ErrorRecovery signal"
    );
}

#[tokio::test]
async fn usage_accumulates_across_iterations() {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::with_responses(vec![
            make_tool_response("exec", "toolu_1", serde_json::json!({"input": "first"})),
            make_text_response("Done"),
        ])
        .models(&["test-model"]),
    ));

    let tools = make_registry_with("exec", Box::new(EchoExecutor));

    let result = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &test_config(),
        &providers,
        &tools,
        &test_tool_ctx(),
        None,
    )
    .await
    .expect("execute");

    assert_eq!(
        result.usage.input_tokens, 180,
        "input tokens should be summed across both LLM calls (80 + 100)"
    );
    assert_eq!(
        result.usage.output_tokens, 80,
        "output tokens should be summed across both LLM calls (30 + 50)"
    );
    assert_eq!(
        result.usage.llm_calls, 2,
        "one tool iteration should produce exactly two LLM calls"
    );
    assert_eq!(
        result.usage.total_tokens(),
        260,
        "total tokens should equal sum of all input and output tokens (180 + 80)"
    );
}

#[tokio::test]
async fn tool_error_captured_not_propagated() {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::with_responses(vec![
            make_tool_response("fail_tool", "tu_1", serde_json::json!({})),
            make_text_response("recovered"),
        ])
        .models(&["test-model"]),
    ));

    let tools = make_registry_with("fail_tool", Box::new(ErrorExecutor));
    let result = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &test_config(),
        &providers,
        &tools,
        &test_tool_ctx(),
        None,
    )
    .await
    .expect("pipeline should complete despite tool error");

    assert!(
        result.tool_calls.iter().any(|tc| tc.is_error),
        "should capture the tool error in tool_calls"
    );
}

#[tokio::test]
async fn max_iterations_stops_loop() {
    let mut providers = ProviderRegistry::new();
    // WHY: Provider always returns tool use: would loop forever without max_iterations.
    // Supply enough unique-id responses to feed several iterations.
    let responses: Vec<_> = (0..10)
        .map(|i| make_tool_response("echo", &format!("tu_{i}"), serde_json::json!({"i": i})))
        .collect();
    providers.register(Box::new(
        MockProvider::with_responses(responses).models(&["test-model"]),
    ));

    let tools = make_registry_with("echo", Box::new(EchoExecutor));
    let mut config = test_config();
    config.limits.max_tool_iterations = 2;
    config.limits.loop_detection_threshold = 100;
    let err = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &config,
        &providers,
        &tools,
        &test_tool_ctx(),
        None,
    )
    .await
    .expect_err("a tool-only loop that never converges must stop with an error, not run forever");

    assert!(
        err.to_string().contains("tool iteration limit"),
        "should have stopped on the tool-iteration cap, got: {err}"
    );
}

#[tokio::test]
async fn text_response_no_tools() {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::with_responses(vec![make_text_response("just text")]).models(&["test-model"]),
    ));

    let tools = ToolRegistry::new();
    let result = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &test_config(),
        &providers,
        &tools,
        &test_tool_ctx(),
        None,
    )
    .await
    .expect("execute");

    assert!(result.tool_calls.is_empty(), "no tool calls expected");
    assert_eq!(
        result.content, "just text",
        "response content should match mock text"
    );
}

#[test]
fn classify_signals_conversation_when_no_tools() {
    let signals = classify_signals(&[], "some text", false, false);
    assert_eq!(
        signals,
        vec![InteractionSignal::Conversation],
        "no tool calls and plain text should produce only Conversation signal"
    );
}

#[test]
fn classify_signals_includes_error_recovery() {
    let calls = vec![ToolCall {
        id: "1".to_owned(),
        name: "test".to_owned(),
        input: serde_json::json!({}),
        result: Some("failed".to_owned()),
        is_error: true,
        duration_ms: 5,
        approval: None,
        receipt: None,
        outcome_detail: None,
    }];
    let signals = classify_signals(&calls, "", false, false);
    assert!(
        signals.contains(&InteractionSignal::ToolExecution),
        "should have ToolExecution"
    );
    assert!(
        signals.contains(&InteractionSignal::ErrorRecovery),
        "should have ErrorRecovery"
    );
}

#[test]
fn classify_signals_server_web_search() {
    let signals = classify_signals(&[], "", true, false);
    assert!(
        signals.contains(&InteractionSignal::Research),
        "should have Research from server web search"
    );
    assert!(
        !signals.contains(&InteractionSignal::Conversation),
        "should not be Conversation when server web search was used"
    );
}

#[test]
fn classify_signals_server_code_execution() {
    let signals = classify_signals(&[], "", false, true);
    assert!(
        signals.contains(&InteractionSignal::ToolExecution),
        "should have ToolExecution from server code execution"
    );
    assert!(
        signals.contains(&InteractionSignal::CodeGeneration),
        "should have CodeGeneration from server code execution"
    );
    assert!(
        !signals.contains(&InteractionSignal::Conversation),
        "should not be Conversation when server code execution was used"
    );
}

#[test]
fn classify_signals_both_server_tools() {
    let signals = classify_signals(&[], "", true, true);
    assert!(
        signals.contains(&InteractionSignal::ToolExecution),
        "both server tools should produce ToolExecution signal"
    );
    assert!(
        signals.contains(&InteractionSignal::Research),
        "server web search should produce Research signal"
    );
    assert!(
        signals.contains(&InteractionSignal::CodeGeneration),
        "server code execution should produce CodeGeneration signal"
    );
    assert!(
        !signals.contains(&InteractionSignal::Conversation),
        "should not produce Conversation signal when server tools were used"
    );
}

// --- Complexity routing wire-in (#3737) ---

#[tokio::test]
async fn test_routing_disabled_uses_turn_model() {
    // WHY: default complexity.enabled=false must preserve existing behaviour
    // exactly — the turn model is the config's `generation.model`, regardless
    // of message content.
    let mut providers = ProviderRegistry::new();
    let mock = MockProvider::with_responses(vec![make_text_response("ok")]).models(&[
        "test-model",
        "fast-tier",
        "mid-tier",
        "big-tier",
    ]);
    providers.register(Box::new(mock));

    let tools = ToolRegistry::new();

    // Use a message that would otherwise route to Opus tier (force-complex marker)
    let mut ctx = test_pipeline_ctx();
    ctx.messages[0].content = "think hard about this architecture decision".to_owned();

    let result = execute(
        &ctx,
        &test_session(),
        &test_config(),
        &providers,
        &tools,
        &test_tool_ctx(),
        None,
    )
    .await
    .expect("execute");

    assert_eq!(result.content, "ok");
    // WHY: can't inspect request model directly through ProviderRegistry, but
    // the fact that execute() succeeded proves resolve_provider_checked found
    // "test-model" — the provider is only registered for that + tier slots,
    // and routing-disabled path asks for exactly "test-model".
    assert_eq!(result.usage.llm_calls, 1);
}

#[tokio::test]
async fn test_routing_enabled_selects_tier_model() {
    // WHY: when complexity.enabled=true, a "think hard" message must route
    // to the opus tier model, not the turn-default model. Verified by
    // registering only opus-tier as a valid model — if routing fails to
    // swap the model, provider resolution fails.
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::with_responses(vec![make_text_response("opus answer")])
            .models(&["opus-tier"]),
    ));

    let tools = ToolRegistry::new();

    let mut config = test_config();
    config.generation.complexity = hermeneus::complexity::ComplexityConfig {
        enabled: true,
        haiku_model: "haiku-tier".to_owned(),
        sonnet_model: "sonnet-tier".to_owned(),
        opus_model: "opus-tier".to_owned(),
        ..hermeneus::complexity::ComplexityConfig::default()
    };

    let mut ctx = test_pipeline_ctx();
    ctx.messages[0].content = "think hard about this architecture decision".to_owned();

    let result = execute(
        &ctx,
        &test_session(),
        &config,
        &providers,
        &tools,
        &test_tool_ctx(),
        None,
    )
    .await
    .expect("execute should resolve opus-tier via complexity routing");

    assert_eq!(result.content, "opus answer");
    assert_eq!(result.usage.llm_calls, 1);
}

#[tokio::test]
async fn test_routing_enabled_preserves_local_deployment_target() {
    // WHY: a locally configured turn model must not be replaced by a cloud
    // tier model just because the complexity score is high. Provider
    // resolution only registers the local model, so this fails if the
    // sovereignty guard is bypassed.
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(DeploymentTargetProvider::new(
        MockProvider::with_responses(vec![make_text_response("local answer")])
            .models(&["local-tier"]),
        DeploymentTarget::Embedded,
    )));

    let tools = ToolRegistry::new();

    let mut config = test_config();
    config.generation.model = "local-tier".to_owned();
    config.generation.complexity = hermeneus::complexity::ComplexityConfig {
        enabled: true,
        haiku_model: "haiku-cloud".to_owned(),
        sonnet_model: "sonnet-cloud".to_owned(),
        opus_model: "opus-cloud".to_owned(),
        ..hermeneus::complexity::ComplexityConfig::default()
    };

    let mut ctx = test_pipeline_ctx();
    ctx.messages[0].content = "think hard about this architecture decision".to_owned();

    let result = execute(
        &ctx,
        &test_session(),
        &config,
        &providers,
        &tools,
        &test_tool_ctx(),
        None,
    )
    .await
    .expect("execute should preserve the embedded provider's local model");

    assert_eq!(result.content, "local answer");
    assert_eq!(result.usage.llm_calls, 1);
}

#[tokio::test]
async fn test_routing_enabled_allows_local_tier_model() {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(DeploymentTargetProvider::new(
        MockProvider::with_responses(vec![make_text_response("configured local")])
            .models(&["local-tier"]),
        DeploymentTarget::Embedded,
    )));
    providers.register(Box::new(DeploymentTargetProvider::new(
        MockProvider::with_responses(vec![make_text_response("local opus answer")])
            .models(&["local-opus"]),
        DeploymentTarget::Embedded,
    )));

    let tools = ToolRegistry::new();

    let mut config = test_config();
    config.generation.model = "local-tier".to_owned();
    config.generation.complexity = hermeneus::complexity::ComplexityConfig {
        enabled: true,
        haiku_model: "local-tier".to_owned(),
        sonnet_model: "local-sonnet".to_owned(),
        opus_model: "local-opus".to_owned(),
        ..hermeneus::complexity::ComplexityConfig::default()
    };

    let mut ctx = test_pipeline_ctx();
    ctx.messages[0].content = "think hard about this architecture decision".to_owned();

    let result = execute(
        &ctx,
        &test_session(),
        &config,
        &providers,
        &tools,
        &test_tool_ctx(),
        None,
    )
    .await
    .expect("execute should allow local tier model routing");

    assert_eq!(result.content, "local opus answer");
    assert_eq!(result.usage.llm_calls, 1);
}

// WHY(#4621): live-prompt sensitivity gate. Mirrors the capability-negotiation
// tests above (`tool_bearing_turn_never_dispatches_to_incapable_provider_without_fallback`,
// `tool_bearing_fallback_chain_skips_incapable_route_for_capable_one`) but for
// deployment-target admission instead of tool-loop capability: recall already
// withholds over-sensitive recalled facts before they reach a provider; these
// prove the live user prompt gets the identical admission check before its own
// dispatch.

#[tokio::test]
async fn confidential_turn_is_blocked_when_no_provider_admits_it() {
    let primary = Arc::new(MockProvider::new("must never be seen").models(&["test-model"]));
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(ArcMockProvider(Arc::clone(&primary))));

    let mut ctx = test_pipeline_ctx();
    ctx.triage_result = Some(crate::pipeline::triage::TriageResult::new(
        crate::pipeline::triage::Intent::Unclassified,
        mneme::knowledge::FactSensitivity::Confidential,
        hermeneus::complexity::ModelTier::Haiku,
        ctx.messages[0].content.len(),
    ));

    let err = execute(
        &ctx,
        &test_session(),
        &test_config(),
        &providers,
        &ToolRegistry::new(),
        &test_tool_ctx(),
        None,
    )
    .await
    .expect_err("a confidential turn must not dispatch to a Cloud-only provider");

    let msg = err.to_string();
    assert!(
        msg.contains("blocked before dispatch"),
        "error should name the sensitivity gate, got: {msg}"
    );
    assert!(
        primary.captured_requests().is_empty(),
        "the classified prompt must never reach the provider — without the gate this \
         mock would have received it and returned \"must never be seen\" successfully"
    );
}

#[tokio::test]
async fn internal_turn_reroutes_to_the_eligible_local_fallback() {
    let cloud_primary = Arc::new(MockProvider::new("must never be seen").models(&["test-model"]));
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(ArcMockProvider(Arc::clone(&cloud_primary))));
    providers.register(Box::new(DeploymentTargetProvider::new(
        MockProvider::with_responses(vec![make_text_response("local answer")])
            .models(&["local-model"]),
        DeploymentTarget::LocalHosted,
    )));

    let mut config = test_config();
    config.generation.fallback_models = vec!["local-model".to_owned()];

    let mut ctx = test_pipeline_ctx();
    ctx.triage_result = Some(crate::pipeline::triage::TriageResult::new(
        crate::pipeline::triage::Intent::Unclassified,
        mneme::knowledge::FactSensitivity::Internal,
        hermeneus::complexity::ModelTier::Haiku,
        ctx.messages[0].content.len(),
    ));

    let result = execute(
        &ctx,
        &test_session(),
        &config,
        &providers,
        &ToolRegistry::new(),
        &test_tool_ctx(),
        None,
    )
    .await
    .expect("an Internal turn should reroute to the LocalHosted fallback rather than block");

    assert_eq!(result.content, "local answer");
    assert_eq!(result.model_used, "local-model");
    assert!(
        cloud_primary.captured_requests().is_empty(),
        "the classified prompt must reach only the admitted local provider, never the \
         configured Cloud primary"
    );
}

/// WHY(#5820): a loop warning ends dispatch early, and the abandoned calls are recorded
/// so their `tool_use` blocks stay paired with a `tool_result`. Those calls never ran, so
/// `after_tool` must not fire for them — a hook that sees them would be told a tool
/// executed when it did not.
#[tokio::test]
async fn after_tool_hook_skips_calls_abandoned_by_a_loop_warning() {
    let mock = Arc::new(
        MockProvider::with_responses(vec![
            // Identical name + input on all four, so the detector trips mid-dispatch.
            make_multi_tool_response(vec![
                ("exec", "toolu_a", serde_json::json!({"input": "same"})),
                ("exec", "toolu_b", serde_json::json!({"input": "same"})),
                ("exec", "toolu_c", serde_json::json!({"input": "same"})),
                ("exec", "toolu_d", serde_json::json!({"input": "same"})),
            ]),
            make_text_response("Done!"),
        ])
        .models(&["test-model"]),
    );
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(ArcMockProvider(Arc::clone(&mock))));

    let tools = make_registry_with("exec", Box::new(EchoExecutor));

    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut hooks = HookRegistry::new();
    hooks.register(
        0,
        Box::new(RecordingAfterToolHook {
            calls: Arc::clone(&calls),
        }),
    );

    let mut config = test_config();
    config.limits.loop_detection_threshold = 2;

    execute(
        &test_pipeline_ctx(),
        &test_session(),
        &config,
        &providers,
        &tools,
        &test_tool_ctx(),
        Some(&hooks),
    )
    .await
    .expect("execute");

    let fired: Vec<String> = calls
        .lock()
        .expect("calls lock")
        .iter()
        .map(|(id, _, _)| id.clone())
        .collect();

    assert!(
        fired.len() < 4,
        "the loop warning must abandon at least one call — got all {} dispatched, so the \
         detector never tripped and this test is not exercising the guard",
        fired.len()
    );
    for abandoned in ["toolu_c", "toolu_d"] {
        assert!(
            !fired.iter().any(|id| id == abandoned),
            "after_tool fired for {abandoned}, which was abandoned by the loop warning and \
             never executed — fired: {fired:?}"
        );
    }
}

/// WHY(#5827): `after_tool` means "a tool ran". A call denied inside dispatch is
/// recorded so its `tool_use` block stays paired with a `tool_result`, but it never
/// executed, and it carries `is_error=true`/`duration_ms=0` — which a hook cannot tell
/// apart from a tool that ran and failed. Audit hooks gating on `is_error` and
/// cost-control hooks metering duration both read that as a real execution.
///
/// The denial here is the no-gate `Mandatory` fallback: an approval-required tool with
/// no approval gate wired is default-denied. Its dispatch item is `Ready`, so it clears
/// the `ready_input_for` guard and reaches the hook loop — this is the path #6512's
/// outcome check deliberately did not cover.
#[tokio::test]
async fn after_tool_hook_skips_calls_denied_before_execution() {
    let mock = Arc::new(
        MockProvider::with_responses(vec![
            make_multi_tool_response(vec![
                ("safe", "toolu_safe", serde_json::json!({})),
                ("danger", "toolu_denied", serde_json::json!({})),
            ]),
            make_text_response("Done!"),
        ])
        .models(&["test-model"]),
    );
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(ArcMockProvider(Arc::clone(&mock))));

    // `danger` is Irreversible, so it requires approval; no gate is wired below, so
    // dispatch default-denies it without ever calling the executor.
    let mut tools = ToolRegistry::new();
    tools
        .register(make_tool_def("safe"), Box::new(EchoExecutor))
        .expect("register safe");
    tools
        .register(
            make_tool_def_rev("danger", organon::types::Reversibility::Irreversible),
            Box::new(EchoExecutor),
        )
        .expect("register danger");

    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut hooks = HookRegistry::new();
    hooks.register(
        0,
        Box::new(RecordingAfterToolHook {
            calls: Arc::clone(&calls),
        }),
    );

    execute(
        &test_pipeline_ctx(),
        &test_session(),
        &test_config(),
        &providers,
        &tools,
        &test_tool_ctx(),
        Some(&hooks),
    )
    .await
    .expect("execute");

    let fired: Vec<String> = calls
        .lock()
        .expect("calls lock")
        .iter()
        .map(|(id, _, _)| id.clone())
        .collect();

    // WHY: the positive case has to hold too. Without it this test would still pass if
    // after_tool never fired at all — a hook that is silently unregistered would read as
    // a fix.
    assert!(
        fired.iter().any(|id| id == "toolu_safe"),
        "after_tool must still fire for the tool that actually executed — fired: {fired:?}"
    );
    assert!(
        !fired.iter().any(|id| id == "toolu_denied"),
        "after_tool fired for a call denied before execution, so a hook was told a tool \
         ran when it did not — fired: {fired:?}"
    );
}
