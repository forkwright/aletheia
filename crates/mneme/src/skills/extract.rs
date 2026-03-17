//! LLM-based skill extraction from promoted candidates.
//!
//! When a [`SkillCandidate`] is promoted (recurrence count ≥ 3), this module
//! builds an extraction prompt from the candidate metadata and tool call
//! sequences, sends it to a cost-effective LLM (Haiku), and parses the
//! structured skill definition from the response.
//!
//! The extracted skill is stored with `status: "pending_review"`: it is NOT
//! automatically activated. A human or orchestrator must approve it first.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use snafu::Snafu;

use crate::skill::SkillContent;
use crate::skills::{SkillCandidate, ToolCallRecord};

/// Errors from the skill extraction pipeline.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum SkillExtractionError {
    /// The LLM provider returned an error.
    #[snafu(display("LLM extraction failed: {message}"))]
    LlmCall {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// The LLM response could not be parsed as valid skill JSON.
    #[snafu(display("failed to parse skill extraction response: {message}"))]
    ParseResponse {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Minimal LLM completion interface for skill extraction.
///
/// Keeps mneme independent of hermeneus. The nous layer bridges this trait
/// to the full provider API, just like [`crate::extract::ExtractionProvider`].
///
/// Uses a boxed future return type to remain dyn-compatible (object-safe).
pub trait SkillExtractionProvider: Send + Sync {
    /// Send a system + user message to the LLM and return the text response.
    fn complete<'a>(
        &'a self,
        system: &'a str,
        user_message: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<String, SkillExtractionError>> + Send + 'a>,
    >;
}

/// A skill definition extracted by the LLM, before human review.
///
/// This is the raw LLM output, parsed from JSON. It gets converted to
/// [`SkillContent`] for storage as a fact with `fact_type = "skill"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractedSkill {
    /// Human-readable skill name.
    pub name: String,
    /// Description of what this skill does and when to use it.
    pub description: String,
    /// Ordered steps to execute the skill.
    pub steps: Vec<String>,
    /// Tools referenced by the skill.
    pub tools_used: Vec<String>,
    /// Domain classification tags.
    pub domain_tags: Vec<String>,
    /// When this skill should be applied (situational guidance).
    pub when_to_use: String,
}

impl ExtractedSkill {
    /// Convert to [`SkillContent`] for fact storage.
    pub fn to_skill_content(&self) -> SkillContent {
        SkillContent {
            name: self.name.clone(),
            description: if self.when_to_use.is_empty() {
                self.description.clone()
            } else {
                format!(
                    "{}\n\n## When to Use\n\n{}",
                    self.description, self.when_to_use
                )
            },
            steps: self.steps.clone(),
            tools_used: self.tools_used.clone(),
            domain_tags: self.domain_tags.clone(),
            origin: "extracted".to_owned(),
        }
    }
}

/// Extracts structured skill definitions from promoted candidates via LLM.
pub struct SkillExtractor<P> {
    provider: P,
}

impl<P: SkillExtractionProvider> SkillExtractor<P> {
    /// Create a new extractor with the given LLM provider.
    pub fn new(provider: P) -> Self {
        Self { provider }
    }

    /// Extract a structured skill definition from a promoted candidate.
    ///
    /// `tool_call_sequences` should contain the tool call sequences from each
    /// session where the pattern was observed (one vec per session).
    pub async fn extract_skill(
        &self,
        candidate: &SkillCandidate,
        tool_call_sequences: &[Vec<ToolCallRecord>],
    ) -> Result<ExtractedSkill, SkillExtractionError> {
        let system = EXTRACTION_SYSTEM_PROMPT;
        let user_message = build_extraction_prompt(candidate, tool_call_sequences);
        let response = self.provider.complete(system, &user_message).await?;
        parse_skill_response(&response)
    }
}

/// Result of a dedup check on a candidate skill.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DedupOutcome {
    /// No duplicate found: promote normally.
    Unique,
    /// Existing skill is better: discard candidate.
    DiscardCandidate {
        /// The ID of the existing skill that wins.
        existing_id: String,
    },
    /// Candidate is better: supersede the existing skill.
    SupersedeExisting {
        /// The ID of the existing skill to supersede.
        existing_id: String,
    },
}

/// Parameters for a dedup comparison between a candidate and an existing skill.
pub struct DedupInput<'a> {
    /// The candidate skill content.
    pub candidate: &'a SkillContent,
    /// Candidate confidence score.
    pub candidate_confidence: f64,
    /// Candidate usage count.
    pub candidate_usage: u32,
    /// The existing skill content.
    pub existing: &'a SkillContent,
    /// Existing skill confidence score.
    pub existing_confidence: f64,
    /// Existing skill usage count.
    pub existing_usage: u32,
    /// Existing skill fact ID.
    pub existing_id: &'a str,
    /// Optional embedding for the candidate.
    pub candidate_embedding: Option<&'a [f32]>,
    /// Optional embedding for the existing skill.
    pub existing_embedding: Option<&'a [f32]>,
}

/// Check whether a candidate skill duplicates an existing one.
///
/// Uses embedding cosine similarity when embeddings are provided.
/// Falls back to tool overlap + name similarity heuristics otherwise.
///
/// Threshold: cosine similarity > 0.90 = duplicate.
pub fn check_dedup(input: &DedupInput<'_>) -> DedupOutcome {
    let candidate = input.candidate;
    let existing = input.existing;
    let candidate_confidence = input.candidate_confidence;
    let candidate_usage = input.candidate_usage;
    let existing_confidence = input.existing_confidence;
    let existing_usage = input.existing_usage;
    let existing_id = input.existing_id;
    let is_duplicate = if let (Some(cand_emb), Some(exist_emb)) =
        (input.candidate_embedding, input.existing_embedding)
    {
        cosine_similarity(cand_emb, exist_emb) > 0.90
    } else {
        // WHY: Fallback to tool overlap when embeddings are unavailable.
        let tool_overlap = compute_tool_overlap(&candidate.tools_used, &existing.tools_used);
        let name_sim = compute_name_similarity(&candidate.name, &existing.name);
        tool_overlap > 0.85 || (tool_overlap > 0.6 && name_sim > 0.5)
    };

    if !is_duplicate {
        return DedupOutcome::Unique;
    }

    let existing_score = existing_confidence + f64::from(existing_usage) * 0.01;
    let candidate_score = candidate_confidence + f64::from(candidate_usage) * 0.01;

    if existing_score >= candidate_score {
        DedupOutcome::DiscardCandidate {
            existing_id: existing_id.to_owned(),
        }
    } else {
        DedupOutcome::SupersedeExisting {
            existing_id: existing_id.to_owned(),
        }
    }
}

/// Compute cosine similarity between two vectors.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0_f64;
    let mut norm_a = 0.0_f64;
    let mut norm_b = 0.0_f64;

    for (x, y) in a.iter().zip(b.iter()) {
        let x = f64::from(*x);
        let y = f64::from(*y);
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom < f64::EPSILON {
        0.0
    } else {
        dot / denom
    }
}

/// Compute Jaccard overlap between two tool lists.
#[expect(
    clippy::cast_precision_loss,
    reason = "tool list lengths are small (<50); precision loss is negligible"
)]
fn compute_tool_overlap(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let set_a: std::collections::HashSet<&str> = a.iter().map(String::as_str).collect();
    let set_b: std::collections::HashSet<&str> = b.iter().map(String::as_str).collect();
    let intersection = set_a.intersection(&set_b).count();
    let union = set_a.union(&set_b).count();
    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}

/// Simple name similarity: longest common subsequence ratio.
#[expect(
    clippy::cast_precision_loss,
    reason = "string char counts are small; precision loss is negligible"
)]
fn compute_name_similarity(a: &str, b: &str) -> f64 {
    let a_lower = a.to_lowercase();
    let b_lower = b.to_lowercase();
    let a_chars: Vec<char> = a_lower.chars().collect();
    let b_chars: Vec<char> = b_lower.chars().collect();

    if a_chars.is_empty() || b_chars.is_empty() {
        return 0.0;
    }

    let mut dp = vec![vec![0usize; b_chars.len() + 1]; a_chars.len() + 1];
    for (i, ac) in a_chars.iter().enumerate() {
        for (j, bc) in b_chars.iter().enumerate() {
            dp[i + 1][j + 1] = if ac == bc {
                dp[i][j] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }

    let lcs = dp[a_chars.len()][b_chars.len()];
    lcs as f64 / a_chars.len().max(b_chars.len()) as f64
}

const EXTRACTION_SYSTEM_PROMPT: &str = r#"You are a skill extraction engine for an AI agent system. Your job is to analyze tool call patterns and produce structured skill definitions.

Given a recurring tool call pattern observed across multiple sessions, generate a reusable skill definition that captures the generalizable workflow.

Rules:
- Name should be descriptive and kebab-case (e.g. "multi-file-refactor", "test-driven-bug-fix")
- Description should explain what the skill accomplishes, not just list steps
- Steps should be generalized — replace specific file names/paths with placeholders
- Tools should only include tool names actually used in the pattern
- Domain tags should classify the skill broadly (e.g. "rust", "debugging", "refactoring", "testing")
- when_to_use should describe the situation that triggers this skill

Respond with ONLY a JSON object, no markdown fences, no explanation:
{
  "name": "...",
  "description": "...",
  "steps": ["...", "..."],
  "tools_used": ["...", "..."],
  "domain_tags": ["...", "..."],
  "when_to_use": "..."
}"#;

/// Build the user message for skill extraction.
fn build_extraction_prompt(
    candidate: &SkillCandidate,
    tool_call_sequences: &[Vec<ToolCallRecord>],
) -> String {
    let mut prompt = String::with_capacity(1024);

    let _ = writeln!(prompt, "## Candidate Pattern");
    let _ = writeln!(prompt, "- Recurrence count: {}", candidate.recurrence_count);
    if let Some(ref pattern) = candidate.pattern_type {
        let _ = writeln!(prompt, "- Detected pattern type: {pattern:?}");
    }
    let _ = writeln!(
        prompt,
        "- Heuristic score: {:.2}",
        candidate.heuristic_score
    );
    let _ = writeln!(
        prompt,
        "- Normalized tool sequence: {}",
        candidate.signature.normalized.join(" → ")
    );
    let _ = writeln!(
        prompt,
        "- Sessions observed: {}",
        candidate.session_refs.len()
    );
    let _ = writeln!(prompt);

    let _ = writeln!(prompt, "## Observed Tool Call Sequences");
    for (i, seq) in tool_call_sequences.iter().enumerate() {
        let _ = writeln!(prompt, "\n### Session {} ({} calls)", i + 1, seq.len());
        for (j, tc) in seq.iter().enumerate() {
            let status = if tc.is_error { " [ERROR]" } else { "" };
            let _ = writeln!(
                prompt,
                "{}. {} ({}ms){}",
                j + 1,
                tc.tool_name,
                tc.duration_ms,
                status
            );
        }
    }

    prompt
}

/// Parse the LLM response into an [`ExtractedSkill`].
///
/// Handles JSON embedded in markdown fences, bare JSON, and minor formatting
/// issues from the LLM.
fn parse_skill_response(response: &str) -> Result<ExtractedSkill, SkillExtractionError> {
    let trimmed = response.trim();

    let json_str = if trimmed.starts_with("```") {
        let start = trimmed.find('\n').map_or(0, |i| i + 1);
        let end = trimmed
            .rfind("```")
            .filter(|&e| e > start)
            .unwrap_or(trimmed.len());
        &trimmed[start..end]
    } else {
        trimmed
    };

    let json_str = json_str.trim();
    let json_str = if let Some(start) = json_str.find('{') {
        let end = json_str.rfind('}').unwrap_or(json_str.len());
        &json_str[start..=end]
    } else {
        json_str
    };

    serde_json::from_str::<ExtractedSkill>(json_str).map_err(|e| {
        ParseResponseSnafu {
            message: format!(
                "invalid JSON in LLM response: {e}. Response was: {}",
                &response[..response.len().min(200)]
            ),
        }
        .build()
    })
}

/// A skill awaiting human review before activation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingSkill {
    /// The extracted skill content.
    pub skill: SkillContent,
    /// The candidate that was promoted to trigger extraction.
    pub candidate_id: String,
    /// Review status: `"pending_review"`, `"approved"`, `"rejected"`.
    pub status: String,
    /// When the skill was extracted.
    pub extracted_at: jiff::Timestamp,
}

impl PendingSkill {
    /// Create a new pending skill from an extracted skill and candidate.
    pub fn new(extracted: &ExtractedSkill, candidate_id: &str) -> Self {
        Self {
            skill: extracted.to_skill_content(),
            candidate_id: candidate_id.to_owned(),
            status: "pending_review".to_owned(),
            extracted_at: jiff::Timestamp::now(),
        }
    }

    /// Serialize to JSON for storage in a fact's `content` field.
    ///
    /// # Errors
    ///
    /// Returns a [`serde_json::Error`] if serialization fails.
    #[must_use = "this returns a Result that may contain a serialization error"]
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserialize from a fact's `content` field.
    ///
    /// # Errors
    ///
    /// Returns a [`serde_json::Error`] if the JSON is malformed.
    #[must_use = "this returns a Result that may contain a construction error"]
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Returns `true` if the skill is pending review.
    pub fn is_pending(&self) -> bool {
        self.status == "pending_review"
    }

    /// Returns `true` if the skill has been approved.
    pub fn is_approved(&self) -> bool {
        self.status == "approved"
    }
}

#[cfg(test)]
#[path = "extract_tests.rs"]
mod tests;
