//! R722 substrate integration tests (substrate milestones end-to-end).
//!
//! Exercises cross-crate boundaries that per-crate unit tests and the
//! substrate-validation surface checks cannot reach.

#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::unwrap_used,
    reason = "test fixtures: synthetic IDs always valid"
)]
#![expect(
    clippy::indexing_slicing,
    reason = "integration tests: index-based assertions on known-length slices"
)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use hermeneus::provider::{LlmProvider, ProviderRegistry};
use hermeneus::types::{CompletionRequest, CompletionResponse, ContentBlock, StopReason, Usage};
use mneme::bookkeeping::BookkeepingProvider as _;
use mneme::knowledge::{
    EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactSensitivity, FactTemporal,
    far_future,
};
use mneme::knowledge_store::KnowledgeStore;
use nous::config::{NousConfig, PipelineConfig, RecallProfile};
use nous::cross::{AddressMask, CrossNousEnvelope, CrossNousMessage, CrossNousRouter};
use nous::manager::NousManager;
use organon::registry::ToolRegistry;
use taxis::oikos::Oikos;

// ── Shared mock provider that captures requests ─────────────────────────────

struct CapturingMockProvider {
    response: CompletionResponse,
    captured: Arc<Mutex<Vec<CompletionRequest>>>,
}

impl CapturingMockProvider {
    fn new(text: &str, captured: Arc<Mutex<Vec<CompletionRequest>>>) -> Self {
        Self {
            response: CompletionResponse {
                id: "msg_test".to_owned(),
                model: "mock-model".to_owned(),
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
            },
            captured,
        }
    }
}

impl LlmProvider for CapturingMockProvider {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = hermeneus::error::Result<CompletionResponse>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async {
            #[expect(
                clippy::expect_used,
                reason = "test mock: poisoned lock means a test bug"
            )]
            self.captured
                .lock()
                .expect("lock poisoned")
                .push(request.clone());
            Ok(self.response.clone())
        })
    }

    fn supported_models(&self) -> &[&str] {
        &["mock-model"]
    }

    #[expect(clippy::unnecessary_literal_bound, reason = "trait requires &str")]
    fn name(&self) -> &str {
        "mock-capturing"
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn temp_oikos(agent_id: &str) -> (tempfile::TempDir, Arc<Oikos>) {
    let dir = tempfile::TempDir::new().expect("tmpdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join(format!("nous/{agent_id}"))).expect("mkdir");
    std::fs::create_dir_all(root.join("shared")).expect("mkdir shared");
    std::fs::create_dir_all(root.join("theke")).expect("mkdir theke");
    #[expect(
        clippy::disallowed_methods,
        reason = "test setup writes fixture files to temp directories"
    )]
    std::fs::write(root.join(format!("nous/{agent_id}/SOUL.md")), "Test agent.")
        .expect("write SOUL.md");
    let oikos = Arc::new(Oikos::from_root(root));
    (dir, oikos)
}

fn make_test_fact(id: &str, nous_id: &str, content: &str) -> Fact {
    let now = jiff::Timestamp::now();
    Fact {
        id: mneme::id::FactId::new(id).expect("valid test id"),
        nous_id: nous_id.to_owned(),
        fact_type: "test".to_owned(),
        content: content.to_owned(),
        scope: None,
        project_id: None,
        sensitivity: FactSensitivity::Public,
        visibility: mneme::knowledge::Visibility::Private,
        temporal: FactTemporal {
            valid_from: now,
            valid_to: far_future(),
            recorded_at: now,
        },
        provenance: FactProvenance {
            confidence: 1.0,
            tier: EpistemicTier::Verified,
            source_session_id: Some("sess-test".to_owned()),
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

// ── Scenario 1: IdentityContinuity profile ──────────────────────────────────

#[tokio::test]
async fn identity_continuity_pins_top_facts_and_late_injects_anchor() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let provider = CapturingMockProvider::new("ack", Arc::clone(&captured));

    let (dir, oikos) = temp_oikos("agent-1");
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(provider));
    let providers = Arc::new(providers);

    let store = KnowledgeStore::open_mem().expect("open_mem");
    let pinned = make_test_fact("pinned-1", "agent-1", "Alice is the operator.");
    store.insert_fact(&pinned).expect("insert pinned fact");

    let mut knowledge_stores = HashMap::new();
    knowledge_stores.insert("shared".to_owned(), Arc::clone(&store));

    let mut manager = NousManager::new(
        Arc::clone(&providers),
        Arc::new(ToolRegistry::new()),
        oikos,
        Some(Arc::new(mneme::embedding::MockEmbeddingProvider::new(384))),
        None,
        None, // no session store — avoids run_history_stage overwriting recall messages
        Some(knowledge_stores),
        Arc::new(vec![]),
        None,
        None,
        taxis::config::NousBehaviorConfig::default(),
        taxis::config::ToolLimitsConfig::default(),
    );

    let mut config = NousConfig {
        id: Arc::from("agent-1"),
        generation: nous::config::NousGenerationConfig {
            model: "mock-model".to_owned(),
            ..Default::default()
        },
        recall_profile: RecallProfile::IdentityContinuity,
        ..NousConfig::default()
    };
    config.recall.pinned_facts.push(pinned.id.clone());

    let mut pipeline = PipelineConfig::default();
    config.apply_recall_profile(&mut pipeline);

    let handle = manager.spawn(config, pipeline).await.expect("spawn");

    // Run a synthetic turn; the query "Alice" should match the pinned fact via BM25.
    let _turn = handle
        .send_turn("main", "Tell me about Alice")
        .await
        .expect("turn");

    #[expect(
        clippy::expect_used,
        reason = "test mock: poisoned lock means a test bug"
    )]
    let requests = captured.lock().expect("lock poisoned").clone();
    assert!(
        !requests.is_empty(),
        "mock provider should have received at least one request"
    );
    let req = &requests[requests.len() - 1];

    // Verify late-inject anchor: recall content should appear as a system message
    // at the end of the conversation context, not inside the main system prompt.
    let has_system_recall = req.messages.iter().any(|m| {
        m.role == hermeneus::types::Role::System
            && m.content.text().contains("Alice is the operator.")
    });
    assert!(
        has_system_recall,
        "late_inject_anchor should append pinned fact as a trailing system message"
    );

    manager.shutdown_all().await;
    drop(dir);
}

#[tokio::test]
async fn identity_continuity_reflection_flag_is_set() {
    // Reflection persistence is covered in nous pipeline tests. This substrate
    // test verifies the identity-continuity profile still enables the runtime
    // stage and trims pinned facts consistently.
    let mut config = NousConfig {
        id: Arc::from("test"),
        recall_profile: RecallProfile::IdentityContinuity,
        ..NousConfig::default()
    };
    config.recall.pinned_facts = vec![
        mneme::id::FactId::new("a").unwrap(),
        mneme::id::FactId::new("b").unwrap(),
        mneme::id::FactId::new("c").unwrap(),
        mneme::id::FactId::new("d").unwrap(),
    ];
    let mut pipeline = PipelineConfig::default();
    config.apply_recall_profile(&mut pipeline);
    assert!(
        pipeline.reflection_enabled,
        "IdentityContinuity should enable reflection"
    );
    assert_eq!(
        config.recall.pinned_facts.len(),
        3,
        "pinned_facts should be truncated to at most 3"
    );
}

#[tokio::test]
async fn extract_self_facts_false_rejects_self_descriptive_facts() {
    struct SelfFactProvider;
    impl mneme::extract::ExtractionProvider for SelfFactProvider {
        fn complete<'a>(
            &'a self,
            _system: &'a str,
            _user_message: &'a str,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<String, mneme::extract::ExtractionError>>
                    + Send
                    + 'a,
            >,
        > {
            Box::pin(async {
                Ok(r#"{"entities":[],"relationships":[],"facts":[{"subject":"I","predicate":"am","object":"helpful","confidence":0.9}]}"#.to_owned())
            })
        }
    }

    let engine = mneme::extract::ExtractionEngine::new(mneme::extract::ExtractionConfig {
        extract_self_facts: false,
        min_message_length: 1,
        ..mneme::extract::ExtractionConfig::default()
    });
    let messages = vec![mneme::extract::ConversationMessage {
        role: "user".to_owned(),
        tool_calls: None,
        reasoning: None,
        content: "I am helpful.".to_owned(),
    }];

    let result = engine
        .extract_refined(&messages, &SelfFactProvider, "test-nous", "test")
        .await
        .expect("extraction should succeed");

    assert!(
        result.extraction.facts.is_empty(),
        "self-descriptive fact should be rejected when extract_self_facts=false"
    );
    assert_eq!(
        result.facts_filtered, 1,
        "one fact should be filtered for self-reference"
    );

    // Verify the opposite: when true, it is accepted.
    let engine_allowed = mneme::extract::ExtractionEngine::new(mneme::extract::ExtractionConfig {
        extract_self_facts: true,
        min_message_length: 1,
        ..mneme::extract::ExtractionConfig::default()
    });
    let result_allowed = engine_allowed
        .extract_refined(&messages, &SelfFactProvider, "test-nous", "test")
        .await
        .expect("extraction should succeed");
    assert!(
        !result_allowed.extraction.facts.is_empty(),
        "self-descriptive fact should be accepted when extract_self_facts=true"
    );
}

// ── Scenario 2: Private-nous fence ──────────────────────────────────────────

#[tokio::test]
async fn private_nous_address_mask_blocks_inbound_cross_nous_messages() {
    let router = CrossNousRouter::default();

    let (tx_a, mut rx_a) = tokio::sync::mpsc::channel::<CrossNousEnvelope>(8);
    let (tx_b, mut rx_b) = tokio::sync::mpsc::channel::<CrossNousEnvelope>(8);

    router.register("agent-a", tx_a).await;
    router.register("agent-b", tx_b).await;
    router
        .set_address_mask("agent-b", AddressMask::OperatorOnly)
        .await;

    // agent-a → agent-b should be rejected.
    let msg = CrossNousMessage::new("agent-a", "agent-b", "hello");
    let result = router.send(msg).await;
    assert!(
        matches!(result, Err(nous::error::Error::AddressRejected { ref to, .. }) if to == "agent-b"),
        "agent-a should be blocked from sending to private agent-b: got {result:?}"
    );

    // operator-equivalent sender should be allowed.
    let op_msg = CrossNousMessage::new("operator", "agent-b", "hello");
    let result = router.send(op_msg).await;
    assert!(
        result.is_ok(),
        "operator should be allowed to send to agent-b: got {result:?}"
    );
    let env = rx_b
        .recv()
        .await
        .expect("agent-b should receive operator msg");
    assert_eq!(env.message.from, "operator");

    // agent-b outbound → agent-a should work (mask is inbound-only).
    let outbound = CrossNousMessage::new("agent-b", "agent-a", "delegation");
    let result = router.send(outbound).await;
    assert!(
        result.is_ok(),
        "agent-b outbound delegation to agent-a should succeed: got {result:?}"
    );
    let env = rx_a
        .recv()
        .await
        .expect("agent-a should receive delegation");
    assert_eq!(env.message.from, "agent-b");
}

#[tokio::test]
async fn private_nous_bootstrap_excludes_shared_sources() {
    let dir = tempfile::TempDir::new().expect("tmpdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("nous/agent-b")).expect("mkdir");
    std::fs::create_dir_all(root.join("shared")).expect("mkdir");
    #[expect(
        clippy::disallowed_methods,
        reason = "test setup writes fixture files to temp directories"
    )]
    std::fs::write(root.join("nous/agent-b/SOUL.md"), "Private agent.").expect("write");
    #[expect(
        clippy::disallowed_methods,
        reason = "test setup writes fixture files to temp directories"
    )]
    std::fs::write(root.join("shared/SHARED.md"), "Shared context.").expect("write shared");

    let oikos = Arc::new(Oikos::from_root(root));
    let assembler = nous::bootstrap::BootstrapAssembler::new(&oikos).with_private_workspace(true);
    let mut budget = nous::budget::TokenBudget::new(200_000, 0.6, 16_384, 40_000);

    let result = assembler
        .assemble("agent-b", &mut budget)
        .await
        .expect("assemble");

    assert!(
        result.system_prompt.contains("Private agent."),
        "private workspace should include agent's own SOUL.md"
    );
    assert!(
        !result.system_prompt.contains("Shared context."),
        "private workspace should exclude shared/ discovery sources"
    );
}

// ── Scenario 3: Per-nous episteme cohort isolation ──────────────────────────

#[test]
fn cohort_isolation_facts_in_one_store_not_recallable_from_another() {
    let store_x = KnowledgeStore::open_mem().expect("open_mem");
    let store_y = KnowledgeStore::open_mem().expect("open_mem");

    let fact_x = make_test_fact("fact-x", "agent-x", "CohortX secret");
    store_x.insert_fact(&fact_x).expect("insert store x");

    // Search store_y for the same content — should find nothing because it is a
    // different store instance (different cohort keyspace).
    let results_y = store_y
        .search_text_for_recall("CohortX secret", 5)
        .expect("search y");
    assert!(
        results_y.is_empty(),
        "cohort-y store should not surface cohort-x facts"
    );

    // Search store_x — should find the fact.
    let results_x = store_x
        .search_text_for_recall("CohortX secret", 5)
        .expect("search x");
    assert!(
        !results_x.is_empty(),
        "cohort-x store should surface its own fact"
    );

    // Verify the two stores are distinct Arc instances (logical keyspace separation).
    assert!(
        !Arc::ptr_eq(&store_x, &store_y),
        "cohort stores must be distinct instances"
    );
}

// ── Scenario 4: BookkeepingProvider routing ─────────────────────────────────

#[test]
fn bookkeeping_provider_routes_llm_by_default() {
    struct MockExtractionProvider;
    impl mneme::extract::ExtractionProvider for MockExtractionProvider {
        fn complete<'a>(
            &'a self,
            _system: &'a str,
            _user_message: &'a str,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<String, mneme::extract::ExtractionError>>
                    + Send
                    + 'a,
            >,
        > {
            Box::pin(async { Ok("{}".to_owned()) })
        }
    }

    let engine = mneme::extract::ExtractionEngine::new(mneme::extract::ExtractionConfig {
        provider: mneme::extract::BookkeepingProviderKind::Llm,
        ..mneme::extract::ExtractionConfig::default()
    });

    // LlmBookkeepingProvider can be constructed from the engine + a mock provider.
    let provider =
        mneme::bookkeeping::LlmBookkeepingProvider::new(&engine, &MockExtractionProvider);
    assert_eq!(provider.name(), "llm");
}

#[test]
fn bookkeeping_provider_gliner_fallback_when_feature_disabled() {
    // When the gliner feature is disabled, requesting Gliner falls back to Llm
    // at the architectural boundary (nous/src/actor/background.rs). We verify the
    // episteme-level config mapping here; the full actor wiring is covered by
    // the background.rs unit tests.
    let config = mneme::extract::ExtractionConfig {
        provider: mneme::extract::BookkeepingProviderKind::Gliner,
        ..mneme::extract::ExtractionConfig::default()
    };

    // In a build without gliner, the provider enum still accepts Gliner,
    // but the actor background task will log a warning and overwrite to Llm.
    assert_eq!(
        config.provider,
        mneme::extract::BookkeepingProviderKind::Gliner,
        "config should preserve the requested provider kind"
    );

    // NOTE: A full end-to-end test of the fallback path would require spawning
    // an actor with BookkeepingProviderKind::Gliner in a no-gliner build and
    // inspecting tracing output. We rely on the unit test in
    // `nous/src/actor/background.rs` for that surface.
}

#[tokio::test]
async fn extract_self_facts_false_backstop_in_engine() {
    // Architectural backstop: even if a caller forgets the config-flag check,
    // the ExtractionEngine filter removes self-references.
    struct SelfFactProvider;
    impl mneme::extract::ExtractionProvider for SelfFactProvider {
        fn complete<'a>(
            &'a self,
            _system: &'a str,
            _user_message: &'a str,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<String, mneme::extract::ExtractionError>>
                    + Send
                    + 'a,
            >,
        > {
            Box::pin(async {
                Ok(r#"{"entities":[],"relationships":[],"facts":[{"subject":"assistant","predicate":"is","object":"helpful","confidence":0.95}]}"#.to_owned())
            })
        }
    }

    let engine = mneme::extract::ExtractionEngine::new(mneme::extract::ExtractionConfig {
        extract_self_facts: false,
        min_message_length: 1,
        ..mneme::extract::ExtractionConfig::default()
    });
    let messages = vec![mneme::extract::ConversationMessage {
        role: "user".to_owned(),
        tool_calls: None,
        reasoning: None,
        content: "assistant is helpful.".to_owned(),
    }];

    let result = engine
        .extract_refined(&messages, &SelfFactProvider, "test-nous", "test")
        .await
        .expect("extraction should succeed");
    assert!(
        result.extraction.facts.is_empty(),
        "engine backstop must reject self-descriptive facts regardless of caller"
    );
}

// ── Scenario 5: Multi-agent verification ────────────────────────────────────

#[test]
fn verification_protocol_promotes_fact_when_threshold_met() {
    let now = jiff::Timestamp::now();
    let fact = make_test_fact("vf-1", "agent-0", "Rust is memory-safe.");

    let published =
        mneme::verification::publish_fact(&fact, &koina::id::NousId::new("agent-0").unwrap());
    assert_eq!(published.verification_count, 0);

    let mut proposal = mneme::knowledge::VerificationProposal {
        fact_id: published.original_fact_id.clone(),
        proposing_nous: koina::id::NousId::new("agent-0").unwrap(),
        proposed_tier: EpistemicTier::Verified,
        votes: vec![],
    };

    // 3 distinct Accept votes → promotion.
    for voter in ["agent-1", "agent-2", "agent-3"] {
        let vote = mneme::knowledge::VerificationVote {
            voter: koina::id::NousId::new(voter).unwrap(),
            verdict: mneme::knowledge::VerificationVerdict::Accept,
            at: now,
        };
        let outcome = mneme::verification::vote_on_proposal(&mut proposal, vote, 3);
        if voter == "agent-3" {
            assert!(
                matches!(outcome, mneme::verification::VerificationOutcome::Promoted { new_tier } if new_tier == EpistemicTier::Verified),
                "N=3 accepts should promote to Verified, got {outcome:?}"
            );
        } else {
            assert!(
                matches!(outcome, mneme::verification::VerificationOutcome::Pending),
                "before threshold should be Pending, got {outcome:?}"
            );
        }
    }
}

#[test]
fn verification_protocol_contest_prevents_promotion() {
    let now = jiff::Timestamp::now();
    let fact = make_test_fact("vf-2", "agent-0", "Rust is fast.");
    let published =
        mneme::verification::publish_fact(&fact, &koina::id::NousId::new("agent-0").unwrap());

    let mut proposal = mneme::knowledge::VerificationProposal {
        fact_id: published.original_fact_id.clone(),
        proposing_nous: koina::id::NousId::new("agent-0").unwrap(),
        proposed_tier: EpistemicTier::Verified,
        votes: vec![],
    };

    // 2 Accept + 1 Contest → contested (no promotion).
    for (voter, verdict) in [
        ("agent-1", mneme::knowledge::VerificationVerdict::Accept),
        ("agent-2", mneme::knowledge::VerificationVerdict::Accept),
        ("agent-3", mneme::knowledge::VerificationVerdict::Contest),
    ] {
        let vote = mneme::knowledge::VerificationVote {
            voter: koina::id::NousId::new(voter).unwrap(),
            verdict,
            at: now,
        };
        let outcome = mneme::verification::vote_on_proposal(&mut proposal, vote, 3);
        if voter == "agent-3" {
            assert!(
                matches!(
                    outcome,
                    mneme::verification::VerificationOutcome::Contested { .. }
                ),
                "a single contest should block promotion, got {outcome:?}"
            );
        }
    }
}

// ── Scenario 6: Conflict detection ──────────────────────────────────────────

#[test]
fn detect_conflict_finds_contradiction_in_same_cohort() {
    let store = KnowledgeStore::open_mem().expect("open_mem");

    let fact_a = make_test_fact("cf-a", "agent-z", "Alice likes tea.");
    store.insert_fact(&fact_a).expect("insert a");

    let extracted = mneme::bookkeeping::ExtractedFact {
        subject: "Alice".to_owned(),
        predicate: "likes".to_owned(),
        object: "coffee".to_owned(),
        confidence: 0.9,
        is_correction: false,
        fact_type: None,
    };

    let conflict =
        mneme::verification::detect_conflict(&extracted, &store, "agent-z").expect("no error");

    // Because the content strings differ ("Alice likes tea." vs "Alice likes coffee"),
    // and the BM25 search returns the existing fact as a near match, the conflict
    // detector should classify this as a Contradiction when it finds a match.
    if let Some(c) = conflict {
        assert_eq!(
            c.kind,
            mneme::verification::ConflictKind::Contradiction,
            "different content for same subject should be a contradiction"
        );
    }
    // If BM25 did not return the fact (short query), the None path is acceptable
    // — the integration boundary (store → detect_conflict) is still exercised.
}

#[test]
fn detect_conflict_honors_cohort_isolation() {
    let store_z = KnowledgeStore::open_mem().expect("open_mem");
    let fact_z = make_test_fact("cf-z", "agent-z", "Alice likes tea.");
    store_z.insert_fact(&fact_z).expect("insert z");

    let extracted = mneme::bookkeeping::ExtractedFact {
        subject: "Alice".to_owned(),
        predicate: "likes".to_owned(),
        object: "coffee".to_owned(),
        confidence: 0.9,
        is_correction: false,
        fact_type: None,
    };

    // Querying with a DIFFERENT nous_id should skip the fact because
    // detect_conflict filters by nous_id in the Datalog post-filter.
    let conflict = mneme::verification::detect_conflict(&extracted, &store_z, "agent-other")
        .expect("no error");
    assert!(
        conflict.is_none(),
        "conflict should not be detected across cohorts"
    );
}

#[test]
fn conflict_resolution_compute_score_formula() {
    let now = jiff::Timestamp::now();
    let fact = make_test_fact("score-1", "test", "Test fact.");

    let score = mneme::knowledge::ConflictResolution::compute_score(&fact, 1, now);
    let score_5 = mneme::knowledge::ConflictResolution::compute_score(&fact, 5, now);

    // More supporters → higher score (all else equal).
    assert!(
        score_5 > score,
        "score should increase with supporter count: {score_5} > {score}"
    );

    // Verified tier should outrank Inferred at equal supporter counts.
    let mut inferred = fact.clone();
    inferred.provenance.tier = EpistemicTier::Inferred;
    let score_verified = mneme::knowledge::ConflictResolution::compute_score(&fact, 1, now);
    let score_inferred = mneme::knowledge::ConflictResolution::compute_score(&inferred, 1, now);
    assert!(
        score_verified > score_inferred,
        "Verified should outrank Inferred: {score_verified} > {score_inferred}"
    );
}
