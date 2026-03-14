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
        existing_fact_id: FactId::from(id),
        existing_content: content.to_owned(),
        existing_confidence: confidence,
        existing_tier: tier,
        cosine_similarity: similarity,
    }
}

// --- Classification parsing ---

#[test]
fn parse_classification_contradicts() {
    assert_eq!(
        ConflictClassification::parse("CONTRADICTS"),
        Some(ConflictClassification::Contradicts)
    );
    assert_eq!(
        ConflictClassification::parse("  contradicts  "),
        Some(ConflictClassification::Contradicts)
    );
}

#[test]
fn parse_classification_refines() {
    assert_eq!(
        ConflictClassification::parse("REFINES"),
        Some(ConflictClassification::Refines)
    );
}

#[test]
fn parse_classification_supplements() {
    assert_eq!(
        ConflictClassification::parse("SUPPLEMENTS"),
        Some(ConflictClassification::Supplements)
    );
}

#[test]
fn parse_classification_unrelated() {
    assert_eq!(
        ConflictClassification::parse("UNRELATED"),
        Some(ConflictClassification::Unrelated)
    );
}

#[test]
fn parse_classification_invalid() {
    assert_eq!(ConflictClassification::parse("UNKNOWN_TYPE"), None);
    assert_eq!(ConflictClassification::parse(""), None);
}

// --- Cosine similarity ---

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
    assert!((sim - 0.0).abs() < f64::EPSILON);
}

#[test]
fn cosine_similarity_different_lengths() {
    let sim = cosine_similarity(&[1.0], &[1.0, 2.0]);
    assert!((sim - 0.0).abs() < f64::EPSILON);
}

#[test]
fn cosine_similarity_antiparallel() {
    // Anti-parallel vectors (exactly opposite direction) should give -1.0.
    // This verifies the dot product sign is preserved and norms are not squared.
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
    // Scaling one vector must not change the cosine similarity.
    // This verifies the denominator uses norms (not squared norms).
    let a = vec![1.0, 2.0, 3.0];
    let b = vec![2.0, 4.0, 6.0]; // b = 2 * a → same direction
    let sim = cosine_similarity(&a, &b);
    assert!(
        (sim - 1.0).abs() < 1e-6,
        "parallel vectors (one scaled) should have sim ~1.0, got {sim}"
    );

    // Scaling in both dimensions independently should not change the angle.
    let c = vec![3.0, 0.0];
    let d = vec![0.0, 7.0]; // orthogonal regardless of scale
    let sim2 = cosine_similarity(&c, &d);
    assert!(
        sim2.abs() < 1e-6,
        "orthogonal vectors with unequal magnitudes should have sim ~0.0, got {sim2}"
    );
}

// --- Intra-batch dedup ---

#[test]
fn intra_batch_dedup_exact_string_match() {
    let facts = vec![
        make_fact("alice works at acme", 0.8, vec![1.0, 0.0, 0.0]),
        make_fact("alice works at acme", 0.9, vec![1.0, 0.0, 0.0]),
    ];
    let (kept, dropped) = intra_batch_dedup(facts);
    assert_eq!(kept.len(), 1);
    assert_eq!(dropped, 1);
    assert!(
        (kept[0].confidence - 0.9).abs() < f64::EPSILON,
        "highest confidence wins"
    );
}

#[test]
fn intra_batch_dedup_cosine_similar() {
    // Nearly identical embeddings (sim > 0.95)
    let facts = vec![
        make_fact("alice works at acme corp", 0.7, vec![1.0, 0.0, 0.01]),
        make_fact("alice is employed at acme", 0.85, vec![1.0, 0.0, 0.0]),
    ];
    let (kept, dropped) = intra_batch_dedup(facts);
    assert_eq!(kept.len(), 1);
    assert_eq!(dropped, 1);
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
    assert_eq!(kept.len(), 2);
    assert_eq!(dropped, 0);
}

#[test]
fn intra_batch_dedup_empty() {
    let (kept, dropped) = intra_batch_dedup(vec![]);
    assert!(kept.is_empty());
    assert_eq!(dropped, 0);
}

#[test]
fn intra_batch_dedup_single() {
    let facts = vec![make_fact("sole fact", 0.5, vec![1.0])];
    let (kept, dropped) = intra_batch_dedup(facts);
    assert_eq!(kept.len(), 1);
    assert_eq!(dropped, 0);
}

// --- Action resolution ---

#[test]
fn resolve_contradicts_new_higher_confidence() {
    let candidate = make_candidate("f-old", "old claim", 0.7, EpistemicTier::Inferred, 0.9);
    let fact = make_fact("new claim", 0.9, vec![]);
    let action = resolve_action(&ConflictClassification::Contradicts, &candidate, &fact);
    assert_eq!(
        action,
        ConflictAction::Supersede {
            old_id: FactId::from("f-old")
        }
    );
}

#[test]
fn resolve_contradicts_new_lower_confidence() {
    let candidate = make_candidate("f-old", "old claim", 0.95, EpistemicTier::Inferred, 0.9);
    let fact = make_fact("new claim", 0.5, vec![]);
    let action = resolve_action(&ConflictClassification::Contradicts, &candidate, &fact);
    assert_eq!(action, ConflictAction::Drop);
}

#[test]
fn resolve_contradicts_equal_confidence_new_wins() {
    let candidate = make_candidate("f-old", "old claim", 0.8, EpistemicTier::Inferred, 0.9);
    let fact = make_fact("new claim", 0.8, vec![]);
    let action = resolve_action(&ConflictClassification::Contradicts, &candidate, &fact);
    assert_eq!(
        action,
        ConflictAction::Supersede {
            old_id: FactId::from("f-old")
        }
    );
}

#[test]
fn resolve_refines_supersedes() {
    let candidate = make_candidate("f-old", "general claim", 0.8, EpistemicTier::Inferred, 0.85);
    let fact = make_fact("specific claim", 0.9, vec![]);
    let action = resolve_action(&ConflictClassification::Refines, &candidate, &fact);
    assert_eq!(
        action,
        ConflictAction::Supersede {
            old_id: FactId::from("f-old")
        }
    );
}

#[test]
fn resolve_supplements_inserts() {
    let candidate = make_candidate("f-old", "existing claim", 0.8, EpistemicTier::Inferred, 0.8);
    let fact = make_fact("additional info", 0.7, vec![]);
    let action = resolve_action(&ConflictClassification::Supplements, &candidate, &fact);
    assert_eq!(action, ConflictAction::Insert);
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
    let action = resolve_action(&ConflictClassification::Unrelated, &candidate, &fact);
    assert_eq!(action, ConflictAction::Insert);
}

// --- Tier protection ---

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
    let action = resolve_action(&ConflictClassification::Contradicts, &candidate, &fact);
    assert_eq!(action, ConflictAction::Drop);
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
    let action = resolve_action(&ConflictClassification::Refines, &candidate, &fact);
    assert_eq!(action, ConflictAction::Drop);
}

#[test]
fn verified_can_be_superseded_by_verified() {
    let candidate = make_candidate("f-old", "old verified", 0.7, EpistemicTier::Verified, 0.9);
    let fact = make_fact_with_tier("new verified", 0.95, EpistemicTier::Verified, vec![]);
    let action = resolve_action(&ConflictClassification::Contradicts, &candidate, &fact);
    assert_eq!(
        action,
        ConflictAction::Supersede {
            old_id: FactId::from("f-old")
        }
    );
}

#[test]
fn verified_can_be_superseded_by_inferred() {
    let candidate = make_candidate("f-old", "old verified", 0.7, EpistemicTier::Verified, 0.9);
    let fact = make_fact_with_tier("new inferred", 0.95, EpistemicTier::Inferred, vec![]);
    let action = resolve_action(&ConflictClassification::Contradicts, &candidate, &fact);
    assert_eq!(
        action,
        ConflictAction::Supersede {
            old_id: FactId::from("f-old")
        }
    );
}

// --- Correction detection ---

#[test]
fn correction_heuristic_detects_patterns() {
    assert!(is_correction_heuristic("Actually, it's 42 not 43"));
    assert!(is_correction_heuristic("I was wrong about the date"));
    assert!(is_correction_heuristic("Correction: the value is 100"));
    assert!(is_correction_heuristic("I was mistaken about that"));
    assert!(is_correction_heuristic(
        "that's incorrect, the real answer is X"
    ));
}

#[test]
fn correction_heuristic_rejects_normal() {
    assert!(!is_correction_heuristic("alice works at acme corp"));
    assert!(!is_correction_heuristic("the project uses rust"));
    assert!(!is_correction_heuristic(""));
}

#[test]
fn correction_boost_adds_02() {
    assert!((apply_correction_boost(0.5) - 0.7).abs() < f64::EPSILON);
    assert!((apply_correction_boost(0.8) - 1.0).abs() < f64::EPSILON);
}

#[test]
fn correction_boost_caps_at_1() {
    assert!((apply_correction_boost(0.9) - 1.0).abs() < f64::EPSILON);
    assert!((apply_correction_boost(1.0) - 1.0).abs() < f64::EPSILON);
}

// --- Correction in conflict resolution ---

#[test]
fn correction_fact_wins_contradiction_regardless_of_confidence() {
    let candidate = make_candidate("f-old", "old claim", 0.95, EpistemicTier::Inferred, 0.9);
    let mut fact = make_fact("corrected claim", 0.5, vec![]);
    fact.is_correction = true;
    let action = resolve_action(&ConflictClassification::Contradicts, &candidate, &fact);
    assert_eq!(
        action,
        ConflictAction::Supersede {
            old_id: FactId::from("f-old")
        }
    );
}

// --- Mock classifier for integration tests ---

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
    assert!(result.is_none());
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
    assert!(result.is_some());
    let (classification, idx) = result.expect("result must be Some when candidates are present");
    assert_eq!(classification, ConflictClassification::Refines);
    assert_eq!(idx, 0);
}

#[test]
fn classify_each_type_produces_correct_action() {
    for (response, expected_action) in [
        (
            "CONTRADICTS",
            ConflictAction::Supersede {
                old_id: FactId::from("f-1"),
            },
        ),
        (
            "REFINES",
            ConflictAction::Supersede {
                old_id: FactId::from("f-1"),
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
        let action = resolve_action(&classification, &candidates[idx], &fact);
        assert_eq!(action, expected_action, "failed for {response}");
    }
}

#[test]
fn no_candidates_results_in_insert() {
    // Simulates Phase 2 returning empty candidates → straight insert
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

// --- Build classification prompt ---

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
    assert!(system.contains("CONTRADICTS"));
    assert!(system.contains("REFINES"));
    assert!(system.contains("SUPPLEMENTS"));
    assert!(system.contains("UNRELATED"));
    assert!(user.contains("alice works at acme"));
    assert!(user.contains("alice works at globex"));
    assert!(user.contains("0.80"));
    assert!(user.contains("0.90"));
}

// --- ConflictAction equality ---

#[test]
fn conflict_action_equality() {
    assert_eq!(ConflictAction::Insert, ConflictAction::Insert);
    assert_eq!(ConflictAction::Drop, ConflictAction::Drop);
    assert_eq!(
        ConflictAction::Supersede {
            old_id: FactId::from("a")
        },
        ConflictAction::Supersede {
            old_id: FactId::from("a")
        }
    );
    assert_ne!(ConflictAction::Insert, ConflictAction::Drop);
}

// --- Serde roundtrip ---

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
        assert_eq!(class, back);
    }
}
