//! Optional benchmark/eval hooks for extraction precision/recall.
//!
//! These helpers let benchmarks and integration tests score an [`Extraction`]
//! against a labeled fixture without depending on a live knowledge store. They
//! are intentionally simple string-overlap metrics over the extracted
//! entities, relationships, and facts.

use super::types::{ExtractedEntity, ExtractedFact, ExtractedRelationship, Extraction};

/// Labeled fixture against which to score an extraction.
#[derive(Debug, Clone, Default)]
pub struct LabeledFixture {
    /// Expected entities by name (e.g. `"Alice"`).
    pub expected_entities: Vec<String>,
    /// Expected relationships as `"source relation target"` strings.
    pub expected_relationships: Vec<String>,
    /// Expected facts as `"subject predicate object"` strings.
    pub expected_facts: Vec<String>,
}

/// Precision/recall/F1 scores for an extraction against a labeled fixture.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExtractionScores {
    /// Fraction of extracted items that were expected.
    pub precision: f64,
    /// Fraction of expected items that were extracted.
    pub recall: f64,
    /// Harmonic mean of precision and recall.
    pub f1: f64,
    /// Number of extracted items that matched an expected item.
    pub true_positives: usize,
    /// Number of extracted items with no matching expected item.
    pub false_positives: usize,
    /// Number of expected items with no matching extracted item.
    pub false_negatives: usize,
}

fn canonicalize(s: &str) -> String {
    s.trim().to_lowercase()
}

fn fact_key(fact: &ExtractedFact) -> String {
    format!("{} {} {}", fact.subject, fact.predicate, fact.object)
}

fn relationship_key(rel: &ExtractedRelationship) -> String {
    format!("{} {} {}", rel.source, rel.relation, rel.target)
}

fn entity_key(entity: &ExtractedEntity) -> String {
    entity.name.clone()
}

/// Score an extraction against a labeled fixture.
///
/// Comparison is done on normalized string keys (trimmed, lowercased) so minor
/// casing or spacing differences do not count as errors. Facts are compared as
/// `subject predicate object`; relationships as `source relation target`;
/// entities by name.
pub fn score_extraction(extraction: &Extraction, fixture: &LabeledFixture) -> ExtractionScores {
    let extracted_facts: Vec<String> = extraction
        .facts
        .iter()
        .map(fact_key)
        .map(|s| canonicalize(&s))
        .collect();
    let extracted_relationships: Vec<String> = extraction
        .relationships
        .iter()
        .map(relationship_key)
        .map(|s| canonicalize(&s))
        .collect();
    let extracted_entities: Vec<String> = extraction
        .entities
        .iter()
        .map(entity_key)
        .map(|s| canonicalize(&s))
        .collect();

    let expected_facts: Vec<String> = fixture
        .expected_facts
        .iter()
        .map(|s| canonicalize(s))
        .collect();
    let expected_relationships: Vec<String> = fixture
        .expected_relationships
        .iter()
        .map(|s| canonicalize(s))
        .collect();
    let expected_entities: Vec<String> = fixture
        .expected_entities
        .iter()
        .map(|s| canonicalize(s))
        .collect();

    let extracted: Vec<String> =
        [extracted_facts, extracted_relationships, extracted_entities].concat();
    let expected: Vec<String> =
        [expected_facts, expected_relationships, expected_entities].concat();

    let mut matched_expected = vec![false; expected.len()];
    let mut true_positives = 0usize;
    for item in &extracted {
        if let Some(idx) = expected.iter().position(|exp| exp == item)
            && let Some(flag) = matched_expected.get_mut(idx)
            && !*flag
        {
            *flag = true;
            true_positives += 1;
        }
    }

    let false_positives = extracted.len().saturating_sub(true_positives);
    let false_negatives = expected.len().saturating_sub(true_positives);

    let to_f64 = |n: usize| f64::from(u32::try_from(n).unwrap_or(u32::MAX));
    let precision = if extracted.is_empty() {
        if expected.is_empty() { 1.0 } else { 0.0 }
    } else {
        to_f64(true_positives) / to_f64(extracted.len())
    };
    let recall = if expected.is_empty() {
        1.0
    } else {
        to_f64(true_positives) / to_f64(expected.len())
    };
    let f1 = if precision + recall > 0.0 {
        2.0 * precision * recall / (precision + recall)
    } else {
        0.0
    };

    ExtractionScores {
        precision,
        recall,
        f1,
        true_positives,
        false_positives,
        false_negatives,
    }
}

#[cfg(test)]
mod tests {
    #![expect(clippy::float_cmp, reason = "test assertions on exact float values")]

    use super::*;

    fn fact(subject: &str, predicate: &str, object: &str) -> ExtractedFact {
        ExtractedFact {
            subject: subject.to_owned(),
            predicate: predicate.to_owned(),
            object: object.to_owned(),
            confidence: 0.9,
            is_correction: false,
            fact_type: None,
        }
    }

    fn entity(name: &str) -> ExtractedEntity {
        ExtractedEntity {
            name: name.to_owned(),
            entity_type: "person".to_owned(),
            description: "test entity".to_owned(),
        }
    }

    #[test]
    fn perfect_match_scores_one() {
        let extraction = Extraction {
            entities: vec![entity("Alice")],
            relationships: vec![],
            facts: vec![fact("Alice", "uses", "Aletheia")],
        };
        let fixture = LabeledFixture {
            expected_entities: vec!["Alice".to_owned()],
            expected_facts: vec!["Alice uses Aletheia".to_owned()],
            expected_relationships: vec![],
        };
        let scores = score_extraction(&extraction, &fixture);
        assert_eq!(scores.precision, 1.0);
        assert_eq!(scores.recall, 1.0);
        assert_eq!(scores.f1, 1.0);
        assert_eq!(scores.true_positives, 2);
        assert_eq!(scores.false_positives, 0);
        assert_eq!(scores.false_negatives, 0);
    }

    #[test]
    fn partial_match_scores_correctly() {
        let extraction = Extraction {
            entities: vec![],
            relationships: vec![],
            facts: vec![fact("Alice", "likes", "Rust")],
        };
        let fixture = LabeledFixture {
            expected_entities: vec![],
            expected_facts: vec!["Alice likes Rust".to_owned(), "Bob likes Python".to_owned()],
            expected_relationships: vec![],
        };
        let scores = score_extraction(&extraction, &fixture);
        assert_eq!(scores.true_positives, 1);
        assert_eq!(scores.false_positives, 0);
        assert_eq!(scores.false_negatives, 1);
        assert_eq!(scores.precision, 1.0);
        assert_eq!(scores.recall, 0.5);
    }

    #[test]
    fn empty_extraction_against_empty_fixture_is_perfect() {
        let extraction = Extraction {
            entities: vec![],
            relationships: vec![],
            facts: vec![],
        };
        let fixture = LabeledFixture::default();
        let scores = score_extraction(&extraction, &fixture);
        assert_eq!(scores.precision, 1.0);
        assert_eq!(scores.recall, 1.0);
        assert_eq!(scores.f1, 1.0);
    }

    #[test]
    fn extra_extractions_count_as_false_positives() {
        let extraction = Extraction {
            entities: vec![],
            relationships: vec![],
            facts: vec![fact("Alice", "likes", "Rust")],
        };
        let fixture = LabeledFixture::default();
        let scores = score_extraction(&extraction, &fixture);
        assert_eq!(scores.true_positives, 0);
        assert_eq!(scores.false_positives, 1);
        assert_eq!(scores.false_negatives, 0);
        assert_eq!(scores.precision, 0.0);
        assert_eq!(scores.recall, 1.0);
    }
}
