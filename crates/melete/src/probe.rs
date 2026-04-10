//! Backward-path probe QA: verify distilled facts before committing to the
//! knowledge graph.
//!
//! Based on MemMA (arxiv 2603.18718, Mar 2026). After distillation produces
//! candidate facts, synthesize test questions that each fact should answer,
//! then verify correctness against the original session log. Facts that fail
//! probes are flagged for re-distillation or human review rather than silently
//! entering the knowledge graph.
//!
//! # Architecture
//!
//! The probe pipeline is intentionally LLM-free in this initial implementation.
//! Probe questions are generated heuristically from the fact content (extracting
//! key entities and relationships). Verification compares the fact content
//! against the original transcript using token overlap. This avoids an
//! additional LLM round-trip per distillation while still catching the most
//! common failure mode: hallucinated facts that have no grounding in the
//! session log.
//!
//! Future work may add an LLM-powered probe generator and verifier for higher
//! recall on subtle distillation errors.

use crate::flush::{FlushItem, MemoryFlush};

/// A probe question generated from a distilled fact.
#[derive(Debug, Clone)]
pub struct Probe {
    /// The question that the fact should be able to answer.
    pub question: String,
    /// Index of the flush item this probe targets.
    pub source_index: usize,
    /// Which flush category the source belongs to.
    pub source_category: ProbeCategory,
}

/// Which category of flush item a probe targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeCategory {
    /// A key decision.
    Decision,
    /// A correction that prevents repeating mistakes.
    Correction,
    /// A learned fact.
    Fact,
}

impl std::fmt::Display for ProbeCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Decision => f.write_str("decision"),
            Self::Correction => f.write_str("correction"),
            Self::Fact => f.write_str("fact"),
        }
    }
}

/// Result of verifying a single probe against the original transcript.
#[derive(Debug, Clone)]
pub struct ProbeVerification {
    /// The probe that was verified.
    pub probe: Probe,
    /// Whether the probe passed verification.
    pub passed: bool,
    /// Token overlap score between the fact content and the transcript (0.0..=1.0).
    pub overlap_score: f64,
    /// Explanation of the verification result.
    pub explanation: String,
}

/// Aggregate result of running all probes for a `MemoryFlush`.
#[derive(Debug, Clone)]
pub struct ProbeReport {
    /// Individual verification results.
    pub verifications: Vec<ProbeVerification>,
    /// Indices of flush items that failed verification, grouped by category.
    pub failed_decisions: Vec<usize>,
    /// Indices of flush items that failed verification.
    pub failed_corrections: Vec<usize>,
    /// Indices of flush items that failed verification.
    pub failed_facts: Vec<usize>,
}

impl ProbeReport {
    /// Returns `true` if all probes passed.
    #[must_use]
    pub fn all_passed(&self) -> bool {
        self.failed_decisions.is_empty()
            && self.failed_corrections.is_empty()
            && self.failed_facts.is_empty()
    }

    /// Total number of probes that failed.
    #[must_use]
    pub fn failure_count(&self) -> usize {
        self.failed_decisions.len() + self.failed_corrections.len() + self.failed_facts.len()
    }

    /// Total number of probes run.
    #[must_use]
    pub fn total_probes(&self) -> usize {
        self.verifications.len()
    }

    /// Pass rate as a fraction (0.0..=1.0).
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "probe counts are tiny (<100); f64 mantissa handles this exactly"
    )]
    #[expect(
        clippy::as_conversions,
        reason = "usize to f64 — probe counts are bounded and small"
    )]
    pub fn pass_rate(&self) -> f64 {
        let total = self.total_probes();
        if total == 0 {
            return 1.0;
        }
        let passed = total - self.failure_count();
        passed as f64 / total as f64
    }
}

/// Configuration for the backward-path probe verifier.
#[derive(Debug, Clone)]
pub struct ProbeConfig {
    /// Minimum token overlap score to pass verification (0.0..=1.0). Default: 0.15.
    pub min_overlap: f64,
    /// Maximum number of probes to generate per flush item. Default: 3.
    pub max_probes_per_item: usize,
}

impl Default for ProbeConfig {
    fn default() -> Self {
        Self {
            min_overlap: 0.15,
            max_probes_per_item: 3,
        }
    }
}

/// Backward-path probe verifier for distilled facts.
///
/// Generates probe questions from candidate facts, then verifies each probe
/// against the original session transcript. Facts without transcript grounding
/// are flagged as potential hallucinations.
#[derive(Debug, Clone)]
pub struct ProbeVerifier {
    config: ProbeConfig,
}

impl ProbeVerifier {
    /// Create a new verifier with the given configuration.
    #[must_use]
    pub fn new(config: ProbeConfig) -> Self {
        Self { config }
    }

    /// Generate probes for all items in a memory flush.
    #[must_use]
    pub fn generate_probes(&self, flush: &MemoryFlush) -> Vec<Probe> {
        let mut probes = Vec::new();

        for (i, item) in flush.decisions.iter().enumerate() {
            probes.extend(self.probes_for_item(item, i, ProbeCategory::Decision));
        }
        for (i, item) in flush.corrections.iter().enumerate() {
            probes.extend(self.probes_for_item(item, i, ProbeCategory::Correction));
        }
        for (i, item) in flush.facts.iter().enumerate() {
            probes.extend(self.probes_for_item(item, i, ProbeCategory::Fact));
        }

        probes
    }

    /// Verify all probes against the original transcript.
    ///
    /// `transcript` is the concatenated session messages that were distilled.
    #[must_use]
    #[expect(
        clippy::indexing_slicing,
        reason = "probe indices are generated by enumerate() over the same flush, so always in-bounds"
    )]
    pub fn verify(&self, flush: &MemoryFlush, transcript: &str) -> ProbeReport {
        let probes = self.generate_probes(flush);
        let transcript_tokens = tokenize(transcript);

        let mut verifications = Vec::with_capacity(probes.len());
        let mut failed_decisions = Vec::new();
        let mut failed_corrections = Vec::new();
        let mut failed_facts = Vec::new();

        for probe in probes {
            // SAFETY: probe indices are generated from flush.{decisions,corrections,facts}
            // by `generate_probes`, which iterates `.iter().enumerate()`, so indices are
            // always in-bounds for the same flush.
            let item = match probe.source_category {
                ProbeCategory::Decision => &flush.decisions[probe.source_index],
                ProbeCategory::Correction => &flush.corrections[probe.source_index],
                ProbeCategory::Fact => &flush.facts[probe.source_index],
            };

            let fact_tokens = tokenize(&item.content);
            let overlap = token_overlap(&fact_tokens, &transcript_tokens);
            let passed = overlap >= self.config.min_overlap;

            let explanation = if passed {
                format!(
                    "{} grounded in transcript (overlap={overlap:.2})",
                    probe.source_category
                )
            } else {
                format!(
                    "{} not grounded in transcript (overlap={overlap:.2} < {:.2})",
                    probe.source_category, self.config.min_overlap
                )
            };

            if !passed {
                match probe.source_category {
                    ProbeCategory::Decision => {
                        if !failed_decisions.contains(&probe.source_index) {
                            failed_decisions.push(probe.source_index);
                        }
                    }
                    ProbeCategory::Correction => {
                        if !failed_corrections.contains(&probe.source_index) {
                            failed_corrections.push(probe.source_index);
                        }
                    }
                    ProbeCategory::Fact => {
                        if !failed_facts.contains(&probe.source_index) {
                            failed_facts.push(probe.source_index);
                        }
                    }
                }
            }

            verifications.push(ProbeVerification {
                probe,
                passed,
                overlap_score: overlap,
                explanation,
            });
        }

        ProbeReport {
            verifications,
            failed_decisions,
            failed_corrections,
            failed_facts,
        }
    }

    /// Filter a `MemoryFlush` to only include items that passed probe verification.
    #[must_use]
    pub fn filter_passed(&self, flush: &MemoryFlush, report: &ProbeReport) -> MemoryFlush {
        MemoryFlush {
            decisions: flush
                .decisions
                .iter()
                .enumerate()
                .filter(|(i, _)| !report.failed_decisions.contains(i))
                .map(|(_, item)| item.clone())
                .collect(),
            corrections: flush
                .corrections
                .iter()
                .enumerate()
                .filter(|(i, _)| !report.failed_corrections.contains(i))
                .map(|(_, item)| item.clone())
                .collect(),
            facts: flush
                .facts
                .iter()
                .enumerate()
                .filter(|(i, _)| !report.failed_facts.contains(i))
                .map(|(_, item)| item.clone())
                .collect(),
            task_state: flush.task_state.clone(),
        }
    }

    /// Generate probes for a single flush item.
    fn probes_for_item(
        &self,
        item: &FlushItem,
        index: usize,
        category: ProbeCategory,
    ) -> Vec<Probe> {
        let key_phrases = extract_key_phrases(&item.content);
        key_phrases
            .into_iter()
            .take(self.config.max_probes_per_item)
            .map(|phrase| Probe {
                question: format!("Does the session mention: {phrase}?"),
                source_index: index,
                source_category: category,
            })
            .collect()
    }
}

/// Extract key phrases from a fact for probe generation.
///
/// Heuristic: split on sentence boundaries and extract phrases containing
/// proper nouns (capitalized words not at sentence start) or technical terms
/// (words containing underscores, dots, or colons).
#[expect(
    clippy::indexing_slicing,
    reason = "slice bounds guarded by explicit len checks immediately above each access"
)]
fn extract_key_phrases(content: &str) -> Vec<String> {
    let mut phrases = Vec::new();

    // Split into sentences (approximate)
    for sentence in content.split(['.', '!', '?']) {
        let trimmed = sentence.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Extract meaningful chunks: look for capitalized words, technical
        // terms (snake_case, paths, namespaces)
        let words: Vec<&str> = trimmed.split_whitespace().collect();
        if words.len() < 2 {
            continue;
        }

        // Take the whole sentence as a phrase if it's short enough
        if words.len() <= 10 {
            phrases.push(trimmed.to_owned());
        } else {
            // For longer sentences, extract the first and last meaningful chunks
            let first_half: String = words[..5].join(" ");
            phrases.push(first_half);

            if words.len() > 6 {
                let last_half: String = words[words.len() - 4..].join(" ");
                phrases.push(last_half);
            }
        }
    }

    if phrases.is_empty() && !content.trim().is_empty() {
        // Fallback: use the whole content as a single phrase
        phrases.push(content.trim().to_owned());
    }

    phrases
}

/// Common English stop words to exclude from overlap comparison.
const STOP_WORDS: &[&str] = &[
    "the", "and", "for", "are", "but", "not", "you", "all", "can", "had", "her", "was", "one",
    "our", "out", "has", "his", "how", "its", "may", "new", "now", "old", "see", "way", "who",
    "did", "get", "let", "say", "she", "too", "use", "will", "with", "this", "that", "from",
    "have", "been", "some", "they", "were", "what", "when", "your", "each", "make", "like",
    "into", "just", "over", "such", "than", "them", "then", "also", "more", "should",
];

/// Tokenize text into lowercase words for overlap comparison, excluding stop words.
fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| w.len() >= 3)
        .map(str::to_lowercase)
        .filter(|w| !STOP_WORDS.contains(&w.as_str()))
        .collect()
}

/// Compute Jaccard-style token overlap between two token sets.
///
/// Returns the fraction of `fact_tokens` that appear in `transcript_tokens`.
#[expect(
    clippy::cast_precision_loss,
    reason = "token counts are small (<1000); f64 mantissa handles this exactly"
)]
#[expect(
    clippy::as_conversions,
    reason = "usize to f64 — token counts are bounded and small"
)]
fn token_overlap(fact_tokens: &[String], transcript_tokens: &[String]) -> f64 {
    if fact_tokens.is_empty() {
        return 0.0;
    }
    let matches = fact_tokens
        .iter()
        .filter(|t| transcript_tokens.contains(t))
        .count();
    matches as f64 / fact_tokens.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flush::FlushSource;

    fn make_item(content: &str) -> FlushItem {
        FlushItem {
            content: content.to_owned(),
            timestamp: "2026-04-10T12:00:00Z".to_owned(),
            source: FlushSource::Extracted,
        }
    }

    fn make_flush(facts: &[&str]) -> MemoryFlush {
        MemoryFlush {
            decisions: vec![],
            corrections: vec![],
            facts: facts.iter().map(|c| make_item(c)).collect(),
            task_state: None,
        }
    }

    #[test]
    fn grounded_fact_passes_verification() {
        let verifier = ProbeVerifier::new(ProbeConfig::default());
        let flush = make_flush(&["The user prefers snake_case naming"]);
        let transcript = "I prefer snake_case naming for all my Rust variables. \
                          Please use that convention going forward.";
        let report = verifier.verify(&flush, transcript);
        assert!(
            report.all_passed(),
            "grounded fact should pass: {:?}",
            report.verifications
        );
    }

    #[test]
    fn ungrounded_fact_fails_verification() {
        let verifier = ProbeVerifier::new(ProbeConfig::default());
        let flush = make_flush(&["The user's favorite color is purple"]);
        let transcript = "Let's discuss the API endpoint design. \
                          The GET /users endpoint should return paginated results.";
        let report = verifier.verify(&flush, transcript);
        assert!(
            !report.all_passed(),
            "ungrounded fact should fail verification"
        );
        assert_eq!(report.failed_facts.len(), 1);
    }

    #[test]
    fn empty_flush_passes() {
        let verifier = ProbeVerifier::new(ProbeConfig::default());
        let flush = MemoryFlush::empty();
        let report = verifier.verify(&flush, "any transcript");
        assert!(report.all_passed());
        assert_eq!(report.total_probes(), 0);
        assert!((report.pass_rate() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn mixed_flush_reports_partial_failure() {
        let verifier = ProbeVerifier::new(ProbeConfig::default());
        let flush = MemoryFlush {
            decisions: vec![make_item("Use PostgreSQL for the database")],
            corrections: vec![],
            facts: vec![
                make_item("The API uses REST endpoints"),
                make_item("The user enjoys skydiving on weekends"),
            ],
            task_state: None,
        };
        let transcript = "We decided to use PostgreSQL for the database backend. \
                          The API will expose REST endpoints for CRUD operations.";
        let report = verifier.verify(&flush, transcript);
        assert!(!report.all_passed());
        // Skydiving fact should fail, the others should pass
        assert!(!report.failed_facts.is_empty(), "hallucinated fact should fail");
    }

    #[test]
    fn filter_passed_removes_failed_items() {
        let verifier = ProbeVerifier::new(ProbeConfig::default());
        let flush = make_flush(&[
            "The project uses Rust and Cargo",
            "The user has three cats named Whiskers",
        ]);
        let transcript = "This is a Rust project managed with Cargo. \
                          We need to add a new crate for the API layer.";
        let report = verifier.verify(&flush, transcript);
        let filtered = verifier.filter_passed(&flush, &report);
        // The grounded fact should survive, the cat fact should be filtered out
        assert!(
            filtered.facts.len() < flush.facts.len(),
            "filtered flush should have fewer facts"
        );
    }

    #[test]
    fn probe_generation_produces_questions() {
        let verifier = ProbeVerifier::new(ProbeConfig::default());
        let flush = make_flush(&["The user prefers detailed code reviews with inline comments"]);
        let probes = verifier.generate_probes(&flush);
        assert!(!probes.is_empty(), "should generate at least one probe");
        for probe in &probes {
            assert!(
                probe.question.starts_with("Does the session mention:"),
                "probe question format"
            );
        }
    }

    #[test]
    fn token_overlap_exact_match() {
        let a = tokenize("hello world test");
        let b = tokenize("hello world test");
        assert!((token_overlap(&a, &b) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn token_overlap_no_match() {
        let a = tokenize("alpha beta gamma");
        let b = tokenize("delta epsilon zeta");
        assert!(token_overlap(&a, &b).abs() < f64::EPSILON);
    }

    #[test]
    fn extract_key_phrases_handles_short_content() {
        let phrases = extract_key_phrases("Use snake_case");
        assert!(!phrases.is_empty());
    }
}
