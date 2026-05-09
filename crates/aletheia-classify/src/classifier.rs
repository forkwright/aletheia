use std::path::Path;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use tracing::debug;

use crate::error::{ArtifactMissingSnafu, InvalidMetadataSnafu, Result};

/// Maximum character length for classification input (100k chars).
const MAX_TEXT_LENGTH: usize = 100_000;

/// Expected metadata schema version for this runtime.
const EXPECTED_SCHEMA_VERSION: &str = "1";

/// Classification output categories in index order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AuthorClass {
    /// User-authored text.
    User = 0,
    /// Subagent-generated response.
    Subagent = 1,
    /// System scaffolding (setup blocks, task descriptions, etc.).
    SystemScaffolding = 2,
    /// Template text or boilerplate.
    Template = 3,
}

impl AuthorClass {
    /// Convert from integer index to enum variant.
    fn from_index(idx: usize) -> Option<Self> {
        match idx {
            0 => Some(Self::User),
            1 => Some(Self::Subagent),
            2 => Some(Self::SystemScaffolding),
            3 => Some(Self::Template),
            _ => None,
        }
    }

    /// Return the integer index corresponding to this variant.
    #[must_use]
    pub fn index(self) -> usize {
        self as usize
    }

    /// Human-readable name for this class.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Subagent => "subagent",
            Self::SystemScaffolding => "system_scaffolding",
            Self::Template => "template",
        }
    }
}

/// Artifact metadata sidecar describing the classifier model.
///
/// Loaded from `metadata.json` alongside the model artifact file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactMetadata {
    /// Metadata schema version (currently "1").
    pub schema_version: String,
    /// Classifier artifact version semver.
    pub artifact_version: String,
    /// Producer identifier (e.g. "gnomon-author-classifier@0.1.0").
    pub producer: String,
    /// Timestamp when the artifact was produced.
    pub produced_at: String,
    /// Model type (e.g. "heuristic_rule_bank").
    pub model_type: String,
    /// Array of class names in index order.
    pub classes: Vec<String>,
    /// Minimum aletheia runtime version required.
    pub runtime_version: Option<String>,
}

impl ArtifactMetadata {
    /// Validate this metadata against runtime constraints.
    fn validate(&self) -> Result<()> {
        if self.schema_version != EXPECTED_SCHEMA_VERSION {
            return Err(crate::error::ClassifyError::VersionMismatch {
                artifact_schema: self.schema_version.clone(),
                runtime_schema: EXPECTED_SCHEMA_VERSION.to_owned(),
            });
        }
        if self.classes.len() != 4 {
            return Err(crate::error::ClassifyError::InvalidOutputShape {
                len: self.classes.len(),
            });
        }
        Ok(())
    }
}

/// Classification result with confidence and metadata.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct AuthorProbs {
    /// Per-class probability scores [user, subagent, system_scaffolding, template].
    pub probabilities: [f32; 4],
    /// Timestamp of classification.
    pub classified_at: Timestamp,
}

impl AuthorProbs {
    /// Return the class with highest probability.
    #[must_use]
    pub fn argmax(&self) -> AuthorClass {
        let max_idx = self
            .probabilities
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        AuthorClass::from_index(max_idx).unwrap_or(AuthorClass::User)
    }

    /// Return the highest probability (confidence).
    #[must_use]
    pub fn confidence(&self) -> f32 {
        self.probabilities.iter().copied().fold(0.0_f32, f32::max)
    }
}

/// Author classifier: heuristic rule bank for distinguishing human-authored
/// text from AI-generated continuations, echoes, and scaffolding.
///
/// WHY heuristic: no ONNX artifact or embedding model is required. The rule
/// bank uses surface features (length, markdown density, self-reference
/// patterns, informal markers) that are cheap to compute and sufficient for
/// the decontamination gate. See #3786 for evaluation results.
pub struct Classifier {
    metadata: ArtifactMetadata,
}

impl Classifier {
    /// Create a new heuristic classifier without loading an artifact.
    ///
    /// Use this when no artifact directory is available. The classifier
    /// operates purely from embedded rules.
    #[must_use]
    pub fn new() -> Self {
        Self {
            metadata: ArtifactMetadata {
                schema_version: EXPECTED_SCHEMA_VERSION.to_owned(),
                artifact_version: "0.1.0-heuristic".to_owned(),
                producer: "aletheia-heuristic-classifier".to_owned(),
                produced_at: Timestamp::now().to_string(),
                model_type: "heuristic_rule_bank".to_owned(),
                classes: vec![
                    "user".to_owned(),
                    "subagent".to_owned(),
                    "system_scaffolding".to_owned(),
                    "template".to_owned(),
                ],
                runtime_version: None,
            },
        }
    }

    /// Load a classifier from an artifact directory.
    ///
    /// Expects `metadata.json` in the provided directory. The heuristic
    /// engine does not require a model file, but will load and validate
    /// metadata when present for observability.
    ///
    /// # Errors
    ///
    /// Returns an error if metadata cannot be read, parsed, or validated.
    pub async fn load(artifact_dir: &Path) -> Result<Self> {
        let metadata_path = artifact_dir.join("metadata.json");

        debug!(
            metadata_path = %metadata_path.display(),
            "loading author classifier metadata"
        );

        let metadata_content =
            std::fs::read_to_string(&metadata_path).context(ArtifactMissingSnafu {
                path: &metadata_path,
            })?;
        let metadata: ArtifactMetadata =
            serde_json::from_str(&metadata_content).context(InvalidMetadataSnafu)?;

        metadata.validate()?;

        debug!(
            producer = %metadata.producer,
            artifact_version = %metadata.artifact_version,
            "author classifier loaded"
        );

        Ok(Self { metadata })
    }

    /// Classify a text string, returning per-class probabilities.
    ///
    /// Uses a heuristic rule bank (length, markdown-fence density, stop-word
    /// patterns, self-reference cues, informal markers) scored and mapped
    /// to a 4-class probability distribution via soft exponential
    /// normalization.
    ///
    /// # Errors
    ///
    /// Returns an error if the text exceeds [`MAX_TEXT_LENGTH`].
    #[tracing::instrument(skip(self), fields(text_len = text.len()))]
    pub fn classify(&self, text: &str) -> Result<AuthorProbs> {
        if text.len() > MAX_TEXT_LENGTH {
            return Err(crate::error::ClassifyError::TextTooLong { len: text.len() });
        }

        let probabilities = heuristic_probabilities(text);

        Ok(AuthorProbs {
            probabilities,
            classified_at: Timestamp::now(),
        })
    }

    /// Reference to the loaded artifact metadata.
    #[must_use]
    pub fn metadata(&self) -> &ArtifactMetadata {
        &self.metadata
    }
}

impl Default for Classifier {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Heuristic rule bank
// ---------------------------------------------------------------------------

/// Compute per-class probabilities from surface features.
///
/// Returns a 4-element array: [user, subagent, system_scaffolding, template].
fn heuristic_probabilities(text: &str) -> [f32; 4] {
    let lower = text.to_lowercase();
    let len = text.chars().count();

    let mut human_score = 0.0_f32;
    let mut agent_score = 0.0_f32;

    // ── Length features ──────────────────────────────────────────────
    if len < 40 {
        human_score += 2.5;
    } else if len > 800 {
        agent_score += 1.5;
    }

    // ── Human: informal expressions ──────────────────────────────────
    if lower.contains("lol")
        || lower.contains("haha")
        || lower.contains("lmao")
        || lower.contains("rofl")
    {
        human_score += 3.0;
    }
    if lower.contains("omg") || lower.contains("wtf") {
        human_score += 2.5;
    }

    // Emojis (common BMP ranges + supplementary)
    if text.chars().any(is_emoji) {
        human_score += 2.5;
    }

    // Lowercase start
    if text
        .trim_start()
        .chars()
        .next()
        .map(char::is_lowercase)
        .unwrap_or(false)
    {
        human_score += 1.5;
    }

    // Missing apostrophes in contractions
    let contractions = [
        "dont ", "cant ", "wont ", "im ", "youre ", "thats ", "isnt ", "wasnt ",
    ];
    if contractions.iter().any(|c| lower.contains(c)) {
        human_score += 1.5;
    }

    // "u" as a standalone word
    if lower.split_whitespace().any(|w| w == "u") {
        human_score += 1.5;
    }

    // Internet abbreviations
    let abbrevs = ["tbh", "imo", "idk", "np", "fyi", "btw"];
    if abbrevs.iter().any(|a| lower.contains(a)) {
        human_score += 1.5;
    }

    // Multiple punctuation
    if text.contains("???") || text.contains("!!!") {
        human_score += 1.5;
    }

    // Gratitude
    if lower.contains("thanks") || lower.contains("thank you") || lower.contains("thx") {
        human_score += 1.0;
    }

    // URL or file path
    if lower.contains("http") || lower.contains("www.") || text.contains('/') {
        human_score += 1.0;
    }

    // Short question
    if text.trim_end().ends_with('?') && len < 100 {
        human_score += 1.0;
    }

    // ── Agent: markdown and structure ────────────────────────────────
    if text.contains("```") {
        agent_score += 5.0;
    }

    let bullet_count = text
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("- ") || t.starts_with("* ") || t.starts_with("• ")
        })
        .count();
    #[expect(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        reason = "usize→f32: count is bounded by min(3), fits exactly in f32"
    )]
    {
        agent_score += bullet_count.min(3) as f32; // kanon:ignore RUST/as-cast
    }

    let numbered_count = text
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            t.len() > 2
                && t.as_bytes().first().is_some_and(u8::is_ascii_digit)
                && t.as_bytes().get(1) == Some(&b'.')
        })
        .count();
    #[expect(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        reason = "usize→f32: count is bounded by min(3), fits exactly in f32"
    )]
    {
        agent_score += numbered_count.min(3) as f32; // kanon:ignore RUST/as-cast
    }

    // Step-numbered instructions (e.g. "Step 1:")
    let step_count = text
        .lines()
        .filter(|l| {
            let t = l.trim_start().to_lowercase();
            t.starts_with("step ")
                && t.len() > 6
                && t.as_bytes().get(5).is_some_and(u8::is_ascii_digit)
        })
        .count();
    #[expect(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        reason = "usize→f32: count is bounded by min(3), fits exactly in f32"
    )]
    {
        agent_score += step_count.min(3) as f32; // kanon:ignore RUST/as-cast
    }

    // ── Agent: self-reference and disclaimers ────────────────────────
    if lower.contains("as an ai") || lower.contains("as a language model") {
        agent_score += 5.0;
    }

    if lower.contains("i don't have personal")
        || lower.contains("i cannot have")
        || lower.contains("i don't have feelings")
    {
        agent_score += 4.0;
    }

    if lower.contains("the user") {
        agent_score += 3.0;
    }

    // ── Agent: boilerplate phrases ───────────────────────────────────
    let boilerplate = [
        "it is important to note",
        "it should be noted",
        "please let me know",
        "in conclusion",
        "to summarize",
        "here is",
        "here are",
        "below is",
        "above is",
        "i would recommend",
        "i suggest",
    ];
    if boilerplate.iter().any(|b| lower.contains(b)) {
        agent_score += 2.5;
    }

    if lower.contains("i hope this helps") || lower.contains("i hope that helps") {
        agent_score += 2.5;
    }

    // No first-person "I" in long text
    if len > 120 && !lower.split_whitespace().any(|w| w == "i") {
        agent_score += 2.0;
    }

    // Formal hedging
    let hedges = ["however,", "therefore,", "furthermore,", "moreover,"];
    if hedges.iter().any(|h| lower.contains(h)) {
        agent_score += 1.5;
    }

    // ── Convert scores to probabilities ──────────────────────────────
    // Exponential mapping gives sharp separation when one score dominates.
    let human_exp = human_score.exp();
    let agent_exp = agent_score.exp();
    let other_exp = 0.5_f32; // shared prior for system_scaffolding + template

    let total = human_exp + agent_exp + other_exp + other_exp;

    [
        human_exp / total, // User
        agent_exp / total, // Subagent
        other_exp / total, // SystemScaffolding
        other_exp / total, // Template
    ]
}

/// Detect whether a character is an emoji.
fn is_emoji(c: char) -> bool {
    matches!(c,
        '\u{1F600}'..='\u{1F64F}' |   // emoticons
        '\u{1F300}'..='\u{1F5FF}' |   // symbols & pictographs
        '\u{1F680}'..='\u{1F6FF}' |   // transport & map
        '\u{1F700}'..='\u{1F77F}' |   // alchemical
        '\u{1F780}'..='\u{1F7FF}' |   // geometric shapes
        '\u{1F800}'..='\u{1F8FF}' |   // supplemental arrows
        '\u{1F900}'..='\u{1F9FF}' |   // supplemental symbols
        '\u{1FA00}'..='\u{1FA6F}' |   // chess symbols
        '\u{1FA70}'..='\u{1FAFF}' |   // symbols and pictographs extended-a
        '\u{2600}'..='\u{26FF}' |     // miscellaneous symbols
        '\u{2700}'..='\u{27BF}'       // dingbats
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn author_probs_argmax_returns_max_class() {
        let probs = AuthorProbs {
            probabilities: [0.1, 0.8, 0.05, 0.05],
            classified_at: Timestamp::now(),
        };
        assert_eq!(probs.argmax(), AuthorClass::Subagent);
    }

    #[test]
    fn author_probs_confidence_returns_max_prob() {
        let probs = AuthorProbs {
            probabilities: [0.1, 0.8, 0.05, 0.05],
            classified_at: Timestamp::now(),
        };
        assert_eq!(probs.confidence(), 0.8);
    }

    #[test]
    fn author_class_index_maps_correctly() {
        assert_eq!(AuthorClass::User.index(), 0);
        assert_eq!(AuthorClass::Subagent.index(), 1);
        assert_eq!(AuthorClass::SystemScaffolding.index(), 2);
        assert_eq!(AuthorClass::Template.index(), 3);
    }

    #[test]
    fn author_class_from_index_all_variants() {
        assert_eq!(AuthorClass::from_index(0), Some(AuthorClass::User));
        assert_eq!(AuthorClass::from_index(1), Some(AuthorClass::Subagent));
        assert_eq!(
            AuthorClass::from_index(2),
            Some(AuthorClass::SystemScaffolding)
        );
        assert_eq!(AuthorClass::from_index(3), Some(AuthorClass::Template));
        assert_eq!(AuthorClass::from_index(4), None);
    }

    #[test]
    fn author_class_as_str() {
        assert_eq!(AuthorClass::User.as_str(), "user");
        assert_eq!(AuthorClass::Subagent.as_str(), "subagent");
        assert_eq!(
            AuthorClass::SystemScaffolding.as_str(),
            "system_scaffolding"
        );
        assert_eq!(AuthorClass::Template.as_str(), "template");
    }

    #[test]
    fn text_length_guard() {
        let long_text = "a".repeat(MAX_TEXT_LENGTH + 1);
        let classifier = Classifier::new();
        assert!(classifier.classify(&long_text).is_err());
    }

    #[tokio::test]
    async fn load_reads_and_validates_metadata() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("metadata.json"),
            r#"{
                "schema_version": "1",
                "artifact_version": "2026.05.08-test",
                "producer": "alice-test-classifier",
                "produced_at": "2026-05-08T00:00:00Z",
                "model_type": "heuristic_rule_bank",
                "classes": ["user", "subagent", "system_scaffolding", "template"],
                "runtime_version": null
            }"#,
        )
        .expect("write metadata");

        let classifier = Classifier::load(dir.path()).await.expect("load classifier");

        assert_eq!(
            classifier.metadata().schema_version,
            EXPECTED_SCHEMA_VERSION
        );
        assert_eq!(classifier.metadata().producer, "alice-test-classifier");
        assert_eq!(
            classifier.metadata().classes,
            ["user", "subagent", "system_scaffolding", "template"]
        );
    }

    #[test]
    fn classify_returns_normalized_non_uniform_scores() {
        let classifier = Classifier::new();
        let probs = classifier
            .classify("lol thanks, can u check this real quick???")
            .expect("classify");
        let sum: f32 = probs.probabilities.iter().sum();

        assert!(
            (sum - 1.0).abs() < 0.001,
            "probabilities should sum to 1.0, got {sum}"
        );
        assert_eq!(probs.argmax(), AuthorClass::User);
        assert!(
            probs
                .probabilities
                .windows(2)
                .any(|pair| (pair[0] - pair[1]).abs() > f32::EPSILON),
            "heuristic classifier should not return a uniform distribution"
        );
    }

    // ── Golden-set accuracy (20 human + 20 agent) ───────────────────

    const HUMAN_SAMPLES: &[&str] = &[
        "lol that's wild",
        "thanks for the help!",
        "u sure about that?",
        "omg i cant believe it",
        "Hey can you check this file? /home/alice/data.csv",
        "wait what",
        "haha no way",
        "idk maybe try restarting",
        "btw the server is down",
        "np, let me know when its fixed",
        "wtf happened here",
        "tbh i dont like this approach",
        "ok cool",
        "why is it broken???",
        "im confused",
        "yo can you help me real quick",
        "thx!",
        "https://example.com/docs",
        "nvm figured it out",
        "sounds good to me",
    ];

    const AGENT_SAMPLES: &[&str] = &[
        "Here is the summary of the code analysis:\n\n```rust\nfn main() {}\n```",
        "As an AI language model, I don't have personal experiences.",
        "It is important to note that the user should verify all outputs.",
        "1. First, install the package\n2. Then run the command\n3. Finally, verify the result",
        "Here is the corrected code:\n\n```python\nprint('hello world')\n```",
        "The user asked about the capital of France. The answer is Paris.",
        "```python\nprint('hello')\n```",
        "Below is the summary:\n\n```rust\nfn main() {}\n```",
        "- Point one: the code is correct\n- Point two: the tests pass\n- Point three: documentation is complete",
        "I don't have personal opinions, but I can provide information on this topic.",
        "Here are the steps:\n\n1. Check the logs\n2. Restart the service",
        "To summarize, the user requested a function that sorts an array.",
        "It should be noted that the following changes were made:\n\n- Added validation\n- Updated tests",
        "I hope this helps. Please let me know if you have any questions.",
        "As an AI, I cannot access external websites in real-time.",
        "```bash\ncargo build --release\n```",
        "I would recommend using the async approach for better performance.",
        "The user wants to know the best practices for error handling in Rust.",
        "1. Overview\n2. Implementation\n3. Testing\n4. Deployment",
        "I don't have feelings, but I appreciate your kind words.",
    ];

    #[test]
    fn golden_set_human_accuracy() {
        let classifier = Classifier::new();
        let mut correct = 0;
        for sample in HUMAN_SAMPLES {
            let probs = classifier.classify(sample).expect("classify");
            let class = probs.argmax();
            if class == AuthorClass::User && probs.confidence() >= 0.85 {
                correct += 1;
            }
        }
        #[expect(
            clippy::as_conversions,
            reason = "usize→f32: sample count (20) fits exactly"
        )]
        let accuracy = (correct as f32) / (HUMAN_SAMPLES.len() as f32); // kanon:ignore RUST/as-cast
        assert!(
            accuracy >= 0.85,
            "human accuracy {accuracy} below 0.85 ({} / {})",
            correct,
            HUMAN_SAMPLES.len()
        );
    }

    #[test]
    fn golden_set_agent_accuracy() {
        let classifier = Classifier::new();
        let mut correct = 0;
        for sample in AGENT_SAMPLES {
            let probs = classifier.classify(sample).expect("classify");
            let class = probs.argmax();
            if class == AuthorClass::Subagent && probs.confidence() >= 0.85 {
                correct += 1;
            }
        }
        #[expect(
            clippy::as_conversions,
            reason = "usize→f32: sample count (20) fits exactly"
        )]
        let accuracy = (correct as f32) / (AGENT_SAMPLES.len() as f32); // kanon:ignore RUST/as-cast
        assert!(
            accuracy >= 0.85,
            "agent accuracy {accuracy} below 0.85 ({} / {})",
            correct,
            AGENT_SAMPLES.len()
        );
    }
}
