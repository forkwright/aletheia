//! Keyword lists for task classification and intent detection.

/// Keywords that suggest a coding or implementation task.
///
/// Sourced from `nous/src/bootstrap/mod.rs`.
pub const CODING_KEYWORDS: &[&str] = &[
    "write",
    "create",
    "generate",
    "code",
    "implement",
    "fix",
    "bug",
    "compile",
    "test",
    "refactor",
    "debug",
    "build",
    "error",
    "function",
    "struct",
    "deploy",
    "lint",
];

/// Keywords that suggest a research or investigation task.
///
/// Sourced from `nous/src/bootstrap/mod.rs`.
pub const RESEARCH_KEYWORDS: &[&str] = &[
    "what",
    "research",
    "find",
    "search",
    "investigate",
    "analyze",
    "review",
    "compare",
    "evaluate",
    "explain",
    "understand",
    "tell me",
];

/// Keywords that suggest a planning or design task.
///
/// Sourced from `nous/src/bootstrap/mod.rs`.
pub const PLANNING_KEYWORDS: &[&str] = &[
    "plan",
    "design",
    "architect",
    "strategy",
    "roadmap",
    "organize",
    "coordinate",
    "priority",
    "prioritize",
    "goal",
    "milestone",
];

/// Keywords that suggest a casual conversation rather than a task.
///
/// Sourced from `nous/src/bootstrap/mod.rs`.
pub const CONVERSATION_KEYWORDS: &[&str] = &[
    "hello",
    "hi",
    "hey",
    "thanks",
    "thank you",
    "ok",
    "okay",
    "yes",
    "no",
    "sure",
    "bye",
];

/// Keywords that map to an analysis intake request.
///
/// Sourced from `poiesis/intake/src/lib.rs`.
pub const INTAKE_ANALYSIS_KEYWORDS: &[&str] = &[
    "analyze",
    "analysis",
    "analyse",
    "investigate",
    "study",
    "evaluate",
    "assess",
    "compare",
    "review",
    "break down",
    "breakdown",
];

/// Keywords that map to a report intake request.
///
/// Sourced from `poiesis/intake/src/lib.rs`.
pub const INTAKE_REPORT_KEYWORDS: &[&str] = &[
    "report",
    "write",
    "document",
    "summary",
    "prose",
    "narrative",
    "brief",
    "whitepaper",
    "white paper",
];

/// Keywords that map to a dashboard intake request.
///
/// Sourced from `poiesis/intake/src/lib.rs`.
pub const INTAKE_DASHBOARD_KEYWORDS: &[&str] = &[
    "dashboard",
    "panel",
    "visual",
    "chart",
    "metric",
    "kpi",
    "graph",
    "tableau",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_keyword_lists_are_well_formed() {
        for list in [
            CODING_KEYWORDS,
            RESEARCH_KEYWORDS,
            PLANNING_KEYWORDS,
            CONVERSATION_KEYWORDS,
            INTAKE_ANALYSIS_KEYWORDS,
            INTAKE_REPORT_KEYWORDS,
            INTAKE_DASHBOARD_KEYWORDS,
        ] {
            assert!(!list.is_empty(), "list must be non-empty");
            let set: std::collections::HashSet<_> = list.iter().copied().collect();
            assert_eq!(set.len(), list.len(), "no duplicates");
            for entry in list {
                assert!(!entry.is_empty(), "no empty strings");
                assert_eq!(entry.trim(), *entry, "no leading/trailing whitespace");
                assert!(
                    entry
                        .chars()
                        .all(|c| !c.is_alphabetic() || c.is_lowercase()),
                    "expected lowercase: {entry}"
                );
            }
        }
    }

    #[test]
    fn intake_keywords_positive_cases() {
        assert!(
            INTAKE_ANALYSIS_KEYWORDS
                .iter()
                .any(|&k| "analyze data".contains(k))
        );
        assert!(
            INTAKE_REPORT_KEYWORDS
                .iter()
                .any(|&k| "write a report".contains(k))
        );
        assert!(
            INTAKE_DASHBOARD_KEYWORDS
                .iter()
                .any(|&k| "create dashboard".contains(k))
        );
    }

    #[test]
    fn intake_keywords_negative_cases() {
        assert!(
            !INTAKE_ANALYSIS_KEYWORDS
                .iter()
                .any(|&k| "hello world".contains(k))
        );
        assert!(
            !INTAKE_REPORT_KEYWORDS
                .iter()
                .any(|&k| "hello world".contains(k))
        );
        assert!(
            !INTAKE_DASHBOARD_KEYWORDS
                .iter()
                .any(|&k| "hello world".contains(k))
        );
    }
}
