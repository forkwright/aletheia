//! Context-dependent extraction refinement: turn classification, correction
//! detection, quality filters, and fact type classification.
//!
//! Different conversation turn types (tool-heavy, discussion, planning,
//! debugging, corrections) warrant different extraction strategies.

use serde::{Deserialize, Serialize};

/// Classifies a conversation turn for context-dependent extraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TurnType {
    /// General conversation: extract facts, entities, relationships.
    Discussion,
    /// Code/tool output dominant: extract decisions, skip noise.
    ToolHeavy,
    /// Architecture/design: extract decisions and rationale.
    Planning,
    /// Error investigation: extract resolution, skip stack traces.
    Debugging,
    /// Explicit corrections: high priority extraction.
    Correction,
    /// How-to/instructions: extract steps and dependencies.
    Procedural,
}

impl TurnType {
    /// Returns the confidence boost applied to facts extracted from this turn type.
    #[must_use]
    pub fn confidence_boost(self) -> f64 {
        match self {
            Self::Planning => 0.1,
            Self::Correction => 0.2,
            _ => 0.0,
        }
    }

    /// Returns the prompt appendix for this turn type's extraction behavior.
    #[must_use]
    pub fn prompt_appendix(self) -> &'static str {
        match self {
            Self::Discussion => DISCUSSION_APPENDIX,
            Self::ToolHeavy => TOOL_HEAVY_APPENDIX,
            Self::Planning => PLANNING_APPENDIX,
            Self::Debugging => DEBUGGING_APPENDIX,
            Self::Correction => CORRECTION_APPENDIX,
            Self::Procedural => PROCEDURAL_APPENDIX,
        }
    }
}

impl std::fmt::Display for TurnType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Discussion => f.write_str("discussion"),
            Self::ToolHeavy => f.write_str("tool_heavy"),
            Self::Planning => f.write_str("planning"),
            Self::Debugging => f.write_str("debugging"),
            Self::Correction => f.write_str("correction"),
            Self::Procedural => f.write_str("procedural"),
        }
    }
}

/// Classify a conversation turn based on content heuristics.
///
/// Priority order (first match wins):
/// 1. Correction patterns → `Correction`
/// 2. Tool output > 60% of content → `ToolHeavy`
/// 3. Planning keywords → `Planning`
/// 4. Error/debug patterns → `Debugging`
/// 5. Procedural patterns → `Procedural`
/// 6. Default → `Discussion`
#[must_use]
pub fn classify_turn(content: &str) -> TurnType {
    let lower = content.to_lowercase();

    if has_correction_patterns(&lower) {
        return TurnType::Correction;
    }

    if is_tool_heavy(content) {
        return TurnType::ToolHeavy;
    }

    if has_planning_patterns(&lower) {
        return TurnType::Planning;
    }

    if has_debugging_patterns(&lower) {
        return TurnType::Debugging;
    }

    if has_procedural_patterns(&lower) {
        return TurnType::Procedural;
    }

    TurnType::Discussion
}

/// Check for correction patterns in lowercased content.
fn has_correction_patterns(lower: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "actually, it's",
        "actually it's",
        "actually, that's",
        "actually that's",
        "i was wrong about",
        "correction:",
        "no, that's not right",
        "no that's not right",
        "update:",
        "i was mistaken",
        "let me correct",
        "to clarify,",
        "i need to correct",
        "that's incorrect",
        "that was incorrect",
    ];
    PATTERNS.iter().any(|p| lower.contains(p))
}

/// Check if tool output dominates the content (> 60%).
fn is_tool_heavy(content: &str) -> bool {
    const MARKERS: &[&str] = &[
        "```", "$ ", "error[", "warning[", "output:", "result:", "stdout:", "stderr:",
    ];

    let total_len = content.len();
    if total_len == 0 {
        return false;
    }

    let marker_adjacent_chars: usize = content
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            MARKERS.iter().any(|m| trimmed.starts_with(m))
                || trimmed.starts_with("│")
                || trimmed.starts_with("├")
                || trimmed.starts_with("└")
                || (trimmed.starts_with('/') && trimmed.contains('.'))
        })
        .map(|line| line.len() + 1) // +1 for newline
        .sum();

    let code_block_chars = count_code_block_chars(content);
    // WHY: Take the max rather than sum; both metrics identify the same code-heavy content.
    let tool_chars = marker_adjacent_chars.max(code_block_chars);
    tool_chars * 100 / total_len > 60
}

/// Count characters inside fenced code blocks.
fn count_code_block_chars(content: &str) -> usize {
    let mut in_block = false;
    let mut count = 0;
    for line in content.lines() {
        if line.trim_start().starts_with("```") {
            in_block = !in_block;
            count += line.len() + 1;
        } else if in_block {
            count += line.len() + 1;
        }
    }
    count
}

fn has_planning_patterns(lower: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "architecture",
        "design",
        "should we",
        "trade-off",
        "tradeoff",
        "trade off",
        "proposal",
        "approach",
        "strategy",
        "plan is to",
        "the plan",
        "going to implement",
        "implementation plan",
        "we could either",
        "option a",
        "option b",
        "pros and cons",
    ];
    // Require at least 2 planning keywords for confidence
    let matches = PATTERNS.iter().filter(|p| lower.contains(**p)).count();
    matches >= 2
}

fn has_debugging_patterns(lower: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "stack trace",
        "stacktrace",
        "panic",
        "segfault",
        "core dump",
        "backtrace",
        "thread 'main' panicked",
        "caused by:",
        "error:",
        "failed to",
        "debug",
        "investigating",
        "root cause",
    ];
    // Require at least 2 debugging keywords
    let matches = PATTERNS.iter().filter(|p| lower.contains(**p)).count();
    matches >= 2
}

fn has_procedural_patterns(lower: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "step 1",
        "step 2",
        "first,",
        "second,",
        "third,",
        "then,",
        "finally,",
        "next,",
        "follow these steps",
        "instructions:",
        "how to",
        "prerequisite",
        "make sure to",
        "before you",
        "after that",
    ];
    let matches = PATTERNS.iter().filter(|p| lower.contains(**p)).count();
    matches >= 2
}

/// Classification of extracted facts for FSRS decay rate tuning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FactType {
    /// Personal identity information (name, role, background).
    Identity,
    /// User preferences and opinions.
    Preference,
    /// Skills, tools, and expertise.
    Skill,
    /// Relationships between entities.
    Relationship,
    /// Time-bound events.
    Event,
    /// Tasks, todos, and action items.
    Task,
    /// General observations and inferences.
    Observation,
}

impl FactType {
    /// Returns the fact type string matching `knowledge::default_stability_hours`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::Preference => "preference",
            Self::Skill => "skill",
            Self::Relationship => "relationship",
            Self::Event => "event",
            Self::Task => "task",
            Self::Observation => "observation",
        }
    }
}

impl std::fmt::Display for FactType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Classify a fact's type from its content using keyword heuristics.
#[must_use]
pub fn classify_fact(content: &str) -> FactType {
    let lower = content.to_lowercase();

    if has_identity_patterns(&lower) {
        return FactType::Identity;
    }
    if has_preference_patterns(&lower) {
        return FactType::Preference;
    }
    if has_skill_patterns(&lower) {
        return FactType::Skill;
    }
    if has_task_patterns(&lower) {
        return FactType::Task;
    }
    if has_event_patterns(&lower) {
        return FactType::Event;
    }
    if has_relationship_patterns(&lower) {
        return FactType::Relationship;
    }

    FactType::Observation
}

fn has_identity_patterns(lower: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "i am",
        "my name is",
        "i identify",
        "i'm a",
        "my role is",
        "i work as",
        "my title is",
        "my background",
    ];
    PATTERNS.iter().any(|p| lower.contains(p))
}

fn has_preference_patterns(lower: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "i prefer",
        "i like",
        "i don't like",
        "i dislike",
        "my favorite",
        "i enjoy",
        "i hate",
        "i avoid",
        "i tend to",
        "i always",
        "i never",
    ];
    PATTERNS.iter().any(|p| lower.contains(p))
}

fn has_skill_patterns(lower: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "i know",
        "i use",
        "i work with",
        "i'm experienced",
        "i'm familiar with",
        "proficient in",
        "expert in",
        "skilled in",
        "i can write",
        "i develop",
    ];
    PATTERNS.iter().any(|p| lower.contains(p))
}

fn has_task_patterns(lower: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "todo", "need to", "should", "will", "plan to", "going to", "must", "have to", "deadline",
        "due date",
    ];
    // Need at least one strong task indicator
    PATTERNS.iter().any(|p| lower.contains(p))
        && (lower.contains("todo")
            || lower.contains("need to")
            || lower.contains("deadline")
            || lower.contains("due date"))
}

fn has_event_patterns(lower: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "yesterday",
        "last week",
        "last month",
        "last year",
        "this morning",
        "today",
        "tomorrow",
        "next week",
        "on monday",
        "on tuesday",
        "on wednesday",
        "on thursday",
        "on friday",
        "happened",
        "occurred",
    ];
    PATTERNS.iter().any(|p| lower.contains(p))
}

fn has_relationship_patterns(lower: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "works with",
        "reports to",
        "manages",
        "collaborates with",
        "is friends with",
        "is married to",
        "lives with",
        "partnered with",
        "employed by",
        "hired",
    ];
    PATTERNS.iter().any(|p| lower.contains(p))
}

/// Result of scanning content for correction signals.
#[derive(Debug, Clone)]
pub struct CorrectionSignal {
    /// Whether the content contains a correction.
    pub is_correction: bool,
    /// Confidence boost to apply (0.2 for corrections, 0.0 otherwise).
    pub confidence_boost: f64,
}

/// Detect whether content contains an explicit correction.
#[must_use]
pub fn detect_correction(content: &str) -> CorrectionSignal {
    let lower = content.to_lowercase();
    let is_correction = has_correction_patterns(&lower);
    CorrectionSignal {
        is_correction,
        confidence_boost: if is_correction { 0.2 } else { 0.0 },
    }
}

/// Reasons a fact may be rejected by quality filters.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FilterReason {
    /// Confidence score below threshold.
    LowConfidence,
    /// Content too short (< 10 chars).
    TooShort,
    /// Content too long (> 500 chars).
    TooLong,
    /// Content is trivial metadata.
    Trivial,
    /// Duplicate of an earlier fact in the same batch.
    Duplicate,
}

impl std::fmt::Display for FilterReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LowConfidence => f.write_str("low_confidence"),
            Self::TooShort => f.write_str("too_short"),
            Self::TooLong => f.write_str("too_long"),
            Self::Trivial => f.write_str("trivial"),
            Self::Duplicate => f.write_str("duplicate"),
        }
    }
}

/// Minimum confidence for a fact to be kept.
pub const CONFIDENCE_THRESHOLD: f64 = 0.3;
/// Minimum fact content length (chars).
pub const MIN_FACT_LENGTH: usize = 10;
/// Maximum fact content length (chars).
pub const MAX_FACT_LENGTH: usize = 500;

/// Result of applying quality filters to a fact.
#[derive(Debug, Clone)]
pub struct FilterResult {
    /// Whether the fact passed all filters.
    pub passed: bool,
    /// If rejected, the reason.
    pub reason: Option<FilterReason>,
}

/// Apply quality filters to a single fact.
///
/// Checks confidence threshold, length bounds, and triviality.
#[must_use]
pub fn filter_fact(content: &str, confidence: f64) -> FilterResult {
    if confidence < CONFIDENCE_THRESHOLD {
        return FilterResult {
            passed: false,
            reason: Some(FilterReason::LowConfidence),
        };
    }

    let trimmed = content.trim();
    if trimmed.len() < MIN_FACT_LENGTH {
        return FilterResult {
            passed: false,
            reason: Some(FilterReason::TooShort),
        };
    }

    if trimmed.len() > MAX_FACT_LENGTH {
        return FilterResult {
            passed: false,
            reason: Some(FilterReason::TooLong),
        };
    }

    if is_trivial(trimmed) {
        return FilterResult {
            passed: false,
            reason: Some(FilterReason::Trivial),
        };
    }

    FilterResult {
        passed: true,
        reason: None,
    }
}

const TRIVIAL_PATTERNS: &[&str] = &[
    "the file is",
    "the file has",
    "the file contains",
    "lines long",
    "lines of code",
    "bytes in size",
    "file size",
    "last modified",
    "was created on",
    "the output is",
    "the result is",
];

/// Check if a fact is trivial metadata that shouldn't be stored.
fn is_trivial(content: &str) -> bool {
    let lower = content.to_lowercase();
    TRIVIAL_PATTERNS.iter().any(|p| lower.contains(p))
}

/// A fact that was rejected by quality filters.
#[derive(Debug, Clone)]
pub struct RejectedFact {
    /// The fact content.
    pub content: String,
    /// The original confidence.
    pub confidence: f64,
    /// Why it was rejected.
    pub reason: FilterReason,
}

/// Result of applying quality filters to a batch.
#[derive(Debug, Clone)]
pub struct BatchFilterResult {
    /// Facts that passed all filters.
    pub passed: Vec<(String, f64)>,
    /// Facts that were rejected, with reasons.
    pub rejected: Vec<RejectedFact>,
}

/// Apply quality filters to a batch of extracted facts, including deduplication.
#[must_use]
pub fn filter_batch(facts: &[(String, f64)]) -> BatchFilterResult {
    let mut passed = Vec::new();
    let mut rejected = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for (content, confidence) in facts {
        let normalized = content.trim().to_lowercase();
        if seen.contains(&normalized) {
            rejected.push(RejectedFact {
                content: content.clone(),
                confidence: *confidence,
                reason: FilterReason::Duplicate,
            });
            continue;
        }

        let result = filter_fact(content, *confidence);
        if result.passed {
            seen.insert(normalized);
            passed.push((content.clone(), *confidence));
        } else if let Some(reason) = result.reason {
            rejected.push(RejectedFact {
                content: content.clone(),
                confidence: *confidence,
                reason,
            });
        }
    }

    BatchFilterResult { passed, rejected }
}

/// Apply a confidence boost, capped at 1.0.
#[must_use]
pub fn boosted_confidence(base: f64, boost: f64) -> f64 {
    (base + boost).min(1.0)
}

const DISCUSSION_APPENDIX: &str = "\
Extract all facts, entities, and relationships from the conversation. \
Focus on personal information, preferences, knowledge claims, and entity relationships.";

const TOOL_HEAVY_APPENDIX: &str = "\
This turn contains significant tool/code output. \
Focus ONLY on decisions and outcomes, not raw tool output. \
Skip: file listings, raw code blocks, terminal output, build logs. \
Extract: what was decided, what was accomplished, what changed.";

const PLANNING_APPENDIX: &str = "\
This turn discusses architecture or design. \
Focus on decisions with rationale and trade-offs considered. \
Extract: chosen approaches, rejected alternatives and why, design constraints. \
Skip: implementation details and boilerplate.";

const DEBUGGING_APPENDIX: &str = "\
This turn involves debugging or error investigation. \
Focus on root causes, resolutions, and lessons learned. \
Skip: stack traces, raw log lines, intermediate debug output. \
Extract: what the error was, why it happened, how it was fixed.";

const CORRECTION_APPENDIX: &str = "\
This turn contains explicit corrections to previously stated information. \
Mark any corrected facts with high confidence. \
Focus on: what was wrong, what the correct information is. \
These corrections should supersede earlier facts on the same topic.";

const PROCEDURAL_APPENDIX: &str = "\
This turn contains step-by-step instructions or procedures. \
Extract: ordered steps, dependencies, prerequisites. \
Skip: verbose explanations between steps. \
Focus on actionable instructions and their ordering.";

#[cfg(test)]
#[path = "refinement_tests.rs"]
mod tests;
