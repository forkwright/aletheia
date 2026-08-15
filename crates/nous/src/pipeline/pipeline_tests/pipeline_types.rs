use std::sync::Arc;

use super::*;

#[test]
fn guard_result_equality() {
    let reason = "test".to_owned();
    let r1 = GuardResult::Rejected {
        reason: reason.clone(),
    };
    let r2 = GuardResult::Rejected { reason };
    assert_eq!(r1, r2, "Rejected with same reason should be equal");
    assert_ne!(
        GuardResult::Allow,
        GuardResult::Rejected {
            reason: "test".to_owned()
        },
        "Allow should not equal Rejected"
    );
}

#[test]
fn turn_usage_total() {
    let usage = TurnUsage {
        input_tokens: 1000,
        output_tokens: 500,
        cache_read_tokens: 800,
        cache_write_tokens: 200,
        llm_calls: 3,
    };
    assert_eq!(usage.total_tokens(), 1500, "total should be input + output");
}

#[test]
fn turn_usage_budgeted_tokens_includes_cache() {
    let usage = TurnUsage {
        input_tokens: 1000,
        output_tokens: 500,
        cache_read_tokens: 800,
        cache_write_tokens: 200,
        llm_calls: 3,
    };
    assert_eq!(
        usage.budgeted_tokens(),
        2500,
        "budgeted_tokens should include cache read/write tokens"
    );
    assert_eq!(
        usage.total_tokens(),
        1500,
        "total_tokens keeps its narrower input+output meaning"
    );
}

#[test]
fn interaction_signal_serde() {
    let signal = InteractionSignal::CodeGeneration;
    let json = serde_json::to_string(&signal).expect("serialize signal");
    assert_eq!(
        json, "\"code_generation\"",
        "signal should serialize to snake_case"
    );
    let back: InteractionSignal = serde_json::from_str(&json).expect("deserialize signal");
    assert_eq!(back, signal, "roundtrip should preserve signal");
}

#[test]
fn pipeline_context_default() {
    let ctx = PipelineContext::default();
    assert!(
        ctx.system_prompt.is_none(),
        "default system_prompt should be None"
    );
    assert!(ctx.messages.is_empty(), "default messages should be empty");
    assert!(
        !ctx.needs_distillation,
        "default needs_distillation should be false"
    );
    assert_eq!(
        ctx.guard_result,
        GuardResult::Allow,
        "default guard should be Allow"
    );
    assert!(
        ctx.working_state.is_none(),
        "default working_state should be None"
    );
    assert!(
        ctx.reflection_result.is_none(),
        "default reflection_result should be None"
    );
}

#[test]
fn guard_result_rate_limited() {
    let g = GuardResult::RateLimited {
        retry_after_ms: 5000,
    };
    assert_ne!(g, GuardResult::Allow, "RateLimited should not equal Allow");
    match g {
        GuardResult::RateLimited { retry_after_ms } => {
            assert_eq!(retry_after_ms, 5000, "retry_after_ms should match");
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn guard_result_loop_detected() {
    let g = GuardResult::LoopDetected {
        pattern: "exec:abc".to_owned(),
    };
    match g {
        GuardResult::LoopDetected { pattern } => {
            assert_eq!(pattern, "exec:abc", "pattern should match");
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn guard_result_rejected() {
    let g = GuardResult::Rejected {
        reason: "unsafe content".to_owned(),
    };
    match g {
        GuardResult::Rejected { reason } => {
            assert!(reason.contains("unsafe"), "reason should contain unsafe");
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn all_interaction_signals_serde_roundtrip() {
    let signals = [
        InteractionSignal::Conversation,
        InteractionSignal::ToolExecution,
        InteractionSignal::CodeGeneration,
        InteractionSignal::Research,
        InteractionSignal::Planning,
        InteractionSignal::ErrorRecovery,
    ];
    for signal in signals {
        let json = serde_json::to_string(&signal).expect("serialize signal");
        let back: InteractionSignal = serde_json::from_str(&json).expect("deserialize signal");
        assert_eq!(
            signal, back,
            "serde roundtrip should preserve signal variant"
        );
    }
}

#[test]
fn turn_usage_default_is_zero() {
    let usage = TurnUsage::default();
    assert_eq!(
        usage.total_tokens(),
        0,
        "default total tokens should be zero"
    );
    assert_eq!(usage.llm_calls, 0, "default llm_calls should be zero");
}

#[test]
fn turn_usage_serde_roundtrip() {
    let usage = TurnUsage {
        input_tokens: 100,
        output_tokens: 50,
        cache_read_tokens: 80,
        cache_write_tokens: 20,
        llm_calls: 2,
    };
    let json = serde_json::to_string(&usage).expect("serialize usage");
    let back: TurnUsage = serde_json::from_str(&json).expect("deserialize usage");
    assert_eq!(
        usage.total_tokens(),
        back.total_tokens(),
        "roundtrip should preserve total tokens"
    );
}

#[tokio::test]
async fn assemble_context_populates_pipeline() {
    use std::fs;

    use tempfile::TempDir;

    use taxis::oikos::Oikos;

    use crate::config::{NousConfig, PipelineConfig};

    let dir = TempDir::new().expect("create temp dir");
    let root = dir.path();
    fs::create_dir_all(root.join("nous/test-agent")).expect("create nous dir");
    fs::create_dir_all(root.join("shared")).expect("create shared dir");
    fs::create_dir_all(root.join("theke")).expect("create theke dir");
    #[expect(
        clippy::disallowed_methods,
        reason = "nous bootstrap and test setup writes configuration files to temp directories; synchronous I/O is required in test contexts"
    )]
    fs::write(root.join("nous/test-agent/SOUL.md"), "I am a test agent.").expect("write SOUL.md");
    #[expect(
        clippy::disallowed_methods,
        reason = "nous bootstrap and test setup writes configuration files to temp directories; synchronous I/O is required in test contexts"
    )]
    fs::write(root.join("theke/USER.md"), "Test user.").expect("write USER.md");

    let oikos = Oikos::from_root(root);
    let nous_config = NousConfig {
        id: Arc::from("test-agent"),
        ..NousConfig::default()
    };
    let pipeline_config = PipelineConfig::default();
    let mut ctx = PipelineContext::default();

    assemble_context(&oikos, &nous_config, &pipeline_config, &mut ctx)
        .await
        .expect("assemble_context should succeed");

    assert!(
        ctx.system_prompt.is_some(),
        "system prompt should be populated"
    );
    let prompt = ctx.system_prompt.expect("system prompt present");
    assert!(
        prompt.contains("I am a test agent."),
        "prompt should contain SOUL.md content"
    );
    assert!(
        prompt.contains("Test user."),
        "prompt should contain USER.md content"
    );
    assert!(
        ctx.remaining_tokens > 0,
        "remaining tokens should be positive"
    );
}

#[tokio::test]
async fn assemble_context_conditional_turn_one_selects_cold_start() {
    use std::fs;

    use tempfile::TempDir;

    use taxis::oikos::Oikos;

    use crate::bootstrap::TaskHint;
    use crate::config::{NousConfig, PipelineConfig};

    let dir = TempDir::new().expect("create temp dir");
    let root = dir.path();
    fs::create_dir_all(root.join("nous/test-agent")).expect("create nous dir");
    fs::create_dir_all(root.join("shared")).expect("create shared dir");
    fs::create_dir_all(root.join("theke")).expect("create theke dir");
    fs::create_dir_all(root.join("_llm")).expect("create _llm dir");
    #[expect(
        clippy::disallowed_methods,
        reason = "nous bootstrap and test setup writes configuration files to temp directories; synchronous I/O is required in test contexts"
    )]
    fs::write(root.join("nous/test-agent/SOUL.md"), "I am a test agent.").expect("write SOUL.md");
    #[expect(
        clippy::disallowed_methods,
        reason = "nous bootstrap and test setup writes configuration files to temp directories; synchronous I/O is required in test contexts"
    )]
    fs::write(root.join("_llm/l1-context.md"), {
        let mut s = String::from("cold start l1 signal: ");
        s.push_str(&"word ".repeat(200));
        s
    })
    .expect("write L1 context");
    #[expect(
        clippy::disallowed_methods,
        reason = "nous bootstrap and test setup writes configuration files to temp directories; synchronous I/O is required in test contexts"
    )]
    fs::write(root.join("_llm/manifest.toml"), "").expect("write manifest.toml");

    let oikos = Oikos::from_root(root);
    let mut config = NousConfig {
        id: Arc::from("test-agent"),
        ..NousConfig::default()
    };
    // WHY: keep the bootstrap cap small so the large L1 section only fits when
    // it is Required (cold start). In-session turns should drop it as Optional.
    config.generation.bootstrap_max_tokens = 20;
    let pipeline_config = PipelineConfig::default();

    let mut cold_ctx = PipelineContext::default();
    assemble_context_conditional(
        &oikos,
        &config,
        &pipeline_config,
        &mut cold_ctx,
        Vec::new(),
        TaskHint::General,
        1,
    )
    .await
    .expect("turn 1 context assembly should succeed");
    let cold_prompt = cold_ctx.system_prompt.expect("system prompt present");
    assert!(
        cold_prompt.contains("cold start l1 signal"),
        "turn 1 (cold start) should keep L1 content as Required"
    );

    let mut warm_ctx = PipelineContext::default();
    assemble_context_conditional(
        &oikos,
        &config,
        &pipeline_config,
        &mut warm_ctx,
        Vec::new(),
        TaskHint::General,
        2,
    )
    .await
    .expect("turn 2 context assembly should succeed");
    let warm_prompt = warm_ctx.system_prompt.expect("system prompt present");
    assert!(
        !warm_prompt.contains("cold start l1 signal"),
        "turn 2 (in session) should drop L1 content as Optional under budget pressure"
    );
}

/// Shared setup for tests that drive a full `run_pipeline` turn.
///
/// WHY(#5025): the pipeline entry point takes twenty-two arguments and needs a
/// populated Oikos root on disk. Holding that in one place keeps each test to
/// the behaviour it is actually asserting, and keeps the temp dir alive for the
/// duration of the run.
struct PipelineHarness {
    _dir: tempfile::TempDir,
    oikos: Oikos,
    nous_config: NousConfig,
    pipeline_config: PipelineConfig,
    tool_ctx: organon::types::ToolContext,
}

impl PipelineHarness {
    fn new() -> Self {
        use std::collections::HashSet;
        use std::fs;
        use std::path::PathBuf;
        use std::sync::RwLock;

        use koina::id::{NousId, SessionId};
        use organon::types::ToolContext;
        use tempfile::TempDir;

        let dir = TempDir::new().expect("create temp dir");
        let root = dir.path();
        fs::create_dir_all(root.join("nous/test-agent")).expect("create nous dir");
        fs::create_dir_all(root.join("shared")).expect("create shared dir");
        fs::create_dir_all(root.join("theke")).expect("create theke dir");
        #[expect(
            clippy::disallowed_methods,
            reason = "nous bootstrap and test setup writes configuration files to temp directories; synchronous I/O is required in test contexts"
        )]
        fs::write(root.join("nous/test-agent/SOUL.md"), "I am a test agent.")
            .expect("write SOUL.md");

        let oikos = Oikos::from_root(root);
        let nous_config = NousConfig {
            id: Arc::from("test-agent"),
            generation: crate::config::NousGenerationConfig {
                model: "test-model".to_owned(),
                ..crate::config::NousGenerationConfig::default()
            },
            ..NousConfig::default()
        };

        let tool_ctx = ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            turn_number: 0,
            workspace: PathBuf::from("/tmp/test"),
            allowed_roots: vec![PathBuf::from("/tmp")],
            services: None,
            active_tools: Arc::new(RwLock::new(HashSet::new())),
            tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
        };

        Self {
            _dir: dir,
            oikos,
            nous_config,
            pipeline_config: PipelineConfig::default(),
            tool_ctx,
        }
    }

    async fn run(
        &self,
        content: &str,
        hooks: Option<&crate::hooks::registry::HookRegistry>,
    ) -> error::Result<TurnResult> {
        use hermeneus::provider::ProviderRegistry;
        use hermeneus::test_utils::MockProvider;
        use organon::registry::ToolRegistry;

        let mut providers = ProviderRegistry::new();
        providers.register(Box::new(
            MockProvider::new("Hello from pipeline!").models(&["test-model"]),
        ));

        let session = crate::session::SessionState::new(
            "test-session".to_owned(),
            "main".to_owned(),
            &self.nous_config,
        );
        let input = PipelineInput {
            content: content.to_owned(),
            session,
            config: self.pipeline_config.clone(),
        };

        run_pipeline(
            input,
            &self.oikos,
            &self.nous_config,
            &self.pipeline_config,
            Arc::new(providers),
            &ToolRegistry::new(),
            &self.tool_ctx,
            None::<Arc<dyn mneme::embedding::EmbeddingProvider>>,
            None::<Arc<dyn crate::recall::VectorSearch>>,
            None,
            None,
            None,
            Vec::new(),
            None,
            None,
            None,
            hooks,
            None,
            None,
            None,
            None,
        )
        .await
    }
}

#[tokio::test]
async fn run_pipeline_simple() {
    let result = PipelineHarness::new()
        .run("Hello", None)
        .await
        .expect("pipeline should succeed");

    assert_eq!(
        result.content, "Hello from pipeline!",
        "pipeline should return mock response"
    );
    assert!(
        result.tool_calls.is_empty(),
        "simple pipeline should have no tool calls"
    );
    assert_eq!(
        result.usage.llm_calls, 1,
        "should have exactly one LLM call"
    );
    assert_eq!(
        result.stop_reason, "end_turn",
        "stop reason should be end_turn"
    );
}

#[tokio::test]
async fn assemble_context_missing_soul_returns_error() {
    use tempfile::TempDir;

    use taxis::oikos::Oikos;

    let dir = TempDir::new().expect("create temp dir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("nous/test-agent")).expect("create nous dir");
    std::fs::create_dir_all(root.join("shared")).expect("create shared dir");
    std::fs::create_dir_all(root.join("theke")).expect("create theke dir");

    let oikos = Oikos::from_root(root);
    let config = crate::config::NousConfig {
        id: Arc::from("test-agent"),
        ..crate::config::NousConfig::default()
    };
    let pipeline_config = crate::config::PipelineConfig::default();
    let mut ctx = PipelineContext::default();

    let err = assemble_context(&oikos, &config, &pipeline_config, &mut ctx).await;
    assert!(err.is_err(), "missing SOUL.md should produce error");
    let msg = err.expect_err("should be error").to_string();
    assert!(msg.contains("SOUL.md"), "got: {msg}");
}

#[test]
fn pipeline_message_serde_roundtrip() {
    let msg = PipelineMessage::text("user", "Hello world", 3);
    let json = serde_json::to_string(&msg).expect("serialize message");
    let back: PipelineMessage = serde_json::from_str(&json).expect("deserialize message");
    assert_eq!(msg.role, back.role, "role should roundtrip");
    assert_eq!(msg.content, back.content, "content should roundtrip");
    assert_eq!(
        msg.token_estimate, back.token_estimate,
        "token_estimate should roundtrip"
    );
}

#[test]
fn tool_call_serde_roundtrip() {
    let tc = ToolCall {
        id: "tc-1".to_owned(),
        name: "exec".to_owned(),
        input: serde_json::json!({"cmd": "ls"}),
        result: Some("output".to_owned()),
        is_error: false,
        duration_ms: 42,
        approval: None,
        receipt: None,
        outcome_detail: Some("1 sub-operation degraded".to_owned()),
    };
    let json = serde_json::to_string(&tc).expect("serialize tool call");
    let back: ToolCall = serde_json::from_str(&json).expect("deserialize tool call");
    assert_eq!(tc.id, back.id, "id should roundtrip");
    assert_eq!(tc.name, back.name, "name should roundtrip");
    assert_eq!(
        tc.duration_ms, back.duration_ms,
        "duration_ms should roundtrip"
    );
    assert_eq!(
        tc.outcome_detail, back.outcome_detail,
        "outcome_detail should roundtrip"
    );
}

#[test]
fn tool_call_with_error() {
    let tc = ToolCall {
        id: "tc-1".to_owned(),
        name: "exec".to_owned(),
        input: serde_json::json!({}),
        result: None,
        is_error: true,
        duration_ms: 0,
        approval: None,
        receipt: None,
        outcome_detail: Some("no such file".to_owned()),
    };
    assert!(tc.is_error, "error tool call should have is_error=true");
    assert!(tc.result.is_none(), "error tool call should have no result");
    assert_eq!(
        tc.outcome_detail.as_deref(),
        Some("no such file"),
        "failure reason should be carried on outcome_detail"
    );
}

fn base_tool_call() -> ToolCall {
    ToolCall {
        id: "tc-1".to_owned(),
        name: "exec".to_owned(),
        input: serde_json::json!({}),
        result: Some("ok".to_owned()),
        is_error: false,
        duration_ms: 0,
        approval: None,
        receipt: None,
        outcome_detail: None,
    }
}

#[test]
fn outcome_label_success_when_no_error_and_no_detail() {
    let tc = base_tool_call();
    assert_eq!(tc.outcome_label(), "success");
}

#[test]
fn outcome_label_partial_success_when_no_error_but_detail_present() {
    let mut tc = base_tool_call();
    tc.outcome_detail = Some("1 sub-operation degraded".to_owned());
    assert_eq!(tc.outcome_label(), "partial_success");
}

#[test]
fn outcome_label_error_when_is_error_regardless_of_detail() {
    let mut tc = base_tool_call();
    tc.is_error = true;
    assert_eq!(tc.outcome_label(), "error");

    tc.outcome_detail = Some("boom".to_owned());
    assert_eq!(tc.outcome_label(), "error");
}

#[test]
fn outcome_label_surfaces_a_known_denial_class() {
    let mut tc = base_tool_call();
    tc.is_error = true;
    tc.approval = Some("denied_by_hook".to_owned());
    assert_eq!(
        tc.outcome_label(),
        "denied_by_hook",
        "a real denial class must surface as the outcome, not collapse to 'error'"
    );
}

#[test]
fn outcome_label_does_not_mistake_an_unknown_approval_note_for_a_denial() {
    let mut tc = base_tool_call();
    tc.is_error = true;
    // WHY not a real denial string: an approval-gate note that happens to
    // survive on an executed-and-failed call must not be misread as a
    // policy denial (#4558) — only `execute::is_denial_outcome`'s closed
    // vocabulary takes precedence over is_error/outcome_detail.
    tc.approval = Some("auto_approved".to_owned());
    assert_eq!(tc.outcome_label(), "error");
}

#[test]
fn check_guard_allows_below_session_token_cap() {
    let config = crate::config::NousConfig::default();
    let session = crate::session::SessionState::new("s-1".to_owned(), "main".to_owned(), &config);
    assert_eq!(check_guard(&session, &config), GuardResult::Allow);
}

#[test]
fn check_guard_rejects_at_session_token_cap() {
    let mut config = crate::config::NousConfig::default();
    config.limits.session_token_cap = 10;
    let mut session =
        crate::session::SessionState::new("s-1".to_owned(), "main".to_owned(), &config);
    session.cumulative_tokens = 10;

    assert!(matches!(
        check_guard(&session, &config),
        GuardResult::Rejected { reason } if reason.contains("token budget exhausted")
    ));
}

#[test]
fn turn_usage_cache_tokens_not_counted_in_total() {
    let usage = TurnUsage {
        input_tokens: 100,
        output_tokens: 50,
        cache_read_tokens: 80,
        cache_write_tokens: 20,
        llm_calls: 1,
    };
    assert_eq!(
        usage.total_tokens(),
        150,
        "cache tokens should not be in total"
    );
}

/// What a `before_compact` hook was handed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BeforeRecord {
    messages_before: usize,
    tokens_before: u64,
}

/// What an `after_compact` hook was handed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AfterRecord {
    messages_distilled: usize,
    messages_before: usize,
    messages_after: usize,
    tokens_before: u64,
    tokens_after: u64,
    full_compaction_triggered: bool,
}

/// Records the compaction contexts the pipeline actually emitted.
///
/// WHY(#5025): the regression class is placeholder telemetry — a field
/// carrying a plausible number rather than the quantity its name promises.
/// Capturing both contexts lets the assertions compare the pair against each
/// other and against observed message counts, rather than against a constant
/// that a placeholder could also satisfy.
#[derive(Debug, Default)]
struct CompactionProbe {
    before: std::sync::Mutex<Option<BeforeRecord>>,
    after: std::sync::Mutex<Option<AfterRecord>>,
}

type HookFuture<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = crate::hooks::HookResult> + Send + 'a>>;

impl crate::hooks::TurnHook for Arc<CompactionProbe> {
    fn name(&self) -> &'static str {
        "compaction_probe"
    }

    fn before_compact<'a>(
        &'a self,
        context: &'a crate::hooks::BeforeCompactionContext<'_>,
    ) -> HookFuture<'a> {
        let record = BeforeRecord {
            messages_before: context.messages_before,
            tokens_before: context.tokens_before,
        };
        Box::pin(async move {
            *self.before.lock().expect("before lock") = Some(record);
            crate::hooks::HookResult::Continue
        })
    }

    fn after_compact<'a>(
        &'a self,
        context: &'a crate::hooks::AfterCompactionContext<'_>,
    ) -> HookFuture<'a> {
        let record = AfterRecord {
            messages_distilled: context.messages_distilled,
            messages_before: context.messages_before,
            messages_after: context.messages_after,
            tokens_before: context.tokens_before,
            tokens_after: context.tokens_after,
            full_compaction_triggered: context.full_compaction_triggered,
        };
        Box::pin(async move {
            *self.after.lock().expect("after lock") = Some(record);
            crate::hooks::HookResult::Continue
        })
    }
}

#[tokio::test]
async fn compaction_hooks_receive_observed_state_not_placeholders() {
    let harness = PipelineHarness::new();
    let probe = Arc::new(CompactionProbe::default());
    let mut hooks = crate::hooks::registry::HookRegistry::new();
    hooks.register(0, Box::new(Arc::clone(&probe)));

    harness
        .run("Hello", Some(&hooks))
        .await
        .expect("pipeline should succeed");

    let before = probe
        .before
        .lock()
        .expect("before lock")
        .expect("before_compact hook should have fired");
    let after = probe
        .after
        .lock()
        .expect("after lock")
        .expect("after_compact hook should have fired");

    assert_eq!(
        after.messages_before, before.messages_before,
        "the pre-compaction snapshot must be the same value in both contexts; \
         a differing figure means one side re-derived it after the fact"
    );
    assert_eq!(
        after.tokens_before, before.tokens_before,
        "tokens_before must be carried from the before-compaction snapshot, not recomputed"
    );

    // WHY(#5025): this is the assertion that fails on the pre-fix code. The old
    // after_compact site set `messages_distilled: ctx.messages.len()`, i.e. the
    // messages *remaining*. On this turn nothing is removed, so the truthful
    // delta is 0 while the old code reported the full message count.
    assert_eq!(
        after.messages_distilled,
        before.messages_before.saturating_sub(after.messages_after),
        "messages_distilled must be the removal delta the field name promises, \
         not the total on either side of the pass"
    );
    assert_eq!(
        after.messages_after, before.messages_before,
        "no compaction is expected on a single short turn, so the count should be unchanged"
    );
    assert_eq!(
        after.messages_distilled, 0,
        "nothing was removed, so no messages were distilled"
    );
    assert_eq!(
        after.tokens_after, before.tokens_before,
        "nothing was compacted, so the token estimate should be unchanged"
    );
    assert!(
        !after.full_compaction_triggered,
        "a single short turn must not report full compaction"
    );
}
