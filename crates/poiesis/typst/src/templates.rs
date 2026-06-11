//! Built-in Typst templates, embedded at compile time.
//!
//! Each template is identified by a short slug string. Call
//! [`crate::render_template`] with the slug to render. Templates expect
//! their data payload to be available at the virtual path `data.json`
//! (which [`crate::render_typst`] populates automatically).

/// Slug for the default one-page report template.
pub const DEFAULT: &str = "default";

/// Source for the [`DEFAULT`] template.
///
/// Embedded at compile time from `templates/default.typ`.
pub(crate) const DEFAULT_SOURCE: &str = include_str!("../templates/default.typ");

/// Slug for the eval-report template.
pub const EVAL_REPORT: &str = "eval-report";

/// Source for the [`EVAL_REPORT`] template.
///
/// Embedded at compile time from `templates/eval-report.typ`.
pub(crate) const EVAL_REPORT_SOURCE: &str = include_str!("../templates/eval-report.typ");

/// Slug for the graph-audit template.
pub const GRAPH_AUDIT: &str = "graph-audit";

/// Source for the [`GRAPH_AUDIT`] template.
///
/// Embedded at compile time from `templates/graph-audit.typ`.
pub(crate) const GRAPH_AUDIT_SOURCE: &str = include_str!("../templates/graph-audit.typ");

/// List of all known template slugs.
pub const SLUGS: &[&str] = &[DEFAULT, EVAL_REPORT, GRAPH_AUDIT];

/// Resolve a slug to its Typst source, or `None` if unknown.
#[must_use]
pub(crate) fn lookup(slug: &str) -> Option<&'static str> {
    match slug {
        DEFAULT => Some(DEFAULT_SOURCE),
        EVAL_REPORT => Some(EVAL_REPORT_SOURCE),
        GRAPH_AUDIT => Some(GRAPH_AUDIT_SOURCE),
        _ => None,
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn default_slug_is_registered() {
        assert!(SLUGS.contains(&DEFAULT), "DEFAULT must be in SLUGS");
    }

    #[test]
    fn eval_report_slug_is_registered() {
        assert!(SLUGS.contains(&EVAL_REPORT), "EVAL_REPORT must be in SLUGS");
    }

    #[test]
    fn graph_audit_slug_is_registered() {
        assert!(SLUGS.contains(&GRAPH_AUDIT), "GRAPH_AUDIT must be in SLUGS");
    }

    #[test]
    fn lookup_default_returns_source() {
        let src = lookup(DEFAULT).expect("default must resolve");
        assert!(
            src.contains("json(\"data.json\")"),
            "default must load data"
        );
    }

    #[test]
    fn lookup_eval_report_returns_source() {
        let src = lookup(EVAL_REPORT).expect("eval-report must resolve");
        assert!(
            src.contains("json(\"data.json\")"),
            "eval-report must load data"
        );
    }

    #[test]
    fn lookup_graph_audit_returns_source() {
        let src = lookup(GRAPH_AUDIT).expect("graph-audit must resolve");
        assert!(
            src.contains("json(\"data.json\")"),
            "graph-audit must load data"
        );
    }

    #[test]
    fn lookup_unknown_returns_none() {
        assert!(lookup("no-such").is_none());
    }
}
