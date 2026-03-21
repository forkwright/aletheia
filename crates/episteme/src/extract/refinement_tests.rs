#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use super::*;

#[test]
fn classify_discussion_default() {
    let content = "Tell me about your project. What frameworks do you use?";
    assert_eq!(
        classify_turn(content),
        TurnType::Discussion,
        "classify discussion default: values should be equal"
    );
}

#[test]
fn classify_tool_heavy() {
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
        "classify tool heavy: values should be equal"
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
        "classify planning: values should be equal"
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
        "classify debugging: values should be equal"
    );
}

#[test]
fn classify_correction() {
    let content = "Actually, it's not Python — I use Rust for this project.";
    assert_eq!(
        classify_turn(content),
        TurnType::Correction,
        "classify correction: values should be equal"
    );
}

#[test]
fn classify_correction_was_wrong() {
    let content = "I was wrong about the database. We use PostgreSQL, not MySQL.";
    assert_eq!(
        classify_turn(content),
        TurnType::Correction,
        "classify correction was wrong: values should be equal"
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
        "classify procedural: values should be equal"
    );
}

#[test]
fn classify_correction_takes_priority_over_debugging() {
    let content = "Actually, it's not a panic — I was wrong about the error. \
        The stack trace was misleading.";
    assert_eq!(
        classify_turn(content),
        TurnType::Correction,
        "classify correction takes priority over debugging: values should be equal"
    );
}

#[test]
fn classify_fact_identity() {
    assert_eq!(
        classify_fact("I am a software engineer"),
        FactType::Identity,
        "classify fact identity: values should be equal"
    );
    assert_eq!(
        classify_fact("My name is Alice"),
        FactType::Identity,
        "classify fact identity: values should be equal"
    );
}

#[test]
fn classify_fact_preference() {
    assert_eq!(
        classify_fact("I prefer Rust over Python"),
        FactType::Preference,
        "classify fact preference: values should be equal"
    );
    assert_eq!(
        classify_fact("I don't like dynamic typing"),
        FactType::Preference,
        "classify fact preference: values should be equal"
    );
}

#[test]
fn classify_fact_skill() {
    assert_eq!(
        classify_fact("I work with Kubernetes daily"),
        FactType::Skill,
        "classify fact skill: values should be equal"
    );
    assert_eq!(
        classify_fact("I know how to use Docker"),
        FactType::Skill,
        "classify fact skill: values should be equal"
    );
}

#[test]
fn classify_fact_task() {
    assert_eq!(
        classify_fact("I need to finish the todo list by Friday"),
        FactType::Task,
        "classify fact task: values should be equal"
    );
}

#[test]
fn classify_fact_event() {
    assert_eq!(
        classify_fact("Yesterday I deployed the new version"),
        FactType::Event,
        "classify fact event: values should be equal"
    );
}

#[test]
fn classify_fact_relationship() {
    assert_eq!(
        classify_fact("Alice works with Bob on the project"),
        FactType::Relationship,
        "classify fact relationship: values should be equal"
    );
}

#[test]
fn classify_fact_observation_default() {
    assert_eq!(
        classify_fact("Rust is a systems programming language"),
        FactType::Observation,
        "classify fact observation default: values should be equal"
    );
}

#[test]
fn detect_correction_positive() {
    let signal = detect_correction("Actually, it's PostgreSQL not MySQL");
    assert!(
        signal.is_correction,
        "detect correction positive: assertion failed"
    );
    assert!(
        (signal.confidence_boost - 0.2).abs() < f64::EPSILON,
        "detect correction positive: assertion failed"
    );
}

#[test]
fn detect_correction_negative() {
    let signal = detect_correction("I use PostgreSQL for my databases");
    assert!(
        !signal.is_correction,
        "detect correction negative: assertion failed"
    );
    assert!(
        (signal.confidence_boost).abs() < f64::EPSILON,
        "detect correction negative: assertion failed"
    );
}

#[test]
fn detect_correction_was_wrong() {
    let signal = detect_correction("I was wrong about the deadline");
    assert!(
        signal.is_correction,
        "detect correction was wrong: assertion failed"
    );
}

#[test]
fn detect_correction_explicit() {
    let signal = detect_correction("Correction: the port is 8080, not 3000");
    assert!(
        signal.is_correction,
        "detect correction explicit: assertion failed"
    );
}

#[test]
fn filter_low_confidence_rejected() {
    let result = filter_fact("This is a valid fact content", 0.2);
    assert!(
        !result.passed,
        "filter low confidence rejected: assertion failed"
    );
    assert_eq!(
        result.reason,
        Some(FilterReason::LowConfidence),
        "filter low confidence rejected: values should be equal"
    );
}

#[test]
fn filter_short_content_rejected() {
    let result = filter_fact("Short", 0.9);
    assert!(
        !result.passed,
        "filter short content rejected: assertion failed"
    );
    assert_eq!(
        result.reason,
        Some(FilterReason::TooShort),
        "filter short content rejected: values should be equal"
    );
}

#[test]
fn filter_long_content_rejected() {
    let long = "x".repeat(501);
    let result = filter_fact(&long, 0.9);
    assert!(
        !result.passed,
        "filter long content rejected: assertion failed"
    );
    assert_eq!(
        result.reason,
        Some(FilterReason::TooLong),
        "filter long content rejected: values should be equal"
    );
}

#[test]
fn filter_trivial_content_rejected() {
    let result = filter_fact("The file is 100 lines long", 0.9);
    assert!(
        !result.passed,
        "filter trivial content rejected: assertion failed"
    );
    assert_eq!(
        result.reason,
        Some(FilterReason::Trivial),
        "filter trivial content rejected: values should be equal"
    );
}

#[test]
fn filter_valid_fact_passes() {
    let result = filter_fact("Alice uses Rust for systems programming", 0.8);
    assert!(result.passed, "filter valid fact passes: assertion failed");
    assert_eq!(
        result.reason, None,
        "filter valid fact passes: values should be equal"
    );
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
        "filter batch deduplication: values should be equal"
    );
    assert_eq!(
        result.rejected.len(),
        1,
        "filter batch deduplication: values should be equal"
    );
    assert_eq!(
        result.rejected[0].reason,
        FilterReason::Duplicate,
        "filter batch deduplication: values should be equal"
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
        "filter batch mixed rejections: values should be equal"
    );
    assert_eq!(
        result.rejected.len(),
        3,
        "filter batch mixed rejections: values should be equal"
    );
}

#[test]
fn boosted_confidence_normal() {
    assert!(
        (boosted_confidence(0.7, 0.2) - 0.9).abs() < f64::EPSILON,
        "boosted confidence normal: assertion failed"
    );
}

#[test]
fn boosted_confidence_capped_at_one() {
    assert!(
        (boosted_confidence(0.9, 0.2) - 1.0).abs() < f64::EPSILON,
        "boosted confidence capped at one: assertion failed"
    );
}

#[test]
fn boosted_confidence_zero_boost() {
    assert!(
        (boosted_confidence(0.5, 0.0) - 0.5).abs() < f64::EPSILON,
        "boosted confidence zero boost: assertion failed"
    );
}

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
        "turn type confidence boost values: assertion failed"
    );
    assert!(
        (TurnType::ToolHeavy.confidence_boost()).abs() < f64::EPSILON,
        "turn type confidence boost values: assertion failed"
    );
    assert!(
        (TurnType::Planning.confidence_boost() - 0.1).abs() < f64::EPSILON,
        "turn type confidence boost values: assertion failed"
    );
    assert!(
        (TurnType::Debugging.confidence_boost()).abs() < f64::EPSILON,
        "turn type confidence boost values: assertion failed"
    );
    assert!(
        (TurnType::Correction.confidence_boost() - 0.2).abs() < f64::EPSILON,
        "turn type confidence boost values: assertion failed"
    );
    assert!(
        (TurnType::Procedural.confidence_boost()).abs() < f64::EPSILON,
        "turn type confidence boost values: assertion failed"
    );
}

#[test]
fn turn_type_display() {
    assert_eq!(
        TurnType::Discussion.to_string(),
        "discussion",
        "turn type display: values should be equal"
    );
    assert_eq!(
        TurnType::ToolHeavy.to_string(),
        "tool_heavy",
        "turn type display: values should be equal"
    );
    assert_eq!(
        TurnType::Planning.to_string(),
        "planning",
        "turn type display: values should be equal"
    );
    assert_eq!(
        TurnType::Debugging.to_string(),
        "debugging",
        "turn type display: values should be equal"
    );
    assert_eq!(
        TurnType::Correction.to_string(),
        "correction",
        "turn type display: values should be equal"
    );
    assert_eq!(
        TurnType::Procedural.to_string(),
        "procedural",
        "turn type display: values should be equal"
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
        assert_eq!(
            tt, back,
            "turn type serde roundtrip: values should be equal"
        );
    }
}

#[test]
fn fact_type_as_str() {
    assert_eq!(
        FactType::Identity.as_str(),
        "identity",
        "fact type as str: values should be equal"
    );
    assert_eq!(
        FactType::Preference.as_str(),
        "preference",
        "fact type as str: values should be equal"
    );
    assert_eq!(
        FactType::Skill.as_str(),
        "skill",
        "fact type as str: values should be equal"
    );
    assert_eq!(
        FactType::Relationship.as_str(),
        "relationship",
        "fact type as str: values should be equal"
    );
    assert_eq!(
        FactType::Event.as_str(),
        "event",
        "fact type as str: values should be equal"
    );
    assert_eq!(
        FactType::Task.as_str(),
        "task",
        "fact type as str: values should be equal"
    );
    assert_eq!(
        FactType::Observation.as_str(),
        "observation",
        "fact type as str: values should be equal"
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
        assert_eq!(
            ft, back,
            "fact type serde roundtrip: values should be equal"
        );
    }
}

#[test]
fn empty_content_classified_as_discussion() {
    assert_eq!(
        classify_turn(""),
        TurnType::Discussion,
        "empty content classified as discussion: values should be equal"
    );
}

#[test]
fn tool_heavy_empty_content_is_not_tool_heavy() {
    assert!(
        !is_tool_heavy(""),
        "tool heavy empty content is not tool heavy: assertion failed"
    );
}

#[test]
fn classify_fact_empty_is_observation() {
    assert_eq!(
        classify_fact(""),
        FactType::Observation,
        "classify fact empty is observation: values should be equal"
    );
}

#[test]
fn full_pipeline_correction_turn() {
    let content = "Actually, it's not Python — I use Rust for backend work. \
        I was wrong about using Django. The framework I actually use is Axum.";

    let turn_type = classify_turn(content);
    assert_eq!(
        turn_type,
        TurnType::Correction,
        "full pipeline correction turn: values should be equal"
    );

    let correction = detect_correction(content);
    assert!(
        correction.is_correction,
        "full pipeline correction turn: assertion failed"
    );

    let fact = "I use Rust for backend work";
    let fact_type = classify_fact(fact);
    assert_eq!(
        fact_type,
        FactType::Skill,
        "full pipeline correction turn: values should be equal"
    );

    let base_confidence = 0.8;
    let boosted = boosted_confidence(
        base_confidence,
        turn_type.confidence_boost() + correction.confidence_boost,
    );
    assert!(
        (boosted - 1.0).abs() < f64::EPSILON,
        "full pipeline correction turn: assertion failed"
    );

    let filter = filter_fact(fact, boosted);
    assert!(
        filter.passed,
        "full pipeline correction turn: assertion failed"
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
        "full pipeline tool heavy turn: values should be equal"
    );
    assert!(
        (turn_type.confidence_boost()).abs() < f64::EPSILON,
        "full pipeline tool heavy turn: assertion failed"
    );

    let appendix = turn_type.prompt_appendix();
    assert!(
        appendix.contains("Skip"),
        "full pipeline tool heavy turn: expected to contain value"
    );
    assert!(
        appendix.contains("decision"),
        "full pipeline tool heavy turn: expected to contain value"
    );
}
