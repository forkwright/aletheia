#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "tests index into fixed-size vectors where panic would itself be a failure signal"
)]

// ── DistillConfig ──

mod distill_config {
    use melete::distill::{DistillConfig, DistillSection};

    #[test]
    fn default_uses_workspace_default_model() {
        // WHY: distillation's default model must track the workspace
        // `koina::defaults::DEFAULT_MODEL` constant. Pinning a literal here
        // is what produced #4235 — `aletheia init` defaulted to Sonnet 4.6
        // while distillation silently downgraded to Sonnet 4.0. Assert
        // against the constant so any future drift fails this test loudly.
        let config = DistillConfig::default();
        assert_eq!(config.model, koina::defaults::DEFAULT_MODEL);
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
            max_backoff_turns: 8,
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
        assert_eq!(
            restored.detect_contradictions,
            original.detect_contradictions
        );
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

// ── DistillSection ──

mod distill_section {
    use melete::distill::DistillSection;

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

// ── DistillEngine: the core async entry point ──

mod distill_engine {
    use hermeneus::test_utils::MockProvider;
    use hermeneus::types::{Content, Message, Role};
    use melete::distill::{DistillConfig, DistillEngine, DistillSection};
    use melete::error::Error;

    fn text_msg(role: Role, text: &str) -> Message {
        Message {
            role,
            content: Content::Text(text.to_owned()),
            cache_breakpoint: false,
        }
    }

    fn long_conversation() -> Vec<Message> {
        // WHY: ten messages comfortably exceed the default verbatim_tail=3
        // and min_messages=6 so the engine has real work to split.
        (0..10)
            .map(|i| {
                let role = if i % 2 == 0 {
                    Role::User
                } else {
                    Role::Assistant
                };
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
            .distill(&long_conversation(), "bad`id\nevil", &provider, 1)
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
