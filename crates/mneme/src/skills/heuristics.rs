//! Heuristic scoring for tool call sequences.
//!
//! Identifies which sequences are worth tracking as skill candidates.
//! The filter has two layers:
//!
//! 1. **Must-pass gates**: hard rejections (too short, too narrow, anti-patterns).
//! 2. **Scored signals**: coherence, diversity, and completion contribute to
//!    the total score (0.0–1.0).

use serde::{Deserialize, Serialize};

use crate::skills::ToolCallRecord;

/// Scoring result for a tool call sequence.
#[derive(Debug, Clone, Default)]
pub struct HeuristicScore {
    /// Overall quality score (0.0–1.0). Meaningful only when `passed_gates` is true.
    pub total: f64,
    /// Whether all must-pass gates were cleared.
    pub passed_gates: bool,
    /// Detected pattern type (if any).
    pub pattern_type: Option<PatternType>,
    /// Human-readable scoring breakdown for debugging.
    pub details: Vec<String>,
}

/// High-level pattern category detected in a tool call sequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PatternType {
    /// Read → analyze → fix cycle (debugging → verification).
    Diagnostic,
    /// Read → understand → transform → verify (code restructuring).
    Refactor,
    /// Search → read → synthesize (information gathering).
    Research,
    /// Create → test → iterate (constructive work).
    Build,
    /// Read → analyze → report (assessment without transformation).
    Review,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ToolCategory {
    Read,
    Search,
    Write,
    Execute,
    Orchestrate,
    Other,
}

fn tool_category(name: &str) -> ToolCategory {
    match name {
        "Read" | "Glob" => ToolCategory::Read,
        "Grep" | "WebSearch" | "WebFetch" => ToolCategory::Search,
        "Write" | "Edit" | "NotebookEdit" => ToolCategory::Write,
        "Bash" => ToolCategory::Execute,
        "Agent" | "TodoWrite" => ToolCategory::Orchestrate,
        _ => ToolCategory::Other,
    }
}

/// Score a tool call sequence for skill potential.
///
/// Returns a [`HeuristicScore`] with `passed_gates = false` if any must-pass
/// gate fails.  When gates pass, `total` reflects the composite quality score.
pub fn score_sequence(tool_calls: &[ToolCallRecord]) -> HeuristicScore {
    let mut score = HeuristicScore::default();

    if tool_calls.len() < 5 {
        score.details.push(format!(
            "REJECT: sequence too short ({} < 5)",
            tool_calls.len()
        ));
        return score;
    }

    let distinct = count_distinct_tools(tool_calls);
    if distinct < 3 {
        score
            .details
            .push(format!("REJECT: too few distinct tools ({distinct} < 3)"));
        return score;
    }

    if is_debugging_spiral(tool_calls) {
        score
            .details
            .push("REJECT: debugging spiral detected".to_owned());
        return score;
    }
    if is_single_file_edit(tool_calls) {
        score
            .details
            .push("REJECT: single-file edit detected".to_owned());
        return score;
    }
    if is_config_specific(tool_calls) {
        score
            .details
            .push("REJECT: config-specific inspection detected".to_owned());
        return score;
    }

    score.passed_gates = true;

    let coherence = coherence_score(tool_calls);
    score.total += coherence;
    score.details.push(format!("coherence: {coherence:.2}"));

    let diversity = diversity_score(tool_calls);
    score.total += diversity;
    score.details.push(format!("diversity: {diversity:.2}"));

    let completion = completion_score(tool_calls);
    score.total += completion;
    score.details.push(format!("completion: {completion:.2}"));

    score.total = score.total.min(1.0);

    score.pattern_type = detect_pattern(tool_calls);
    if let Some(ref p) = score.pattern_type {
        score.details.push(format!("pattern: {p:?}"));
    }

    score
}

fn count_distinct_tools(tool_calls: &[ToolCallRecord]) -> usize {
    let mut seen = std::collections::HashSet::new();
    for tc in tool_calls {
        seen.insert(tc.tool_name.as_str());
    }
    seen.len()
}

/// Debugging spiral: lots of Bash back-and-forth without meaningful progress.
///
/// Signs: >50% of calls are Bash AND error rate >20%.
#[expect(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    reason = "usize→f64: tool call counts are small; precision loss is impossible in practice"
)]
fn is_debugging_spiral(tool_calls: &[ToolCallRecord]) -> bool {
    let bash_count = tool_calls
        .iter()
        .filter(|tc| tc.tool_name == "Bash")
        .count();
    let error_count = tool_calls.iter().filter(|tc| tc.is_error).count();
    let total = tool_calls.len();

    let bash_ratio = bash_count as f64 / total as f64;
    let error_ratio = error_count as f64 / total as f64;

    bash_ratio > 0.50 && error_ratio > 0.20
}

/// Single-file edit: simple targeted change with no broader exploration.
///
/// Detected when only Read + Write/Edit tools appear (possibly with Bash),
/// AND the write count is exactly 1, AND no search tools are used.
fn is_single_file_edit(tool_calls: &[ToolCallRecord]) -> bool {
    let categories: Vec<ToolCategory> = tool_calls
        .iter()
        .map(|tc| tool_category(&tc.tool_name))
        .collect();

    let write_count = categories
        .iter()
        .filter(|c| **c == ToolCategory::Write)
        .count();
    let search_count = categories
        .iter()
        .filter(|c| **c == ToolCategory::Search)
        .count();
    let read_count = categories
        .iter()
        .filter(|c| **c == ToolCategory::Read)
        .count();

    write_count == 1 && search_count == 0 && read_count >= 1
}

/// Config-specific inspection: only reading and running commands, no writes,
/// no search expansion.  These are "look at the config" sessions, not
/// transferable skills.
fn is_config_specific(tool_calls: &[ToolCallRecord]) -> bool {
    let categories: Vec<ToolCategory> = tool_calls
        .iter()
        .map(|tc| tool_category(&tc.tool_name))
        .collect();

    let write_count = categories
        .iter()
        .filter(|c| **c == ToolCategory::Write)
        .count();
    let search_count = categories
        .iter()
        .filter(|c| **c == ToolCategory::Search)
        .count();
    let read_count = categories
        .iter()
        .filter(|c| **c == ToolCategory::Read)
        .count();
    let exec_count = categories
        .iter()
        .filter(|c| **c == ToolCategory::Execute)
        .count();

    write_count == 0
        && search_count == 0
        && read_count >= 2
        && (read_count + exec_count) == tool_calls.len()
}

/// Coherence score (0.0–0.30): rewards tool sequences that follow logical order.
///
/// Good transitions: Search→Read, Read→Write, Write→Bash, Grep→Read
fn coherence_score(tool_calls: &[ToolCallRecord]) -> f64 {
    let categories: Vec<ToolCategory> = tool_calls
        .iter()
        .map(|tc| tool_category(&tc.tool_name))
        .collect();

    let mut good_transitions = 0usize;
    let total_transitions = categories.len().saturating_sub(1);
    if total_transitions == 0 {
        return 0.0;
    }

    #[expect(
        clippy::indexing_slicing,
        reason = "windows(2) guarantees each window has exactly 2 elements"
    )]
    for window in categories.windows(2) {
        let (prev, next) = (&window[0], &window[1]);
        let is_good = matches!(
            (prev, next),
            (
                ToolCategory::Search,
                ToolCategory::Read | ToolCategory::Write
            ) | (
                ToolCategory::Read,
                ToolCategory::Write | ToolCategory::Execute
            ) | (ToolCategory::Write, ToolCategory::Execute)
        );
        if is_good {
            good_transitions += 1;
        }
    }

    #[expect(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        reason = "usize→f64: tool call counts are small; precision loss is impossible in practice"
    )]
    let ratio = good_transitions as f64 / total_transitions as f64;
    (ratio * 0.30).min(0.30)
}

/// Diversity score (0.0–0.40): rewards a healthy mix of tool categories.
fn diversity_score(tool_calls: &[ToolCallRecord]) -> f64 {
    let mut categories = std::collections::HashSet::new();
    for tc in tool_calls {
        let cat = tool_category(&tc.tool_name);
        if matches!(
            cat,
            ToolCategory::Read | ToolCategory::Search | ToolCategory::Write | ToolCategory::Execute
        ) {
            categories.insert(tc.tool_name.as_str());
        }
    }

    let mut distinct_cats = std::collections::HashSet::new();
    for tc in tool_calls {
        distinct_cats.insert(tool_category(&tc.tool_name));
    }
    distinct_cats.remove(&ToolCategory::Orchestrate);
    distinct_cats.remove(&ToolCategory::Other);

    match distinct_cats.len() {
        0..=2 => 0.0,
        3 => 0.20,
        4 => 0.30,
        _ => 0.40,
    }
}

/// Completion score (0.0–0.30): rewards sequences ending with verification.
fn completion_score(tool_calls: &[ToolCallRecord]) -> f64 {
    let last_few = tool_calls.iter().rev().take(3);
    for tc in last_few {
        if tc.tool_name == "Bash" {
            return 0.30;
        }
    }
    let has_bash = tool_calls.iter().any(|tc| tc.tool_name == "Bash");
    if has_bash {
        return 0.15;
    }
    if let Some(last) = tool_calls.last()
        && matches!(tool_category(&last.tool_name), ToolCategory::Write)
    {
        return 0.15;
    }
    0.0
}

#[expect(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    reason = "usize→f64: tool call counts are small; precision loss is impossible in practice"
)]
fn detect_pattern(tool_calls: &[ToolCallRecord]) -> Option<PatternType> {
    let categories: Vec<ToolCategory> = tool_calls
        .iter()
        .map(|tc| tool_category(&tc.tool_name))
        .collect();

    let total = tool_calls.len() as f64;
    let read_count = categories
        .iter()
        .filter(|c| **c == ToolCategory::Read)
        .count();
    let search_count = categories
        .iter()
        .filter(|c| **c == ToolCategory::Search)
        .count();
    let write_count = categories
        .iter()
        .filter(|c| **c == ToolCategory::Write)
        .count();
    let exec_count = categories
        .iter()
        .filter(|c| **c == ToolCategory::Execute)
        .count();

    if write_count >= 2 && exec_count >= 2 && has_write_exec_cycle(&categories) {
        return Some(PatternType::Build);
    }
    let explore_ratio = (search_count + read_count) as f64 / total;
    if explore_ratio > 0.60 && write_count == 0 {
        return Some(PatternType::Research);
    }
    if exec_count > 0
        && write_count >= 1
        && search_count + read_count > write_count
        && ends_with_execute(&categories)
        && (search_count > 0 || read_count >= 2)
    {
        return Some(PatternType::Diagnostic);
    }
    if read_count >= 2 && write_count >= 2 && exec_count > 0 && ends_with_execute(&categories) {
        return Some(PatternType::Refactor);
    }
    let read_heavy = (read_count + search_count) as f64 / total > 0.50;
    let write_light = (write_count as f64 / total) < 0.20;
    if read_heavy && write_light {
        return Some(PatternType::Review);
    }
    None
}

fn has_write_exec_cycle(categories: &[ToolCategory]) -> bool {
    #[expect(
        clippy::indexing_slicing,
        reason = "windows(2) guarantees each window has exactly 2 elements"
    )]
    categories
        .windows(2)
        .any(|w| w[0] == ToolCategory::Write && w[1] == ToolCategory::Execute)
}

fn ends_with_execute(categories: &[ToolCategory]) -> bool {
    categories
        .iter()
        .rev()
        .take(3)
        .any(|c| *c == ToolCategory::Execute)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::ToolCallRecord;

    fn tc(name: &str) -> ToolCallRecord {
        ToolCallRecord::new(name, 100)
    }

    fn tc_err(name: &str) -> ToolCallRecord {
        ToolCallRecord::errored(name, 100)
    }

    /// Build a sequence of tools from names.
    fn seq(names: &[&str]) -> Vec<ToolCallRecord> {
        names.iter().map(|n| tc(n)).collect()
    }

    #[test]
    fn gate_rejects_short_sequence() {
        let calls = seq(&["Read", "Edit", "Bash", "Read"]);
        let score = score_sequence(&calls);
        assert!(
            !score.passed_gates,
            "sequence with fewer than 5 calls should fail the length gate"
        );
        assert!(
            score.total < f64::EPSILON,
            "rejected sequence should have a zero score"
        );
    }

    #[test]
    fn gate_rejects_too_few_distinct_tools() {
        let calls = seq(&["Read", "Read", "Read", "Edit", "Edit", "Edit"]);
        let score = score_sequence(&calls);
        assert!(
            !score.passed_gates,
            "sequence with fewer than 3 distinct tools should fail the diversity gate"
        );
    }

    #[test]
    fn gate_passes_for_valid_sequence() {
        let calls = seq(&["Grep", "Read", "Read", "Edit", "Bash", "Bash"]);
        let score = score_sequence(&calls);
        assert!(
            score.passed_gates,
            "valid sequence with sufficient length and tool diversity should pass all gates"
        );
    }

    #[test]
    fn antipattern_debugging_spiral_rejected() {
        let mut calls = vec![
            tc("Read"),
            tc_err("Bash"),
            tc_err("Bash"),
            tc_err("Bash"),
            tc("Bash"),
            tc("Bash"),
            tc("Bash"),
            tc_err("Bash"),
            tc("Grep"),
        ];
        calls.push(tc("Bash")); // 10 total: 8 Bash (80%), 4 errors (40%)
        let score = score_sequence(&calls);
        assert!(
            !score.passed_gates,
            "debugging spiral (high Bash ratio + high error rate) should fail gates"
        );
        assert!(
            score.details.iter().any(|d| d.contains("debugging spiral")),
            "rejection details should mention debugging spiral"
        );
    }

    #[test]
    fn antipattern_debugging_spiral_requires_both_conditions() {
        let calls = seq(&["Read", "Grep", "Bash", "Bash", "Bash", "Bash", "Edit"]);
        let score = score_sequence(&calls);
        assert!(
            !score.details.iter().any(|d| d.contains("debugging spiral")),
            "high Bash ratio without high error rate should not trigger spiral detection"
        );
    }

    #[test]
    fn antipattern_single_file_edit_rejected() {
        let calls = seq(&["Read", "Read", "Edit", "Read", "Bash", "Read"]);
        let score = score_sequence(&calls);
        assert!(
            !score.passed_gates,
            "single-file edit pattern should fail gates"
        );
        assert!(
            score.details.iter().any(|d| d.contains("single-file edit")),
            "rejection details should mention single-file edit"
        );
    }

    #[test]
    fn antipattern_single_file_edit_not_triggered_with_search() {
        let calls = seq(&["Grep", "Read", "Edit", "Read", "Bash", "Bash"]);
        let score = score_sequence(&calls);
        assert!(
            !score.details.iter().any(|d| d.contains("single-file edit")),
            "presence of search tools should prevent single-file edit detection"
        );
    }

    #[test]
    fn antipattern_single_file_edit_not_triggered_with_multiple_writes() {
        let calls = seq(&["Read", "Edit", "Edit", "Write", "Bash", "Bash"]);
        let score = score_sequence(&calls);
        assert!(
            !score.details.iter().any(|d| d.contains("single-file edit")),
            "multiple writes should prevent single-file edit detection"
        );
    }

    #[test]
    fn antipattern_config_specific_rejected() {
        // WHY: Read, Glob, and Bash only: no writes, no search.
        // Three distinct tool names passes the distinct-tool gate, but the
        // config-specific anti-pattern then fires because all activity is
        // just reading/glob and running checks without writing anything.
        let calls = seq(&["Read", "Glob", "Bash", "Read", "Bash", "Glob"]);
        let score = score_sequence(&calls);
        assert!(
            !score.passed_gates,
            "config-specific inspection pattern should fail gates"
        );
        assert!(
            score.details.iter().any(|d| d.contains("config-specific")),
            "rejection details should mention config-specific inspection"
        );
    }

    #[test]
    fn antipattern_config_specific_not_triggered_with_writes() {
        let calls = seq(&["Read", "Read", "Bash", "Read", "Edit", "Bash"]);
        let score = score_sequence(&calls);
        assert!(
            !score.details.iter().any(|d| d.contains("config-specific")),
            "presence of write tools should prevent config-specific detection"
        );
    }

    #[test]
    fn pattern_research_detected() {
        let calls = seq(&[
            "Grep",
            "WebSearch",
            "Read",
            "Read",
            "WebFetch",
            "Read",
            "Read",
        ]);
        let score = score_sequence(&calls);
        assert!(
            score.passed_gates,
            "research-pattern sequence should pass all gates"
        );
        assert_eq!(
            score.pattern_type,
            Some(PatternType::Research),
            "search-heavy read-only sequence should be classified as Research"
        );
    }

    #[test]
    fn pattern_build_detected() {
        let calls = seq(&["Read", "Write", "Bash", "Edit", "Bash", "Edit", "Bash"]);
        let score = score_sequence(&calls);
        assert!(
            score.passed_gates,
            "build-pattern sequence should pass all gates"
        );
        assert_eq!(
            score.pattern_type,
            Some(PatternType::Build),
            "write-exec cycle sequence should be classified as Build"
        );
    }

    #[test]
    fn pattern_diagnostic_detected() {
        let calls = seq(&["Grep", "Read", "Read", "Read", "Edit", "Bash"]);
        let score = score_sequence(&calls);
        assert!(
            score.passed_gates,
            "diagnostic-pattern sequence should pass all gates"
        );
        assert_eq!(
            score.pattern_type,
            Some(PatternType::Diagnostic),
            "search-read-fix-verify sequence should be classified as Diagnostic"
        );
    }

    #[test]
    fn pattern_refactor_detected() {
        let calls = seq(&["Read", "Read", "Read", "Edit", "Edit", "Write", "Bash"]);
        let score = score_sequence(&calls);
        assert!(
            score.passed_gates,
            "refactor-pattern sequence should pass all gates"
        );
        assert_eq!(
            score.pattern_type,
            Some(PatternType::Refactor),
            "read-transform-verify sequence should be classified as Refactor"
        );
    }

    #[test]
    fn pattern_review_detected() {
        // NOTE: Review: heavy read, small write (a note/comment), ends with Write
        // not Execute, so Diagnostic/Refactor don't fire.
        // write_count=1 means Research (write==0) is excluded.
        let calls = seq(&["Read", "Read", "Grep", "Read", "Read", "Write"]);
        let score = score_sequence(&calls);
        assert!(
            score.passed_gates,
            "review-pattern sequence should pass all gates"
        );
        assert_eq!(
            score.pattern_type,
            Some(PatternType::Review),
            "read-heavy write-light sequence should be classified as Review"
        );
    }

    #[test]
    fn score_total_positive_for_good_sequence() {
        let calls = seq(&["Grep", "Read", "Read", "Edit", "Edit", "Bash"]);
        let score = score_sequence(&calls);
        assert!(
            score.passed_gates,
            "well-formed sequence should pass all gates"
        );
        assert!(
            score.total > 0.0,
            "well-formed sequence should have a positive score"
        );
    }

    #[test]
    fn score_total_at_most_one() {
        let calls = seq(&[
            "Grep",
            "WebSearch",
            "Read",
            "Edit",
            "Bash",
            "Edit",
            "Bash",
            "Write",
            "Bash",
        ]);
        let score = score_sequence(&calls);
        assert!(
            score.passed_gates,
            "diverse high-quality sequence should pass all gates"
        );
        assert!(
            score.total <= 1.0,
            "total score should be clamped to 1.0, got {}",
            score.total
        );
    }

    #[test]
    fn any_passing_score_has_passed_gates() {
        let sequences: &[&[&str]] = &[
            &["Grep", "Read", "Read", "Edit", "Bash", "Bash"],
            &["Read", "Read", "Grep", "Write", "Bash", "Bash"],
        ];
        for names in sequences {
            let calls = seq(names);
            let score = score_sequence(&calls);
            if score.total > 0.0 {
                assert!(
                    score.passed_gates,
                    "total={} but passed_gates=false for {names:?}",
                    score.total
                );
            }
        }
    }

    #[test]
    fn details_contain_breakdown() {
        let calls = seq(&["Grep", "Read", "Read", "Edit", "Bash", "Bash"]);
        let score = score_sequence(&calls);
        assert!(
            score.passed_gates,
            "valid sequence should pass gates before checking details"
        );
        assert!(
            score.details.iter().any(|d| d.contains("coherence:")),
            "scoring details should include coherence breakdown"
        );
        assert!(
            score.details.iter().any(|d| d.contains("diversity:")),
            "scoring details should include diversity breakdown"
        );
        assert!(
            score.details.iter().any(|d| d.contains("completion:")),
            "scoring details should include completion breakdown"
        );
    }
}
