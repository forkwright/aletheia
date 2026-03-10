//! LLM-based skill extraction from promoted candidates.
//!
//! When a [`SkillCandidate`] is promoted (recurrence count ≥ 3), this module
//! builds an extraction prompt from the candidate metadata and tool call
//! sequences, sends it to a cost-effective LLM (Haiku), and parses the
//! structured skill definition from the response.
//!
//! The extracted skill is stored with `status: "pending_review"` — it is NOT
//! automatically activated. A human or orchestrator must approve it first.

use serde::{Deserialize, Serialize};
use snafu::Snafu;
use std::fmt::Write as _;

use crate::skill::SkillContent;
use crate::skills::{SkillCandidate, ToolCallRecord};

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Provider trait
// ---------------------------------------------------------------------------

/// Minimal LLM completion interface for skill extraction.
///
/// Keeps mneme independent of hermeneus. The nous layer bridges this trait
/// to the full provider API, just like [`crate::extract::ExtractionProvider`].
pub trait SkillExtractionProvider: Send + Sync {
    /// Send a system + user message to the LLM and return the text response.
    fn complete(&self, system: &str, user_message: &str) -> Result<String, SkillExtractionError>;
}

// ---------------------------------------------------------------------------
// Extracted skill (intermediate representation)
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Extractor
// ---------------------------------------------------------------------------

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
    pub fn extract_skill(
        &self,
        candidate: &SkillCandidate,
        tool_call_sequences: &[Vec<ToolCallRecord>],
    ) -> Result<ExtractedSkill, SkillExtractionError> {
        let system = EXTRACTION_SYSTEM_PROMPT;
        let user_message = build_extraction_prompt(candidate, tool_call_sequences);
        let response = self.provider.complete(system, &user_message)?;
        parse_skill_response(&response)
    }
}

// ---------------------------------------------------------------------------
// Prompt construction
// ---------------------------------------------------------------------------

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

    // Candidate metadata
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

    // Tool call sequences
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

// ---------------------------------------------------------------------------
// Response parsing
// ---------------------------------------------------------------------------

/// Parse the LLM response into an [`ExtractedSkill`].
///
/// Handles JSON embedded in markdown fences, bare JSON, and minor formatting
/// issues from the LLM.
fn parse_skill_response(response: &str) -> Result<ExtractedSkill, SkillExtractionError> {
    let trimmed = response.trim();

    // Strip markdown code fences if present
    let json_str = if trimmed.starts_with("```") {
        // Find the opening fence end (after ```json or ```)
        let start = trimmed.find('\n').map_or(0, |i| i + 1);
        let end = trimmed
            .rfind("```")
            .filter(|&e| e > start)
            .unwrap_or(trimmed.len());
        &trimmed[start..end]
    } else {
        trimmed
    };

    // Try to find JSON object boundaries
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

// ---------------------------------------------------------------------------
// Pending skill wrapper
// ---------------------------------------------------------------------------

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
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserialize from a fact's `content` field.
    ///
    /// # Errors
    ///
    /// Returns a [`serde_json::Error`] if the JSON is malformed.
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::heuristics::PatternType;
    use crate::skills::signature::SequenceSignature;

    // -- Mock provider --------------------------------------------------------

    struct MockProvider {
        response: Result<String, SkillExtractionError>,
    }

    impl MockProvider {
        fn ok(response: &str) -> Self {
            Self {
                response: Ok(response.to_owned()),
            }
        }

        fn err(msg: &str) -> Self {
            Self {
                response: Err(LlmCallSnafu {
                    message: msg.to_owned(),
                }
                .build()),
            }
        }
    }

    impl SkillExtractionProvider for MockProvider {
        fn complete(
            &self,
            _system: &str,
            _user_message: &str,
        ) -> Result<String, SkillExtractionError> {
            self.response.as_ref().cloned().map_err(|_prev| {
                LlmCallSnafu {
                    message: "mock error".to_owned(),
                }
                .build()
            })
        }
    }

    fn sample_candidate() -> SkillCandidate {
        SkillCandidate {
            id: "cand-001".to_owned(),
            nous_id: "test-nous".to_owned(),
            signature: SequenceSignature {
                normalized: vec![
                    "Grep".to_owned(),
                    "Read".to_owned(),
                    "Edit".to_owned(),
                    "Bash".to_owned(),
                ],
                hash: 12345,
            },
            recurrence_count: 3,
            session_refs: vec!["s1".to_owned(), "s2".to_owned(), "s3".to_owned()],
            first_seen: jiff::Timestamp::now(),
            last_seen: jiff::Timestamp::now(),
            heuristic_score: 0.72,
            pattern_type: Some(PatternType::Diagnostic),
        }
    }

    fn sample_sequences() -> Vec<Vec<ToolCallRecord>> {
        vec![
            vec![
                ToolCallRecord::new("Grep", 120),
                ToolCallRecord::new("Read", 80),
                ToolCallRecord::new("Read", 90),
                ToolCallRecord::new("Edit", 150),
                ToolCallRecord::new("Bash", 200),
                ToolCallRecord::new("Bash", 100),
            ],
            vec![
                ToolCallRecord::new("Grep", 110),
                ToolCallRecord::new("Read", 75),
                ToolCallRecord::new("Edit", 160),
                ToolCallRecord::new("Edit", 90),
                ToolCallRecord::new("Bash", 180),
                ToolCallRecord::new("Bash", 120),
            ],
        ]
    }

    fn valid_json_response() -> String {
        r#"{
            "name": "test-driven-bug-fix",
            "description": "Diagnose test failures, fix source code, and validate",
            "steps": [
                "Search for failing test references",
                "Read relevant source files",
                "Edit source to fix the issue",
                "Run tests to verify"
            ],
            "tools_used": ["Grep", "Read", "Edit", "Bash"],
            "domain_tags": ["testing", "debugging"],
            "when_to_use": "When a test failure needs diagnosis and fix"
        }"#
        .to_owned()
    }

    // -- Prompt construction --------------------------------------------------

    #[test]
    fn build_prompt_includes_candidate_metadata() {
        let candidate = sample_candidate();
        let seqs = sample_sequences();
        let prompt = build_extraction_prompt(&candidate, &seqs);

        assert!(prompt.contains("Recurrence count: 3"));
        assert!(prompt.contains("Diagnostic"));
        assert!(prompt.contains("0.72"));
        assert!(prompt.contains("Grep → Read → Edit → Bash"));
    }

    #[test]
    fn build_prompt_includes_all_sessions() {
        let candidate = sample_candidate();
        let seqs = sample_sequences();
        let prompt = build_extraction_prompt(&candidate, &seqs);

        assert!(prompt.contains("Session 1"));
        assert!(prompt.contains("Session 2"));
        assert!(prompt.contains("6 calls"));
    }

    #[test]
    fn build_prompt_includes_tool_call_details() {
        let candidate = sample_candidate();
        let seqs = vec![vec![
            ToolCallRecord::new("Read", 50),
            ToolCallRecord::errored("Bash", 300),
        ]];
        let prompt = build_extraction_prompt(&candidate, &seqs);

        assert!(prompt.contains("Read (50ms)"));
        assert!(prompt.contains("Bash (300ms) [ERROR]"));
    }

    #[test]
    fn build_prompt_handles_empty_sequences() {
        let candidate = sample_candidate();
        let prompt = build_extraction_prompt(&candidate, &[]);

        assert!(prompt.contains("Candidate Pattern"));
        assert!(prompt.contains("Observed Tool Call Sequences"));
    }

    #[test]
    fn build_prompt_no_pattern_type() {
        let mut candidate = sample_candidate();
        candidate.pattern_type = None;
        let seqs = sample_sequences();
        let prompt = build_extraction_prompt(&candidate, &seqs);

        assert!(!prompt.contains("pattern type"));
    }

    // -- Response parsing -----------------------------------------------------

    #[test]
    fn parse_valid_json_response() {
        let response = valid_json_response();
        let skill = parse_skill_response(&response).unwrap();

        assert_eq!(skill.name, "test-driven-bug-fix");
        assert_eq!(skill.steps.len(), 4);
        assert_eq!(skill.tools_used, vec!["Grep", "Read", "Edit", "Bash"]);
        assert_eq!(skill.domain_tags, vec!["testing", "debugging"]);
        assert!(!skill.when_to_use.is_empty());
    }

    #[test]
    fn parse_json_in_markdown_fences() {
        let response = format!("```json\n{}\n```", valid_json_response());
        let skill = parse_skill_response(&response).unwrap();
        assert_eq!(skill.name, "test-driven-bug-fix");
    }

    #[test]
    fn parse_json_in_bare_fences() {
        let response = format!("```\n{}\n```", valid_json_response());
        let skill = parse_skill_response(&response).unwrap();
        assert_eq!(skill.name, "test-driven-bug-fix");
    }

    #[test]
    fn parse_json_with_surrounding_text() {
        let response = format!(
            "Here is the extracted skill:\n{}\nDone.",
            valid_json_response()
        );
        let skill = parse_skill_response(&response).unwrap();
        assert_eq!(skill.name, "test-driven-bug-fix");
    }

    #[test]
    fn parse_malformed_json_returns_error() {
        let response = "not json at all";
        let result = parse_skill_response(response);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("invalid JSON"));
    }

    #[test]
    fn parse_incomplete_json_returns_error() {
        let response = r#"{"name": "test", "description": "d"}"#;
        let result = parse_skill_response(response);
        // Missing required fields
        assert!(result.is_err());
    }

    #[test]
    fn parse_empty_response_returns_error() {
        let result = parse_skill_response("");
        assert!(result.is_err());
    }

    #[test]
    fn parse_empty_fields_succeeds() {
        let response = r#"{
            "name": "minimal-skill",
            "description": "A minimal skill",
            "steps": [],
            "tools_used": [],
            "domain_tags": [],
            "when_to_use": ""
        }"#;
        let skill = parse_skill_response(response).unwrap();
        assert_eq!(skill.name, "minimal-skill");
        assert!(skill.steps.is_empty());
    }

    // -- Extractor end-to-end -------------------------------------------------

    #[test]
    fn extractor_returns_skill_on_valid_response() {
        let provider = MockProvider::ok(&valid_json_response());
        let extractor = SkillExtractor::new(provider);
        let candidate = sample_candidate();
        let seqs = sample_sequences();

        let skill = extractor.extract_skill(&candidate, &seqs).unwrap();
        assert_eq!(skill.name, "test-driven-bug-fix");
    }

    #[test]
    fn extractor_returns_error_on_provider_failure() {
        let provider = MockProvider::err("API rate limited");
        let extractor = SkillExtractor::new(provider);
        let candidate = sample_candidate();
        let seqs = sample_sequences();

        let result = extractor.extract_skill(&candidate, &seqs);
        assert!(result.is_err());
    }

    #[test]
    fn extractor_returns_error_on_malformed_response() {
        let provider = MockProvider::ok("this is not json");
        let extractor = SkillExtractor::new(provider);
        let candidate = sample_candidate();
        let seqs = sample_sequences();

        let result = extractor.extract_skill(&candidate, &seqs);
        assert!(result.is_err());
    }

    // -- ExtractedSkill → SkillContent ----------------------------------------

    #[test]
    fn to_skill_content_sets_origin_extracted() {
        let extracted = ExtractedSkill {
            name: "test-skill".to_owned(),
            description: "A test skill".to_owned(),
            steps: vec!["step 1".to_owned()],
            tools_used: vec!["Read".to_owned()],
            domain_tags: vec!["test".to_owned()],
            when_to_use: "When testing".to_owned(),
        };
        let content = extracted.to_skill_content();
        assert_eq!(content.origin, "extracted");
    }

    #[test]
    fn to_skill_content_includes_when_to_use_in_description() {
        let extracted = ExtractedSkill {
            name: "test-skill".to_owned(),
            description: "A test skill".to_owned(),
            steps: vec![],
            tools_used: vec![],
            domain_tags: vec![],
            when_to_use: "When you need to test".to_owned(),
        };
        let content = extracted.to_skill_content();
        assert!(content.description.contains("When to Use"));
        assert!(content.description.contains("When you need to test"));
    }

    #[test]
    fn to_skill_content_omits_when_to_use_if_empty() {
        let extracted = ExtractedSkill {
            name: "test-skill".to_owned(),
            description: "A test skill".to_owned(),
            steps: vec![],
            tools_used: vec![],
            domain_tags: vec![],
            when_to_use: String::new(),
        };
        let content = extracted.to_skill_content();
        assert_eq!(content.description, "A test skill");
    }

    // -- PendingSkill ---------------------------------------------------------

    #[test]
    fn pending_skill_new_sets_status() {
        let extracted = ExtractedSkill {
            name: "test".to_owned(),
            description: "d".to_owned(),
            steps: vec![],
            tools_used: vec![],
            domain_tags: vec![],
            when_to_use: String::new(),
        };
        let pending = PendingSkill::new(&extracted, "cand-001");
        assert!(pending.is_pending());
        assert!(!pending.is_approved());
        assert_eq!(pending.candidate_id, "cand-001");
        assert_eq!(pending.status, "pending_review");
    }

    #[test]
    fn pending_skill_serialization_roundtrip() {
        let extracted = ExtractedSkill {
            name: "roundtrip-skill".to_owned(),
            description: "Tests serialization".to_owned(),
            steps: vec!["step 1".to_owned()],
            tools_used: vec!["Read".to_owned()],
            domain_tags: vec!["test".to_owned()],
            when_to_use: "For tests".to_owned(),
        };
        let pending = PendingSkill::new(&extracted, "cand-002");
        let json = pending.to_json().unwrap();
        let back = PendingSkill::from_json(&json).unwrap();
        assert_eq!(back.skill.name, "roundtrip-skill");
        assert_eq!(back.candidate_id, "cand-002");
        assert!(back.is_pending());
    }

    #[test]
    fn pending_skill_approved_status() {
        let mut pending = PendingSkill {
            skill: SkillContent {
                name: "s".to_owned(),
                description: "d".to_owned(),
                steps: vec![],
                tools_used: vec![],
                domain_tags: vec![],
                origin: "extracted".to_owned(),
            },
            candidate_id: "c".to_owned(),
            status: "approved".to_owned(),
            extracted_at: jiff::Timestamp::now(),
        };
        assert!(pending.is_approved());
        assert!(!pending.is_pending());

        pending.status = "rejected".to_owned();
        assert!(!pending.is_approved());
        assert!(!pending.is_pending());
    }

    // -- System prompt --------------------------------------------------------

    #[test]
    fn system_prompt_requests_json() {
        assert!(EXTRACTION_SYSTEM_PROMPT.contains("JSON"));
        assert!(EXTRACTION_SYSTEM_PROMPT.contains("name"));
        assert!(EXTRACTION_SYSTEM_PROMPT.contains("steps"));
        assert!(EXTRACTION_SYSTEM_PROMPT.contains("tools_used"));
        assert!(EXTRACTION_SYSTEM_PROMPT.contains("domain_tags"));
    }
}
