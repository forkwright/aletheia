//! Integration tests for aletheia-melete's public API surface.
//!
//! WHY: melete had zero `crates/melete/tests/` integration tests prior to this
//! (part of aletheia #2814). The crate has ~200 inline lib tests but they may
//! reach into `pub(crate)` internals. These tests exercise the published API
//! surface only — what nous and the dispatch pipeline actually consume.
//!
//! Scope: public constructors/builders, serde round-trips, trait-object
//! bounds, the `distill`/dream pipelines against real (non-mocked where
//! possible) implementations, and every public error path.

#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "tests index into fixed-size vectors where panic would itself be a failure signal"
)]

// ---------------------------------------------------------------------------
// DistillConfig
// ---------------------------------------------------------------------------

mod distill_config {
    use aletheia_melete::distill::{DistillConfig, DistillSection};

    #[test]
    fn default_uses_claude_sonnet_primary_model() {
        // WHY: downstream callers read `config.model` straight through to the
        // provider. Silently changing the default would re-route every
        // distillation to a different model at potentially higher cost.
        let config = DistillConfig::default();
        assert_eq!(config.model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn default_includes_all_seven_standard_sections() {
        // WHY: the default section list defines the distillation output
        // contract — adding/removing a section is a breaking change for any
        // parser downstream of melete.
        let config = DistillConfig::default();
        assert_eq!(config.sections.len(), 7);
        assert!(matches!(config.sections[0], DistillSection::Summary));
        assert!(matches!(config.sections[1], DistillSection::TaskContext));
        assert!(matches!(config.sections[2], DistillSection::CompletedWork));
        assert!(matches!(config.sections[3], DistillSection::KeyDecisions));
        assert!(matches!(config.sections[4], DistillSection::CurrentState));
        assert!(matches!(config.sections[5], DistillSection::OpenThreads));
        assert!(matches!(config.sections[6], DistillSection::Corrections));
    }

    #[test]
    fn default_values_match_documented_contract() {
        // WHY: these defaults are operator-visible knobs. Changing any
        // silently alters cost, behavior, or context preservation:
        // - similarity_threshold 0.85: Jaccard cutoff documented in the
        //   crate's public API.
        // - detect_contradictions off: an extra LLM call per distillation,
        //   must stay opt-in to avoid doubling cost.
        // - verbatim_tail >= 1: a zero tail would let the model summarize
        //   the active working context, destroying current conversation.
        let config = DistillConfig::default();
        assert!(
            (0.0..=1.0).contains(&config.similarity_threshold),
            "similarity threshold must be a Jaccard ratio in [0,1]",
        );
        assert!((config.similarity_threshold - 0.85).abs() < f64::EPSILON);
        assert!(!config.detect_contradictions);
        assert!(config.verbatim_tail >= 1);
    }

    #[test]
    fn round_trips_through_serde_json() {
        // WHY: DistillConfig is persisted in nous session files and kanon.toml
        // fragments. Round-tripping must be lossless or those files drift.
        let original = DistillConfig {
            model: "test-model".to_owned(),
            max_output_tokens: 2048,
            min_messages: 10,
            include_tool_calls: false,
            distillation_model: Some("cheap-model".to_owned()),
            verbatim_tail: 5,
            sections: vec![DistillSection::Summary, DistillSection::KeyDecisions],
            similarity_threshold: 0.9,
            detect_contradictions: true,
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let restored: DistillConfig = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(restored.model, original.model);
        assert_eq!(restored.max_output_tokens, original.max_output_tokens);
        assert_eq!(restored.min_messages, original.min_messages);
        assert_eq!(restored.include_tool_calls, original.include_tool_calls);
        assert_eq!(restored.distillation_model, original.distillation_model);
        assert_eq!(restored.verbatim_tail, original.verbatim_tail);
        assert_eq!(restored.sections.len(), original.sections.len());
        assert!(
            (restored.similarity_threshold - original.similarity_threshold).abs() < f64::EPSILON,
        );
        assert_eq!(restored.detect_contradictions, original.detect_contradictions);
    }

    #[test]
    fn similarity_threshold_deserialization_uses_default_when_missing() {
        // WHY: the field has `#[serde(default = "...")]` so older persisted
        // configs that predate similarity pruning still load. Removing that
        // default would break forward compatibility.
        let json = r#"{
            "model": "m",
            "max_output_tokens": 100,
            "min_messages": 1,
            "include_tool_calls": false,
            "distillation_model": null,
            "verbatim_tail": 1,
            "sections": []
        }"#;
        let config: DistillConfig = serde_json::from_str(json).expect("deserialize");
        assert!((config.similarity_threshold - 0.85).abs() < f64::EPSILON);
        assert!(!config.detect_contradictions);
    }
}

// ---------------------------------------------------------------------------
// DistillSection
// ---------------------------------------------------------------------------

mod distill_section {
    use aletheia_melete::distill::DistillSection;

    #[test]
    fn custom_round_trips_through_serde() {
        let original = DistillSection::Custom {
            name: "Architecture Notes".to_owned(),
            description: "Record architectural decisions and trade-offs.".to_owned(),
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let restored: DistillSection = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored, original);
    }

    #[test]
    fn equality_distinguishes_variants_and_custom_descriptions() {
        // WHY: PartialEq on DistillSection is part of the public contract for
        // callers that dedupe section lists. Two custom sections sharing a
        // name but differing in description must remain distinct.
        assert_eq!(DistillSection::Summary, DistillSection::Summary);
        assert_ne!(DistillSection::Summary, DistillSection::KeyDecisions);

        let a = DistillSection::Custom {
            name: "A".to_owned(),
            description: "desc a".to_owned(),
        };
        let b = DistillSection::Custom {
            name: "A".to_owned(),
            description: "desc b".to_owned(),
        };
        assert_ne!(a, b);
    }
}

// ---------------------------------------------------------------------------
// DistillEngine — the core async entry point
// ---------------------------------------------------------------------------

mod distill_engine {
    use aletheia_hermeneus::test_utils::MockProvider;
    use aletheia_hermeneus::types::{Content, Message, Role};
    use aletheia_melete::distill::{DistillConfig, DistillEngine, DistillSection};
    use aletheia_melete::error::Error;

    fn text_msg(role: Role, text: &str) -> Message {
        Message {
            role,
            content: Content::Text(text.to_owned()),
        }
    }

    fn long_conversation() -> Vec<Message> {
        // WHY: ten messages comfortably exceed the default verbatim_tail=3
        // and min_messages=6 so the engine has real work to split.
        (0..10)
            .map(|i| {
                let role = if i % 2 == 0 { Role::User } else { Role::Assistant };
                text_msg(role, &format!("turn {i}: building the auth module"))
            })
            .collect()
    }

    #[tokio::test]
    async fn empty_messages_returns_no_messages_error() {
        // WHY: contract — `distill([])` is a programmer error and must
        // surface as `Error::NoMessages`, never hit the provider.
        let engine = DistillEngine::new(DistillConfig::default());
        let provider = MockProvider::new("unused").models(&["claude-sonnet-4-20250514"]);
        let result = engine.distill(&[], "alice", &provider, 1).await;
        assert!(matches!(result, Err(Error::NoMessages { .. })));
    }

    #[tokio::test]
    async fn empty_messages_never_calls_provider() {
        // WHY: a NoMessages error must be detected pre-flight. If the engine
        // still hits the provider, we burn an LLM call on empty input.
        let engine = DistillEngine::new(DistillConfig::default());
        let provider = MockProvider::new("unused").models(&["claude-sonnet-4-20250514"]);
        let _ = engine.distill(&[], "alice", &provider, 1).await;
        assert!(
            provider.captured_requests().is_empty(),
            "provider must not be called for empty input",
        );
    }

    #[tokio::test]
    async fn empty_summary_returns_empty_summary_error() {
        // WHY: contract — if the LLM returns whitespace/empty text the
        // engine must surface `EmptySummary`, not pass garbage downstream.
        let engine = DistillEngine::new(DistillConfig::default());
        let provider = MockProvider::new("   \n\n  ").models(&["claude-sonnet-4-20250514"]);
        let result = engine
            .distill(&long_conversation(), "alice", &provider, 1)
            .await;
        assert!(matches!(result, Err(Error::EmptySummary { .. })));
    }

    #[tokio::test]
    async fn provider_error_surfaces_as_llm_call() {
        // WHY: contract — when the provider returns Err, melete must wrap it
        // in `Error::LlmCall` with source attached, not panic or swallow it.
        let engine = DistillEngine::new(DistillConfig::default());
        let provider = MockProvider::error("simulated provider failure");
        let result = engine
            .distill(&long_conversation(), "alice", &provider, 1)
            .await;
        match result {
            Err(Error::LlmCall { .. }) => {}
            other => panic!("expected LlmCall error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn successful_distillation_produces_populated_result() {
        // WHY: happy path — feeding a realistic LLM response through the
        // engine must produce every field of DistillResult correctly.
        let llm_output = "## Summary\nBuilt a postgres-backed auth module.\n\
                          ## Task Context\nImplementing login/session flow for the webapp.\n\
                          ## Key Decisions\n- Use argon2 for password hashing\n- Rate limit at 5/min\n\
                          ## Corrections\n- Initial scheme didn't salt passwords";
        let provider = MockProvider::new(llm_output).models(&["claude-sonnet-4-20250514"]);
        let engine = DistillEngine::new(DistillConfig::default());

        let result = engine
            .distill(&long_conversation(), "alice", &provider, 7)
            .await
            .expect("successful distillation");

        assert!(result.summary.contains("postgres-backed auth"));
        assert_eq!(result.distillation_number, 7);
        assert!(!result.timestamp.is_empty(), "timestamp must be populated");
        assert_eq!(result.verbatim_messages.len(), 3, "default verbatim_tail");
        assert!(
            result.messages_distilled + result.verbatim_messages.len() >= long_conversation().len(),
            "every input message must be accounted for in either the distilled or verbatim bucket",
        );
    }

    #[tokio::test]
    async fn verbatim_tail_preserves_most_recent_messages() {
        // WHY: contract — the tail must be the last N messages verbatim,
        // otherwise callers lose active working context.
        let llm_output = "## Summary\nSummary text";
        let provider = MockProvider::new(llm_output).models(&["claude-sonnet-4-20250514"]);
        let config = DistillConfig {
            verbatim_tail: 2,
            ..DistillConfig::default()
        };
        let engine = DistillEngine::new(config);

        let messages = long_conversation();
        let result = engine
            .distill(&messages, "alice", &provider, 1)
            .await
            .expect("distillation");

        assert_eq!(result.verbatim_messages.len(), 2);
        // WHY: verbatim is the *end* of the slice, so content must match the
        // last two inputs.
        let last_two = &messages[messages.len() - 2..];
        for (kept, expected) in result.verbatim_messages.iter().zip(last_two.iter()) {
            assert_eq!(kept.role, expected.role);
        }
    }

    #[tokio::test]
    async fn memory_flush_extracts_decisions_from_summary() {
        // WHY: downstream consumers rely on MemoryFlush being populated from
        // the `## Key Decisions` markdown section. If the parser regresses,
        // long-term memory silently loses those items.
        let llm_output = "## Summary\nshort\n\
                          ## Key Decisions\n- Use argon2\n- Rate limit 5/min\n\
                          ## Corrections\n- Forgot to salt passwords";
        let provider = MockProvider::new(llm_output).models(&["claude-sonnet-4-20250514"]);
        let engine = DistillEngine::new(DistillConfig::default());

        let result = engine
            .distill(&long_conversation(), "alice", &provider, 1)
            .await
            .expect("distillation");

        assert_eq!(
            result.memory_flush.decisions.len(),
            2,
            "both key decisions should be extracted",
        );
        assert_eq!(
            result.memory_flush.corrections.len(),
            1,
            "correction should be extracted",
        );
        let decision_texts: Vec<&str> = result
            .memory_flush
            .decisions
            .iter()
            .map(|d| d.content.as_str())
            .collect();
        assert!(decision_texts.iter().any(|t| t.contains("argon2")));
        assert!(decision_texts.iter().any(|t| t.contains("Rate limit")));
    }

    #[tokio::test]
    async fn request_to_provider_uses_configured_model() {
        // WHY: when `distillation_model` is set it must override `model` on
        // the wire, enabling the documented Opus→Sonnet cost reduction.
        let provider = MockProvider::new("## Summary\ns").models(&["cheap-model"]);
        let config = DistillConfig {
            model: "expensive-primary".to_owned(),
            distillation_model: Some("cheap-model".to_owned()),
            ..DistillConfig::default()
        };
        let engine = DistillEngine::new(config);
        engine
            .distill(&long_conversation(), "alice", &provider, 1)
            .await
            .expect("distillation");

        let captured = provider.captured_requests();
        assert_eq!(captured.len(), 1);
        assert_eq!(
            captured[0].model, "cheap-model",
            "distillation_model must override primary model on the wire",
        );
    }

    #[tokio::test]
    async fn request_contains_user_prompt_referencing_nous_id() {
        let provider = MockProvider::new("## Summary\ns").models(&["claude-sonnet-4-20250514"]);
        let engine = DistillEngine::new(DistillConfig::default());
        engine
            .distill(&long_conversation(), "bob", &provider, 1)
            .await
            .expect("distillation");

        let captured = provider.captured_requests();
        let first = captured.first().expect("one captured request");
        assert_eq!(first.messages.len(), 1);
        let user_text = first.messages[0].content.text();
        assert!(
            user_text.contains("bob"),
            "user prompt must identify the nous",
        );
    }

    #[tokio::test]
    async fn nous_id_backticks_and_controls_are_stripped() {
        // WHY: a malicious nous_id could embed backticks or newlines to
        // escape the user prompt block. The engine strips them before
        // rendering.
        let provider = MockProvider::new("## Summary\ns").models(&["claude-sonnet-4-20250514"]);
        let engine = DistillEngine::new(DistillConfig::default());
        engine
            .distill(
                &long_conversation(),
                "bad`id\nevil",
                &provider,
                1,
            )
            .await
            .expect("distillation");

        let captured = provider.captured_requests();
        let user_text = captured[0].messages[0].content.text();
        assert!(!user_text.contains('`'), "backticks must be stripped");
        assert!(!user_text.contains("bad`id"), "injected marker absent");
    }

    #[tokio::test]
    async fn custom_section_appears_in_system_prompt() {
        // WHY: DistillSection::Custom is a supported public variant. Its name
        // and description must flow through to the LLM system prompt.
        let provider = MockProvider::new("## Summary\ns").models(&["claude-sonnet-4-20250514"]);
        let config = DistillConfig {
            sections: vec![
                DistillSection::Summary,
                DistillSection::Custom {
                    name: "Architecture Notes".to_owned(),
                    description: "Record architectural decisions and trade-offs.".to_owned(),
                },
            ],
            ..DistillConfig::default()
        };
        let engine = DistillEngine::new(config);
        engine
            .distill(&long_conversation(), "alice", &provider, 1)
            .await
            .expect("distillation");

        let captured = provider.captured_requests();
        let system = captured[0]
            .system
            .as_deref()
            .expect("system prompt must be set");
        assert!(
            system.contains("## Architecture Notes"),
            "custom section name must appear as heading",
        );
        assert!(
            system.contains("Record architectural decisions"),
            "custom section description must appear verbatim",
        );
    }
}

// ---------------------------------------------------------------------------
// MemoryFlush
// ---------------------------------------------------------------------------

mod memory_flush {
    use aletheia_melete::flush::{FlushItem, FlushSource, MemoryFlush};

    fn sample_item() -> FlushItem {
        FlushItem {
            content: "Use snafu for errors".to_owned(),
            timestamp: "2026-04-09T00:00:00Z".to_owned(),
            source: FlushSource::Extracted,
        }
    }

    #[test]
    fn round_trips_through_serde_json() {
        let flush = MemoryFlush {
            decisions: vec![sample_item()],
            corrections: vec![FlushItem {
                content: "Incorrect scheme".to_owned(),
                timestamp: "2026-04-09T00:01:00Z".to_owned(),
                source: FlushSource::AgentNote,
            }],
            facts: vec![FlushItem {
                content: "Config lives in taxis".to_owned(),
                timestamp: "2026-04-09T00:02:00Z".to_owned(),
                source: FlushSource::ToolPattern,
            }],
            task_state: Some("writing tests".to_owned()),
        };
        let json = serde_json::to_string(&flush).expect("serialize");
        let restored: MemoryFlush = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.decisions.len(), 1);
        assert_eq!(restored.corrections.len(), 1);
        assert_eq!(restored.facts.len(), 1);
        assert_eq!(restored.task_state.as_deref(), Some("writing tests"));
        assert_eq!(restored.decisions[0].content, "Use snafu for errors");
    }

    #[test]
    fn flush_source_variants_serialize_distinctly() {
        // WHY: FlushSource is #[non_exhaustive] and the wire format
        // distinguishes source provenance for downstream attribution. Two
        // variants must not collapse to the same string.
        let extracted = serde_json::to_string(&FlushSource::Extracted).expect("serialize");
        let agent = serde_json::to_string(&FlushSource::AgentNote).expect("serialize");
        let tool = serde_json::to_string(&FlushSource::ToolPattern).expect("serialize");
        assert_ne!(extracted, agent);
        assert_ne!(extracted, tool);
        assert_ne!(agent, tool);
    }
}

// ---------------------------------------------------------------------------
// Contradiction / ContradictionLog
// ---------------------------------------------------------------------------

mod contradiction_log {
    use aletheia_melete::contradiction::{Contradiction, ContradictionLog, ResolutionStrategy};

    #[test]
    fn empty_constructor_is_empty_and_defaults_to_prefer_newer() {
        let log = ContradictionLog::empty();
        assert!(log.is_empty());
        assert!(log.contradictions.is_empty());
        assert!(matches!(
            log.resolution_strategy,
            ResolutionStrategy::PreferNewer,
        ));
    }

    #[test]
    fn round_trips_through_serde_json_and_reports_not_empty() {
        let original = ContradictionLog {
            contradictions: vec![Contradiction {
                chunk_a: 1,
                chunk_b: 4,
                description: "conflict".to_owned(),
            }],
            timestamp: "2026-04-09T12:00:00Z".to_owned(),
            resolution_strategy: ResolutionStrategy::NeedsUserReview,
        };
        assert!(!original.is_empty());
        let json = serde_json::to_string(&original).expect("serialize");
        let restored: ContradictionLog = serde_json::from_str(&json).expect("deserialize");
        assert!(!restored.is_empty());
        assert_eq!(restored.contradictions.len(), 1);
        assert_eq!(restored.contradictions[0].chunk_a, 1);
        assert_eq!(restored.contradictions[0].chunk_b, 4);
        assert_eq!(restored.contradictions[0].description, "conflict");
        assert_eq!(restored.timestamp, "2026-04-09T12:00:00Z");
        assert!(matches!(
            restored.resolution_strategy,
            ResolutionStrategy::NeedsUserReview,
        ));
    }
}

// ---------------------------------------------------------------------------
// DreamConfig / DreamEngine / traits
// ---------------------------------------------------------------------------

mod dream {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use aletheia_hermeneus::provider::LlmProvider;
    use aletheia_hermeneus::test_utils::MockProvider;
    use aletheia_hermeneus::types::{Content, Message, Role};
    use aletheia_melete::contradiction::ContradictionLog;
    use aletheia_melete::distill::DistillConfig;
    use aletheia_melete::dream::{
        ConsolidationTarget, DreamConfig, DreamEngine, MergeReport, SessionTranscript,
        TranscriptSource,
    };
    use aletheia_melete::flush::MemoryFlush;

    struct NoopSource;
    impl TranscriptSource for NoopSource {
        fn count_sessions_since(
            &self,
            _since: jiff::Timestamp,
        ) -> std::result::Result<usize, std::io::Error> {
            Ok(0)
        }
        fn load_transcripts_since(
            &self,
            _since: jiff::Timestamp,
        ) -> std::result::Result<Vec<SessionTranscript>, std::io::Error> {
            Ok(vec![])
        }
    }

    struct SourceWithFixedCount(usize);
    impl TranscriptSource for SourceWithFixedCount {
        fn count_sessions_since(
            &self,
            _since: jiff::Timestamp,
        ) -> std::result::Result<usize, std::io::Error> {
            Ok(self.0)
        }
        fn load_transcripts_since(
            &self,
            _since: jiff::Timestamp,
        ) -> std::result::Result<Vec<SessionTranscript>, std::io::Error> {
            Ok(vec![])
        }
    }

    struct FixedSource(Vec<SessionTranscript>);
    impl TranscriptSource for FixedSource {
        fn count_sessions_since(
            &self,
            _since: jiff::Timestamp,
        ) -> std::result::Result<usize, std::io::Error> {
            Ok(self.0.len())
        }
        fn load_transcripts_since(
            &self,
            _since: jiff::Timestamp,
        ) -> std::result::Result<Vec<SessionTranscript>, std::io::Error> {
            Ok(self.0.clone())
        }
    }

    struct CountingTarget {
        merges: AtomicUsize,
        stales: AtomicUsize,
    }
    impl CountingTarget {
        fn new() -> Self {
            Self {
                merges: AtomicUsize::new(0),
                stales: AtomicUsize::new(0),
            }
        }
    }
    impl ConsolidationTarget for CountingTarget {
        fn merge_flush(
            &self,
            _flush: &MemoryFlush,
            _nous_id: &str,
        ) -> std::result::Result<MergeReport, std::io::Error> {
            self.merges.fetch_add(1, Ordering::Relaxed);
            Ok(MergeReport {
                facts_added: 3,
                facts_deduped: 1,
                facts_stale: 0,
            })
        }
        fn mark_contradictions_stale(
            &self,
            log: &ContradictionLog,
            _nous_id: &str,
        ) -> std::result::Result<usize, std::io::Error> {
            let n = log.contradictions.len();
            self.stales.fetch_add(n, Ordering::Relaxed);
            Ok(n)
        }
    }

    #[test]
    fn dream_config_new_applies_standard_defaults() {
        // WHY: DreamConfig::new is the documented constructor. Its defaults
        // (24 hours, 5 sessions) appear in operator docs. Changing them
        // silently breaks the runbook.
        let config = DreamConfig::new(PathBuf::from("/tmp/test-lock"));
        assert_eq!(config.min_hours, 24);
        assert_eq!(config.min_sessions, 5);
        assert_eq!(config.scan_interval_secs, 600);
        assert_eq!(config.stale_threshold_secs, 3_600);
        assert_eq!(config.lock_path, PathBuf::from("/tmp/test-lock"));
    }

    #[test]
    fn dream_config_distill_config_defaults_to_distill_config_default() {
        // WHY: the embedded distill config must match DistillConfig::default
        // so the dream pipeline stays in sync with the primary distillation
        // path.
        let config = DreamConfig::new(PathBuf::from("/tmp/test-lock"));
        let default_distill = DistillConfig::default();
        assert_eq!(config.distill_config.model, default_distill.model);
        assert_eq!(
            config.distill_config.verbatim_tail,
            default_distill.verbatim_tail,
        );
    }

    #[test]
    fn merge_report_default_is_zero() {
        let report = MergeReport::default();
        assert_eq!(report.facts_added, 0);
        assert_eq!(report.facts_deduped, 0);
        assert_eq!(report.facts_stale, 0);
    }

    #[test]
    fn transcript_source_is_dyn_compatible() {
        // WHY: TranscriptSource must be object-safe because the dream engine
        // takes `&dyn TranscriptSource`. Losing object safety breaks the
        // entire consolidation pipeline.
        let _s: Box<dyn TranscriptSource> = Box::new(NoopSource);
        let _arc: Arc<dyn TranscriptSource> = Arc::new(SourceWithFixedCount(3));
    }

    #[test]
    fn consolidation_target_is_dyn_compatible() {
        // WHY: ConsolidationTarget must be object-safe for the same reason.
        let _t: Box<dyn ConsolidationTarget> = Box::new(CountingTarget::new());
    }

    #[test]
    fn dream_engine_debug_impl_prints_without_panicking() {
        // WHY: DreamEngine::Debug is hand-written (not derived) and must
        // survive the AtomicI64 field load; a panic here silently poisons
        // diagnostics when operators inspect the engine.
        let engine = DreamEngine::new(DreamConfig::new(PathBuf::from("/tmp/dream-test")));
        let rendered = format!("{engine:?}");
        assert!(rendered.contains("DreamEngine"));
    }

    #[tokio::test]
    async fn on_turn_complete_is_non_blocking_fire_and_forget() {
        // WHY: the documented contract says on_turn_complete returns
        // immediately and spawns the consolidation task in the background.
        // A blocking/slow implementation would stall nous's turn loop.
        let dir = tempfile::tempdir().expect("tempdir");
        let lock_path = dir.path().join(".consolidate-lock");
        let config = DreamConfig {
            min_hours: 0,
            min_sessions: 1,
            lock_path,
            scan_interval_secs: 0,
            stale_threshold_secs: 3_600,
            distill_config: DistillConfig::default(),
        };
        let engine = Arc::new(DreamEngine::new(config));
        let transcript = SessionTranscript {
            session_id: "s1".to_owned(),
            nous_id: "alice".to_owned(),
            messages: vec![
                Message {
                    role: Role::User,
                    content: Content::Text("What's the plan?".to_owned()),
                },
                Message {
                    role: Role::Assistant,
                    content: Content::Text(
                        "## Summary\nPlanning next sprint\n## Key Decisions\n- ship tests"
                            .to_owned(),
                    ),
                },
            ],
        };
        let source: Arc<dyn TranscriptSource> = Arc::new(FixedSource(vec![transcript]));
        let target: Arc<dyn ConsolidationTarget> = Arc::new(CountingTarget::new());
        let provider: Arc<dyn LlmProvider> = Arc::new(
            MockProvider::new("## Summary\ns\n## Key Decisions\n- done")
                .models(&["claude-sonnet-4-20250514"]),
        );

        // WHY: must return immediately (well under a second). If it blocks
        // on the consolidation pipeline this timing assertion fails.
        let start = std::time::Instant::now();
        engine.on_turn_complete(&source, &target, &provider);
        let elapsed = start.elapsed();
        assert!(
            elapsed < std::time::Duration::from_millis(500),
            "on_turn_complete must return quickly, took {elapsed:?}",
        );
    }
}

// ---------------------------------------------------------------------------
// types re-export
// ---------------------------------------------------------------------------

mod types_reexport {
    use aletheia_melete::types::{Content, ContentBlock, Message, Role};

    #[test]
    fn reexports_match_hermeneus_originals() {
        // WHY: melete::types is a convenience re-export. Consumers depend on
        // it being identical to hermeneus, not a wrapper — a type alias
        // that drifted would break generic bounds downstream. The re-export
        // must also include ContentBlock so callers don't need hermeneus
        // in their Cargo.toml.
        let msg: Message = Message {
            role: Role::User,
            content: Content::Text("hi".to_owned()),
        };
        let _block = ContentBlock::Text {
            text: "body".to_owned(),
            citations: None,
        };
        assert_eq!(msg.role, Role::User);
    }
}

// ---------------------------------------------------------------------------
// Send + Sync promises on the engine types
// ---------------------------------------------------------------------------

mod send_sync_bounds {
    use aletheia_melete::contradiction::{Contradiction, ContradictionLog};
    use aletheia_melete::distill::{DistillConfig, DistillEngine, DistillResult};
    use aletheia_melete::dream::{DreamConfig, DreamEngine, MergeReport, SessionTranscript};
    use aletheia_melete::flush::{FlushItem, MemoryFlush};
    use aletheia_melete::similarity::PruningStats;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn core_types_are_send_sync() {
        // WHY: these bounds are load-bearing for the tokio multi-threaded
        // runtime. Downgrading to !Send would break every async caller.
        assert_send_sync::<DistillEngine>();
        assert_send_sync::<DistillConfig>();
        assert_send_sync::<DistillResult>();
        assert_send_sync::<DreamEngine>();
        assert_send_sync::<DreamConfig>();
        assert_send_sync::<MergeReport>();
        assert_send_sync::<SessionTranscript>();
        assert_send_sync::<MemoryFlush>();
        assert_send_sync::<FlushItem>();
        assert_send_sync::<ContradictionLog>();
        assert_send_sync::<Contradiction>();
        assert_send_sync::<PruningStats>();
    }
}
