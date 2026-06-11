//! Citation presence checker.

// WHY: any table-like construct without a nearby source marker suggests
// the data was asserted without a traceable reference.

use super::{Finding, FindingKind};

/// Markers that indicate a table or data display in the document.
const TABLE_MARKERS: &[&str] = &["#summus-table(", "| --- |", "| :--- |", "| ---: |", "|---|"];

/// Markers that indicate a citation or source declaration.
const CITATION_MARKERS: &[&str] = &["#source[", "Source:", "#bibliography(", "cite(", "[^"];

/// Check `all_lines` for tables that lack a nearby citation.
///
/// `window` is the number of lines before and after a table marker to
/// search for a citation marker. Using all lines (including comments) is
/// intentional: citations in adjacent comment blocks count.
pub(crate) fn check(all_lines: &[(usize, &str)], window: usize) -> Vec<Finding> {
    let mut findings = Vec::new();
    let n = all_lines.len();

    for (idx, &(line_num, line)) in all_lines.iter().enumerate() {
        if !is_table_marker(line) {
            continue;
        }

        // Search within `window` lines before and after this table marker.
        let lo = idx.saturating_sub(window);
        let hi = (idx + window + 1).min(n);

        let has_citation = all_lines
            .get(lo..hi)
            .is_some_and(|slice| slice.iter().any(|(_, l)| is_citation_marker(l)));

        if !has_citation {
            findings.push(Finding {
                line_start: line_num,
                line_end: line_num,
                message: format!("table at line {line_num} has no citation within {window} lines"),
                kind: FindingKind::MissingCitation,
                fix: None,
            });
        }
    }

    findings
}

fn is_table_marker(line: &str) -> bool {
    TABLE_MARKERS.iter().any(|m| line.contains(m))
}

fn is_citation_marker(line: &str) -> bool {
    CITATION_MARKERS.iter().any(|m| line.contains(m))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_without_citation_flagged() {
        let lines: Vec<(usize, &str)> = vec![
            (1, "#summus-table((1fr,), (\"Col\",), (\"Val\",))"),
            (2, "Some prose."),
            (3, "More prose."),
        ];
        let findings = check(&lines, 10);
        assert!(
            !findings.is_empty(),
            "table without citation must produce a finding"
        );
    }

    #[test]
    fn table_with_citation_passes() {
        let lines: Vec<(usize, &str)> = vec![
            (1, "#summus-table((1fr,), (\"Col\",), (\"Val\",))"),
            (2, "#source[Internal database query, 2026-04-01]"),
        ];
        let findings = check(&lines, 10);
        assert!(
            findings.is_empty(),
            "table with nearby citation must not be flagged"
        );
    }

    #[test]
    fn markdown_table_without_citation_flagged() {
        let lines: Vec<(usize, &str)> = vec![
            (1, "| Name | Value |"),
            (2, "| --- |"),
            (3, "| alpha | 1 |"),
        ];
        let findings = check(&lines, 10);
        assert!(
            !findings.is_empty(),
            "markdown table separator without citation must be flagged"
        );
    }
}
