use super::*;

fn input(text: &str) -> ComplexityInput<'_> {
    ComplexityInput {
        message_text: text,
        tool_count: 0,
        message_count: 5,
        depth: 0,
        tier_override: None,
        model_override: None,
    }
}

// --- Scoring edge cases ---

#[test]
fn empty_message_scores_low() {
    let result = score_complexity(&input(""));
    assert!(
        result.score <= DEFAULT_LOW_THRESHOLD,
        "empty message should score low, got {}",
        result.score
    );
    assert_eq!(result.tier, ModelTier::Haiku);
}

#[test]
fn simple_greeting_scores_low() {
    let result = score_complexity(&input("hi"));
    assert!(
        result.score <= DEFAULT_LOW_THRESHOLD,
        "greeting should score low, got {}",
        result.score
    );
    assert_eq!(result.tier, ModelTier::Haiku);
}

#[test]
fn yes_no_scores_very_low() {
    for word in &["yes", "no", "ok", "thanks", "lgtm", "ship it"] {
        let result = score_complexity(&input(word));
        assert!(
            result.score <= 15,
            "'{word}' should score very low, got {}",
            result.score
        );
    }
}

#[test]
fn complex_intent_boosts_score() {
    let result = score_complexity(&input(
        "Please analyze this code and investigate why the tests are failing in the CI pipeline",
    ));
    assert!(
        result.score > DEFAULT_LOW_THRESHOLD,
        "complex intent should boost score, got {}",
        result.score
    );
}

#[test]
fn multi_step_boosts_score() {
    let result = score_complexity(&input(
        "First refactor the config module, then update the tests, and finally fix the CI",
    ));
    assert!(
        result.score >= 50,
        "multi-step request should score at least medium, got {}",
        result.score
    );
}

#[test]
fn code_block_boosts_score() {
    let result = score_complexity(&input(
        "Review this code:\n```rust\nfn main() { println!(\"hello\"); }\n```\nIs it correct?",
    ));
    assert!(
        result.score > DEFAULT_LOW_THRESHOLD,
        "code block should boost score, got {}",
        result.score
    );
}

#[test]
fn very_long_message_boosts_score() {
    let long_text = "a ".repeat(600);
    let result = score_complexity(&input(&long_text));
    assert!(
        result.score > DEFAULT_LOW_THRESHOLD,
        "long message should boost score, got {}",
        result.score
    );
}

#[test]
fn first_message_gets_boost() {
    let mut inp = input("hello there, I need help with something");
    inp.message_count = 0;
    let first = score_complexity(&inp);

    inp.message_count = 10;
    let later = score_complexity(&inp);

    assert!(
        first.score > later.score,
        "first message should score higher: {} vs {}",
        first.score,
        later.score
    );
}

#[test]
fn many_tools_boost_score() {
    let mut inp = input("Can you help me with this task?");
    inp.tool_count = 0;
    let no_tools = score_complexity(&inp);

    inp.tool_count = 8;
    let many_tools = score_complexity(&inp);

    assert!(
        many_tools.score > no_tools.score,
        "many tools should increase score: {} vs {}",
        many_tools.score,
        no_tools.score
    );
}

#[test]
fn deep_conversation_boosts_score() {
    let mut inp = input("What about this approach?");
    inp.message_count = 5;
    let shallow = score_complexity(&inp);

    inp.message_count = 25;
    let deep = score_complexity(&inp);

    assert!(
        deep.score > shallow.score,
        "deep conversation should score higher: {} vs {}",
        deep.score,
        shallow.score
    );
}

#[test]
fn judgment_request_boosts_score() {
    let result = score_complexity(&input(
        "What do you think about this architecture? Should I use microservices or a monolith?",
    ));
    assert!(
        result.score >= 50,
        "judgment request should score high, got {}",
        result.score
    );
}

// --- Explicit markers ---

#[test]
fn force_complex_marker_returns_opus() {
    let result = score_complexity(&input("Think hard about the best database schema"));
    assert_eq!(result.tier, ModelTier::Opus);
    assert_eq!(result.score, 95);
    assert!(
        result.reason.contains("user override"),
        "reason should mention user override"
    );
}

#[test]
fn force_routine_marker_returns_haiku() {
    let result = score_complexity(&input("quick question: what port does the server use?"));
    assert_eq!(result.tier, ModelTier::Haiku);
    assert_eq!(result.score, 5);
}

// --- Agent and depth overrides ---

#[test]
fn agent_override_bypasses_scoring() {
    let mut inp = input("simple question");
    inp.tier_override = Some(ModelTier::Opus);
    let result = score_complexity(&inp);
    assert_eq!(result.tier, ModelTier::Opus);
    assert_eq!(result.score, 100);
    assert_eq!(result.reason, "agent override");
}

#[test]
fn cross_agent_always_opus() {
    let mut inp = input("hello");
    inp.depth = 1;
    let result = score_complexity(&inp);
    assert_eq!(result.tier, ModelTier::Opus);
    assert_eq!(result.score, 90);
}

// --- Score clamping ---

#[test]
fn score_never_exceeds_100() {
    let result = score_complexity(&input(
        "Think hard about this complex philosophical analysis. Analyze the architecture, \
         design the migration strategy, and evaluate all the tradeoffs. What do you think is \
         the best approach? Consider the ethical implications and synthesize a plan.",
    ));
    assert!(
        result.score <= 100,
        "score should be clamped to 100, got {}",
        result.score
    );
}

#[test]
fn score_clamps_low_inputs() {
    let result = score_complexity(&input("no"));
    assert_eq!(
        result.tier,
        ModelTier::Haiku,
        "very negative raw score should clamp to 0 and route to haiku"
    );
}

// --- Threshold routing ---

#[test]
fn tier_from_score_low() {
    assert_eq!(tier_from_score(0, 30, 70), ModelTier::Haiku);
    assert_eq!(tier_from_score(15, 30, 70), ModelTier::Haiku);
    assert_eq!(tier_from_score(30, 30, 70), ModelTier::Haiku);
}

#[test]
fn tier_from_score_medium() {
    assert_eq!(tier_from_score(31, 30, 70), ModelTier::Sonnet);
    assert_eq!(tier_from_score(50, 30, 70), ModelTier::Sonnet);
    assert_eq!(tier_from_score(69, 30, 70), ModelTier::Sonnet);
}

#[test]
fn tier_from_score_high() {
    assert_eq!(tier_from_score(70, 30, 70), ModelTier::Opus);
    assert_eq!(tier_from_score(85, 30, 70), ModelTier::Opus);
    assert_eq!(tier_from_score(100, 30, 70), ModelTier::Opus);
}

#[test]
fn custom_thresholds_shift_tiers() {
    assert_eq!(tier_from_score(40, 50, 80), ModelTier::Haiku);
    assert_eq!(tier_from_score(60, 50, 80), ModelTier::Sonnet);
    assert_eq!(tier_from_score(85, 50, 80), ModelTier::Opus);
}

// --- Model routing ---

#[test]
fn route_model_user_override_wins() {
    let mut inp = input("analyze this complex problem");
    inp.model_override = Some("claude-opus-4-6");
    let config = ComplexityConfig {
        enabled: true,
        ..Default::default()
    };

    let decision = route_model(&inp, &config);
    assert!(decision.is_override, "should be flagged as override");
    assert_eq!(decision.model, "claude-opus-4-6");
}

#[test]
fn route_model_disabled_returns_sonnet() {
    let config = ComplexityConfig {
        enabled: false,
        ..Default::default()
    };

    let decision = route_model(&input("analyze this"), &config);
    assert_eq!(decision.model, names::SONNET);
    assert!(!decision.is_override);
}

#[test]
fn route_model_simple_query_routes_haiku() {
    let config = ComplexityConfig {
        enabled: true,
        ..Default::default()
    };

    let decision = route_model(&input("yes"), &config);
    assert_eq!(decision.model, names::HAIKU);
}

#[test]
fn route_model_complex_query_routes_opus() {
    let config = ComplexityConfig {
        enabled: true,
        ..Default::default()
    };

    let decision = route_model(
        &input("Think hard about the best migration strategy"),
        &config,
    );
    assert_eq!(decision.model, names::OPUS);
}

#[test]
fn route_model_custom_model_names() {
    let config = ComplexityConfig {
        enabled: true,
        haiku_model: "custom-fast".to_owned(),
        sonnet_model: "custom-balanced".to_owned(),
        opus_model: "custom-powerful".to_owned(),
        ..Default::default()
    };

    let decision = route_model(&input("ok"), &config);
    assert_eq!(decision.model, "custom-fast");
}

#[test]
fn config_default_is_disabled() {
    let config = ComplexityConfig::default();
    assert!(!config.enabled, "complexity routing should be opt-in");
}

// --- Display ---

#[test]
fn routing_decision_display_override() {
    let decision = RoutingDecision {
        model: "claude-opus-4-6".to_owned(),
        complexity: ComplexityScore {
            score: 50,
            tier: ModelTier::Sonnet,
            reason: "baseline".to_owned(),
        },
        is_override: true,
    };
    let display = format!("{decision}");
    assert!(
        display.contains("user override"),
        "display should mention override: {display}"
    );
}

#[test]
fn routing_decision_display_routed() {
    let decision = RoutingDecision {
        model: "claude-sonnet-4-6".to_owned(),
        complexity: ComplexityScore {
            score: 45,
            tier: ModelTier::Sonnet,
            reason: "single-tool code review".to_owned(),
        },
        is_override: false,
    };
    let display = format!("{decision}");
    assert!(
        display.contains("complexity: 45"),
        "display should include score: {display}"
    );
    assert!(
        display.contains("single-tool code review"),
        "display should include reason: {display}"
    );
}

#[test]
fn model_tier_display() {
    let haiku = ModelTier::Haiku;
    let sonnet = ModelTier::Sonnet;
    let opus = ModelTier::Opus;
    assert_eq!(format!("{haiku}"), "haiku");
    assert_eq!(format!("{sonnet}"), "sonnet");
    assert_eq!(format!("{opus}"), "opus");
}

// --- Routing outcome for competence feedback ---

#[test]
fn routing_outcome_captures_escalation() {
    let outcome = RoutingOutcome {
        decision: RoutingDecision {
            model: names::SONNET.to_owned(),
            complexity: ComplexityScore {
                score: 45,
                tier: ModelTier::Sonnet,
                reason: "baseline".to_owned(),
            },
            is_override: false,
        },
        success: true,
        self_escalated: true,
    };
    assert!(
        outcome.self_escalated,
        "should record self-escalation for competence tracking"
    );
}

// --- Multi-sentence scoring ---

#[test]
fn multi_sentence_boosts_score() {
    let single = score_complexity(&input("Fix the bug"));
    let multi = score_complexity(&input(
        "The login page crashes on submit. The error log shows a null pointer. \
         Users are reporting data loss. Please investigate the root cause.",
    ));
    assert!(
        multi.score > single.score,
        "multi-sentence should score higher: {} vs {}",
        multi.score,
        single.score
    );
}

// --- Philosophical content ---

#[test]
fn philosophical_content_boosts_score() {
    let result = score_complexity(&input(
        "What are the epistemological implications of using LLMs for automated reasoning?",
    ));
    assert!(
        result.score >= 50,
        "philosophical content should score high, got {}",
        result.score
    );
}
