//! Structural pattern checks: AI structural tells.

use super::{Finding, FindingKind};

/// Transition words that indicate AI-generated filler structure.
const TRANSITION_WORDS: &[&str] = &[
    "furthermore",
    "additionally",
    "moreover",
    "consequently",
    "accordingly",
    "therefore",
    "nevertheless",
    "nonetheless",
    "subsequently",
    "ultimately",
];

/// Minimum consecutive paragraphs that all begin with a transition word before flagging.
const TRANSITION_DENSITY_THRESHOLD: usize = 3;

/// Check for AI structural tells in `effective_lines`.
///
/// Returns findings for transition-word-dense runs of paragraphs.
pub(crate) fn check_structure(effective_lines: &[(usize, &str)]) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut consecutive = 0usize;
    let mut run_start = 0usize;

    for &(line_num, line) in effective_lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            // Empty line ends the current paragraph run.
            if consecutive >= TRANSITION_DENSITY_THRESHOLD {
                findings.push(Finding {
                    line_start: run_start,
                    line_end: line_num.saturating_sub(1),
                    message: format!(
                        "{consecutive} consecutive paragraphs begin with transition words — structural tell"
                    ),
                    kind: FindingKind::StructuralPattern,
                    fix: None,
                });
            }
            consecutive = 0;
            run_start = 0;
            continue;
        }

        // Check if this line starts with a transition word.
        let lower = trimmed.to_lowercase();
        let starts_with_transition = TRANSITION_WORDS
            .iter()
            .any(|tw| lower.starts_with(tw) || lower.starts_with(&format!("{tw},")));

        if starts_with_transition {
            if consecutive == 0 {
                run_start = line_num;
            }
            consecutive += 1;
        } else if trimmed.starts_with('#') {
            // WHY: section headings break structural continuity and must end a run.
            if consecutive >= TRANSITION_DENSITY_THRESHOLD {
                findings.push(Finding {
                    line_start: run_start,
                    line_end: line_num.saturating_sub(1),
                    message: format!(
                        "{consecutive} consecutive paragraphs begin with transition words — structural tell"
                    ),
                    kind: FindingKind::StructuralPattern,
                    fix: None,
                });
            }
            consecutive = 0;
            run_start = 0;
        } else if !trimmed.starts_with('|') {
            // Non-heading, non-table prose that doesn't start with a transition word resets the run.
            if consecutive >= TRANSITION_DENSITY_THRESHOLD {
                findings.push(Finding {
                    line_start: run_start,
                    line_end: line_num.saturating_sub(1),
                    message: format!(
                        "{consecutive} consecutive paragraphs begin with transition words — structural tell"
                    ),
                    kind: FindingKind::StructuralPattern,
                    fix: None,
                });
            }
            consecutive = 0;
            run_start = 0;
        }
    }

    // Flush any open run at end of file.
    if consecutive >= TRANSITION_DENSITY_THRESHOLD {
        let last_line = effective_lines.last().map_or(run_start, |(n, _)| *n);
        findings.push(Finding {
            line_start: run_start,
            line_end: last_line,
            message: format!(
                "{consecutive} consecutive paragraphs begin with transition words — structural tell"
            ),
            kind: FindingKind::StructuralPattern,
            fix: None,
        });
    }

    findings
}

/// Check that `effective_lines` contains both a lead section and a closing section.
///
/// The lead section is identified by one of: a line starting with `## Summary`,
/// `## Executive Summary`, `## Overview`, or `## Key Findings`.
/// The closing section is identified by a line starting with `## Appendix`,
/// `## References`, or `## Sources`.
pub(crate) fn check_sections(effective_lines: &[(usize, &str)]) -> Vec<Finding> {
    let lead_markers = [
        "## summary",
        "## executive summary",
        "## overview",
        "## key findings",
    ];
    let closing_markers = ["## appendix", "## references", "## sources"];

    let has_lead = effective_lines.iter().any(|(_, l)| {
        let lower = l.trim().to_lowercase();
        lead_markers.iter().any(|m| lower == *m)
    });

    let has_closing = effective_lines.iter().any(|(_, l)| {
        let lower = l.trim().to_lowercase();
        closing_markers.iter().any(|m| lower == *m)
    });

    let mut findings = Vec::new();

    if !has_lead {
        findings.push(Finding {
            line_start: 1,
            line_end: 1,
            message:
                "document is missing a lead section (Summary, Executive Summary, Overview, or Key Findings)"
                    .to_owned(),
            kind: FindingKind::RequiredSectionMissing,
            fix: None,
        });
    }

    if !has_closing {
        findings.push(Finding {
            line_start: 1,
            line_end: 1,
            message: "document is missing a closing section (Appendix, References, or Sources)"
                .to_owned(),
            kind: FindingKind::RequiredSectionMissing,
            fix: None,
        });
    }

    findings
}

/// Check that H2 headings do not exceed `max_len` characters (excluding `## ` prefix).
pub(crate) fn check_header_length(
    effective_lines: &[(usize, &str)],
    max_len: usize,
) -> Vec<Finding> {
    let mut findings = Vec::new();

    for &(line_num, line) in effective_lines {
        let trimmed = line.trim();
        let Some(heading_text) = trimmed.strip_prefix("## ") else {
            continue;
        };

        if heading_text.chars().count() > max_len {
            findings.push(Finding {
                line_start: line_num,
                line_end: line_num,
                message: format!(
                    "H2 heading at line {line_num} is {} characters (max {max_len}): {:?}",
                    heading_text.chars().count(),
                    heading_text
                ),
                kind: FindingKind::HeaderLength,
                fix: None,
            });
        }
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_density_below_threshold_no_finding() {
        let lines: Vec<(usize, &str)> = vec![
            (1, "Furthermore, the analysis shows three cases."),
            (2, "Additionally, we see two more."),
        ];
        let findings = check_structure(&lines);
        assert!(
            findings.is_empty(),
            "two consecutive transition paragraphs must not flag (threshold is 3)"
        );
    }

    #[test]
    fn transition_density_at_threshold_flagged() {
        let lines: Vec<(usize, &str)> = vec![
            (1, "Furthermore, the analysis shows three cases."),
            (2, "Additionally, we see two more."),
            (3, "Moreover, results confirm the pattern."),
        ];
        let findings = check_structure(&lines);
        assert!(
            !findings.is_empty(),
            "three consecutive transition paragraphs must be flagged"
        );
    }

    #[test]
    fn transition_density_resets_at_section_heading() {
        let lines: Vec<(usize, &str)> = vec![
            (1, "Furthermore, the analysis shows three cases."),
            (2, "## New Section"),
            (3, "Additionally, we see two more."),
            (4, "Moreover, results confirm the pattern."),
        ];
        let findings = check_structure(&lines);
        assert!(
            findings.is_empty(),
            "transition paragraphs separated by a section heading must not accumulate into one run"
        );
    }

    #[test]
    fn transition_density_resets_at_blank_line() {
        let lines: Vec<(usize, &str)> = vec![
            (1, "Furthermore, the analysis shows three cases."),
            (2, ""),
            (3, "Additionally, we see two more."),
            (4, ""),
            (5, "Moreover, results confirm the pattern."),
        ];
        let findings = check_structure(&lines);
        assert!(
            findings.is_empty(),
            "transition paragraphs separated by blank lines must not accumulate into one run"
        );
    }

    #[test]
    fn sections_both_present_passes() {
        let lines: Vec<(usize, &str)> = vec![
            (1, "## Summary"),
            (2, "The analysis shows 47 cases."),
            (3, "## Appendix"),
        ];
        let findings = check_sections(&lines);
        assert!(
            findings.is_empty(),
            "document with both lead and closing sections must pass"
        );
    }

    #[test]
    fn sections_missing_lead_flagged() {
        let lines: Vec<(usize, &str)> = vec![(1, "## Appendix")];
        let findings = check_sections(&lines);
        assert!(
            findings.iter().any(|f| f.message.contains("lead section")),
            "missing lead section must be flagged"
        );
    }

    #[test]
    fn sections_missing_closing_flagged() {
        let lines: Vec<(usize, &str)> = vec![(1, "## Summary"), (2, "Analysis complete.")];
        let findings = check_sections(&lines);
        assert!(
            findings
                .iter()
                .any(|f| f.message.contains("closing section")),
            "missing closing section must be flagged"
        );
    }

    #[test]
    fn header_length_within_limit_passes() {
        let lines: Vec<(usize, &str)> = vec![(1, "## Short Heading")];
        let findings = check_header_length(&lines, 60);
        assert!(findings.is_empty(), "short heading must pass");
    }

    #[test]
    fn header_length_over_limit_flagged() {
        let mut heading = "## ".to_owned();
        heading.push_str(&"x".repeat(70));
        let owned: Vec<(usize, String)> = vec![(1, heading)];
        let lines: Vec<(usize, &str)> = owned.iter().map(|(n, s)| (*n, s.as_str())).collect();
        let findings = check_header_length(&lines, 60);
        assert!(!findings.is_empty(), "heading over limit must be flagged");
    }
}
