//! Instinct system — behavioral memory from tool usage patterns.
//!
//! Observes tool usage, aggregates patterns, and creates preference facts
//! when consistent behavioral patterns emerge. Not facts about the world —
//! facts about how to operate.
//!
//! Example: "When asked about code, use file search before web search"
//! (observed 47/50 times) → stored as a `FactType::Preference` fact.

use serde::{Deserialize, Serialize};

/// Maximum length for parameter values before truncation.
const MAX_PARAM_VALUE_LEN: usize = 200;

/// Maximum length for context summaries.
const MAX_CONTEXT_SUMMARY_LEN: usize = 100;

/// Minimum observations before a behavioral pattern is created.
const MIN_OBSERVATIONS: u32 = 5;

/// Minimum success rate (0.0–1.0) before a behavioral pattern is created.
const MIN_SUCCESS_RATE: f64 = 0.80;

/// Patterns matching potential secret values in tool parameters.
const SECRET_PATTERNS: &[&str] = &[
    "api_key",
    "api-key",
    "apikey",
    "secret",
    "password",
    "passwd",
    "token",
    "auth",
    "credential",
    "private_key",
    "private-key",
    "access_key",
    "access-key",
];

/// A recorded tool usage observation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolObservation {
    /// Name of the tool that was called.
    pub tool_name: String,
    /// Sanitized parameters (secrets stripped, values truncated).
    pub parameters: serde_json::Value,
    /// Outcome of the tool call.
    pub outcome: ToolOutcome,
    /// Brief summary of the context that prompted this tool call (≤100 chars).
    pub context_summary: String,
    /// Which nous made the observation.
    pub nous_id: String,
    /// When the observation was recorded.
    pub observed_at: jiff::Timestamp,
}

/// Outcome of a tool execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ToolOutcome {
    /// Tool completed successfully.
    Success,
    /// Tool failed with an error.
    Failure {
        /// Error description.
        error: String,
    },
    /// Tool partially succeeded.
    Partial {
        /// Description of partial result.
        note: String,
    },
}

impl ToolOutcome {
    /// Whether this outcome counts as a success for aggregation.
    #[must_use]
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success)
    }

    /// Serialize to a storage-friendly string.
    #[must_use]
    pub fn as_stored_string(&self) -> String {
        match self {
            Self::Success => "success".to_owned(),
            Self::Failure { error } => format!("failure:{error}"),
            Self::Partial { note } => format!("partial:{note}"),
        }
    }

    /// Parse from a stored string.
    #[must_use]
    pub fn from_stored_string(s: &str) -> Self {
        if s == "success" {
            Self::Success
        } else if let Some(error) = s.strip_prefix("failure:") {
            Self::Failure {
                error: error.to_owned(),
            }
        } else if let Some(note) = s.strip_prefix("partial:") {
            Self::Partial {
                note: note.to_owned(),
            }
        } else {
            Self::Failure {
                error: s.to_owned(),
            }
        }
    }
}

/// An aggregated behavioral pattern derived from tool observations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehavioralPattern {
    /// Human-readable pattern description.
    pub pattern: String,
    /// Tool name this pattern is about.
    pub tool_name: String,
    /// Simplified context category (code, research, system, memory, communication, other).
    pub context_type: String,
    /// Number of successful observations.
    pub success_count: u32,
    /// Total number of observations.
    pub total_count: u32,
    /// Success rate (0.0–1.0).
    pub success_rate: f64,
    /// When first observed.
    pub first_observed: jiff::Timestamp,
    /// When last observed.
    pub last_observed: jiff::Timestamp,
}

impl BehavioralPattern {
    /// Whether this pattern meets the thresholds for instinct fact creation.
    #[must_use]
    pub fn meets_thresholds(&self) -> bool {
        self.success_count >= MIN_OBSERVATIONS && self.success_rate >= MIN_SUCCESS_RATE
    }

    /// Generate the fact content string for this pattern.
    #[must_use]
    pub fn to_fact_content(&self) -> String {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "success_rate is [0.0, 1.0], multiplied by 100 fits in u32"
        )]
        let pct = (self.success_rate * 100.0) as u32;
        format!(
            "When working on {} tasks, tool '{}' is preferred (success rate: {}%, n={})",
            self.context_type, self.tool_name, pct, self.total_count
        )
    }
}

/// Context categories for tool usage classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextCategory {
    /// File operations, grep, code-related queries.
    Code,
    /// Web search, API lookups, documentation.
    Research,
    /// Exec, process management, health checks.
    System,
    /// Memory search, fact operations.
    Memory,
    /// Message, session send/ask.
    Communication,
    /// Anything else.
    Other,
}

impl ContextCategory {
    /// Classify a tool name and context summary into a context category.
    #[must_use]
    pub fn classify(tool_name: &str, context_summary: &str) -> Self {
        let tool_lower = tool_name.to_lowercase();
        let ctx_lower = context_summary.to_lowercase();

        // Tool-name-based classification
        if is_code_tool(&tool_lower) {
            return Self::Code;
        }
        if is_research_tool(&tool_lower) {
            return Self::Research;
        }
        if is_system_tool(&tool_lower) {
            return Self::System;
        }
        if is_memory_tool(&tool_lower) {
            return Self::Memory;
        }
        if is_communication_tool(&tool_lower) {
            return Self::Communication;
        }

        // Context-summary-based fallback
        if is_code_context(&ctx_lower) {
            return Self::Code;
        }
        if is_research_context(&ctx_lower) {
            return Self::Research;
        }
        if is_system_context(&ctx_lower) {
            return Self::System;
        }
        if is_memory_context(&ctx_lower) {
            return Self::Memory;
        }
        if is_communication_context(&ctx_lower) {
            return Self::Communication;
        }

        Self::Other
    }

    /// String representation for storage.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Code => "code",
            Self::Research => "research",
            Self::System => "system",
            Self::Memory => "memory",
            Self::Communication => "communication",
            Self::Other => "other",
        }
    }

    /// Parse from stored string.
    #[must_use]
    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "code" => Self::Code,
            "research" => Self::Research,
            "system" => Self::System,
            "memory" => Self::Memory,
            "communication" => Self::Communication,
            _ => Self::Other,
        }
    }
}

impl std::fmt::Display for ContextCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// --- Tool-name classification helpers ---

fn is_code_tool(tool: &str) -> bool {
    tool.contains("grep")
        || tool.contains("read_file")
        || tool.contains("write_file")
        || tool.contains("edit")
        || tool.contains("search_file")
        || tool.contains("list_dir")
        || tool.contains("glob")
        || tool.contains("find_file")
        || tool.contains("code")
        || tool.contains("lint")
        || tool.contains("format")
        || tool.contains("compile")
        || tool.contains("build")
}

fn is_research_tool(tool: &str) -> bool {
    tool.contains("web_search")
        || tool.contains("web_fetch")
        || tool.contains("browse")
        || tool.contains("http")
        || tool.contains("api_call")
        || tool.contains("docs")
        || tool.contains("lookup")
}

fn is_system_tool(tool: &str) -> bool {
    tool.contains("exec")
        || tool.contains("shell")
        || tool.contains("process")
        || tool.contains("health")
        || tool.contains("status")
        || tool.contains("monitor")
        || tool.contains("restart")
        || tool.contains("kill")
}

fn is_memory_tool(tool: &str) -> bool {
    tool.contains("memory")
        || tool.contains("recall")
        || tool.contains("fact")
        || tool.contains("knowledge")
        || tool.contains("remember")
        || tool.contains("forget")
}

fn is_communication_tool(tool: &str) -> bool {
    tool.contains("message")
        || tool.contains("send")
        || tool.contains("ask")
        || tool.contains("notify")
        || tool.contains("session")
        || tool.contains("chat")
}

// --- Context-summary classification helpers ---

fn is_code_context(ctx: &str) -> bool {
    ctx.contains("code")
        || ctx.contains("file")
        || ctx.contains("function")
        || ctx.contains("class")
        || ctx.contains("module")
        || ctx.contains("compile")
        || ctx.contains("bug")
        || ctx.contains("syntax")
}

fn is_research_context(ctx: &str) -> bool {
    ctx.contains("search")
        || ctx.contains("research")
        || ctx.contains("documentation")
        || ctx.contains("api")
        || ctx.contains("web")
        || ctx.contains("look up")
}

fn is_system_context(ctx: &str) -> bool {
    ctx.contains("process")
        || ctx.contains("system")
        || ctx.contains("server")
        || ctx.contains("deploy")
        || ctx.contains("health")
        || ctx.contains("restart")
}

fn is_memory_context(ctx: &str) -> bool {
    ctx.contains("remember")
        || ctx.contains("memory")
        || ctx.contains("recall")
        || ctx.contains("fact")
        || ctx.contains("knowledge")
}

fn is_communication_context(ctx: &str) -> bool {
    ctx.contains("message")
        || ctx.contains("communicate")
        || ctx.contains("notify")
        || ctx.contains("send")
        || ctx.contains("session")
}

// --- Sanitization ---

/// Sanitize tool parameters by stripping secrets and truncating values.
///
/// - Keys matching secret patterns have their values replaced with `"[REDACTED]"`.
/// - String values are truncated to 200 characters.
/// - Nested objects and arrays are processed recursively.
#[must_use]
pub fn sanitize_parameters(params: &serde_json::Value) -> serde_json::Value {
    match params {
        serde_json::Value::Object(map) => {
            let mut sanitized = serde_json::Map::new();
            for (key, value) in map {
                let key_lower = key.to_lowercase();
                if SECRET_PATTERNS.iter().any(|p| key_lower.contains(p)) {
                    sanitized.insert(
                        key.clone(),
                        serde_json::Value::String("[REDACTED]".to_owned()),
                    );
                } else {
                    sanitized.insert(key.clone(), sanitize_value(value));
                }
            }
            serde_json::Value::Object(sanitized)
        }
        other => sanitize_value(other),
    }
}

/// Sanitize a single JSON value (truncate strings, recurse into containers).
fn sanitize_value(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => {
            if s.len() > MAX_PARAM_VALUE_LEN {
                let truncated: String = s.chars().take(MAX_PARAM_VALUE_LEN).collect();
                serde_json::Value::String(format!("{truncated}..."))
            } else {
                serde_json::Value::String(s.clone())
            }
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(sanitize_value).collect())
        }
        serde_json::Value::Object(map) => {
            sanitize_parameters(&serde_json::Value::Object(map.clone()))
        }
        other => other.clone(),
    }
}

/// Truncate a context summary to the maximum allowed length.
#[must_use]
pub fn truncate_context_summary(summary: &str) -> String {
    if summary.len() <= MAX_CONTEXT_SUMMARY_LEN {
        summary.to_owned()
    } else {
        let truncated: String = summary.chars().take(MAX_CONTEXT_SUMMARY_LEN).collect();
        format!("{truncated}...")
    }
}

/// Datalog DDL for the `tool_observations` relation.
pub const TOOL_OBSERVATIONS_DDL: &str = r":create tool_observations {
    id: String =>
    tool_name: String,
    parameters: String,
    outcome: String,
    context_summary: String,
    nous_id: String,
    observed_at: String
}";

/// Aggregate raw observations into behavioral patterns.
///
/// Groups by (`tool_name`, `context_category`), computes success rates, and
/// returns patterns that meet the minimum thresholds.
#[must_use]
pub fn aggregate_observations(observations: &[ToolObservation]) -> Vec<BehavioralPattern> {
    use std::collections::HashMap;

    #[derive(Default)]
    struct Accum {
        success_count: u32,
        total_count: u32,
        first_observed: Option<jiff::Timestamp>,
        last_observed: Option<jiff::Timestamp>,
    }

    let mut groups: HashMap<(String, String), Accum> = HashMap::new();

    for obs in observations {
        let category = ContextCategory::classify(&obs.tool_name, &obs.context_summary);
        let key = (obs.tool_name.clone(), category.as_str().to_owned());
        let accum = groups.entry(key).or_default();

        accum.total_count += 1;
        if obs.outcome.is_success() {
            accum.success_count += 1;
        }

        match accum.first_observed {
            Some(ref ts) if obs.observed_at < *ts => {
                accum.first_observed = Some(obs.observed_at);
            }
            None => {
                accum.first_observed = Some(obs.observed_at);
            }
            _ => {}
        }

        match accum.last_observed {
            Some(ref ts) if obs.observed_at > *ts => {
                accum.last_observed = Some(obs.observed_at);
            }
            None => {
                accum.last_observed = Some(obs.observed_at);
            }
            _ => {}
        }
    }

    groups
        .into_iter()
        .filter_map(|((tool_name, context_type), accum)| {
            let success_rate = if accum.total_count > 0 {
                f64::from(accum.success_count) / f64::from(accum.total_count)
            } else {
                0.0
            };

            let pattern = BehavioralPattern {
                pattern: String::new(), // filled below
                tool_name,
                context_type,
                success_count: accum.success_count,
                total_count: accum.total_count,
                success_rate,
                first_observed: accum.first_observed.unwrap_or_else(jiff::Timestamp::now),
                last_observed: accum.last_observed.unwrap_or_else(jiff::Timestamp::now),
            };

            if pattern.meets_thresholds() {
                let content = pattern.to_fact_content();
                Some(BehavioralPattern {
                    pattern: content,
                    ..pattern
                })
            } else {
                None
            }
        })
        .collect()
}

/// Initial stability hours for instinct facts (7 days).
///
/// Low stability means the fact must be confirmed through continued observation
/// or it decays naturally via FSRS.
pub const INSTINCT_STABILITY_HOURS: f64 = 168.0;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::parse_timestamp;

    fn ts(s: &str) -> jiff::Timestamp {
        parse_timestamp(s).expect("valid test timestamp")
    }

    fn make_observation(
        tool_name: &str,
        context: &str,
        outcome: ToolOutcome,
        timestamp: &str,
    ) -> ToolObservation {
        ToolObservation {
            tool_name: tool_name.to_owned(),
            parameters: serde_json::json!({}),
            outcome,
            context_summary: context.to_owned(),
            nous_id: "test-nous".to_owned(),
            observed_at: ts(timestamp),
        }
    }

    // --- 1. Observation recording fields ---

    #[test]
    fn observation_stores_correct_fields() {
        let obs = ToolObservation {
            tool_name: "grep".to_owned(),
            parameters: serde_json::json!({"pattern": "TODO", "path": "/src"}),
            outcome: ToolOutcome::Success,
            context_summary: "searching for TODOs in code".to_owned(),
            nous_id: "nous-1".to_owned(),
            observed_at: ts("2026-03-01T12:00:00Z"),
        };
        assert_eq!(obs.tool_name, "grep");
        assert_eq!(obs.nous_id, "nous-1");
        assert!(obs.outcome.is_success());
        assert_eq!(obs.context_summary, "searching for TODOs in code");
    }

    // --- 2. Aggregation: 10 successful calls → pattern ---

    #[test]
    fn aggregation_creates_pattern_from_successful_calls() {
        let observations: Vec<_> = (0..10)
            .map(|i| {
                make_observation(
                    "grep",
                    "searching code files",
                    ToolOutcome::Success,
                    &format!("2026-03-{:02}T12:00:00Z", i + 1),
                )
            })
            .collect();

        let patterns = aggregate_observations(&observations);
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].tool_name, "grep");
        assert_eq!(patterns[0].context_type, "code");
        assert_eq!(patterns[0].success_count, 10);
        assert_eq!(patterns[0].total_count, 10);
        assert!((patterns[0].success_rate - 1.0).abs() < f64::EPSILON);
    }

    // --- 3. Threshold: below minimum observations ---

    #[test]
    fn aggregation_no_pattern_below_minimum_observations() {
        let observations: Vec<_> = (0..3)
            .map(|i| {
                make_observation(
                    "grep",
                    "code search",
                    ToolOutcome::Success,
                    &format!("2026-03-{:02}T12:00:00Z", i + 1),
                )
            })
            .collect();

        let patterns = aggregate_observations(&observations);
        assert!(
            patterns.is_empty(),
            "3 observations should not produce a pattern (minimum is 5)"
        );
    }

    // --- 4. Success rate threshold ---

    #[test]
    fn aggregation_no_pattern_below_success_rate() {
        let mut observations: Vec<_> = (0..4)
            .map(|i| {
                make_observation(
                    "web_search",
                    "research topic",
                    ToolOutcome::Success,
                    &format!("2026-03-{:02}T12:00:00Z", i + 1),
                )
            })
            .collect();
        // Add 6 failures → 4/10 = 40% success rate
        for i in 4..10 {
            observations.push(make_observation(
                "web_search",
                "research topic",
                ToolOutcome::Failure {
                    error: "timeout".to_owned(),
                },
                &format!("2026-03-{:02}T12:00:00Z", i + 1),
            ));
        }

        let patterns = aggregate_observations(&observations);
        assert!(
            patterns.is_empty(),
            "40% success rate should not produce a pattern (minimum is 80%)"
        );
    }

    // --- 5. Fact content generation ---

    #[test]
    fn pattern_generates_correct_fact_content() {
        let pattern = BehavioralPattern {
            pattern: String::new(),
            tool_name: "grep".to_owned(),
            context_type: "code".to_owned(),
            success_count: 8,
            total_count: 10,
            success_rate: 0.8,
            first_observed: ts("2026-03-01T00:00:00Z"),
            last_observed: ts("2026-03-10T00:00:00Z"),
        };
        let content = pattern.to_fact_content();
        assert!(content.contains("code"));
        assert!(content.contains("grep"));
        assert!(content.contains("80%"));
        assert!(content.contains("n=10"));
    }

    // --- 6. Sanitization: secrets stripped ---

    #[test]
    fn sanitize_strips_secret_parameters() {
        let params = serde_json::json!({
            "url": "https://acme.corp/api",
            "api_key": "test-key-not-real",
            "password": "hunter2",
            "query": "normal value"
        });
        let sanitized = sanitize_parameters(&params);
        let obj = sanitized.as_object().expect("should be object");
        assert_eq!(obj["api_key"], "[REDACTED]");
        assert_eq!(obj["password"], "[REDACTED]");
        assert_eq!(obj["url"], "https://acme.corp/api");
        assert_eq!(obj["query"], "normal value");
    }

    // --- 7. Context classification ---

    #[test]
    fn classify_grep_as_code() {
        assert_eq!(
            ContextCategory::classify("grep", "anything"),
            ContextCategory::Code
        );
    }

    #[test]
    fn classify_web_search_as_research() {
        assert_eq!(
            ContextCategory::classify("web_search", "anything"),
            ContextCategory::Research
        );
    }

    #[test]
    fn classify_exec_as_system() {
        assert_eq!(
            ContextCategory::classify("exec", "running command"),
            ContextCategory::System
        );
    }

    #[test]
    fn classify_memory_search_as_memory() {
        assert_eq!(
            ContextCategory::classify("memory_search", "looking up facts"),
            ContextCategory::Memory
        );
    }

    #[test]
    fn classify_send_message_as_communication() {
        assert_eq!(
            ContextCategory::classify("send_message", "notifying user"),
            ContextCategory::Communication
        );
    }

    #[test]
    fn classify_unknown_tool_with_code_context() {
        assert_eq!(
            ContextCategory::classify("custom_tool", "editing code file"),
            ContextCategory::Code
        );
    }

    #[test]
    fn classify_unknown_tool_unknown_context_as_other() {
        assert_eq!(
            ContextCategory::classify("custom_tool", "doing something"),
            ContextCategory::Other
        );
    }

    // --- 8. Sanitization: value truncation ---

    #[test]
    fn sanitize_truncates_long_values() {
        let long_value = "x".repeat(300);
        let params = serde_json::json!({"content": long_value});
        let sanitized = sanitize_parameters(&params);
        let content = sanitized["content"].as_str().expect("should be string");
        assert!(
            content.len() <= MAX_PARAM_VALUE_LEN + 5,
            "value should be truncated to ~200 chars"
        );
        assert!(content.ends_with("..."));
    }

    // --- 9. Context summary truncation ---

    #[test]
    fn truncate_context_summary_short_passthrough() {
        let short = "brief summary";
        assert_eq!(truncate_context_summary(short), short);
    }

    #[test]
    fn truncate_context_summary_long_truncated() {
        let long = "a".repeat(200);
        let truncated = truncate_context_summary(&long);
        assert!(truncated.len() <= MAX_CONTEXT_SUMMARY_LEN + 5);
        assert!(truncated.ends_with("..."));
    }

    // --- 10. ToolOutcome serialization roundtrip ---

    #[test]
    fn tool_outcome_stored_string_roundtrip() {
        let outcomes = [
            ToolOutcome::Success,
            ToolOutcome::Failure {
                error: "timeout".to_owned(),
            },
            ToolOutcome::Partial {
                note: "partial data".to_owned(),
            },
        ];
        for outcome in &outcomes {
            let stored = outcome.as_stored_string();
            let back = ToolOutcome::from_stored_string(&stored);
            assert_eq!(outcome, &back);
        }
    }

    // --- 11. BehavioralPattern threshold check ---

    #[test]
    fn pattern_meets_thresholds_at_boundary() {
        let pattern = BehavioralPattern {
            pattern: String::new(),
            tool_name: "grep".to_owned(),
            context_type: "code".to_owned(),
            success_count: 5,
            total_count: 6,
            success_rate: 5.0 / 6.0,
            first_observed: ts("2026-03-01T00:00:00Z"),
            last_observed: ts("2026-03-06T00:00:00Z"),
        };
        assert!(
            pattern.meets_thresholds(),
            "5 successes with 83% rate should meet thresholds"
        );
    }

    #[test]
    fn pattern_does_not_meet_thresholds_below() {
        let pattern = BehavioralPattern {
            pattern: String::new(),
            tool_name: "grep".to_owned(),
            context_type: "code".to_owned(),
            success_count: 4,
            total_count: 5,
            success_rate: 0.8,
            first_observed: ts("2026-03-01T00:00:00Z"),
            last_observed: ts("2026-03-05T00:00:00Z"),
        };
        assert!(
            !pattern.meets_thresholds(),
            "4 successes should not meet threshold (minimum is 5)"
        );
    }

    // --- 12. Multiple tools aggregate independently ---

    #[test]
    fn aggregation_separates_tools() {
        let mut observations = Vec::new();
        for i in 0..6 {
            observations.push(make_observation(
                "grep",
                "code search",
                ToolOutcome::Success,
                &format!("2026-03-{:02}T12:00:00Z", i + 1),
            ));
            observations.push(make_observation(
                "web_search",
                "research topic",
                ToolOutcome::Success,
                &format!("2026-03-{:02}T13:00:00Z", i + 1),
            ));
        }

        let patterns = aggregate_observations(&observations);
        assert_eq!(patterns.len(), 2, "should have separate patterns per tool");
        let names: Vec<_> = patterns.iter().map(|p| p.tool_name.as_str()).collect();
        assert!(names.contains(&"grep"));
        assert!(names.contains(&"web_search"));
    }

    // --- 13. Nested secret sanitization ---

    #[test]
    fn sanitize_handles_nested_objects() {
        let params = serde_json::json!({
            "config": {
                "api_key": "test-key-not-real",
                "name": "test"
            }
        });
        let sanitized = sanitize_parameters(&params);
        let config = sanitized["config"].as_object().expect("nested object");
        assert_eq!(config["api_key"], "[REDACTED]");
        assert_eq!(config["name"], "test");
    }

    // --- 14. Observation serde roundtrip ---

    #[test]
    fn observation_serde_roundtrip() {
        let obs = ToolObservation {
            tool_name: "read_file".to_owned(),
            parameters: serde_json::json!({"path": "/tmp/test.txt"}),
            outcome: ToolOutcome::Success,
            context_summary: "reading test file".to_owned(),
            nous_id: "nous-1".to_owned(),
            observed_at: ts("2026-03-01T12:00:00Z"),
        };
        let json = serde_json::to_string(&obs).expect("serialize");
        let back: ToolObservation = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(obs.tool_name, back.tool_name);
        assert_eq!(obs.nous_id, back.nous_id);
    }

    // --- 15. ContextCategory display/parse ---

    #[test]
    fn context_category_roundtrip() {
        let categories = [
            ContextCategory::Code,
            ContextCategory::Research,
            ContextCategory::System,
            ContextCategory::Memory,
            ContextCategory::Communication,
            ContextCategory::Other,
        ];
        for cat in categories {
            let s = cat.as_str();
            let back = ContextCategory::from_str_lossy(s);
            assert_eq!(cat, back, "roundtrip failed for {s}");
        }
    }
}
