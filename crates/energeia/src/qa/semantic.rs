// WHY: Semantic evaluation uses LLM to assess acceptance criteria that cannot
// be verified mechanically. This module provides criteria classification,
// evaluation prompt construction, and structured response parsing.

use std::fmt::Write as _;

use serde::Deserialize;

use crate::types::{CriterionResult, CriterionType};

// ---------------------------------------------------------------------------
// Criteria classification
// ---------------------------------------------------------------------------

/// Keywords indicating a criterion verifiable by mechanical checks.
const MECHANICAL_KEYWORDS: &[&str] = &[
    "compiles",
    "passes",
    "no warnings",
    "format",
    "clippy",
    "test",
    "zero",
    "builds",
    "cargo",
    "succeeds",
    "no errors",
];

/// Keywords indicating a criterion requiring LLM evaluation.
const SEMANTIC_KEYWORDS: &[&str] = &[
    "correct",
    "clean",
    "readable",
    "documented",
    "handles",
    "implements",
    "design",
    "appropriate",
    "properly",
    "meaningful",
    "idiomatic",
    "follows",
    "consistent",
];

/// Classify acceptance criteria as mechanical or semantic.
///
/// Rules:
/// - Contains mechanical keyword → `CriterionType::Mechanical`
/// - Contains semantic keyword → `CriterionType::Semantic`
/// - If both match, mechanical takes precedence (cheaper to verify)
/// - Default (no keyword match) → `CriterionType::Semantic`
pub(crate) fn classify_criteria(criteria: &[String]) -> Vec<(String, CriterionType)> {
    criteria
        .iter()
        .map(|criterion| {
            let classification = classify_single(criterion);
            (criterion.clone(), classification)
        })
        .collect()
}

fn classify_single(criterion: &str) -> CriterionType {
    let lower = criterion.to_lowercase();

    // WHY: Check mechanical first — it's cheaper (no LLM cost).
    if MECHANICAL_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
        return CriterionType::Mechanical;
    }

    if SEMANTIC_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
        return CriterionType::Semantic;
    }

    // NOTE: Default to semantic — safe fallback since it can handle anything.
    CriterionType::Semantic
}

// ---------------------------------------------------------------------------
// Evaluation prompt construction
// ---------------------------------------------------------------------------

/// Few-shot example: passing evaluation.
const EXAMPLE_PASS_JSON: &str = r#"{
  "verdict": "green",
  "criteria": [
    {
      "criterion_id": "C1",
      "verdict": "pass",
      "confidence": 0.95,
      "evidence": "Function `flush()` implemented at line 47, called in Drop impl at line 62",
      "reasoning": "The requirement asks for a flush method called on DROP. Both are present and correctly wired."
    }
  ],
  "summary": "All criteria met. Implementation is clean and well-tested."
}"#;

/// Few-shot example: failing evaluation.
const EXAMPLE_FAIL_JSON: &str = r#"{
  "verdict": "red",
  "criteria": [
    {
      "criterion_id": "C1",
      "verdict": "pass",
      "confidence": 0.90,
      "evidence": "Tests added in test_calibration module",
      "reasoning": "Calibration tests exist and cover the main path."
    },
    {
      "criterion_id": "C2",
      "verdict": "fail",
      "confidence": 0.85,
      "evidence": "No fallback logic found when data count < 10",
      "reasoning": "The requirement specifies fallback to hardcoded weights with insufficient data, but calibrate_weights() panics on empty input."
    }
  ],
  "summary": "C2 fails: missing fallback for insufficient calibration data."
}"#;

/// Expected JSON output schema for the LLM evaluator.
const OUTPUT_SCHEMA: &str = r#"{
  "verdict": "green | yellow | red",
  "criteria": [
    {
      "criterion_id": "C<N>",
      "verdict": "pass | fail",
      "confidence": 0.0-1.0,
      "evidence": "specific code references from the diff",
      "reasoning": "chain-of-thought explaining the verdict"
    }
  ],
  "summary": "one-line aggregate summary"
}"#;

/// Construct an XML-structured evaluation prompt for the LLM QA evaluator.
///
/// The prompt includes the task description, numbered criteria, the PR diff
/// (truncated at 100K chars to stay within context limits), few-shot examples,
/// and the expected output schema.
pub(crate) fn build_qa_prompt(
    description: &str,
    criteria: &[(String, CriterionType)],
    diff: &str,
) -> String {
    let mut out = String::new();

    out.push_str("<task>\n");
    out.push_str("Evaluate the following code changes against the acceptance criteria.\n");
    let _ = writeln!(out, "Original task: {description}");
    out.push_str("</task>\n\n");

    out.push_str("<criteria>\n");
    for (i, (text, _)) in criteria.iter().enumerate() {
        let _ = writeln!(out, "  <criterion id=\"C{}\">{text}</criterion>", i + 1);
    }
    out.push_str("</criteria>\n\n");

    // NOTE: Truncate large diffs to avoid exceeding context limits.
    // 100K chars is roughly 25K tokens. Use get() for safe slicing.
    let truncated_diff = if diff.len() > 100_000 {
        let boundary = (0..=100_000)
            .rev()
            .find(|&i| diff.is_char_boundary(i))
            .unwrap_or(0);
        let truncated = diff.get(..boundary).unwrap_or_default();
        format!("{truncated}\n\n[... diff truncated at 100K chars ...]")
    } else {
        diff.to_owned()
    };

    out.push_str("<diff>\n");
    out.push_str(&truncated_diff);
    out.push_str("\n</diff>\n\n");

    out.push_str("<examples>\n");
    out.push_str("<example_pass>\n");
    out.push_str(EXAMPLE_PASS_JSON);
    out.push_str("\n</example_pass>\n");
    out.push_str("<example_fail>\n");
    out.push_str(EXAMPLE_FAIL_JSON);
    out.push_str("\n</example_fail>\n");
    out.push_str("</examples>\n\n");

    out.push_str("<output_format>\n");
    out.push_str("Respond with ONLY a JSON object matching this schema:\n");
    out.push_str(OUTPUT_SCHEMA);
    out.push_str("\n</output_format>\n");

    out
}

// ---------------------------------------------------------------------------
// Response parsing
// ---------------------------------------------------------------------------

/// JSON response from the LLM QA evaluator.
#[derive(Debug, Deserialize)]
struct QaResponse {
    /// Per-criterion evaluation results.
    criteria: Vec<QaCriterionResult>,
}

/// Single criterion result from the JSON response.
#[derive(Debug, Deserialize)]
struct QaCriterionResult {
    /// Criterion identifier matching the prompt (e.g. "C1").
    criterion_id: String,
    /// Per-criterion verdict: "pass" or "fail".
    verdict: String,
    /// Specific code references supporting the verdict.
    #[serde(default)]
    evidence: String,
}

/// Parse a QA evaluation response, trying JSON first then legacy format.
///
/// This function is infallible: unparseable responses produce criteria marked
/// as failed with explanatory evidence.
pub fn parse_qa_response(raw: &str, criteria: &[(String, CriterionType)]) -> Vec<CriterionResult> {
    // NOTE: Try direct JSON parse.
    if let Ok(response) = serde_json::from_str::<QaResponse>(raw) {
        return map_json_results(&response, criteria);
    }

    // NOTE: Try extracting JSON from markdown code fence.
    if let Some(json_block) = extract_json_block(raw)
        && let Ok(response) = serde_json::from_str::<QaResponse>(&json_block)
    {
        return map_json_results(&response, criteria);
    }

    // NOTE: Fallback to legacy line-based parsing.
    parse_legacy_format(raw, criteria)
}

/// Map JSON criterion results to internal `CriterionResult` values.
fn map_json_results(
    response: &QaResponse,
    criteria: &[(String, CriterionType)],
) -> Vec<CriterionResult> {
    let mut results = Vec::new();

    for json_cr in &response.criteria {
        // WHY: Criterion IDs are "C1", "C2", etc. Parse the numeric suffix.
        let idx = json_cr
            .criterion_id
            .strip_prefix('C')
            .and_then(|n| n.parse::<usize>().ok())
            .map(|n| n.saturating_sub(1));

        if let Some(i) = idx
            && let Some((text, classification)) = criteria.get(i)
        {
            let passed = json_cr.verdict.trim().eq_ignore_ascii_case("pass");
            results.push(CriterionResult {
                criterion: text.clone(),
                classification: *classification,
                passed,
                evidence: json_cr.evidence.clone(),
            });
        }
    }

    // WHY: If the LLM skipped some criteria, mark them as failed.
    let evaluated: Vec<String> = results.iter().map(|r| r.criterion.clone()).collect();
    for (text, classification) in criteria {
        if !evaluated.iter().any(|ec| ec == text) {
            results.push(CriterionResult {
                criterion: text.clone(),
                classification: *classification,
                passed: false,
                evidence: "criterion not evaluated by LLM".to_owned(),
            });
        }
    }

    results
}

/// Extract JSON content from a markdown code fence.
fn extract_json_block(raw: &str) -> Option<String> {
    let start = raw
        .find("```json")
        .map(|i| i + 7)
        .or_else(|| raw.find("```").map(|i| i + 3))?;

    // WHY: Use get() to avoid panicking on non-ASCII boundaries.
    let tail = raw.get(start..)?;
    let end = tail.find("```").map(|i| start + i)?;
    Some(raw.get(start..end)?.trim().to_owned())
}

/// Parse legacy `CRITERION`/`VERDICT`/`EVIDENCE` line-based format.
fn parse_legacy_format(
    response: &str,
    criteria: &[(String, CriterionType)],
) -> Vec<CriterionResult> {
    let mut results = Vec::new();

    let mut current_verdict: Option<bool> = None;
    let mut current_evidence = String::new();
    let mut current_criterion_idx: Option<usize> = None;

    for line in response.lines() {
        let trimmed = line.trim().trim_start_matches("- ");

        if let Some(criterion_text) = trimmed.strip_prefix("CRITERION:") {
            // NOTE: Flush previous criterion if any.
            if let Some(idx) = current_criterion_idx {
                flush_criterion(
                    idx,
                    current_verdict,
                    &current_evidence,
                    criteria,
                    &mut results,
                );
            }

            current_criterion_idx = find_matching_criterion(criterion_text.trim(), criteria);
            current_verdict = None;
            current_evidence.clear();
        } else if let Some(verdict_text) = trimmed.strip_prefix("VERDICT:") {
            current_verdict = Some(verdict_text.trim().eq_ignore_ascii_case("PASS"));
        } else if let Some(evidence_text) = trimmed.strip_prefix("EVIDENCE:") {
            evidence_text.trim().clone_into(&mut current_evidence);
        }
    }

    // NOTE: Flush the last criterion.
    if let Some(idx) = current_criterion_idx {
        flush_criterion(
            idx,
            current_verdict,
            &current_evidence,
            criteria,
            &mut results,
        );
    }

    // NOTE: Mark uncovered criteria as failed.
    let evaluated: Vec<String> = results.iter().map(|r| r.criterion.clone()).collect();
    for (text, classification) in criteria {
        if !evaluated.iter().any(|ec| ec == text) {
            results.push(CriterionResult {
                criterion: text.clone(),
                classification: *classification,
                passed: false,
                evidence: "criterion not evaluated by LLM".to_owned(),
            });
        }
    }

    results
}

fn flush_criterion(
    idx: usize,
    verdict: Option<bool>,
    evidence: &str,
    criteria: &[(String, CriterionType)],
    results: &mut Vec<CriterionResult>,
) {
    if let Some((text, classification)) = criteria.get(idx) {
        let passed = verdict.unwrap_or(false);
        results.push(CriterionResult {
            criterion: text.clone(),
            classification: *classification,
            passed,
            evidence: if evidence.is_empty() {
                "no evidence provided".to_owned()
            } else {
                evidence.to_owned()
            },
        });
    }
}

/// Find the criterion index that best matches the given text.
fn find_matching_criterion(text: &str, criteria: &[(String, CriterionType)]) -> Option<usize> {
    let text_lower = text.to_lowercase();

    // NOTE: Try exact match first.
    for (i, (criterion, _)) in criteria.iter().enumerate() {
        if criterion.to_lowercase() == text_lower {
            return Some(i);
        }
    }

    // NOTE: Fall back to substring match.
    for (i, (criterion, _)) in criteria.iter().enumerate() {
        let criterion_lower = criterion.to_lowercase();
        if text_lower.contains(&criterion_lower) || criterion_lower.contains(&text_lower) {
            return Some(i);
        }
    }

    None
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test assertions"
)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // classify_criteria
    // -----------------------------------------------------------------------

    #[test]
    fn classifies_cargo_build_as_mechanical() {
        let criteria = vec!["`cargo build --workspace` succeeds with zero warnings".to_owned()];
        let result = classify_criteria(&criteria);
        assert_eq!(result[0].1, CriterionType::Mechanical);
    }

    #[test]
    fn classifies_clippy_as_mechanical() {
        let criteria = vec!["`cargo clippy --workspace` passes".to_owned()];
        let result = classify_criteria(&criteria);
        assert_eq!(result[0].1, CriterionType::Mechanical);
    }

    #[test]
    fn classifies_implements_as_semantic() {
        let criteria = vec!["Implements proper error handling with context".to_owned()];
        let result = classify_criteria(&criteria);
        assert_eq!(result[0].1, CriterionType::Semantic);
    }

    #[test]
    fn defaults_to_semantic_on_no_keyword() {
        let criteria = vec!["The module does its thing".to_owned()];
        let result = classify_criteria(&criteria);
        assert_eq!(result[0].1, CriterionType::Semantic);
    }

    #[test]
    fn mechanical_takes_precedence_over_semantic() {
        // NOTE: Contains both "test" (mechanical) and "handles" (semantic).
        let criteria = vec!["Test handles edge cases correctly".to_owned()];
        let result = classify_criteria(&criteria);
        assert_eq!(result[0].1, CriterionType::Mechanical);
    }

    #[test]
    fn classifies_multiple_criteria() {
        let criteria = vec![
            "`cargo build` succeeds".to_owned(),
            "Implements feature X".to_owned(),
            "Some random criterion".to_owned(),
        ];
        let result = classify_criteria(&criteria);

        assert_eq!(result.len(), 3);
        assert_eq!(result[0].1, CriterionType::Mechanical);
        assert_eq!(result[1].1, CriterionType::Semantic);
        assert_eq!(result[2].1, CriterionType::Semantic);
    }

    #[test]
    fn case_insensitive_matching() {
        let criteria = vec!["CARGO BUILD succeeds".to_owned()];
        let result = classify_criteria(&criteria);
        assert_eq!(result[0].1, CriterionType::Mechanical);
    }

    // -----------------------------------------------------------------------
    // build_qa_prompt
    // -----------------------------------------------------------------------

    #[test]
    fn build_qa_prompt_has_xml_structure() {
        let criteria = vec![
            ("feature works".to_owned(), CriterionType::Semantic),
            ("errors handled".to_owned(), CriterionType::Semantic),
        ];
        let diff = "+++ b/src/lib.rs\n+pub fn foo() {}\n";

        let prompt = build_qa_prompt("Test task", &criteria, diff);

        assert!(prompt.contains("<task>"));
        assert!(prompt.contains("</task>"));
        assert!(prompt.contains("<criteria>"));
        assert!(prompt.contains("</criteria>"));
        assert!(prompt.contains("<diff>"));
        assert!(prompt.contains("</diff>"));
        assert!(prompt.contains("<examples>"));
        assert!(prompt.contains("<output_format>"));
    }

    #[test]
    fn build_qa_prompt_includes_criteria_with_ids() {
        let criteria = vec![
            ("feature works".to_owned(), CriterionType::Semantic),
            ("errors handled".to_owned(), CriterionType::Semantic),
        ];

        let prompt = build_qa_prompt("Test task", &criteria, "some diff");

        assert!(prompt.contains(r#"<criterion id="C1">feature works</criterion>"#));
        assert!(prompt.contains(r#"<criterion id="C2">errors handled</criterion>"#));
    }

    #[test]
    fn build_qa_prompt_truncates_large_diffs() {
        let criteria = vec![("x".to_owned(), CriterionType::Semantic)];
        let large_diff = "x".repeat(200_000);

        let prompt = build_qa_prompt("Test task", &criteria, &large_diff);

        assert!(prompt.contains("truncated at 100K"));
        assert!(prompt.len() < 200_000);
    }

    // -----------------------------------------------------------------------
    // parse_qa_response
    // -----------------------------------------------------------------------

    #[test]
    fn parse_valid_json_response() {
        let json = r#"{
          "verdict": "green",
          "criteria": [
            {
              "criterion_id": "C1",
              "verdict": "pass",
              "confidence": 0.95,
              "evidence": "Function implemented at line 47"
            },
            {
              "criterion_id": "C2",
              "verdict": "fail",
              "confidence": 0.85,
              "evidence": "No fallback logic found"
            }
          ],
          "summary": "C2 fails."
        }"#;

        let criteria = vec![
            ("implements flush".to_owned(), CriterionType::Semantic),
            ("handles fallback".to_owned(), CriterionType::Semantic),
        ];

        let results = parse_qa_response(json, &criteria);

        assert_eq!(results.len(), 2);
        assert!(results[0].passed);
        assert!(!results[1].passed);
    }

    #[test]
    fn parse_json_in_markdown_fence() {
        let raw = "Here is my evaluation:\n\n```json\n{\n  \"verdict\": \"green\",\n  \"criteria\": [\n    {\n      \"criterion_id\": \"C1\",\n      \"verdict\": \"pass\",\n      \"confidence\": 0.90,\n      \"evidence\": \"Tests added\"\n    }\n  ],\n  \"summary\": \"All good.\"\n}\n```\n\nDone.";

        let criteria = vec![("tests added".to_owned(), CriterionType::Semantic)];
        let results = parse_qa_response(raw, &criteria);

        assert_eq!(results.len(), 1);
        assert!(results[0].passed);
    }

    #[test]
    fn parse_legacy_format_mixed() {
        let response = "\
- CRITERION: implements feature X
- VERDICT: PASS
- EVIDENCE: Done

- CRITERION: handles errors properly
- VERDICT: FAIL
- EVIDENCE: Missing error context
";

        let criteria = vec![
            ("implements feature X".to_owned(), CriterionType::Semantic),
            (
                "handles errors properly".to_owned(),
                CriterionType::Semantic,
            ),
        ];

        let results = parse_qa_response(response, &criteria);

        assert_eq!(results.len(), 2);
        assert!(results[0].passed);
        assert!(!results[1].passed);
        assert!(results[1].evidence.contains("Missing error context"));
    }

    #[test]
    fn parse_empty_response_marks_all_failed() {
        let criteria = vec![("feature A".to_owned(), CriterionType::Semantic)];
        let results = parse_qa_response("", &criteria);

        assert_eq!(results.len(), 1);
        assert!(!results[0].passed);
    }

    #[test]
    fn json_missing_criteria_filled_as_failed() {
        let json = r#"{
          "verdict": "green",
          "criteria": [
            {
              "criterion_id": "C1",
              "verdict": "pass",
              "confidence": 0.95,
              "evidence": "Done"
            }
          ],
          "summary": "Partial evaluation."
        }"#;

        let criteria = vec![
            ("feature A".to_owned(), CriterionType::Semantic),
            ("feature B".to_owned(), CriterionType::Semantic),
        ];

        let results = parse_qa_response(json, &criteria);

        assert_eq!(results.len(), 2);
        assert!(results[0].passed);
        assert!(!results[1].passed);
        assert!(results[1].evidence.contains("not evaluated"));
    }

    #[test]
    fn extract_json_block_finds_json_fence() {
        let raw = "text\n```json\n{\"a\": 1}\n```\nmore text";
        let extracted = extract_json_block(raw).unwrap();
        assert_eq!(extracted, r#"{"a": 1}"#);
    }

    #[test]
    fn extract_json_block_returns_none_when_missing() {
        assert!(extract_json_block("no fences here").is_none());
    }
}
