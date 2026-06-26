#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::type_complexity,
    clippy::unnecessary_literal_bound,
    reason = "test helpers and assertions may use concise panic-oriented shapes"
)]

use std::collections::HashSet;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hermeneus::provider::{LlmProvider, ProviderRegistry};
use hermeneus::test_utils::MockProvider;
use hermeneus::types::{CompletionRequest, CompletionResponse, ContentBlock, StopReason, Usage};
use koina::event::EventEmitter;
use koina::id::{NousId, SessionId, ToolName};
use mneme::id::FactId;
use mneme::knowledge::{
    EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactSensitivity, FactTemporal,
    Visibility, far_future, parse_timestamp,
};
use mneme::knowledge_store::KnowledgeStore;
use mneme::side_query::SideQueryRanker;
use mneme::store::SessionStore;
use organon::registry::{ToolExecutor, ToolRegistry};
use organon::types::{
    InputSchema, Reversibility, ToolCategory, ToolContext, ToolDef, ToolGroupId, ToolInput,
    ToolResult,
};
use tokio::sync::{Mutex as TokioMutex, mpsc};

use super::*;
use crate::budget::{StageTimingStatus, TimeBudget};
use crate::compact::CompactConfig;
use crate::config::{NousConfig, PipelineConfig};
use crate::error;
use crate::pipeline::{DegradedMode, PipelineContext, PipelineInput, ReflectionStatus};
use crate::session::SessionState;
use crate::stream::TurnStreamEvent;

fn make_text_response(text: &str) -> CompletionResponse {
    CompletionResponse {
        id: "resp-text".to_owned(),
        model: "test-model".to_owned(),
        stop_reason: StopReason::EndTurn,
        content: vec![ContentBlock::Text {
            text: text.to_owned(),
            citations: None,
        }],
        usage: Usage {
            input_tokens: 10,
            output_tokens: 5,
            ..Usage::default()
        },
        cost_usd: None,
        duration_ms: None,
    }
}

fn make_tool_response(tool_name: &str, tool_id: &str) -> CompletionResponse {
    CompletionResponse {
        id: "resp-tool".to_owned(),
        model: "test-model".to_owned(),
        stop_reason: StopReason::ToolUse,
        content: vec![ContentBlock::ToolUse {
            id: tool_id.to_owned(),
            name: tool_name.to_owned(),
            input: serde_json::json!({}),
        }],
        usage: Usage {
            input_tokens: 10,
            output_tokens: 5,
            ..Usage::default()
        },
        cost_usd: None,
        duration_ms: None,
    }
}

fn make_side_effect_tool_def(name: &str) -> ToolDef {
    ToolDef {
        name: ToolName::new(name).expect("valid"),
        description: format!("Test tool: {name}"),
        extended_description: None,
        input_schema: InputSchema {
            properties: indexmap::IndexMap::default(),
            required: vec![],
        },
        category: ToolCategory::Workspace,
        // WHY(#4713): PartiallyReversible maps to Required (auto-approved without a gate);
        // Irreversible would map to Mandatory (auto-denied) and the tool would never execute.
        reversibility: Reversibility::PartiallyReversible,
        auto_activate: true,
        groups: vec![ToolGroupId::Read],
        tags: vec![],
    }
}

struct CountingExecutor {
    executions: Arc<AtomicUsize>,
}

impl ToolExecutor for CountingExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = organon::error::Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            self.executions.fetch_add(1, Ordering::SeqCst);
            Ok(ToolResult::text(format!(
                "side effect recorded: {}",
                input.name.as_str()
            )))
        })
    }
}

fn make_msg(role: &str, content: &str) -> PipelineMessage {
    PipelineMessage {
        role: role.to_owned(),
        content: content.to_owned(),
        token_estimate: 0,
        cache_breakpoint: false,
    }
}

fn config_with_preserve(preserve: usize) -> CompactConfig {
    CompactConfig {
        preserve_turns: preserve,
        ..CompactConfig::default()
    }
}

#[test]
fn structural_summary_header_present() {
    let msgs = vec![make_msg("user", "hello")];
    let config = config_with_preserve(0);
    let summary = build_structural_summary(&msgs, &config);
    assert!(summary.starts_with("Previous conversation context:"));
}

#[test]
fn structural_summary_preserves_recent_turns() {
    // preserve_turns=2: only the first 3 messages get summarized
    let msgs = vec![
        make_msg("user", "msg1"),
        make_msg("assistant", "msg2"),
        make_msg("user", "msg3"),
        make_msg("assistant", "msg4"),
        make_msg("user", "msg5"),
    ];
    let config = config_with_preserve(2);
    let summary = build_structural_summary(&msgs, &config);

    assert!(summary.contains("msg1"), "msg1 should be summarized");
    assert!(summary.contains("msg2"), "msg2 should be summarized");
    assert!(summary.contains("msg3"), "msg3 should be summarized");
    assert!(
        !summary.contains("msg4"),
        "msg4 should be preserved (not summarized)"
    );
    assert!(
        !summary.contains("msg5"),
        "msg5 should be preserved (not summarized)"
    );
    assert!(summary.contains("3 messages summarized"));
}

#[test]
fn structural_summary_truncates_long_content() {
    let long_content = "x".repeat(500);
    let msgs = vec![make_msg("user", &long_content)];
    let config = config_with_preserve(0);
    let summary = build_structural_summary(&msgs, &config);

    // Content should be truncated to 200 chars + "..."
    assert!(summary.contains("..."), "should have ellipsis marker");
    // Summary shouldn't contain the full 500-char content
    assert!(
        !summary.contains(&"x".repeat(201)),
        "should not contain 201+ consecutive x's"
    );
}

#[test]
fn structural_summary_no_truncation_for_short_content() {
    let msgs = vec![make_msg("user", "short")];
    let config = config_with_preserve(0);
    let summary = build_structural_summary(&msgs, &config);
    assert!(summary.contains("short"));
}

#[test]
fn structural_summary_empty_messages_zero_count() {
    let msgs: Vec<PipelineMessage> = Vec::new();
    let config = config_with_preserve(3);
    let summary = build_structural_summary(&msgs, &config);
    assert!(summary.contains("0 messages summarized"));
}

#[test]
fn structural_summary_preserve_exceeds_len() {
    // If preserve_turns > messages.len(), everything is preserved and nothing summarized
    let msgs = vec![make_msg("user", "only one")];
    let config = config_with_preserve(10);
    let summary = build_structural_summary(&msgs, &config);
    assert!(summary.contains("0 messages summarized"));
    assert!(!summary.contains("only one"));
}

#[test]
fn structural_summary_includes_role_prefix() {
    let msgs = vec![
        make_msg("user", "question"),
        make_msg("assistant", "answer"),
        make_msg("tool_result", "output"),
    ];
    let config = config_with_preserve(0);
    let summary = build_structural_summary(&msgs, &config);
    assert!(summary.contains("[user]"));
    assert!(summary.contains("[assistant]"));
    assert!(summary.contains("[tool_result]"));
}

#[test]
fn structural_summary_handles_multibyte_content() {
    // Ensure char-based truncation doesn't panic on multibyte characters
    let multibyte = "héllo wörld 🌍 ".repeat(50); // well over 200 chars
    let msgs = vec![make_msg("user", &multibyte)];
    let config = config_with_preserve(0);
    let summary = build_structural_summary(&msgs, &config);
    assert!(summary.contains("héllo"));
    assert!(summary.contains("..."));
}

#[test]
fn structural_summary_preserve_exactly_equals_len() {
    // preserve_turns == len: everything is preserved, nothing summarized
    let msgs = vec![make_msg("user", "one"), make_msg("assistant", "two")];
    let config = config_with_preserve(2);
    let summary = build_structural_summary(&msgs, &config);
    assert!(summary.contains("0 messages summarized"));
}

#[tokio::test]
async fn full_compaction_uses_llm_summary() {
    let mut config = NousConfig::default();
    config.generation.model = "test-model".to_owned();
    config.generation.context_window = 100;
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::new("llm compacted summary").models(&["test-model"]),
    ));
    let mut ctx = PipelineContext {
        messages: vec![
            PipelineMessage {
                role: "user".to_owned(),
                content: "old context".to_owned(),
                token_estimate: 90,
                cache_breakpoint: false,
            },
            PipelineMessage {
                role: "assistant".to_owned(),
                content: "recent".to_owned(),
                token_estimate: 1,
                cache_breakpoint: false,
            },
        ],
        ..PipelineContext::default()
    };
    let (emitter, captured) = capturing_emitter();

    run_full_compact_stage(&config, &mut ctx, &providers, &emitter)
        .await
        .expect("full compaction should complete");

    assert!(
        ctx.messages
            .first()
            .expect("summary message should exist")
            .content
            .contains("llm compacted summary"),
        "full compaction should use provider summary"
    );

    let events = captured.lock().expect("metric lock");
    assert!(
        events.iter().any(|(name, labels)| {
            name == "StageCompleted"
                && labels
                    .iter()
                    .any(|(k, v)| k == "stage" && v == "full_compact")
        }),
        "happy path should emit StageCompleted for full_compact"
    );
    assert!(
        !events.iter().any(|(name, labels)| {
            name == "StageError"
                && labels
                    .iter()
                    .any(|(k, v)| k == "stage" && v == "full_compact")
        }),
        "happy path should not emit StageError for full_compact"
    );
}

#[tokio::test]
async fn full_compaction_falls_back_when_llm_unavailable() {
    let mut config = NousConfig::default();
    config.generation.model = "missing-model".to_owned();
    config.generation.context_window = 100;
    let providers = ProviderRegistry::new();
    let mut ctx = PipelineContext {
        messages: vec![PipelineMessage {
            role: "user".to_owned(),
            content: "old context".to_owned(),
            token_estimate: 90,
            cache_breakpoint: false,
        }],
        ..PipelineContext::default()
    };
    let (emitter, captured) = capturing_emitter();

    run_full_compact_stage(&config, &mut ctx, &providers, &emitter)
        .await
        .expect("structural fallback should keep compaction non-fatal");

    assert!(
        ctx.messages
            .first()
            .expect("summary message should exist")
            .content
            .contains("Previous conversation context:"),
        "fallback should use structural summary"
    );

    let events = captured.lock().expect("metric lock");
    assert!(
        events.iter().any(|(name, labels)| {
            name == "StageError"
                && labels
                    .iter()
                    .any(|(k, v)| k == "stage" && v == "full_compact")
                && labels
                    .iter()
                    .any(|(k, v)| k == "error_type" && v == "llm_fallback")
        }),
        "fallback should emit StageError {{ stage: full_compact, error_type: llm_fallback }}"
    );
    assert!(
        events.iter().any(|(name, labels)| {
            name == "StageCompleted"
                && labels
                    .iter()
                    .any(|(k, v)| k == "stage" && v == "full_compact")
        }),
        "fallback should still emit StageCompleted for full_compact"
    );
}

// --- Reflection stage tests ---

#[tokio::test]
async fn reflection_stage_disabled_skips() {
    let config = NousConfig::default();
    let pipeline_config = PipelineConfig::default();
    let mut ctx = PipelineContext::default();
    let emitter = EventEmitter::new();

    run_reflection_stage(&config, &pipeline_config, &mut ctx, None, None, &emitter)
        .await
        .expect("disabled reflection should not error");

    let result = ctx
        .reflection_result
        .expect("reflection_result should be set");
    assert_eq!(result.status, ReflectionStatus::Disabled);
    assert_eq!(result.facts_emitted, 0);
}

#[tokio::test]
async fn reflection_stage_enabled_no_store() {
    let config = NousConfig::default();
    let pipeline_config = PipelineConfig {
        reflection_enabled: true,
        ..PipelineConfig::default()
    };
    let mut ctx = PipelineContext::default();
    let emitter = EventEmitter::new();

    run_reflection_stage(&config, &pipeline_config, &mut ctx, None, None, &emitter)
        .await
        .expect("enabled reflection without store should not error");

    let result = ctx
        .reflection_result
        .expect("reflection_result should be set");
    assert_eq!(result.status, ReflectionStatus::NoStore);
    assert_eq!(result.facts_emitted, 0);
}

#[tokio::test]
async fn reflection_stage_enabled_with_store_and_no_candidates_skips() {
    let config = NousConfig::default();
    let pipeline_config = PipelineConfig {
        reflection_enabled: true,
        ..PipelineConfig::default()
    };
    let store = KnowledgeStore::open_mem().expect("knowledge store");
    let mut ctx = PipelineContext::default();
    let emitter = EventEmitter::new();

    let verified = reflection_test_fact(
        "verified-fact",
        config.id.as_ref(),
        "Alice has verified account recovery enabled",
        EpistemicTier::Verified,
    );
    store.insert_fact(&verified).expect("seed verified fact");

    run_reflection_stage(
        &config,
        &pipeline_config,
        &mut ctx,
        Some(Arc::clone(&store)),
        Some("session-reflect"),
        &emitter,
    )
    .await
    .expect("reflection with no candidates should not error");

    let result = ctx
        .reflection_result
        .expect("reflection_result should be set");
    assert_eq!(result.status, ReflectionStatus::Skipped);
    assert_eq!(result.facts_emitted, 0);
}

#[tokio::test]
async fn reflection_stage_persists_reflected_facts_idempotently() {
    let config = NousConfig::default();
    let pipeline_config = PipelineConfig {
        reflection_enabled: true,
        ..PipelineConfig::default()
    };
    let store = KnowledgeStore::open_mem().expect("knowledge store");
    let mut ctx = PipelineContext::default();
    let emitter = EventEmitter::new();

    let source = reflection_test_fact(
        "source-fact",
        config.id.as_ref(),
        "Alice prefers concise daily planning notes",
        EpistemicTier::Inferred,
    );
    store.insert_fact(&source).expect("seed source fact");

    run_reflection_stage(
        &config,
        &pipeline_config,
        &mut ctx,
        Some(Arc::clone(&store)),
        Some("session-reflect"),
        &emitter,
    )
    .await
    .expect("reflection should persist promoted fact");

    let result = ctx
        .reflection_result
        .as_ref()
        .expect("reflection_result should be set");
    assert_eq!(result.status, ReflectionStatus::Completed);
    assert_eq!(result.facts_emitted, 1);

    let query_now = mneme::knowledge::format_timestamp(&jiff::Timestamp::now());
    let facts = store
        .query_facts(config.id.as_ref(), &query_now, 10)
        .expect("query reflected facts");
    let reflected: Vec<_> = facts
        .iter()
        .filter(|fact| fact.provenance.tier == EpistemicTier::Reflected)
        .collect();
    assert_eq!(reflected.len(), 1, "one reflected fact should be persisted");
    assert_eq!(reflected[0].content, source.content);
    assert_eq!(
        reflected[0].provenance.source_session_id.as_deref(),
        Some("session-reflect")
    );
    assert_ne!(
        reflected[0].id, source.id,
        "reflection must not overwrite the source fact"
    );

    run_reflection_stage(
        &config,
        &pipeline_config,
        &mut ctx,
        Some(Arc::clone(&store)),
        Some("session-reflect"),
        &emitter,
    )
    .await
    .expect("second reflection pass should upsert same reflected fact");

    let facts_after_second_pass = store
        .query_facts(config.id.as_ref(), &query_now, 10)
        .expect("query after second reflection");
    let reflected_after_second_pass = facts_after_second_pass
        .iter()
        .filter(|fact| fact.provenance.tier == EpistemicTier::Reflected)
        .count();
    assert_eq!(
        reflected_after_second_pass, 1,
        "reflection should be idempotent, not duplicate facts"
    );
}

fn reflection_test_fact(id: &str, nous_id: &str, content: &str, tier: EpistemicTier) -> Fact {
    Fact {
        id: FactId::new(id).expect("valid test fact id"),
        nous_id: nous_id.to_owned(),
        fact_type: "preference".to_owned(),
        content: content.to_owned(),
        scope: None,
        project_id: None,
        sensitivity: FactSensitivity::Public,
        visibility: Visibility::Private,
        temporal: FactTemporal {
            valid_from: parse_timestamp("2026-01-01T00:00:00Z").expect("valid timestamp"),
            valid_to: far_future(),
            recorded_at: parse_timestamp("2026-06-01T00:00:00Z").expect("valid timestamp"),
        },
        provenance: FactProvenance {
            confidence: 0.9,
            tier,
            source_session_id: Some("seed-session".to_owned()),
            stability_hours: 720.0,
        },
        lifecycle: FactLifecycle {
            superseded_by: None,
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        },
        access: FactAccess {
            access_count: 0,
            last_accessed_at: None,
        },
    }
}

#[test]
fn apply_recall_result_injects_into_system_prompt_by_default() {
    let mut ctx = PipelineContext {
        system_prompt: Some("base prompt".to_owned()),
        messages: vec![make_msg("user", "hello")],
        remaining_tokens: 100,
        ..PipelineContext::default()
    };
    let recall = crate::recall::RecallStageResult {
        candidates_found: 1,
        results_injected: 1,
        tokens_consumed: 10,
        recall_section: Some("## Recalled Knowledge\n- fact".to_owned()),
        fact_ids: vec!["f1".to_owned()],
        deployment_target: hermeneus::provider::DeploymentTarget::Cloud,
        filtered_facts: Vec::new(),
    };
    let span = tracing::info_span!("test");
    super::apply_recall_result(
        Ok(recall),
        &mut ctx,
        &span,
        false,
        &EventEmitter::new(),
        "test-nous",
    );
    assert!(
        ctx.system_prompt
            .as_ref()
            .is_some_and(|p| p.contains("Recalled Knowledge")),
        "recall should be appended to system prompt"
    );
    assert_eq!(ctx.messages.len(), 1, "messages should not grow");
    assert_eq!(ctx.remaining_tokens, 90, "tokens should be deducted");
}

#[test]
fn apply_recall_result_late_inject_appends_system_message() {
    let mut ctx = PipelineContext {
        system_prompt: Some("base prompt".to_owned()),
        messages: vec![make_msg("user", "hello")],
        remaining_tokens: 100,
        ..PipelineContext::default()
    };
    let recall = crate::recall::RecallStageResult {
        candidates_found: 1,
        results_injected: 1,
        tokens_consumed: 10,
        recall_section: Some("## Recalled Knowledge\n- fact".to_owned()),
        fact_ids: vec!["f1".to_owned()],
        deployment_target: hermeneus::provider::DeploymentTarget::Cloud,
        filtered_facts: Vec::new(),
    };
    let span = tracing::info_span!("test");
    super::apply_recall_result(
        Ok(recall),
        &mut ctx,
        &span,
        true,
        &EventEmitter::new(),
        "test-nous",
    );
    assert!(
        !ctx.system_prompt
            .as_ref()
            .is_some_and(|p| p.contains("Recalled Knowledge")),
        "recall should NOT be appended to system prompt"
    );
    assert_eq!(ctx.messages.len(), 2, "messages should grow by 1");
    assert!(
        ctx.messages
            .get(1)
            .is_some_and(|m| m.role == "system" && m.content.contains("Recalled Knowledge"))
    );
    assert_eq!(ctx.remaining_tokens, 90, "tokens should be deducted");
}

#[test]
fn apply_recall_result_error_emits_stage_error_metric() {
    let mut ctx = PipelineContext {
        system_prompt: Some("base prompt".to_owned()),
        messages: vec![make_msg("user", "hello")],
        remaining_tokens: 100,
        ..PipelineContext::default()
    };
    let span = tracing::info_span!("test");
    let (emitter, captured) = capturing_emitter();
    let err = error::PipelineStageSnafu {
        stage: "recall".to_owned(),
        message: "injected test failure".to_owned(),
    }
    .build();
    super::apply_recall_result(Err(err), &mut ctx, &span, false, &emitter, "test-nous");

    let events = captured.lock().expect("metric lock");
    assert!(
        events.iter().any(|(name, labels)| {
            name == "StageError"
                && labels.iter().any(|(k, v)| k == "stage" && v == "recall")
                && labels
                    .iter()
                    .any(|(k, v)| k == "error_type" && v == "recall_failed")
                && labels
                    .iter()
                    .any(|(k, v)| k == "nous_id" && v == "test-nous")
        }),
        "recall failure should emit StageError {{ stage: recall, error_type: recall_failed, nous_id: test-nous }}"
    );
}

// --- Execute-stage timeout / degraded-mode tests (#4690) ---

fn execute_stage_config() -> NousConfig {
    NousConfig {
        id: Arc::from("test-agent"),
        generation: crate::config::NousGenerationConfig {
            model: "test-model".to_owned(),
            ..crate::config::NousGenerationConfig::default()
        },
        ..NousConfig::default()
    }
}

fn execute_stage_tool_ctx() -> ToolContext {
    ToolContext {
        nous_id: NousId::new("test-agent").expect("valid"),
        session_id: SessionId::new(),
        turn_number: 0,
        workspace: PathBuf::from("/tmp/test"),
        allowed_roots: vec![PathBuf::from("/tmp")],
        services: None,
        active_tools: Arc::new(std::sync::RwLock::new(HashSet::new())),
        tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
    }
}

fn execute_stage_pipeline_input(
    session: SessionState,
    pipeline_config: &PipelineConfig,
) -> PipelineInput {
    PipelineInput {
        content: "hello".to_owned(),
        session,
        config: pipeline_config.clone(),
    }
}

fn execute_stage_time_budget(pipeline_config: &PipelineConfig) -> TimeBudget {
    TimeBudget::new(pipeline_config.stage_budget.clone())
}

fn capturing_emitter() -> (
    EventEmitter,
    Arc<Mutex<Vec<(String, Vec<(String, String)>)>>>,
) {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let captured_clone = Arc::clone(&captured);
    let emitter = EventEmitter::with_metric_sink(move |name, labels, _value| {
        let labels: Vec<(String, String)> = labels
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        captured_clone
            .lock()
            .expect("metric lock")
            .push((name.to_owned(), labels));
    });
    (emitter, captured)
}

fn has_metric_event(
    events: &[(String, Vec<(String, String)>)],
    event_name: &str,
    error_type: &str,
) -> bool {
    events.iter().any(|(name, labels)| {
        name == event_name
            && labels
                .iter()
                .any(|(k, v)| k == "error_type" && v == error_type)
    })
}

/// Provider that sleeps longer than the execute budget so the stage times out.
struct SleepingProvider {
    sleep: Duration,
}

impl LlmProvider for SleepingProvider {
    fn complete<'a>(
        &'a self,
        _request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = hermeneus::error::Result<CompletionResponse>> + Send + 'a>>
    {
        let sleep = self.sleep;
        Box::pin(async move {
            tokio::time::sleep(sleep).await;
            Ok(CompletionResponse {
                id: "resp-sleep".to_owned(),
                model: "test-model".to_owned(),
                stop_reason: StopReason::EndTurn,
                content: vec![ContentBlock::Text {
                    text: "should never arrive".to_owned(),
                    citations: None,
                }],
                usage: Usage::default(),
                cost_usd: None,
                duration_ms: None,
            })
        })
    }

    fn supported_models(&self) -> &[&str] {
        &["test-model"]
    }

    fn name(&self) -> &str {
        "sleeping"
    }
}

fn sleeping_providers(sleep: Duration) -> ProviderRegistry {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(SleepingProvider { sleep }));
    providers
}

#[tokio::test(start_paused = true)]
async fn execute_timeout_with_distillation_returns_degraded_response() {
    let config = execute_stage_config();
    let mut pipeline_config = PipelineConfig::default();
    pipeline_config.stage_budget.execute_secs = 1;

    let store = SessionStore::open_in_memory().expect("in-memory store");
    store
        .create_session("ses-1", "test-agent", "main", None, None)
        .expect("create session");
    store
        .insert_distillation_summary("ses-1", "cached distillation summary")
        .expect("insert summary");
    let session = SessionState::new("ses-1".to_owned(), "main".to_owned(), &config);
    let input = execute_stage_pipeline_input(session, &pipeline_config);
    let ctx = PipelineContext::default();
    let providers = sleeping_providers(Duration::from_secs(10));
    let tools = ToolRegistry::new();
    let tool_ctx = execute_stage_tool_ctx();
    let mut time_budget = execute_stage_time_budget(&pipeline_config);
    let (emitter, captured) = capturing_emitter();

    let result = run_execute_stage(
        &config,
        &pipeline_config,
        &ctx,
        &input,
        &providers,
        &tools,
        &tool_ctx,
        None,
        None,
        &mut time_budget,
        &emitter,
        None,
        Some(&TokioMutex::new(store)),
    )
    .await
    .expect("degraded response should succeed");

    assert!(
        matches!(
            result.degraded,
            Some(DegradedMode::TurnBudgetExceeded { .. })
        ),
        "timeout should return TurnBudgetExceeded degraded mode, got {:?}",
        result.degraded
    );
    assert!(
        result.stop_reason == "turn_timeout",
        "stop reason should indicate turn timeout, got {:?}",
        result.stop_reason
    );

    let events = captured.lock().expect("metric lock");
    assert!(
        has_metric_event(&events, "StageTimeout", "timeout"),
        "timeout metric should be emitted"
    );
    assert!(
        has_metric_event(&events, "StageError", "turn_timeout"),
        "turn_timeout metric should distinguish a budget-exceeded result"
    );

    let summary = time_budget.summary();
    let execute_record = summary
        .iter()
        .find(|r| r.name == "execute")
        .expect("execute timing record");
    assert_eq!(
        execute_record.status,
        StageTimingStatus::TimedOut,
        "time budget should record the execute stage as timed out"
    );
}

#[tokio::test(start_paused = true)]
async fn execute_timeout_without_distillation_returns_hard_timeout() {
    let config = execute_stage_config();
    let mut pipeline_config = PipelineConfig::default();
    pipeline_config.stage_budget.execute_secs = 1;

    let store = SessionStore::open_in_memory().expect("in-memory store");
    let session = SessionState::new("ses-2".to_owned(), "main".to_owned(), &config);
    let input = execute_stage_pipeline_input(session, &pipeline_config);
    let ctx = PipelineContext::default();
    let providers = sleeping_providers(Duration::from_secs(10));
    let tools = ToolRegistry::new();
    let tool_ctx = execute_stage_tool_ctx();
    let mut time_budget = execute_stage_time_budget(&pipeline_config);
    let (emitter, captured) = capturing_emitter();

    let result = run_execute_stage(
        &config,
        &pipeline_config,
        &ctx,
        &input,
        &providers,
        &tools,
        &tool_ctx,
        None,
        None,
        &mut time_budget,
        &emitter,
        None,
        Some(&TokioMutex::new(store)),
    )
    .await
    .expect("timeout without cache should return a degraded TurnResult");

    assert!(
        matches!(
            result.degraded,
            Some(DegradedMode::TurnBudgetExceeded { .. })
        ),
        "timeout should return TurnBudgetExceeded degraded mode, got {:?}",
        result.degraded
    );
    assert!(
        result.stop_reason == "turn_timeout",
        "stop reason should indicate turn timeout, got {:?}",
        result.stop_reason
    );

    let events = captured.lock().expect("metric lock");
    assert!(
        has_metric_event(&events, "StageTimeout", "timeout"),
        "timeout metric should be emitted"
    );
    assert!(
        has_metric_event(&events, "StageError", "turn_timeout"),
        "turn_timeout metric should distinguish a budget-exceeded result"
    );

    let summary = time_budget.summary();
    let execute_record = summary
        .iter()
        .find(|r| r.name == "execute")
        .expect("execute timing record");
    assert_eq!(
        execute_record.status,
        StageTimingStatus::TimedOut,
        "time budget should record the execute stage as timed out"
    );
}

// --- ProviderRecallBridge side-query ranking tests (#5560) ---

#[tokio::test(flavor = "multi_thread")]
async fn provider_recall_bridge_bounds_rankings_to_manifest_ids() {
    // WHY(#5560): fabricated IDs must be dropped before they can bias recall.
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::new(r#"["fabricated-id", "real-id"]"#).models(&["test-model"]),
    ));
    let bridge = ProviderRecallBridge {
        providers: &providers,
        model: "test-model",
    };

    let manifest_text = "- real-id Project conventions\n- other-id Another entry\n";
    let result = bridge
        .rank_memories("query", manifest_text, 5)
        .expect("rank_memories should succeed");

    assert_eq!(
        result,
        vec!["real-id"],
        "rank_memories should only return IDs present in the manifest"
    );
}

#[tokio::test(start_paused = true)]
async fn execute_timeout_streaming_with_distillation_returns_degraded_response() {
    let config = execute_stage_config();
    let mut pipeline_config = PipelineConfig::default();
    pipeline_config.stage_budget.execute_secs = 1;

    let store = SessionStore::open_in_memory().expect("in-memory store");
    store
        .create_session("ses-3", "test-agent", "main", None, None)
        .expect("create session");
    store
        .insert_distillation_summary("ses-3", "cached distillation summary")
        .expect("insert summary");
    let session = SessionState::new("ses-3".to_owned(), "main".to_owned(), &config);
    let input = execute_stage_pipeline_input(session, &pipeline_config);
    let ctx = PipelineContext::default();
    let providers = sleeping_providers(Duration::from_secs(10));
    let tools = ToolRegistry::new();
    let tool_ctx = execute_stage_tool_ctx();
    let mut time_budget = execute_stage_time_budget(&pipeline_config);
    let (emitter, _captured) = capturing_emitter();
    let (tx, mut rx) = mpsc::channel::<TurnStreamEvent>(16);

    tokio::spawn(async move { while rx.recv().await.is_some() {} });

    let result = run_execute_stage(
        &config,
        &pipeline_config,
        &ctx,
        &input,
        &providers,
        &tools,
        &tool_ctx,
        Some(&tx),
        None,
        &mut time_budget,
        &emitter,
        None,
        Some(&TokioMutex::new(store)),
    )
    .await
    .expect("streaming timeout should degrade");

    assert!(
        matches!(
            result.degraded,
            Some(DegradedMode::TurnBudgetExceeded { .. })
        ),
        "streaming timeout should return TurnBudgetExceeded degraded mode, got {:?}",
        result.degraded
    );
    assert!(
        result.stop_reason == "turn_timeout",
        "stop reason should indicate turn timeout, got {:?}",
        result.stop_reason
    );
}

/// Provider that returns a tool-use response on the first call, then sleeps.
struct ToolThenSleepProvider {
    sleep: Duration,
    calls: AtomicUsize,
}

impl LlmProvider for ToolThenSleepProvider {
    fn complete<'a>(
        &'a self,
        _request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = hermeneus::error::Result<CompletionResponse>> + Send + 'a>>
    {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        let sleep = self.sleep;
        Box::pin(async move {
            if call == 0 {
                Ok(make_tool_response("side_effect_tool", "tu-1"))
            } else {
                tokio::time::sleep(sleep).await;
                Ok(make_text_response("done"))
            }
        })
    }

    fn supported_models(&self) -> &[&str] {
        &["test-model"]
    }

    fn name(&self) -> &str {
        "tool-then-sleep"
    }
}

#[tokio::test(start_paused = true)]
async fn execute_timeout_preserves_side_effecting_tool_results() {
    let mut config = execute_stage_config();
    config.tool_groups = organon::types::ToolGroupPolicy::AllowAll {
        reason: "test policy".to_owned(),
    };
    let mut pipeline_config = PipelineConfig::default();
    pipeline_config.stage_budget.execute_secs = 1;

    let executions = Arc::new(AtomicUsize::new(0));
    let mut tools = ToolRegistry::new();
    tools
        .register(
            make_side_effect_tool_def("side_effect_tool"),
            Box::new(CountingExecutor {
                executions: Arc::clone(&executions),
            }),
        )
        .expect("register tool");

    let session = SessionState::new("ses-side-effect".to_owned(), "main".to_owned(), &config);
    let input = execute_stage_pipeline_input(session, &pipeline_config);
    let ctx = PipelineContext::default();
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(ToolThenSleepProvider {
        sleep: Duration::from_secs(5),
        calls: AtomicUsize::new(0),
    }));
    let tool_ctx = execute_stage_tool_ctx();
    let mut time_budget = execute_stage_time_budget(&pipeline_config);
    let (emitter, captured) = capturing_emitter();

    let result = run_execute_stage(
        &config,
        &pipeline_config,
        &ctx,
        &input,
        &providers,
        &tools,
        &tool_ctx,
        None,
        None,
        &mut time_budget,
        &emitter,
        None,
        None,
    )
    .await
    .expect("cooperative timeout should return a TurnResult");

    assert!(
        matches!(
            result.degraded,
            Some(DegradedMode::TurnBudgetExceeded { .. })
        ),
        "expected TurnBudgetExceeded, got {:?}",
        result.degraded
    );
    assert_eq!(result.stop_reason, "turn_timeout");
    assert_eq!(
        result.tool_calls.len(),
        1,
        "the single tool result observed before the deadline must be preserved"
    );
    assert_eq!(result.tool_calls[0].name, "side_effect_tool");
    assert!(
        result.tool_calls[0]
            .result
            .as_deref()
            .expect("tool result")
            .contains("side effect recorded"),
        "tool result must not be orphaned"
    );
    assert_eq!(
        executions.load(Ordering::SeqCst),
        1,
        "side-effecting tool must have executed exactly once"
    );

    let events = captured.lock().expect("metric lock");
    assert!(has_metric_event(&events, "StageTimeout", "timeout"));
    assert!(has_metric_event(&events, "StageError", "turn_timeout"));

    let summary = time_budget.summary();
    let execute_record = summary
        .iter()
        .find(|r| r.name == "execute")
        .expect("execute timing record");
    assert_eq!(execute_record.status, StageTimingStatus::TimedOut);
}
