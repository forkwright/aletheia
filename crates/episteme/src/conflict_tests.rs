#![expect(clippy::expect_used, reason = "test assertions")]
use super::*;

fn make_fact(content: &str, confidence: f64, embedding: Vec<f32>) -> FactForConflictCheck {
    FactForConflictCheck {
        content: content.to_owned(),
        confidence,
        tier: EpistemicTier::Inferred,
        subject: "alice".to_owned(),
        is_correction: false,
        embedding,
    }
}

fn make_fact_with_tier(
    content: &str,
    confidence: f64,
    tier: EpistemicTier,
    embedding: Vec<f32>,
) -> FactForConflictCheck {
    FactForConflictCheck {
        content: content.to_owned(),
        confidence,
        tier,
        subject: "alice".to_owned(),
        is_correction: false,
        embedding,
    }
}

fn make_candidate(
    id: &str,
    content: &str,
    confidence: f64,
    tier: EpistemicTier,
    similarity: f64,
) -> ConflictCandidate {
    ConflictCandidate {
        existing_fact_id: FactId::new(id).expect("valid test id"),
        existing_content: content.to_owned(),
        existing_confidence: confidence,
        existing_tier: tier,
        cosine_similarity: similarity,
    }
}

#[test]
fn parse_classification_contradicts() {
    assert_eq!(
        ConflictClassification::parse("CONTRADICTS"),
        Some(ConflictClassification::Contradicts),
        "parse classification contradicts: values should be equal"
    );
    assert_eq!(
        ConflictClassification::parse("  contradicts  "),
        Some(ConflictClassification::Contradicts),
        "parse classification contradicts: values should be equal"
    );
}

#[test]
fn parse_classification_refines() {
    assert_eq!(
        ConflictClassification::parse("REFINES"),
        Some(ConflictClassification::Refines),
        "parse classification refines: values should be equal"
    );
}

#[test]
fn parse_classification_supplements() {
    assert_eq!(
        ConflictClassification::parse("SUPPLEMENTS"),
        Some(ConflictClassification::Supplements),
        "parse classification supplements: values should be equal"
    );
}

#[test]
fn parse_classification_unrelated() {
    assert_eq!(
        ConflictClassification::parse("UNRELATED"),
        Some(ConflictClassification::Unrelated),
        "parse classification unrelated: values should be equal"
    );
}

#[test]
fn parse_classification_invalid() {
    assert_eq!(
        ConflictClassification::parse("UNKNOWN_TYPE"),
        None,
        "parse classification invalid: values should be equal"
    );
    assert_eq!(
        ConflictClassification::parse(""),
        None,
        "parse classification invalid: values should be equal"
    );
}

#[test]
fn cosine_similarity_identical() {
    let v = vec![1.0, 2.0, 3.0];
    let sim = cosine_similarity(&v, &v);
    assert!(
        (sim - 1.0).abs() < 1e-6,
        "identical vectors should have sim ~1.0"
    );
}

#[test]
fn cosine_similarity_orthogonal() {
    let a = vec![1.0, 0.0];
    let b = vec![0.0, 1.0];
    let sim = cosine_similarity(&a, &b);
    assert!(sim.abs() < 1e-6, "orthogonal vectors should have sim ~0.0");
}

#[test]
fn cosine_similarity_empty() {
    let sim = cosine_similarity(&[], &[]);
    assert!(
        (sim - 0.0).abs() < f64::EPSILON,
        "cosine similarity empty: assertion failed"
    );
}

#[test]
fn cosine_similarity_different_lengths() {
    let sim = cosine_similarity(&[1.0], &[1.0, 2.0]);
    assert!(
        (sim - 0.0).abs() < f64::EPSILON,
        "cosine similarity different lengths: assertion failed"
    );
}

#[test]
fn cosine_similarity_antiparallel() {
    let a = vec![1.0, 0.0, 0.0];
    let b = vec![-1.0, 0.0, 0.0];
    let sim = cosine_similarity(&a, &b);
    assert!(
        (sim - (-1.0)).abs() < 1e-6,
        "anti-parallel vectors should have sim ~-1.0, got {sim}"
    );
}

#[test]
fn cosine_similarity_scale_invariant() {
    // WHY: Scaling one vector must not change the cosine similarity.
    // This verifies the denominator uses norms (not squared norms).
    let a = vec![1.0, 2.0, 3.0];
    let b = vec![2.0, 4.0, 6.0]; // b = 2 * a → same direction
    let sim = cosine_similarity(&a, &b);
    assert!(
        (sim - 1.0).abs() < 1e-6,
        "parallel vectors (one scaled) should have sim ~1.0, got {sim}"
    );

    let c = vec![3.0, 0.0];
    let d = vec![0.0, 7.0]; // orthogonal regardless of scale
    let sim2 = cosine_similarity(&c, &d);
    assert!(
        sim2.abs() < 1e-6,
        "orthogonal vectors with unequal magnitudes should have sim ~0.0, got {sim2}"
    );
}

#[test]
fn intra_batch_dedup_exact_string_match() {
    let facts = vec![
        make_fact("alice works at acme", 0.8, vec![1.0, 0.0, 0.0]),
        make_fact("alice works at acme", 0.9, vec![1.0, 0.0, 0.0]),
    ];
    let (kept, dropped) = intra_batch_dedup(facts);
    assert_eq!(
        kept.len(),
        1,
        "intra batch dedup exact string match: values should be equal"
    );
    assert_eq!(
        dropped, 1,
        "intra batch dedup exact string match: values should be equal"
    );
    assert!(
        (kept[0].confidence - 0.9).abs() < f64::EPSILON,
        "highest confidence wins"
    );
}

#[test]
fn intra_batch_dedup_cosine_similar() {
    let facts = vec![
        make_fact("alice works at acme corp", 0.7, vec![1.0, 0.0, 0.01]),
        make_fact("alice is employed at acme", 0.85, vec![1.0, 0.0, 0.0]),
    ];
    let (kept, dropped) = intra_batch_dedup(facts);
    assert_eq!(
        kept.len(),
        1,
        "intra batch dedup cosine similar: values should be equal"
    );
    assert_eq!(
        dropped, 1,
        "intra batch dedup cosine similar: values should be equal"
    );
    assert!(
        (kept[0].confidence - 0.85).abs() < f64::EPSILON,
        "highest confidence wins"
    );
}

#[test]
fn intra_batch_dedup_different_facts_preserved() {
    let facts = vec![
        make_fact("alice works at acme", 0.8, vec![1.0, 0.0, 0.0]),
        make_fact("bob lives in london", 0.9, vec![0.0, 1.0, 0.0]),
    ];
    let (kept, dropped) = intra_batch_dedup(facts);
    assert_eq!(
        kept.len(),
        2,
        "intra batch dedup different facts preserved: values should be equal"
    );
    assert_eq!(
        dropped, 0,
        "intra batch dedup different facts preserved: values should be equal"
    );
}

#[test]
fn intra_batch_dedup_empty() {
    let (kept, dropped) = intra_batch_dedup(vec![]);
    assert!(kept.is_empty(), "intra batch dedup empty: expected empty");
    assert_eq!(
        dropped, 0,
        "intra batch dedup empty: values should be equal"
    );
}

#[test]
fn intra_batch_dedup_single() {
    let facts = vec![make_fact("sole fact", 0.5, vec![1.0])];
    let (kept, dropped) = intra_batch_dedup(facts);
    assert_eq!(
        kept.len(),
        1,
        "intra batch dedup single: values should be equal"
    );
    assert_eq!(
        dropped, 0,
        "intra batch dedup single: values should be equal"
    );
}

#[test]
fn resolve_contradicts_new_higher_confidence() {
    let candidate = make_candidate("f-old", "old claim", 0.7, EpistemicTier::Inferred, 0.9);
    let fact = make_fact("new claim", 0.9, vec![]);
    let action = resolve_action(
        &ConflictClassification::Contradicts,
        &candidate,
        &fact,
        1,
        1,
    );
    assert_eq!(
        action,
        ConflictAction::Supersede {
            old_id: FactId::new("f-old").expect("valid test id")
        },
        "resolve contradicts new higher confidence: values should be equal"
    );
}

/// #4415: on an equal-confidence contradiction, the fact with more independent
/// converging sources wins the tie; equal multiplicity falls back to recency.
#[test]
fn resolve_contradicts_equal_confidence_breaks_on_multiplicity() {
    let candidate = make_candidate(
        "f-consolidated",
        "established",
        0.8,
        EpistemicTier::Inferred,
        0.9,
    );
    let fact = make_fact("new singleton claim", 0.8, vec![]);

    // Existing fact consolidated from 5 sources vs a single new observation:
    // the existing converging evidence wins, so the new fact is dropped.
    let action = resolve_action(
        &ConflictClassification::Contradicts,
        &candidate,
        &fact,
        5,
        1,
    );
    assert_eq!(
        action,
        ConflictAction::Drop,
        "higher-multiplicity existing fact should win the equal-confidence tie"
    );

    // Equal multiplicity falls back to the recency default (new supersedes).
    let tie = resolve_action(
        &ConflictClassification::Contradicts,
        &candidate,
        &fact,
        1,
        1,
    );
    assert_eq!(
        tie,
        ConflictAction::Supersede {
            old_id: FactId::new("f-consolidated").expect("valid test id")
        },
        "equal multiplicity should fall back to recency (new wins)"
    );
}

#[test]
fn resolve_contradicts_new_lower_confidence() {
    let candidate = make_candidate("f-old", "old claim", 0.95, EpistemicTier::Inferred, 0.9);
    let fact = make_fact("new claim", 0.5, vec![]);
    let action = resolve_action(
        &ConflictClassification::Contradicts,
        &candidate,
        &fact,
        1,
        1,
    );
    assert_eq!(
        action,
        ConflictAction::Drop,
        "resolve contradicts new lower confidence: values should be equal"
    );
}

#[test]
fn resolve_contradicts_equal_confidence_new_wins() {
    let candidate = make_candidate("f-old", "old claim", 0.8, EpistemicTier::Inferred, 0.9);
    let fact = make_fact("new claim", 0.8, vec![]);
    let action = resolve_action(
        &ConflictClassification::Contradicts,
        &candidate,
        &fact,
        1,
        1,
    );
    assert_eq!(
        action,
        ConflictAction::Supersede {
            old_id: FactId::new("f-old").expect("valid test id")
        },
        "resolve contradicts equal confidence new wins: values should be equal"
    );
}

#[test]
fn resolve_refines_supersedes() {
    let candidate = make_candidate("f-old", "general claim", 0.8, EpistemicTier::Inferred, 0.85);
    let fact = make_fact("specific claim", 0.9, vec![]);
    let action = resolve_action(&ConflictClassification::Refines, &candidate, &fact, 1, 1);
    assert_eq!(
        action,
        ConflictAction::Supersede {
            old_id: FactId::new("f-old").expect("valid test id")
        },
        "resolve refines supersedes: values should be equal"
    );
}

#[test]
fn resolve_supplements_inserts() {
    let candidate = make_candidate("f-old", "existing claim", 0.8, EpistemicTier::Inferred, 0.8);
    let fact = make_fact("additional info", 0.7, vec![]);
    let action = resolve_action(
        &ConflictClassification::Supplements,
        &candidate,
        &fact,
        1,
        1,
    );
    assert_eq!(
        action,
        ConflictAction::Insert,
        "resolve supplements inserts: values should be equal"
    );
}

#[test]
fn resolve_unrelated_inserts() {
    let candidate = make_candidate(
        "f-old",
        "existing claim",
        0.8,
        EpistemicTier::Inferred,
        0.75,
    );
    let fact = make_fact("different topic", 0.9, vec![]);
    let action = resolve_action(&ConflictClassification::Unrelated, &candidate, &fact, 1, 1);
    assert_eq!(
        action,
        ConflictAction::Insert,
        "resolve unrelated inserts: values should be equal"
    );
}

#[test]
fn verified_not_superseded_by_assumed_contradicts() {
    let candidate = make_candidate(
        "f-verified",
        "verified fact",
        0.7,
        EpistemicTier::Verified,
        0.9,
    );
    let fact = make_fact_with_tier(
        "assumed contradiction",
        0.95,
        EpistemicTier::Assumed,
        vec![],
    );
    let action = resolve_action(
        &ConflictClassification::Contradicts,
        &candidate,
        &fact,
        1,
        1,
    );
    assert_eq!(
        action,
        ConflictAction::Drop,
        "verified not superseded by assumed contradicts: values should be equal"
    );
}

#[test]
fn verified_not_superseded_by_assumed_refines() {
    let candidate = make_candidate(
        "f-verified",
        "verified fact",
        0.7,
        EpistemicTier::Verified,
        0.9,
    );
    let fact = make_fact_with_tier("assumed refinement", 0.95, EpistemicTier::Assumed, vec![]);
    let action = resolve_action(&ConflictClassification::Refines, &candidate, &fact, 1, 1);
    assert_eq!(
        action,
        ConflictAction::Drop,
        "verified not superseded by assumed refines: values should be equal"
    );
}

#[test]
fn verified_can_be_superseded_by_verified() {
    let candidate = make_candidate("f-old", "old verified", 0.7, EpistemicTier::Verified, 0.9);
    let fact = make_fact_with_tier("new verified", 0.95, EpistemicTier::Verified, vec![]);
    let action = resolve_action(
        &ConflictClassification::Contradicts,
        &candidate,
        &fact,
        1,
        1,
    );
    assert_eq!(
        action,
        ConflictAction::Supersede {
            old_id: FactId::new("f-old").expect("valid test id")
        },
        "verified can be superseded by verified: values should be equal"
    );
}

#[test]
fn verified_not_superseded_by_inferred_contradicts() {
    let candidate = make_candidate("f-old", "old verified", 0.7, EpistemicTier::Verified, 0.9);
    let fact = make_fact_with_tier("new inferred", 0.95, EpistemicTier::Inferred, vec![]);
    let action = resolve_action(
        &ConflictClassification::Contradicts,
        &candidate,
        &fact,
        1,
        1,
    );
    assert_eq!(
        action,
        ConflictAction::Drop,
        "verified not superseded by inferred contradicts: values should be equal"
    );
}

#[test]
fn verified_not_superseded_by_inferred_refines() {
    let candidate = make_candidate("f-old", "old verified", 0.7, EpistemicTier::Verified, 0.9);
    let fact = make_fact_with_tier(
        "new inferred refinement",
        0.95,
        EpistemicTier::Inferred,
        vec![],
    );
    let action = resolve_action(&ConflictClassification::Refines, &candidate, &fact, 1, 1);
    assert_eq!(
        action,
        ConflictAction::Drop,
        "verified not superseded by inferred refines: values should be equal"
    );
}

#[test]
fn correction_heuristic_detects_patterns() {
    assert!(
        is_correction_heuristic("Actually, it's 42 not 43"),
        "correction heuristic detects patterns: assertion failed"
    );
    assert!(
        is_correction_heuristic("I was wrong about the date"),
        "correction heuristic detects patterns: assertion failed"
    );
    assert!(
        is_correction_heuristic("Correction: the value is 100"),
        "correction heuristic detects patterns: assertion failed"
    );
    assert!(
        is_correction_heuristic("I was mistaken about that"),
        "correction heuristic detects patterns: assertion failed"
    );
    assert!(
        is_correction_heuristic("that's incorrect, the real answer is X"),
        "correction heuristic detects patterns: assertion failed"
    );
}

#[test]
fn correction_heuristic_rejects_normal() {
    assert!(
        !is_correction_heuristic("alice works at acme corp"),
        "correction heuristic rejects normal: assertion failed"
    );
    assert!(
        !is_correction_heuristic("the project uses rust"),
        "correction heuristic rejects normal: assertion failed"
    );
    assert!(
        !is_correction_heuristic(""),
        "correction heuristic rejects normal: assertion failed"
    );
}

#[test]
fn correction_boost_adds_02() {
    assert!(
        (apply_correction_boost(0.5) - 0.7).abs() < f64::EPSILON,
        "correction boost adds 02: assertion failed"
    );
    assert!(
        (apply_correction_boost(0.8) - 1.0).abs() < f64::EPSILON,
        "correction boost adds 02: assertion failed"
    );
}

#[test]
fn correction_boost_caps_at_1() {
    assert!(
        (apply_correction_boost(0.9) - 1.0).abs() < f64::EPSILON,
        "correction boost caps at 1: assertion failed"
    );
    assert!(
        (apply_correction_boost(1.0) - 1.0).abs() < f64::EPSILON,
        "correction boost caps at 1: assertion failed"
    );
}

#[test]
fn correction_fact_wins_contradiction_regardless_of_confidence() {
    let candidate = make_candidate("f-old", "old claim", 0.95, EpistemicTier::Inferred, 0.9);
    let mut fact = make_fact("corrected claim", 0.5, vec![]);
    fact.is_correction = true;
    let action = resolve_action(
        &ConflictClassification::Contradicts,
        &candidate,
        &fact,
        1,
        1,
    );
    assert_eq!(
        action,
        ConflictAction::Supersede {
            old_id: FactId::new("f-old").expect("valid test id")
        },
        "correction fact wins contradiction regardless of confidence: values should be equal"
    );
}

struct MockClassifier {
    response: String,
}

impl ConflictClassifier for MockClassifier {
    fn classify(
        &self,
        _existing_content: &str,
        _existing_confidence: f64,
        _existing_tier: &str,
        _new_content: &str,
        _new_confidence: f64,
        _new_tier: &str,
    ) -> Result<String, ConflictError> {
        Ok(self.response.clone())
    }
}

#[test]
fn classify_no_candidates_returns_none() {
    let classifier = MockClassifier {
        response: "CONTRADICTS".to_owned(),
    };
    let fact = make_fact("test", 0.8, vec![1.0]);
    let result = classify_against_candidates(&classifier, &fact, &[])
        .expect("classify_against_candidates must not fail");
    assert!(
        result.is_none(),
        "classify no candidates returns none: expected None"
    );
}

#[test]
fn classify_returns_classification() {
    let classifier = MockClassifier {
        response: "REFINES".to_owned(),
    };
    let fact = make_fact("test", 0.8, vec![1.0]);
    let candidates = vec![make_candidate(
        "f-1",
        "existing",
        0.7,
        EpistemicTier::Inferred,
        0.85,
    )];
    let result = classify_against_candidates(&classifier, &fact, &candidates)
        .expect("classify must succeed");
    assert!(
        result.is_some(),
        "classify returns classification: expected Some"
    );
    let (classification, idx) = result.expect("result must be Some when candidates are present");
    assert_eq!(
        classification,
        ConflictClassification::Refines,
        "classify returns classification: values should be equal"
    );
    assert_eq!(
        idx, 0,
        "classify returns classification: values should be equal"
    );
}

#[test]
fn classify_malformed_response_returns_unclassifiable_error() {
    let classifier = MockClassifier {
        response: "I cannot determine the relationship".to_owned(),
    };
    let fact = make_fact("alice works at globex", 0.8, vec![1.0]);
    let candidates = vec![make_candidate(
        "f-1",
        "alice works at acme",
        0.7,
        EpistemicTier::Inferred,
        0.85,
    )];

    let err = classify_against_candidates(&classifier, &fact, &candidates)
        .expect_err("malformed classifier output must fail closed");

    assert!(
        matches!(
            err,
            ConflictError::Unclassifiable {
                ref response_snippet,
                threshold,
                ..
            } if response_snippet.contains("cannot determine")
                && (threshold - DEFAULT_UNCLASSIFIABLE_RATE_THRESHOLD).abs() < f64::EPSILON
        ),
        "expected unclassifiable error, got {err:?}"
    );
}

#[test]
fn classify_each_type_produces_correct_action() {
    for (response, expected_action) in [
        (
            "CONTRADICTS",
            ConflictAction::Supersede {
                old_id: FactId::new("f-1").expect("valid test id"),
            },
        ),
        (
            "REFINES",
            ConflictAction::Supersede {
                old_id: FactId::new("f-1").expect("valid test id"),
            },
        ),
        ("SUPPLEMENTS", ConflictAction::Insert),
        ("UNRELATED", ConflictAction::Insert),
    ] {
        let classifier = MockClassifier {
            response: response.to_owned(),
        };
        let fact = make_fact("new fact", 0.9, vec![1.0]);
        let candidates = vec![make_candidate(
            "f-1",
            "existing",
            0.7,
            EpistemicTier::Inferred,
            0.85,
        )];
        let (classification, idx) = classify_against_candidates(&classifier, &fact, &candidates)
            .expect("classify must succeed")
            .expect("must be Some when candidates exist");
        let action = resolve_action(&classification, &candidates[idx], &fact, 1, 1);
        assert_eq!(action, expected_action, "failed for {response}");
    }
}

#[test]
fn no_candidates_results_in_insert() {
    // NOTE: Simulates Phase 2 returning empty candidates → straight insert
    let fact = make_fact("brand new fact with no matches", 0.8, vec![1.0, 0.0]);
    let candidates: Vec<ConflictCandidate> = vec![];
    let result = classify_against_candidates(
        &MockClassifier {
            response: "CONTRADICTS".to_owned(),
        },
        &fact,
        &candidates,
    )
    .expect("classify must succeed with empty candidates");
    assert!(
        result.is_none(),
        "no candidates should return None (insert)"
    );
}

#[test]
fn classification_prompt_contains_facts() {
    let (system, user) = build_classification_prompt(
        "alice works at acme",
        0.8,
        "inferred",
        "alice works at globex",
        0.9,
        "inferred",
    );
    assert!(
        system.contains("CONTRADICTS"),
        "classification prompt contains facts: expected to contain value"
    );
    assert!(
        system.contains("REFINES"),
        "classification prompt contains facts: expected to contain value"
    );
    assert!(
        system.contains("SUPPLEMENTS"),
        "classification prompt contains facts: expected to contain value"
    );
    assert!(
        system.contains("UNRELATED"),
        "classification prompt contains facts: expected to contain value"
    );
    assert!(
        user.contains("alice works at acme"),
        "classification prompt contains facts: expected to contain value"
    );
    assert!(
        user.contains("alice works at globex"),
        "classification prompt contains facts: expected to contain value"
    );
    assert!(
        user.contains("0.80"),
        "classification prompt contains facts: expected to contain value"
    );
    assert!(
        user.contains("0.90"),
        "classification prompt contains facts: expected to contain value"
    );
}

#[test]
fn conflict_action_equality() {
    assert_eq!(
        ConflictAction::Insert,
        ConflictAction::Insert,
        "conflict action equality: values should be equal"
    );
    assert_eq!(
        ConflictAction::Drop,
        ConflictAction::Drop,
        "conflict action equality: values should be equal"
    );
    assert_eq!(
        ConflictAction::Supersede {
            old_id: FactId::new("a").expect("valid test id")
        },
        ConflictAction::Supersede {
            old_id: FactId::new("a").expect("valid test id")
        },
        "conflict action equality: values should be equal"
    );
    assert_ne!(
        ConflictAction::Insert,
        ConflictAction::Drop,
        "conflict action equality: values should differ"
    );
}

#[test]
fn classification_serde_roundtrip() {
    for class in [
        ConflictClassification::Contradicts,
        ConflictClassification::Refines,
        ConflictClassification::Supplements,
        ConflictClassification::Unrelated,
    ] {
        let json = serde_json::to_string(&class).expect("serialization must succeed");
        let back: ConflictClassification =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(
            class, back,
            "classification serde roundtrip: values should be equal"
        );
    }
}

/// Mutants surviving the release substance audit, killed here.
///
/// WHY a named module: run `32456630380` evaluated 323 of episteme's mutants and
/// reported 17 `MissedMutant` blockers, every one in this file's subject. Each test
/// below names the exact mutation it kills, so a later reader can tell an
/// intentional boundary from an accident -- and so that removing one is a visible
/// choice rather than a quiet loss of coverage.
mod audit_surfaced_mutants {
    use super::*;

    /// Kills: `replace < with ==` at the `denom < f64::EPSILON` guard.
    ///
    /// A zero vector makes `denom` exactly 0.0, which is BELOW epsilon but not EQUAL
    /// to it. Under `==` the guard does not fire, `dot / denom` is `0.0 / 0.0`, and the
    /// function returns NaN -- which then compares false against every threshold, so a
    /// degenerate embedding would silently never match anything.
    #[test]
    fn a_zero_vector_scores_zero_rather_than_nan() {
        let score = cosine_similarity(&[0.0, 0.0, 0.0], &[1.0, 2.0, 3.0]);
        assert!(!score.is_nan(), "a zero vector must not produce NaN");
        assert!(
            (score - 0.0).abs() < f64::EPSILON,
            "expected exactly 0.0, got {score}"
        );
    }

    /// Kills: `replace < with <=` at the same guard.
    ///
    /// This is the one input where `<` and `<=` disagree, so it is the only thing that
    /// can distinguish them. `2^-52` IS `f64::EPSILON`, and it is exactly representable
    /// in f32, so `denom` lands precisely on the boundary: `norm_a.sqrt()` is `2^-52`
    /// and `norm_b.sqrt()` is 1.0. The guard must NOT fire -- epsilon is a real
    /// magnitude, not a degenerate one.
    #[test]
    fn a_denominator_exactly_at_epsilon_is_not_treated_as_degenerate() {
        // 2^-52 as an f32 bit pattern: sign 0, exponent 127-52 = 75, mantissa 0.
        // Written this way rather than `f64::EPSILON as f32` because the workspace
        // denies `as` conversions -- and the assertion below validates the constant,
        // so a wrong bit pattern fails loudly instead of silently weakening the test.
        let epsilon_as_f32 = f32::from_bits(0x2580_0000);
        // Bit patterns, not values: the claim IS bit-exactness, and `float_cmp` is
        // right that a strict `==` on floats is usually a mistake. Saying it this way
        // states the actual requirement rather than suppressing the lint that noticed.
        assert_eq!(
            f64::from(epsilon_as_f32).to_bits(),
            f64::EPSILON.to_bits(),
            "the fixture is only meaningful if f32 represents f64::EPSILON exactly"
        );

        let score = cosine_similarity(&[epsilon_as_f32], &[1.0]);
        assert!(
            (score - 1.0).abs() < 1e-12,
            "parallel vectors are similarity 1.0 regardless of magnitude, got {score}"
        );
    }

    /// Kills: `replace > with >=` at line 251, the exact-content branch.
    ///
    /// On equal confidence the FIRST fact must win. Under `>=` the later duplicate
    /// replaces it, which is a silent reordering: callers downstream treat position as
    /// arrival order, and two facts with identical content and confidence are
    /// indistinguishable except by which one the batch saw first.
    #[test]
    fn an_equal_confidence_content_duplicate_keeps_the_first() {
        let mut first = make_fact("alice lives in berlin", 0.8, vec![1.0, 0.0]);
        first.subject = "first".to_owned();
        let mut second = make_fact("alice lives in berlin", 0.8, vec![1.0, 0.0]);
        second.subject = "second".to_owned();

        let (kept, dropped) = intra_batch_dedup(vec![first, second]);

        assert_eq!(dropped, 1);
        assert_eq!(kept.len(), 1);
        assert_eq!(
            kept[0].subject, "first",
            "equal confidence must not let a later duplicate displace an earlier one"
        );
    }

    /// Kills: `replace > with >=` at line 259, the embedding-similarity branch.
    ///
    /// Distinct from the test above: that one dedups on `content ==` and never reaches
    /// this branch. Here the contents differ, so dedup happens via cosine similarity
    /// over the threshold, and the same equal-confidence rule has to hold on a
    /// different line.
    #[test]
    fn an_equal_confidence_similar_duplicate_keeps_the_first() {
        let mut first = make_fact("alice lives in berlin", 0.8, vec![1.0, 0.0]);
        first.subject = "first".to_owned();
        let mut second = make_fact("alice resides in berlin", 0.8, vec![1.0, 0.0]);
        second.subject = "second".to_owned();

        assert!(
            cosine_similarity(&first.embedding, &second.embedding)
                >= DEFAULT_INTRA_BATCH_DEDUP_THRESHOLD,
            "the fixture must dedup via similarity, not via identical content"
        );
        assert_ne!(
            first.content, second.content,
            "identical content would short-circuit before the branch under test"
        );

        let (kept, dropped) = intra_batch_dedup(vec![first, second]);

        assert_eq!(dropped, 1);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].subject, "first");
    }

    /// The other direction, for both branches: a HIGHER confidence duplicate must
    /// displace the earlier one. Without this, `>` could be replaced by `false` -- the
    /// tests above would still pass while the replacement rule stopped working.
    #[test]
    fn a_higher_confidence_duplicate_does_displace_the_first() {
        let mut first = make_fact("alice lives in berlin", 0.5, vec![1.0, 0.0]);
        first.subject = "first".to_owned();
        let mut second = make_fact("alice lives in berlin", 0.9, vec![1.0, 0.0]);
        second.subject = "second".to_owned();

        let (kept, dropped) = intra_batch_dedup(vec![first, second]);

        assert_eq!(dropped, 1);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].subject, "second");
        assert!((kept[0].confidence - 0.9).abs() < f64::EPSILON);
    }
}
