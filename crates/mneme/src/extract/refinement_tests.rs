#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use super::*;

// -- TurnType classification tests --

#[test]
fn classify_discussion_default() {
    let content = "Tell me about your project. What frameworks do you use?";
    assert_eq!(classify_turn(content), TurnType::Discussion);
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
    assert_eq!(classify_turn(content), TurnType::ToolHeavy);
}

#[test]
fn classify_planning() {
    let content = "Let's discuss the architecture and design of the system. \
        Should we use a microservices approach? The trade-off between \
        monolith and microservices is important for our strategy.";
    assert_eq!(classify_turn(content), TurnType::Planning);
}

#[test]
fn classify_debugging() {
    let content = "I'm getting a panic in the application. \
        The stack trace shows the error is in the parser module. \
        The root cause seems to be an off-by-one error.";
    assert_eq!(classify_turn(content), TurnType::Debugging);
}

#[test]
fn classify_correction() {
    let content = "Actually, it's not Python — I use Rust for this project.";
    assert_eq!(classify_turn(content), TurnType::Correction);
}

#[test]
fn classify_correction_was_wrong() {
    let content = "I was wrong about the database. We use PostgreSQL, not MySQL.";
    assert_eq!(classify_turn(content), TurnType::Correction);
}

#[test]
fn classify_procedural() {
    let content = "Follow these steps to deploy:\n\
        Step 1: Build the Docker image\n\
        Step 2: Push to the registry\n\
        Then, configure the load balancer.";
    assert_eq!(classify_turn(content), TurnType::Procedural);
}

#[test]
fn classify_correction_takes_priority_over_debugging() {
    let content = "Actually, it's not a panic — I was wrong about the error. \
        The stack trace was misleading.";
    assert_eq!(classify_turn(content), TurnType::Correction);
}

// -- FactType classification tests --

#[test]
fn classify_fact_identity() {
    assert_eq!(
        classify_fact("I am a software engineer"),
        FactType::Identity
    );
    assert_eq!(classify_fact("My name is Alice"), FactType::Identity);
}

#[test]
fn classify_fact_preference() {
    assert_eq!(
        classify_fact("I prefer Rust over Python"),
        FactType::Preference
    );
    assert_eq!(
        classify_fact("I don't like dynamic typing"),
        FactType::Preference
    );
}

#[test]
fn classify_fact_skill() {
    assert_eq!(
        classify_fact("I work with Kubernetes daily"),
        FactType::Skill
    );
    assert_eq!(classify_fact("I know how to use Docker"), FactType::Skill);
}

#[test]
fn classify_fact_task() {
    assert_eq!(
        classify_fact("I need to finish the todo list by Friday"),
        FactType::Task
    );
}

#[test]
fn classify_fact_event() {
    assert_eq!(
        classify_fact("Yesterday I deployed the new version"),
        FactType::Event
    );
}

#[test]
fn classify_fact_relationship() {
    assert_eq!(
        classify_fact("Alice works with Bob on the project"),
        FactType::Relationship
    );
}

#[test]
fn classify_fact_observation_default() {
    assert_eq!(
        classify_fact("Rust is a systems programming language"),
        FactType::Observation
    );
}

// -- Correction detection tests --

#[test]
fn detect_correction_positive() {
    let signal = detect_correction("Actually, it's PostgreSQL not MySQL");
    assert!(signal.is_correction);
    assert!((signal.confidence_boost - 0.2).abs() < f64::EPSILON);
}

#[test]
fn detect_correction_negative() {
    let signal = detect_correction("I use PostgreSQL for my databases");
    assert!(!signal.is_correction);
    assert!((signal.confidence_boost).abs() < f64::EPSILON);
}

#[test]
fn detect_correction_was_wrong() {
    let signal = detect_correction("I was wrong about the deadline");
    assert!(signal.is_correction);
}

#[test]
fn detect_correction_explicit() {
    let signal = detect_correction("Correction: the port is 8080, not 3000");
    assert!(signal.is_correction);
}

// -- Quality filter tests --

#[test]
fn filter_low_confidence_rejected() {
    let result = filter_fact("This is a valid fact content", 0.2);
    assert!(!result.passed);
    assert_eq!(result.reason, Some(FilterReason::LowConfidence));
}

#[test]
fn filter_short_content_rejected() {
    let result = filter_fact("Short", 0.9);
    assert!(!result.passed);
    assert_eq!(result.reason, Some(FilterReason::TooShort));
}

#[test]
fn filter_long_content_rejected() {
    let long = "x".repeat(501);
    let result = filter_fact(&long, 0.9);
    assert!(!result.passed);
    assert_eq!(result.reason, Some(FilterReason::TooLong));
}

#[test]
fn filter_trivial_content_rejected() {
    let result = filter_fact("The file is 100 lines long", 0.9);
    assert!(!result.passed);
    assert_eq!(result.reason, Some(FilterReason::Trivial));
}

#[test]
fn filter_valid_fact_passes() {
    let result = filter_fact("Alice uses Rust for systems programming", 0.8);
    assert!(result.passed);
    assert_eq!(result.reason, None);
}

#[test]
fn filter_batch_deduplication() {
    let facts = vec![
        ("Alice uses Rust".to_owned(), 0.9),
        ("Alice uses Rust".to_owned(), 0.8),
        ("Bob uses Python".to_owned(), 0.7),
    ];
    let result = filter_batch(&facts);
    assert_eq!(result.passed.len(), 2);
    assert_eq!(result.rejected.len(), 1);
    assert_eq!(result.rejected[0].reason, FilterReason::Duplicate);
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
    assert_eq!(result.passed.len(), 1);
    assert_eq!(result.rejected.len(), 3);
}

// -- Confidence boost tests --

#[test]
fn boosted_confidence_normal() {
    assert!((boosted_confidence(0.7, 0.2) - 0.9).abs() < f64::EPSILON);
}

#[test]
fn boosted_confidence_capped_at_one() {
    assert!((boosted_confidence(0.9, 0.2) - 1.0).abs() < f64::EPSILON);
}

#[test]
fn boosted_confidence_zero_boost() {
    assert!((boosted_confidence(0.5, 0.0) - 0.5).abs() < f64::EPSILON);
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
    assert!((TurnType::Discussion.confidence_boost()).abs() < f64::EPSILON);
    assert!((TurnType::ToolHeavy.confidence_boost()).abs() < f64::EPSILON);
    assert!((TurnType::Planning.confidence_boost() - 0.1).abs() < f64::EPSILON);
    assert!((TurnType::Debugging.confidence_boost()).abs() < f64::EPSILON);
    assert!((TurnType::Correction.confidence_boost() - 0.2).abs() < f64::EPSILON);
    assert!((TurnType::Procedural.confidence_boost()).abs() < f64::EPSILON);
}

// -- Display / serde tests --

#[test]
fn turn_type_display() {
    assert_eq!(TurnType::Discussion.to_string(), "discussion");
    assert_eq!(TurnType::ToolHeavy.to_string(), "tool_heavy");
    assert_eq!(TurnType::Planning.to_string(), "planning");
    assert_eq!(TurnType::Debugging.to_string(), "debugging");
    assert_eq!(TurnType::Correction.to_string(), "correction");
    assert_eq!(TurnType::Procedural.to_string(), "procedural");
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
        assert_eq!(tt, back);
    }
}

#[test]
fn fact_type_as_str() {
    assert_eq!(FactType::Identity.as_str(), "identity");
    assert_eq!(FactType::Preference.as_str(), "preference");
    assert_eq!(FactType::Skill.as_str(), "skill");
    assert_eq!(FactType::Relationship.as_str(), "relationship");
    assert_eq!(FactType::Event.as_str(), "event");
    assert_eq!(FactType::Task.as_str(), "task");
    assert_eq!(FactType::Observation.as_str(), "observation");
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
        assert_eq!(ft, back);
    }
}

#[test]
fn empty_content_classified_as_discussion() {
    assert_eq!(classify_turn(""), TurnType::Discussion);
}

#[test]
fn tool_heavy_empty_content_is_not_tool_heavy() {
    assert!(!is_tool_heavy(""));
}

#[test]
fn classify_fact_empty_is_observation() {
    assert_eq!(classify_fact(""), FactType::Observation);
}

// -- Integration-style test --

#[test]
fn full_pipeline_correction_turn() {
    let content = "Actually, it's not Python — I use Rust for backend work. \
        I was wrong about using Django. The framework I actually use is Axum.";

    // 1. Classify turn
    let turn_type = classify_turn(content);
    assert_eq!(turn_type, TurnType::Correction);

    // 2. Detect correction
    let correction = detect_correction(content);
    assert!(correction.is_correction);

    // 3. Classify extracted facts
    let fact = "I use Rust for backend work";
    let fact_type = classify_fact(fact);
    assert_eq!(fact_type, FactType::Skill);

    // 4. Apply confidence boost
    let base_confidence = 0.8;
    let boosted = boosted_confidence(
        base_confidence,
        turn_type.confidence_boost() + correction.confidence_boost,
    );
    // 0.8 + 0.2 (turn) + 0.2 (correction) = 1.0 (capped)
    assert!((boosted - 1.0).abs() < f64::EPSILON);

    // 5. Quality filter passes
    let filter = filter_fact(fact, boosted);
    assert!(filter.passed);
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
    assert_eq!(turn_type, TurnType::ToolHeavy);
    assert!((turn_type.confidence_boost()).abs() < f64::EPSILON);

    // Appendix should mention skipping raw output
    let appendix = turn_type.prompt_appendix();
    assert!(appendix.contains("Skip"));
    assert!(appendix.contains("decision"));
}
