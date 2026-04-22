//! Pre-LLM triage stage: intent, sensitivity, and complexity classification.
//!
//! Runs before model invocation to classify input on three dimensions:
//! - **Intent**: what kind of request is this? (code-write, research, Q&A, meta)
//! - **Sensitivity**: does the input carry Internal / Confidential data?
//! - **Tier**: cheap / mid / heavy? (feeds complexity routing)
//!
//! All classification is observational and heuristic (keyword-based). No LLM call.
//! Results are logged and optionally stamped on the turn artifact.

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use aletheia_lexica::keywords;
use eidos::knowledge::FactSensitivity;
use hermeneus::complexity::ModelTier;

// --- Intent classifier regexes ---

#[expect(
    clippy::expect_used,
    reason = "compile-time-constant regex literals cannot fail"
)]
static CODE_WRITE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(write|implement|code|create|generate|build|fix|refactor|debug|test)\b")
        .expect("compile-time-constant regex literals cannot fail")
});

#[expect(
    clippy::expect_used,
    reason = "compile-time-constant regex literals cannot fail"
)]
static RESEARCH_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(what|research|find|search|investigate|analyze|explain|tell me|compare|review)\b",
    )
    .expect("compile-time-constant regex literals cannot fail")
});

#[expect(
    clippy::expect_used,
    reason = "compile-time-constant regex literals cannot fail"
)]
static PLANNING_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(plan|design|architect|strategy|roadmap|organize|prioritize|goal|milestone)\b",
    )
    .expect("compile-time-constant regex literals cannot fail")
});

#[expect(
    clippy::expect_used,
    reason = "compile-time-constant regex literals cannot fail"
)]
static META_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(help|you|instruction|rule|guideline|config|setting|system|prompt)\b")
        .expect("compile-time-constant regex literals cannot fail")
});

// --- Sensitivity detection patterns ---

#[expect(
    clippy::expect_used,
    reason = "compile-time-constant regex literals cannot fail"
)]
static INTERNAL_MARKERS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(internal|confidential|secret|private|restricted|password|token|api.?key|credential)\b")
        .expect("compile-time-constant regex literals cannot fail")
});

#[expect(
    clippy::expect_used,
    reason = "compile-time-constant regex literals cannot fail"
)]
static CONFIDENTIAL_MARKERS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(ssn|social.?security|credit.?card|bank|account|salary|medical|health|diagnosis|nda|proprietary)\b")
        .expect("compile-time-constant regex literals cannot fail")
});

/// Classification of user intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Intent {
    /// Writing or implementing code: "write a function", "fix this bug"
    CodeWrite,
    /// Research or investigation: "what is X", "find me...", "explain..."
    Research,
    /// Planning or design: "plan a migration", "design an architecture"
    Planning,
    /// Meta requests about the system itself: "how do you work", "what are your rules"
    Meta,
    /// Unable to classify with confidence.
    Unclassified,
}

impl std::fmt::Display for Intent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CodeWrite => f.write_str("code-write"),
            Self::Research => f.write_str("research"),
            Self::Planning => f.write_str("planning"),
            Self::Meta => f.write_str("meta"),
            Self::Unclassified => f.write_str("unclassified"),
        }
    }
}

/// Result of pre-LLM triage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriageResult {
    /// Classified intent of the request.
    pub intent: Intent,
    /// Detected data sensitivity level.
    pub sensitivity: FactSensitivity,
    /// Suggested complexity tier for routing.
    pub tier: ModelTier,
    /// Input length (for observability).
    pub input_len: usize,
}

impl TriageResult {
    /// Create a new triage result.
    #[must_use]
    pub fn new(
        intent: Intent,
        sensitivity: FactSensitivity,
        tier: ModelTier,
        input_len: usize,
    ) -> Self {
        Self {
            intent,
            sensitivity,
            tier,
            input_len,
        }
    }
}

/// The triage stage: pre-LLM classification of intent, sensitivity, and complexity.
pub struct TriageStage;

impl TriageStage {
    /// Classify a user message on intent, sensitivity, and complexity tiers.
    ///
    /// # Arguments
    ///
    /// * `input` - The user's message content.
    ///
    /// # Returns
    ///
    /// A [`TriageResult`] with classifications. Never fails (graceful degradation
    /// to `Unclassified` or `Public` / `Haiku` on uncertain signals).
    #[instrument(skip(input), fields(intent = tracing::field::Empty, sensitivity = tracing::field::Empty, tier = tracing::field::Empty))]
    pub fn classify(input: &str) -> TriageResult {
        let input_lower = input.to_lowercase();
        let input_len = input.len();

        // Classify intent
        let intent = Self::classify_intent(&input_lower);

        // Detect sensitivity
        let sensitivity = Self::detect_sensitivity(input);

        // Route tier
        let tier = Self::route_tier(&input_lower, intent);

        let result = TriageResult {
            intent,
            sensitivity,
            tier,
            input_len,
        };

        // Record in the current span
        let span = tracing::Span::current();
        span.record("intent", intent.to_string());
        span.record("sensitivity", sensitivity.as_str());
        span.record("tier", tier.to_string());

        result
    }

    /// Classify intent using keyword matching.
    fn classify_intent(input: &str) -> Intent {
        // Check meta first (system-directed)
        if META_PATTERN.is_match(input) && input.len() < 100 {
            return Intent::Meta;
        }

        // Check code keywords
        let code_hits = keywords::CODING_KEYWORDS
            .iter()
            .filter(|&kw| input.contains(kw))
            .count();
        if code_hits >= 2 || CODE_WRITE_PATTERN.is_match(input) {
            return Intent::CodeWrite;
        }

        // Check research keywords
        let research_hits = keywords::RESEARCH_KEYWORDS
            .iter()
            .filter(|&kw| input.contains(kw))
            .count();
        if research_hits >= 2 || RESEARCH_PATTERN.is_match(input) {
            return Intent::Research;
        }

        // Check planning keywords
        let planning_hits = keywords::PLANNING_KEYWORDS
            .iter()
            .filter(|&kw| input.contains(kw))
            .count();
        if planning_hits >= 2 || PLANNING_PATTERN.is_match(input) {
            return Intent::Planning;
        }

        // Check conversation keywords (fallback)
        let conversation_hits = keywords::CONVERSATION_KEYWORDS
            .iter()
            .filter(|&kw| input.contains(kw))
            .count();
        if conversation_hits > 0 && input.len() < 50 {
            return Intent::Meta;
        }

        Intent::Unclassified
    }

    /// Detect sensitivity level based on markers in the input.
    fn detect_sensitivity(input: &str) -> FactSensitivity {
        if CONFIDENTIAL_MARKERS.is_match(input) {
            return FactSensitivity::Confidential;
        }

        if INTERNAL_MARKERS.is_match(input) {
            return FactSensitivity::Internal;
        }

        FactSensitivity::Public
    }

    /// Route to a complexity tier based on intent and input signals.
    fn route_tier(input: &str, intent: Intent) -> ModelTier {
        let input_len = input.len();

        // Very short inputs default to cheap tier
        if input_len < 30 {
            return ModelTier::Haiku;
        }

        // Planning, research, and code-write default to mid tier
        match intent {
            Intent::CodeWrite | Intent::Research | Intent::Planning => ModelTier::Sonnet,
            Intent::Meta | Intent::Unclassified => {
                // Moderate length → mid tier; very long → high tier
                if input_len > 200 {
                    ModelTier::Sonnet
                } else {
                    ModelTier::Haiku
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triage_classifies_code_write_intent() {
        let result = TriageStage::classify("write me a function that reverses a string");
        assert_eq!(result.intent, Intent::CodeWrite);
    }

    #[test]
    fn triage_classifies_research_intent() {
        let result = TriageStage::classify("what is machine learning");
        assert_eq!(result.intent, Intent::Research);
    }

    #[test]
    fn triage_classifies_planning_intent() {
        let result = TriageStage::classify("design a microservice architecture");
        assert_eq!(result.intent, Intent::Planning);
    }

    #[test]
    fn triage_classifies_meta_intent() {
        let result = TriageStage::classify("how do you work");
        assert_eq!(result.intent, Intent::Meta);
    }

    #[test]
    fn triage_falls_back_to_unclassified_on_ambiguity() {
        let result = TriageStage::classify("the color blue is interesting");
        assert_eq!(result.intent, Intent::Unclassified);
    }

    #[test]
    fn triage_detects_public_sensitivity() {
        let result = TriageStage::classify("what is rust");
        assert_eq!(result.sensitivity, FactSensitivity::Public);
    }

    #[test]
    fn triage_detects_internal_sensitivity() {
        let result = TriageStage::classify("what is my internal password for the system");
        assert!(
            matches!(
                result.sensitivity,
                FactSensitivity::Internal | FactSensitivity::Confidential
            ),
            "Expected at least Internal, got {:?}",
            result.sensitivity
        );
    }

    #[test]
    fn triage_detects_confidential_sensitivity() {
        let result = TriageStage::classify("my ssn is 123-45-6789");
        assert_eq!(result.sensitivity, FactSensitivity::Confidential);
    }

    #[test]
    fn triage_routes_short_input_to_haiku() {
        let result = TriageStage::classify("hi");
        assert_eq!(result.tier, ModelTier::Haiku);
    }

    #[test]
    fn triage_routes_code_write_to_sonnet() {
        let result = TriageStage::classify("implement a binary search algorithm in Rust");
        assert_eq!(result.tier, ModelTier::Sonnet);
    }

    #[test]
    fn triage_routes_research_to_sonnet() {
        let result = TriageStage::classify("explain the CAP theorem in distributed systems");
        assert_eq!(result.tier, ModelTier::Sonnet);
    }

    #[test]
    fn triage_includes_input_length() {
        let input = "this is a test";
        let result = TriageStage::classify(input);
        assert_eq!(result.input_len, input.len());
    }
}
