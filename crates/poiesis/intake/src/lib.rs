#![deny(missing_docs)]
//! Parse Slack-style request text into a structured intake request.
//!
//! Keyword-based classification (no LLM call) for v1.  Reuses keyword patterns
//! from [`aletheia_lexica`] where applicable.

use aletheia_lexica::keywords::{
    INTAKE_ANALYSIS_KEYWORDS as ANALYSIS_KEYWORDS, INTAKE_DASHBOARD_KEYWORDS as DASHBOARD_KEYWORDS,
    INTAKE_REPORT_KEYWORDS as REPORT_KEYWORDS,
};
use snafu::Snafu;

/// Classification of an intake request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RequestKind {
    /// Research or analytical task.
    Analysis,
    /// Written report or narrative document.
    Report,
    /// Dashboard or visual panel.
    Dashboard,
    /// Could not be classified.
    Unclassified,
}

/// A parsed intake request.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct IntakeRequest {
    /// Classified kind of the request.
    pub kind: RequestKind,
    /// URL-safe slug derived from the description.
    pub slug: String,
    /// Normalised description text.
    pub description: String,
    /// Extracted requirement bullets (empty if none found).
    pub requirements: Vec<String>,
}

/// Errors from intake parsing.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum Error {
    /// The intake text could not be parsed.
    #[snafu(display("intake parse failed: {message}"))]
    ParseIntake {
        /// Human-readable reason.
        message: String,
    },
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, Error>;

// ── Classification ────────────────────────────────────────────────────────────

/// Parse free-form intake text into a structured [`IntakeRequest`].
///
/// Classification is keyword-based and case-insensitive.  The first matching
/// category wins in the order: Analysis, Report, Dashboard.  If no keyword
/// matches the request is [`RequestKind::Unclassified`].
///
/// # Errors
///
/// Returns [`Error::ParseIntake`] when the input is empty or cannot be
/// normalised.
pub fn parse_intake(text: &str) -> Result<IntakeRequest> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(Error::ParseIntake {
            message: "intake text is empty".to_owned(),
        });
    }

    let normalised = trimmed.to_lowercase();
    let kind = classify(&normalised);
    let description = trimmed.to_owned();
    let slug = slugify(&description);
    let requirements = extract_requirements(&description);

    Ok(IntakeRequest {
        kind,
        slug,
        description,
        requirements,
    })
}

fn classify(normalised: &str) -> RequestKind {
    if contains_any(normalised, ANALYSIS_KEYWORDS) {
        return RequestKind::Analysis;
    }
    if contains_any(normalised, REPORT_KEYWORDS) {
        return RequestKind::Report;
    }
    if contains_any(normalised, DASHBOARD_KEYWORDS) {
        return RequestKind::Dashboard;
    }
    RequestKind::Unclassified
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|&n| haystack.contains(n))
}

/// Generate a URL-safe slug from the first few words of a description.
fn slugify(description: &str) -> String {
    let words: Vec<&str> = description.split_whitespace().take(8).collect();
    let raw = words.join(" ");
    raw.to_lowercase()
        .replace(|c: char| !c.is_alphanumeric() && c != ' ', "-")
        .replace(' ', "-")
        .trim_matches('-')
        .to_string()
}

/// Extract bullet-looking requirements from the description.
fn extract_requirements(description: &str) -> Vec<String> {
    description
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with('-') || trimmed.starts_with('*') {
                let without_bullet = trimmed
                    .trim_start_matches('-')
                    .trim_start_matches('*')
                    .trim();
                if without_bullet.is_empty() {
                    None
                } else {
                    Some(without_bullet.to_owned())
                }
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn parse_intake_classifies_analysis() {
        let req = parse_intake("analyze the Q3 revenue trends").expect("parse");
        assert_eq!(req.kind, RequestKind::Analysis);
        assert!(!req.slug.is_empty());
        assert_eq!(req.description, "analyze the Q3 revenue trends");
    }

    #[test]
    fn parse_intake_classifies_report() {
        let req = parse_intake("write a report on customer churn").expect("parse");
        assert_eq!(req.kind, RequestKind::Report);
        assert!(!req.slug.is_empty());
        assert_eq!(req.description, "write a report on customer churn");
    }

    #[test]
    fn parse_intake_classifies_dashboard() {
        let req = parse_intake("dashboard for server metrics").expect("parse");
        assert_eq!(req.kind, RequestKind::Dashboard);
        assert!(!req.slug.is_empty());
        assert_eq!(req.description, "dashboard for server metrics");
    }

    #[test]
    fn parse_intake_falls_back_to_unclassified() {
        let req = parse_intake("hello world").expect("parse");
        assert_eq!(req.kind, RequestKind::Unclassified);
    }

    #[test]
    fn parse_intake_extracts_requirements() {
        let text = "analyze the data\n- must include charts\n- compare with last year";
        let req = parse_intake(text).expect("parse");
        assert_eq!(req.requirements.len(), 2);
        assert_eq!(
            req.requirements.first().expect("first requirement"),
            "must include charts"
        );
        assert_eq!(
            req.requirements.get(1).expect("second requirement"),
            "compare with last year"
        );
    }

    #[test]
    fn parse_intake_empty_input_errors() {
        let err = parse_intake("   ").expect_err("should fail");
        match err {
            Error::ParseIntake { message } => {
                assert!(message.contains("empty"));
            }
        }
    }
}
