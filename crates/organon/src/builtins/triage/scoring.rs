//! Relevance and priority scoring for GitHub issues.
//!
//! Produces a 0.0--1.0 relevance score with traceable rationale,
//! and a combined priority score for ranking.

use super::{GitHubIssue, RelevanceResult};

/// Score an issue's relevance to the agent's context (0.0--1.0).
///
/// Returns `(score, rationale)` where rationale explains the scoring.
///
/// Scoring factors:
/// - Label matching against known actionable labels
/// - Keyword overlap with agent context
/// - Issue completeness (has body, has labels)
/// - Priority label presence
pub(crate) fn score_relevance(issue: &GitHubIssue, context_keywords: &[&str]) -> (f64, String) {
    let mut score = 0.0_f64;
    let mut reasons: Vec<String> = Vec::new();

    // Factor 1: Actionable labels (max 0.3)
    let actionable_labels = [
        "bug",
        "enhancement",
        "feature",
        "refactor",
        "performance",
        "security",
        "tech-debt",
        "good first issue",
    ];
    let label_lower: Vec<String> = issue.labels.iter().map(|l| l.to_lowercase()).collect();
    let label_matches: Vec<&str> = actionable_labels
        .iter()
        .filter(|al| label_lower.iter().any(|l| l.contains(**al)))
        .copied()
        .collect();

    if !label_matches.is_empty() {
        #[expect(
            clippy::as_conversions,
            clippy::cast_precision_loss,
            reason = "label count is small enough that usize->f64 is exact"
        )]
        let count = label_matches.len() as f64; // kanon:ignore RUST/as-cast
        let label_score = 0.1 + (0.2 * f64::min(count / 2.0, 1.0));
        score += label_score;
        reasons.push(format!(
            "actionable labels [{}]: +{label_score:.2}",
            label_matches.join(", ")
        ));
    }

    // Factor 2: Keyword overlap (max 0.35)
    if !context_keywords.is_empty() {
        let title_lower = issue.title.to_lowercase();
        let body_lower = issue.body.to_lowercase();
        let matched: Vec<&&str> = context_keywords
            .iter()
            .filter(|kw| {
                let kw_lower = kw.to_lowercase();
                title_lower.contains(&kw_lower) || body_lower.contains(&kw_lower)
            })
            .collect();

        if !matched.is_empty() {
            #[expect(
                clippy::as_conversions,
                clippy::cast_precision_loss,
                reason = "keyword counts are small enough that usize->f64 is exact"
            )]
            let ratio = matched.len() as f64 / context_keywords.len() as f64; // kanon:ignore RUST/as-cast
            let kw_score = 0.35 * ratio;
            score += kw_score;
            reasons.push(format!(
                "keyword matches [{}/{}]: +{kw_score:.2}",
                matched.len(),
                context_keywords.len()
            ));
        }
    }

    // Factor 3: Issue completeness (max 0.15)
    let mut completeness = 0.0;
    if !issue.body.is_empty() {
        completeness += 0.08;
    }
    if !issue.labels.is_empty() {
        completeness += 0.04;
    }
    if issue.milestone.is_some() {
        completeness += 0.03;
    }
    if completeness > 0.0 {
        score += completeness;
        reasons.push(format!("completeness: +{completeness:.2}"));
    }

    // Factor 4: Priority label (max 0.2)
    if let Some(ref priority) = issue.priority_label {
        let priority_lower = priority.to_lowercase();
        let priority_score = if priority_lower.contains("critical") || priority_lower.contains("p0")
        {
            0.2
        } else if priority_lower.contains("high") || priority_lower.contains("p1") {
            0.15
        } else if priority_lower.contains("medium") || priority_lower.contains("p2") {
            0.1
        } else {
            0.05
        };
        score += priority_score;
        reasons.push(format!("priority {priority}: +{priority_score:.2}"));
    }

    // Clamp to [0.0, 1.0]
    score = score.clamp(0.0, 1.0);

    let rationale = if reasons.is_empty() {
        "no matching signals found".to_owned()
    } else {
        reasons.join("; ")
    };

    (score, rationale)
}

/// Compute a combined priority score for ranking staged prompts.
///
/// Formula: `relevance * priority_weight * impact_estimate`
pub(crate) fn compute_priority_score(result: &RelevanceResult) -> f64 {
    let priority_weight = match result.issue.priority_label.as_deref() {
        Some(p) if p.to_lowercase().contains("critical") || p.to_lowercase().contains("p0") => 2.0,
        Some(p) if p.to_lowercase().contains("high") || p.to_lowercase().contains("p1") => 1.5,
        Some(p) if p.to_lowercase().contains("medium") || p.to_lowercase().contains("p2") => 1.0,
        Some(_) => 0.8,
        None => 0.7,
    };

    // Estimate impact from label signals
    let impact = estimate_impact(&result.issue);

    result.relevance * priority_weight * impact
}

/// Estimate the impact factor of an issue (0.5--2.0).
fn estimate_impact(issue: &GitHubIssue) -> f64 {
    let label_lower: Vec<String> = issue.labels.iter().map(|l| l.to_lowercase()).collect();

    let mut impact: f64 = 1.0;

    // Security issues have higher impact
    if label_lower.iter().any(|l| l.contains("security")) {
        impact *= 1.5;
    }

    // Bugs have moderate impact boost
    if label_lower.iter().any(|l| l.contains("bug")) {
        impact *= 1.2;
    }

    // Performance issues
    if label_lower
        .iter()
        .any(|l| l.contains("performance") || l.contains("perf"))
    {
        impact *= 1.1;
    }

    impact.clamp(0.5, 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_issue(title: &str, labels: &[&str], priority: Option<&str>) -> GitHubIssue {
        GitHubIssue {
            number: 1,
            title: title.to_owned(),
            body: "Some issue body with details".to_owned(),
            labels: labels.iter().map(|s| (*s).to_owned()).collect(),
            milestone: None,
            author: "alice".to_owned(),
            created_at: "2026-01-01T00:00:00Z".to_owned(),
            priority_label: priority.map(str::to_owned),
        }
    }

    #[test]
    fn relevance_score_within_range() {
        let issue = make_issue("Fix memory leak", &["bug"], Some("priority/high"));
        let (score, rationale) = score_relevance(&issue, &["memory", "performance"]);
        assert!(score >= 0.0, "score must be non-negative: {score}");
        assert!(score <= 1.0, "score must not exceed 1.0: {score}");
        assert!(!rationale.is_empty(), "rationale must not be empty");
    }

    #[test]
    fn higher_relevance_for_matching_keywords() {
        let issue = make_issue("Fix memory leak in cache", &["bug"], None);
        let (score_match, _) = score_relevance(&issue, &["memory", "cache"]);
        let (score_no_match, _) = score_relevance(&issue, &["authentication", "oauth"]);
        assert!(
            score_match > score_no_match,
            "matching keywords should score higher: {score_match} vs {score_no_match}"
        );
    }

    #[test]
    fn actionable_labels_boost_score() {
        let issue_with = make_issue("Something", &["bug", "security"], None);
        let issue_without = make_issue("Something", &["question"], None);
        let (score_with, _) = score_relevance(&issue_with, &[]);
        let (score_without, _) = score_relevance(&issue_without, &[]);
        assert!(
            score_with > score_without,
            "actionable labels should boost: {score_with} vs {score_without}"
        );
    }

    #[test]
    fn priority_label_boosts_score() {
        let issue_high = make_issue("Task", &["enhancement"], Some("priority/high"));
        let issue_none = make_issue("Task", &["enhancement"], None);
        let (score_high, _) = score_relevance(&issue_high, &[]);
        let (score_none, _) = score_relevance(&issue_none, &[]);
        assert!(
            score_high > score_none,
            "priority should boost: {score_high} vs {score_none}"
        );
    }

    #[test]
    fn no_signals_produces_low_score() {
        let issue = GitHubIssue {
            number: 1,
            title: "vague".to_owned(),
            body: String::new(),
            labels: vec![],
            milestone: None,
            author: "bob".to_owned(),
            created_at: "2026-01-01T00:00:00Z".to_owned(),
            priority_label: None,
        };
        let (score, rationale) = score_relevance(&issue, &[]);
        assert!(score < 0.1, "empty issue should score very low: {score}");
        assert!(
            rationale.contains("no matching signals"),
            "rationale should indicate no signals"
        );
    }

    #[test]
    fn priority_score_respects_weight() {
        let high = RelevanceResult {
            issue: make_issue("Fix", &["bug"], Some("priority/critical")),
            relevance: 0.8,
            rationale: String::new(),
        };
        let low = RelevanceResult {
            issue: make_issue("Fix", &["bug"], None),
            relevance: 0.8,
            rationale: String::new(),
        };
        assert!(
            compute_priority_score(&high) > compute_priority_score(&low),
            "critical priority should rank higher"
        );
    }

    #[test]
    fn security_impact_multiplier() {
        let security = RelevanceResult {
            issue: make_issue("XSS vuln", &["security", "bug"], Some("priority/high")),
            relevance: 0.7,
            rationale: String::new(),
        };
        let normal = RelevanceResult {
            issue: make_issue("Add feature", &["enhancement"], Some("priority/high")),
            relevance: 0.7,
            rationale: String::new(),
        };
        assert!(
            compute_priority_score(&security) > compute_priority_score(&normal),
            "security issues should score higher due to impact"
        );
    }
}
