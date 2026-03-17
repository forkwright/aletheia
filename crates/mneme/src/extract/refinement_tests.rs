#![expect(clippy::expect_used, reason = "test assertions")]
use super::*;

// -- TurnType classification tests --

#[test]
fn classify_discussion_default() {
    let content = "Tell me about your project. What frameworks do you use?";
    assert_eq!(
        classify_turn(content),
        TurnType::Discussion,
        "classify_turn(content) should equal expected value"
    );
}

#[test]
fn classify_tool_heavy() {
    // Content where > 60% is in code blocks
    let content = "Here's the output:\n\
        ```\n\
        line 1 of tool output that is quite long to make up most of the content\n\
        line 2 of tool output that is quite long to make up most of the content\n\
        line 3 of tool output that is quite long to make up most of the content\n\
        line 4 of tool output that is quite long to make up most of the content\n\
        line 5 of tool output that is quite long to make up most of the content\n\
        ```";
    assert_eq!(
        classify_turn(content),
        TurnType::ToolHeavy,
        "classify_turn(content) should equal expected value"
    );
}

#[test]
fn classify_planning() {
    let content = "Let's discuss the architecture and design of the system. \
        Should we use a microservices approach? The trade-off between \
        monolith and microservices is important for our strategy.";
    assert_eq!(
        classify_turn(content),
        TurnType::Planning,
        "classify_turn(content) should equal expected value"
    );
}

#[test]
fn classify_debugging() {
    let content = "I'm getting a panic in the application. \
        The stack trace shows the error is in the parser module. \
        The root cause seems to be an off-by-one error.";
    assert_eq!(
        classify_turn(content),
        TurnType::Debugging,
        "classify_turn(content) should equal expected value"
    );
}

#[test]
fn classify_correction() {
    let content = "Actually, it's not Python — I use Rust for this project.";
    assert_eq!(
        classify_turn(content),
        TurnType::Correction,
        "classify_turn(content) should equal expected value"
    );
}

#[test]
fn classify_correction_was_wrong() {
    let content = "I was wrong about the database. We use PostgreSQL, not MySQL.";
    assert_eq!(
        classify_turn(content),
        TurnType::Correction,
        "classify_turn(content) should equal expected value"
    );
}

#[test]
fn classify_procedural() {
    let content = "Follow these steps to deploy:\n\
        Step 1: Build the Docker image\n\
        Step 2: Push to the registry\n\
        Then, configure the load balancer.";
    assert_eq!(
        classify_turn(content),
        TurnType::Procedural,
        "classify_turn(content) should equal expected value"
    );
}

#[test]
fn classify_correction_takes_priority_over_debugging() {
    let content = "Actually, it's not a panic — I was wrong about the error. \
        The stack trace was misleading.";
    assert_eq!(
        classify_turn(content),
        TurnType::Correction,
        "classify_turn(content) should equal expected value"
    );
}

// -- FactType classification tests --

#[test]
fn classify_fact_identity() {
    assert_eq!(
        classify_fact("I am a software engineer"),
        FactType::Identity,
        "result should equal expected value",
    );
    assert_eq!(
        classify_fact("My name is Alice"),
        FactType::Identity,
        "result should equal expected value"
    );
}

#[test]
fn classify_fact_preference() {
    assert_eq!(
        classify_fact("I prefer Rust over Python"),
        FactType::Preference,
        "result should equal expected value",
    );
    assert_eq!(
        classify_fact("I don't like dynamic typing"),
        FactType::Preference,
        "result should equal expected value",
    );
}

#[test]
fn classify_fact_skill() {
    assert_eq!(
        classify_fact("I work with Kubernetes daily"),
        FactType::Skill,
        "result should equal expected value",
    );
    assert_eq!(
        classify_fact("I know how to use Docker"),
        FactType::Skill,
        "result should equal expected value"
    );
}

#[test]
fn classify_fact_task() {
    assert_eq!(
        classify_fact("I need to finish the todo list by Friday"),
        FactType::Task,
        "result should equal expected value",
    );
}

#[test]
fn classify_fact_event() {
    assert_eq!(
        classify_fact("Yesterday I deployed the new version"),
        FactType::Event,
        "result should equal expected value",
    );
}

#[test]
fn classify_fact_relationship() {
    assert_eq!(
        classify_fact("Alice works with Bob on the project"),
        FactType::Relationship,
        "result should equal expected value",
    );
}

#[test]
fn classify_fact_observation_default() {
    assert_eq!(
        classify_fact("Rust is a systems programming language"),
        FactType::Observation,
        "result should equal expected value",
    );
}

// -- Correction detection tests --

#[test]
fn detect_correction_positive() {
    let signal = detect_correction("Actually, it's PostgreSQL not MySQL");
    assert!(
        signal.is_correction,
        "assertion failed in detect correction positive"
    );
    assert!(
        (signal.confidence_boost - 0.2).abs() < f64::EPSILON,
        "assertion failed in detect correction positive"
    );
}

#[test]
fn detect_correction_negative() {
    let signal = detect_correction("I use PostgreSQL for my databases");
    assert!(!signal.is_correction, "is_correction should be false");
    assert!(
        (signal.confidence_boost).abs() < f64::EPSILON,
        "assertion failed in detect correction negative"
    );
}

#[test]
fn detect_correction_was_wrong() {
    let signal = detect_correction("I was wrong about the deadline");
    assert!(
        signal.is_correction,
        "assertion failed in detect correction was wrong"
    );
}

#[test]
fn detect_correction_explicit() {
    let signal = detect_correction("Correction: the port is 8080, not 3000");
    assert!(
        signal.is_correction,
        "assertion failed in detect correction explicit"
    );
}

// -- Quality filter tests --

#[test]
fn filter_low_confidence_rejected() {
    let result = filter_fact("This is a valid fact content", 0.2);
    assert!(!result.passed, "passed should be false");
    assert_eq!(
        result.reason,
        Some(FilterReason::LowConfidence),
        "reason should match result"
    );
}

#[test]
fn filter_short_content_rejected() {
    let result = filter_fact("Short", 0.9);
    assert!(!result.passed, "passed should be false");
    assert_eq!(
        result.reason,
        Some(FilterReason::TooShort),
        "reason should match Some(FilterReason::TooShort)"
    );
}

#[test]
fn filter_long_content_rejected() {
    let long = "x".repeat(501);
    let result = filter_fact(&long, 0.9);
    assert!(!result.passed, "passed should be false");
    assert_eq!(
        result.reason,
        Some(FilterReason::TooLong),
        "reason should match Some(FilterReason::TooLong)"
    );
}

#[test]
fn filter_trivial_content_rejected() {
    let result = filter_fact("The file is 100 lines long", 0.9);
    assert!(!result.passed, "passed should be false");
    assert_eq!(
        result.reason,
        Some(FilterReason::Trivial),
        "reason should match Some(FilterReason::Trivial)"
    );
}

#[test]
fn filter_valid_fact_passes() {
    let result = filter_fact("Alice uses Rust for systems programming", 0.8);
    assert!(
        result.passed,
        "assertion failed in filter valid fact passes"
    );
    assert_eq!(result.reason, None, "reason should equal expected value");
}

#[test]
fn filter_batch_deduplication() {
    let facts = vec![
        ("Alice uses Rust".to_owned(), 0.9),
        ("Alice uses Rust".to_owned(), 0.8),
        ("Bob uses Python".to_owned(), 0.7),
    ];
    let result = filter_batch(&facts);
    assert_eq!(
        result.passed.len(),
        2,
        "passed length should equal expected value"
    );
    assert_eq!(
        result.rejected.len(),
        1,
        "rejected length should equal expected value"
    );
    assert_eq!(
        result.rejected[0].reason,
        FilterReason::Duplicate,
        "reason should equal expected value"
    );
}

#[test]
fn filter_batch_mixed_rejections() {
    let facts = vec![
        ("Valid fact about Rust programming".to_owned(), 0.9),
        ("Too low".to_owned(), 0.1),
        ("Short".to_owned(), 0.9),
        ("The file is 500 lines long".to_owned(), 0.9),
    ];
    let result = filter_batch(&facts);
    assert_eq!(
        result.passed.len(),
        1,
        "passed length should equal expected value"
    );
    assert_eq!(
        result.rejected.len(),
        3,
        "rejected length should equal expected value"
    );
}

// -- Confidence boost tests --

#[test]
fn boosted_confidence_normal() {
    assert!(
        (boosted_confidence(0.7, 0.2) - 0.9).abs() < f64::EPSILON,
        "assertion failed in boosted confidence normal"
    );
}

#[test]
fn boosted_confidence_capped_at_one() {
    assert!(
        (boosted_confidence(0.9, 0.2) - 1.0).abs() < f64::EPSILON,
        "assertion failed in boosted confidence capped at one"
    );
}

#[test]
fn boosted_confidence_zero_boost() {
    assert!(
        (boosted_confidence(0.5, 0.0) - 0.5).abs() < f64::EPSILON,
        "assertion failed in boosted confidence zero boost"
    );
}

// -- Prompt appendix tests --

#[test]
fn each_turn_type_has_distinct_appendix() {
    let types = [
        TurnType::Discussion,
        TurnType::ToolHeavy,
        TurnType::Planning,
        TurnType::Debugging,
        TurnType::Correction,
        TurnType::Procedural,
    ];
    let appendices: Vec<&str> = types.iter().map(|t| t.prompt_appendix()).collect();
    for (i, a) in appendices.iter().enumerate() {
        for (j, b) in appendices.iter().enumerate() {
            if i != j {
                assert_ne!(
                    a, b,
                    "turn types {:?} and {:?} should have distinct appendices",
                    types[i], types[j]
                );
            }
        }
    }
}

#[test]
fn turn_type_confidence_boost_values() {
    assert!(
        (TurnType::Discussion.confidence_boost()).abs() < f64::EPSILON,
        "assertion failed in turn type confidence boost values"
    );
    assert!(
        (TurnType::ToolHeavy.confidence_boost()).abs() < f64::EPSILON,
        "assertion failed in turn type confidence boost values"
    );
    assert!(
        (TurnType::Planning.confidence_boost() - 0.1).abs() < f64::EPSILON,
        "assertion failed in turn type confidence boost values"
    );
    assert!(
        (TurnType::Debugging.confidence_boost()).abs() < f64::EPSILON,
        "assertion failed in turn type confidence boost values"
    );
    assert!(
        (TurnType::Correction.confidence_boost() - 0.2).abs() < f64::EPSILON,
        "assertion failed in turn type confidence boost values"
    );
    assert!(
        (TurnType::Procedural.confidence_boost()).abs() < f64::EPSILON,
        "assertion failed in turn type confidence boost values"
    );
}

// -- Display / serde tests --

#[test]
fn turn_type_display() {
    assert_eq!(
        TurnType::Discussion.to_string(),
        "discussion",
        "to_string( should equal expected value"
    );
    assert_eq!(
        TurnType::ToolHeavy.to_string(),
        "tool_heavy",
        "to_string( should equal expected value"
    );
    assert_eq!(
        TurnType::Planning.to_string(),
        "planning",
        "to_string( should equal expected value"
    );
    assert_eq!(
        TurnType::Debugging.to_string(),
        "debugging",
        "to_string( should equal expected value"
    );
    assert_eq!(
        TurnType::Correction.to_string(),
        "correction",
        "to_string( should equal expected value"
    );
    assert_eq!(
        TurnType::Procedural.to_string(),
        "procedural",
        "to_string( should equal expected value"
    );
}

#[test]
fn turn_type_serde_roundtrip() {
    for tt in [
        TurnType::Discussion,
        TurnType::ToolHeavy,
        TurnType::Planning,
        TurnType::Debugging,
        TurnType::Correction,
        TurnType::Procedural,
    ] {
        let json = serde_json::to_string(&tt).expect("TurnType serialization must succeed");
        let back: TurnType =
            serde_json::from_str(&json).expect("TurnType deserialization must succeed");
        assert_eq!(tt, back, "tt should match back");
    }
}

#[test]
fn fact_type_as_str() {
    assert_eq!(
        FactType::Identity.as_str(),
        "identity",
        "FactType::Identity should equal expected value"
    );
    assert_eq!(
        FactType::Preference.as_str(),
        "preference",
        "FactType::Preference should equal expected value"
    );
    assert_eq!(
        FactType::Skill.as_str(),
        "skill",
        "FactType::Skill should equal expected value"
    );
    assert_eq!(
        FactType::Relationship.as_str(),
        "relationship",
        "FactType::Relationship should equal expected value"
    );
    assert_eq!(
        FactType::Event.as_str(),
        "event",
        "FactType::Event should equal expected value"
    );
    assert_eq!(
        FactType::Task.as_str(),
        "task",
        "FactType::Task should equal expected value"
    );
    assert_eq!(
        FactType::Observation.as_str(),
        "observation",
        "FactType::Observation should equal expected value"
    );
}

#[test]
fn fact_type_serde_roundtrip() {
    for ft in [
        FactType::Identity,
        FactType::Preference,
        FactType::Skill,
        FactType::Relationship,
        FactType::Event,
        FactType::Task,
        FactType::Observation,
    ] {
        let json = serde_json::to_string(&ft).expect("FactType serialization must succeed");
        let back: FactType =
            serde_json::from_str(&json).expect("TurnType deserialization must succeed");
        assert_eq!(ft, back, "ft should match back");
    }
}

#[test]
fn empty_content_classified_as_discussion() {
    assert_eq!(
        classify_turn(""),
        TurnType::Discussion,
        "classify_turn(\"\") should equal expected value"
    );
}

#[test]
fn tool_heavy_empty_content_is_not_tool_heavy() {
    assert!(!is_tool_heavy(""), "is_tool_heavy(\"\") should be false");
}

#[test]
fn classify_fact_empty_is_observation() {
    assert_eq!(
        classify_fact(""),
        FactType::Observation,
        "classify_fact(\"\") should equal expected value"
    );
}

// -- Integration-style test --

#[test]
fn full_pipeline_correction_turn() {
    let content = "Actually, it's not Python — I use Rust for backend work. \
        I was wrong about using Django. The framework I actually use is Axum.";

    // 1. Classify turn
    let turn_type = classify_turn(content);
    assert_eq!(
        turn_type,
        TurnType::Correction,
        "turn_type should equal expected value"
    );

    // 2. Detect correction
    let correction = detect_correction(content);
    assert!(
        correction.is_correction,
        "assertion failed in full pipeline correction turn"
    );

    // 3. Classify extracted facts
    let fact = "I use Rust for backend work";
    let fact_type = classify_fact(fact);
    assert_eq!(
        fact_type,
        FactType::Skill,
        "fact_type should equal expected value"
    );

    // 4. Apply confidence boost
    let base_confidence = 0.8;
    let boosted = boosted_confidence(
        base_confidence,
        turn_type.confidence_boost() + correction.confidence_boost,
    );
    // 0.8 + 0.2 (turn) + 0.2 (correction) = 1.0 (capped)
    assert!(
        (boosted - 1.0).abs() < f64::EPSILON,
        "assertion failed in full pipeline correction turn"
    );

    // 5. Quality filter passes
    let filter = filter_fact(fact, boosted);
    assert!(
        filter.passed,
        "assertion failed in full pipeline correction turn"
    );
}

#[test]
fn full_pipeline_tool_heavy_turn() {
    let content = "I ran the build:\n\
        ```\n\
        $ cargo build --release\n\
        Compiling aletheia v0.1.0\n\
        Compiling mneme v0.1.0\n\
        Compiling nous v0.1.0\n\
        Compiling hermeneus v0.1.0\n\
        Finished release [optimized] target(s)\n\
        ```\n\
        Build succeeded.";

    let turn_type = classify_turn(content);
    assert_eq!(
        turn_type,
        TurnType::ToolHeavy,
        "turn_type should equal expected value"
    );
    assert!(
        (turn_type.confidence_boost()).abs() < f64::EPSILON,
        "assertion failed in full pipeline tool heavy turn"
    );

    // Appendix should mention skipping raw output
    let appendix = turn_type.prompt_appendix();
    assert!(appendix.contains("Skip"), "appendix should contain Skip");
    assert!(
        appendix.contains("decision"),
        "appendix should contain decision"
    );
}
