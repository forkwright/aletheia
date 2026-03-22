//! Multi-level parallel research: domain types, prompts, merge, and deduplication.

use std::collections::HashSet;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

/// The four research domains investigated in parallel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ResearchDomain {
    /// Technology stack analysis: dependencies, versions, compatibility.
    Stack,
    /// Feature landscape and competitive analysis.
    Features,
    /// System design patterns, crate structure, integration points.
    Architecture,
    /// Known issues, antipatterns, failure modes, gotchas.
    Pitfalls,
}

impl ResearchDomain {
    /// All four research domains.
    pub const ALL: [Self; 4] = [
        Self::Stack,
        Self::Features,
        Self::Architecture,
        Self::Pitfalls,
    ];

    /// Short identifier for serialization and logging.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stack => "stack",
            Self::Features => "features",
            Self::Architecture => "architecture",
            Self::Pitfalls => "pitfalls",
        }
    }

    /// Markdown heading for the domain section.
    #[must_use]
    pub fn heading(self) -> &'static str {
        match self {
            Self::Stack => "Stack",
            Self::Features => "Features",
            Self::Architecture => "Architecture",
            Self::Pitfalls => "Pitfalls",
        }
    }
}

impl std::fmt::Display for ResearchDomain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Outcome status of a single researcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum FindingStatus {
    /// Researcher returned full results.
    Complete,
    /// Researcher returned partial or unvalidated results.
    Partial,
    /// Researcher encountered an error.
    Failed,
    /// Researcher exceeded the timeout.
    TimedOut,
}

/// Result from a single domain researcher.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchFinding {
    /// Which domain this finding covers.
    pub domain: ResearchDomain,
    /// The researcher's output text.
    pub content: String,
    /// Whether the researcher completed successfully.
    pub status: FindingStatus,
}

/// Configurable parameters for the research phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchConfig {
    /// Timeout per researcher in seconds (default: 300).
    pub timeout_secs: u64,
    /// Which domains to investigate.
    pub domains: Vec<ResearchDomain>,
}

impl Default for ResearchConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 300,
            domains: ResearchDomain::ALL.to_vec(),
        }
    }
}

/// Merged output from all researchers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchOutput {
    /// Individual findings per domain (deduplicated).
    pub findings: Vec<ResearchFinding>,
    /// Unified structured markdown combining all domains.
    pub markdown: String,
}

/// Build a domain-specific system prompt for a researcher.
#[must_use]
pub fn domain_prompt(domain: ResearchDomain, project_goal: &str) -> String {
    let role = match domain {
        ResearchDomain::Stack => {
            "You are a technology stack researcher. Analyze the technology landscape \
             for this project: what languages, frameworks, databases, and libraries \
             are standard choices. What are the tradeoffs between the top options? \
             What does the community use in 2025/2026?"
        }
        ResearchDomain::Features => {
            "You are a features researcher. Analyze the feature landscape for this \
             domain: what capabilities are table-stakes (users expect them), what \
             are differentiators (set products apart), and what are advanced/v2 \
             features? Be specific and enumerate concrete features."
        }
        ResearchDomain::Architecture => {
            "You are an architecture researcher. Analyze architectural patterns for \
             this domain: what system designs, data models, API patterns, and \
             structural choices are standard? What are the scaling considerations \
             and common design decisions?"
        }
        ResearchDomain::Pitfalls => {
            "You are a pitfalls researcher. Identify the known failure modes, \
             anti-patterns, gotchas, and common mistakes for this domain. What do \
             developers typically get wrong? What technical debt accumulates? What \
             security/performance traps exist?"
        }
    };

    format!(
        "{role}\n\n# Project: {project_goal}\n\n\
         Provide your findings as structured markdown with clear headings and bullet points."
    )
}

/// Merge findings from parallel researchers into a unified output.
///
/// Deduplicates content across domains and formats as structured markdown.
#[must_use]
pub fn merge_research(findings: Vec<ResearchFinding>) -> ResearchOutput {
    let deduped = deduplicate_findings(findings);
    let markdown = format_markdown(&deduped);
    ResearchOutput {
        findings: deduped,
        markdown,
    }
}

/// Remove duplicate content lines that appear across multiple domain findings.
///
/// Preserves the first occurrence and strips later duplicates. Headings, list
/// markers, and blank lines are never treated as duplicates.
fn deduplicate_findings(findings: Vec<ResearchFinding>) -> Vec<ResearchFinding> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut result = Vec::with_capacity(findings.len());

    for finding in findings {
        if finding.status == FindingStatus::Failed || finding.status == FindingStatus::TimedOut {
            result.push(finding);
            continue;
        }

        let mut deduped_lines: Vec<&str> = Vec::new();
        for line in finding.content.lines() {
            let normalized = normalize_for_dedup(line);
            if normalized.is_empty() || seen.insert(normalized) {
                deduped_lines.push(line);
            }
        }

        result.push(ResearchFinding {
            domain: finding.domain,
            content: deduped_lines.join("\n"),
            status: finding.status,
        });
    }

    result
}

/// Normalize a line for dedup comparison.
///
/// Returns empty string for lines that should never be deduplicated:
/// blank lines, headings, and short list markers.
fn normalize_for_dedup(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    // WHY: headings structure the document; removing a duplicate heading breaks formatting
    if trimmed.starts_with('#') {
        return String::new();
    }
    // WHY: short list markers like "- " or "* " are structural, not content
    if (trimmed.starts_with('-') || trimmed.starts_with('*')) && trimmed.len() < 15 {
        return String::new();
    }
    trimmed.to_lowercase()
}

/// Format findings as structured markdown with sections per domain.
fn format_markdown(findings: &[ResearchFinding]) -> String {
    let mut out = String::from("# Research Summary\n");

    for finding in findings {
        let _ = write!(out, "\n## {}\n\n", finding.domain.heading());
        match finding.status {
            FindingStatus::Complete => {
                out.push_str(&finding.content);
                out.push('\n');
            }
            FindingStatus::Partial => {
                out.push_str(&finding.content);
                out.push_str("\n\n> **Note:** This section contains partial results.\n");
            }
            FindingStatus::Failed => {
                out.push_str("*Research failed for this domain.*\n");
            }
            FindingStatus::TimedOut => {
                out.push_str("*Research timed out for this domain.*\n");
            }
        }
    }

    out.truncate(out.trim_end().len());
    out
}

/// Research depth level (0-3), determining how many researchers to dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ResearchLevel {
    /// Well-understood domain, existing patterns. No research needed.
    Skip,
    /// Mostly understood with a few unknowns. Quick validation (pitfalls only).
    Quick,
    /// New domain or significant complexity. Full 4-dimension research.
    Standard,
    /// Novel architecture, high risk, or unfamiliar domain. Extended research.
    DeepDive,
}

impl ResearchLevel {
    /// Human-readable name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Skip => "Skip",
            Self::Quick => "Quick",
            Self::Standard => "Standard",
            Self::DeepDive => "Deep Dive",
        }
    }

    /// Which domains to research at this level.
    #[must_use]
    pub fn domains(self) -> Vec<ResearchDomain> {
        match self {
            Self::Skip => Vec::new(),
            Self::Quick => vec![ResearchDomain::Pitfalls],
            Self::Standard | Self::DeepDive => ResearchDomain::ALL.to_vec(),
        }
    }

    /// Whether synthesis across domains is needed.
    #[must_use]
    pub fn needs_synthesis(self) -> bool {
        matches!(self, Self::Standard | Self::DeepDive)
    }

    /// Build a [`ResearchConfig`] for this level with the given timeout.
    #[must_use]
    pub fn to_config(self, timeout_secs: u64) -> ResearchConfig {
        ResearchConfig {
            timeout_secs,
            domains: self.domains(),
        }
    }
}

impl ResearchLevel {
    /// Numeric depth value (0-3).
    #[must_use]
    pub fn depth(self) -> u8 {
        match self {
            Self::Skip => 0,
            Self::Quick => 1,
            Self::Standard => 2,
            Self::DeepDive => 3,
        }
    }
}

impl std::fmt::Display for ResearchLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "L{} ({})", self.depth(), self.name())
    }
}

/// Signals used to auto-select a research level.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "domain model: each bool is an independent complexity signal, not a state machine"
)]
pub struct ComplexitySignals {
    /// Number of requirements in the phase.
    pub requirement_count: u32,
    /// Whether requirements mention unfamiliar technologies.
    pub has_novel_technology: bool,
    /// Whether requirements mention security or auth.
    pub has_security_concerns: bool,
    /// Whether requirements mention data migration.
    pub has_data_migration: bool,
    /// Whether requirements mention external integrations.
    pub has_external_integrations: bool,
    /// Whether the phase goal mentions architectural decisions.
    pub has_architectural_decisions: bool,
    /// Whether similar code exists in the codebase.
    pub has_existing_patterns: bool,
    /// Explicit user override (bypasses auto-detection).
    pub user_override: Option<ResearchLevel>,
}

/// Select the appropriate research level from complexity signals.
#[must_use]
pub fn select_research_level(signals: &ComplexitySignals) -> ResearchLevel {
    if let Some(level) = signals.user_override {
        return level;
    }

    let mut score: i32 = 0;

    if signals.requirement_count <= 2 {
        // no change
    } else if signals.requirement_count <= 5 {
        score += 1;
    } else if signals.requirement_count <= 10 {
        score += 2;
    } else {
        score += 3;
    }

    if signals.has_novel_technology {
        score += 3;
    }
    if signals.has_security_concerns {
        score += 2;
    }
    if signals.has_data_migration {
        score += 2;
    }
    if signals.has_external_integrations {
        score += 2;
    }
    if signals.has_architectural_decisions {
        score += 2;
    }
    if signals.has_existing_patterns {
        score -= 2;
    }

    match score {
        ..=0 => ResearchLevel::Skip,
        1..=2 => ResearchLevel::Quick,
        3..=6 => ResearchLevel::Standard,
        _ => ResearchLevel::DeepDive,
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions on known-length collections"
)]
mod tests {
    use super::*;

    // -- ResearchDomain --

    #[test]
    fn domain_all_contains_four_variants() {
        assert_eq!(ResearchDomain::ALL.len(), 4);
    }

    #[test]
    fn domain_as_str_roundtrips_through_display() {
        for domain in &ResearchDomain::ALL {
            assert_eq!(domain.to_string(), domain.as_str());
        }
    }

    #[test]
    fn domain_serde_roundtrip() {
        for domain in &ResearchDomain::ALL {
            let json = serde_json::to_string(domain).unwrap();
            let back: ResearchDomain = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, domain, "roundtrip failed for {domain:?}");
        }
    }

    // -- domain_prompt --

    #[test]
    fn domain_prompt_contains_project_goal() {
        for domain in &ResearchDomain::ALL {
            let prompt = domain_prompt(*domain, "build a chat app");
            assert!(
                prompt.contains("build a chat app"),
                "prompt for {domain} missing project goal"
            );
        }
    }

    #[test]
    fn domain_prompt_stack_mentions_technology() {
        let prompt = domain_prompt(ResearchDomain::Stack, "test");
        assert!(prompt.contains("technology"));
    }

    #[test]
    fn domain_prompt_pitfalls_mentions_failure_modes() {
        let prompt = domain_prompt(ResearchDomain::Pitfalls, "test");
        assert!(prompt.contains("failure modes"));
    }

    // -- FindingStatus --

    #[test]
    fn finding_status_serde_roundtrip() {
        let statuses = [
            FindingStatus::Complete,
            FindingStatus::Partial,
            FindingStatus::Failed,
            FindingStatus::TimedOut,
        ];
        for status in &statuses {
            let json = serde_json::to_string(status).unwrap();
            let back: FindingStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, status, "roundtrip failed for {status:?}");
        }
    }

    // -- ResearchConfig --

    #[test]
    fn default_config_has_all_domains_and_5min_timeout() {
        let config = ResearchConfig::default();
        assert_eq!(config.timeout_secs, 300);
        assert_eq!(config.domains.len(), 4);
    }

    #[test]
    fn config_serde_roundtrip() {
        let config = ResearchConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let back: ResearchConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.timeout_secs, config.timeout_secs);
        assert_eq!(back.domains.len(), config.domains.len());
    }

    // -- merge_research / deduplicate --

    #[test]
    fn merge_empty_findings_produces_header_only() {
        let output = merge_research(Vec::new());
        assert!(output.findings.is_empty());
        assert!(output.markdown.contains("# Research Summary"));
    }

    #[test]
    fn merge_preserves_all_domains() {
        let findings = vec![
            ResearchFinding {
                domain: ResearchDomain::Stack,
                content: "Use Rust with Tokio for async.".into(),
                status: FindingStatus::Complete,
            },
            ResearchFinding {
                domain: ResearchDomain::Features,
                content: "Support real-time streaming.".into(),
                status: FindingStatus::Complete,
            },
        ];
        let output = merge_research(findings);
        assert_eq!(output.findings.len(), 2);
        assert!(output.markdown.contains("## Stack"));
        assert!(output.markdown.contains("## Features"));
    }

    #[test]
    fn merge_deduplicates_identical_lines_across_domains() {
        let shared_line = "Use Rust for the backend implementation.";
        let findings = vec![
            ResearchFinding {
                domain: ResearchDomain::Stack,
                content: format!("{shared_line}\nStack-specific detail."),
                status: FindingStatus::Complete,
            },
            ResearchFinding {
                domain: ResearchDomain::Architecture,
                content: format!("{shared_line}\nArchitecture-specific detail."),
                status: FindingStatus::Complete,
            },
        ];
        let output = merge_research(findings);

        let stack = &output.findings[0];
        let arch = &output.findings[1];
        assert!(
            stack.content.contains(shared_line),
            "first occurrence should be preserved"
        );
        assert!(
            !arch.content.contains(shared_line),
            "duplicate line should be removed from later finding"
        );
        assert!(arch.content.contains("Architecture-specific detail."));
    }

    #[test]
    fn merge_preserves_headings_even_if_duplicated() {
        let findings = vec![
            ResearchFinding {
                domain: ResearchDomain::Stack,
                content: "### Overview\nStack content.".into(),
                status: FindingStatus::Complete,
            },
            ResearchFinding {
                domain: ResearchDomain::Features,
                content: "### Overview\nFeature content.".into(),
                status: FindingStatus::Complete,
            },
        ];
        let output = merge_research(findings);

        assert!(output.findings[0].content.contains("### Overview"));
        assert!(output.findings[1].content.contains("### Overview"));
    }

    #[test]
    fn merge_preserves_blank_lines() {
        let findings = vec![ResearchFinding {
            domain: ResearchDomain::Stack,
            content: "Line one.\n\nLine two.".into(),
            status: FindingStatus::Complete,
        }];
        let output = merge_research(findings);
        assert!(output.findings[0].content.contains("\n\n"));
    }

    #[test]
    fn merge_handles_failed_findings() {
        let findings = vec![
            ResearchFinding {
                domain: ResearchDomain::Stack,
                content: "Good content.".into(),
                status: FindingStatus::Complete,
            },
            ResearchFinding {
                domain: ResearchDomain::Features,
                content: String::new(),
                status: FindingStatus::Failed,
            },
        ];
        let output = merge_research(findings);
        assert!(output.markdown.contains("*Research failed"));
    }

    #[test]
    fn merge_handles_timed_out_findings() {
        let findings = vec![ResearchFinding {
            domain: ResearchDomain::Pitfalls,
            content: String::new(),
            status: FindingStatus::TimedOut,
        }];
        let output = merge_research(findings);
        assert!(output.markdown.contains("*Research timed out"));
    }

    #[test]
    fn merge_partial_findings_get_note() {
        let findings = vec![ResearchFinding {
            domain: ResearchDomain::Stack,
            content: "Partial data here.".into(),
            status: FindingStatus::Partial,
        }];
        let output = merge_research(findings);
        assert!(output.markdown.contains("partial results"));
    }

    #[test]
    fn dedup_is_case_insensitive() {
        let findings = vec![
            ResearchFinding {
                domain: ResearchDomain::Stack,
                content: "Use Tokio for async runtime.".into(),
                status: FindingStatus::Complete,
            },
            ResearchFinding {
                domain: ResearchDomain::Architecture,
                content: "use tokio for async runtime.".into(),
                status: FindingStatus::Complete,
            },
        ];
        let output = merge_research(findings);
        assert!(
            output.findings[1].content.is_empty()
                || !output.findings[1]
                    .content
                    .to_lowercase()
                    .contains("use tokio for async runtime"),
            "case-insensitive duplicate should be removed"
        );
    }

    #[test]
    fn dedup_does_not_touch_failed_findings() {
        let shared = "shared content across domains";
        let findings = vec![
            ResearchFinding {
                domain: ResearchDomain::Stack,
                content: shared.into(),
                status: FindingStatus::Complete,
            },
            ResearchFinding {
                domain: ResearchDomain::Features,
                content: shared.into(),
                status: FindingStatus::Failed,
            },
        ];
        let output = merge_research(findings);
        assert_eq!(
            output.findings[1].content, shared,
            "failed findings are not deduplicated"
        );
    }

    // -- normalize_for_dedup --

    #[test]
    fn normalize_blank_line_returns_empty() {
        assert!(normalize_for_dedup("").is_empty());
        assert!(normalize_for_dedup("   ").is_empty());
    }

    #[test]
    fn normalize_heading_returns_empty() {
        assert!(normalize_for_dedup("# Heading").is_empty());
        assert!(normalize_for_dedup("## Sub").is_empty());
    }

    #[test]
    fn normalize_short_list_marker_returns_empty() {
        assert!(normalize_for_dedup("- item").is_empty());
        assert!(normalize_for_dedup("* short").is_empty());
    }

    #[test]
    fn normalize_long_list_item_returns_lowercase() {
        let result = normalize_for_dedup("- This is a long list item with real content");
        assert_eq!(result, "- this is a long list item with real content");
    }

    #[test]
    fn normalize_regular_line_returns_lowercase_trimmed() {
        assert_eq!(normalize_for_dedup("  Hello World  "), "hello world");
    }

    // -- format_markdown --

    #[test]
    fn format_markdown_no_trailing_whitespace() {
        let findings = vec![ResearchFinding {
            domain: ResearchDomain::Stack,
            content: "Content.".into(),
            status: FindingStatus::Complete,
        }];
        let md = format_markdown(&findings);
        assert!(!md.ends_with('\n'), "should not end with trailing newline");
        assert!(!md.ends_with(' '), "should not end with trailing space");
    }

    // -- ResearchLevel --

    #[test]
    fn level_skip_has_no_domains() {
        assert!(ResearchLevel::Skip.domains().is_empty());
    }

    #[test]
    fn level_quick_has_pitfalls_only() {
        let domains = ResearchLevel::Quick.domains();
        assert_eq!(domains.len(), 1);
        assert_eq!(domains[0], ResearchDomain::Pitfalls);
    }

    #[test]
    fn level_standard_has_all_domains() {
        assert_eq!(ResearchLevel::Standard.domains().len(), 4);
    }

    #[test]
    fn level_deep_dive_has_all_domains() {
        assert_eq!(ResearchLevel::DeepDive.domains().len(), 4);
    }

    #[test]
    fn level_synthesis_needed_for_standard_and_deep_dive() {
        assert!(!ResearchLevel::Skip.needs_synthesis());
        assert!(!ResearchLevel::Quick.needs_synthesis());
        assert!(ResearchLevel::Standard.needs_synthesis());
        assert!(ResearchLevel::DeepDive.needs_synthesis());
    }

    #[test]
    fn level_display_includes_number_and_name() {
        assert_eq!(ResearchLevel::Skip.to_string(), "L0 (Skip)");
        assert_eq!(ResearchLevel::Quick.to_string(), "L1 (Quick)");
        assert_eq!(ResearchLevel::Standard.to_string(), "L2 (Standard)");
        assert_eq!(ResearchLevel::DeepDive.to_string(), "L3 (Deep Dive)");
    }

    #[test]
    fn level_ordering() {
        assert!(ResearchLevel::Skip < ResearchLevel::Quick);
        assert!(ResearchLevel::Quick < ResearchLevel::Standard);
        assert!(ResearchLevel::Standard < ResearchLevel::DeepDive);
    }

    #[test]
    fn level_to_config_uses_given_timeout() {
        let config = ResearchLevel::Standard.to_config(120);
        assert_eq!(config.timeout_secs, 120);
        assert_eq!(config.domains.len(), 4);
    }

    #[test]
    fn level_serde_roundtrip() {
        let levels = [
            ResearchLevel::Skip,
            ResearchLevel::Quick,
            ResearchLevel::Standard,
            ResearchLevel::DeepDive,
        ];
        for level in &levels {
            let json = serde_json::to_string(level).unwrap();
            let back: ResearchLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, level, "roundtrip failed for {level:?}");
        }
    }

    // -- select_research_level --

    #[test]
    fn select_level_user_override_takes_priority() {
        let signals = ComplexitySignals {
            has_novel_technology: true,
            has_security_concerns: true,
            user_override: Some(ResearchLevel::Skip),
            ..Default::default()
        };
        assert_eq!(select_research_level(&signals), ResearchLevel::Skip);
    }

    #[test]
    fn select_level_simple_task_returns_skip() {
        let signals = ComplexitySignals {
            requirement_count: 1,
            ..Default::default()
        };
        assert_eq!(select_research_level(&signals), ResearchLevel::Skip);
    }

    #[test]
    fn select_level_moderate_complexity_returns_quick() {
        let signals = ComplexitySignals {
            requirement_count: 4,
            ..Default::default()
        };
        assert_eq!(select_research_level(&signals), ResearchLevel::Quick);
    }

    #[test]
    fn select_level_novel_tech_returns_standard() {
        let signals = ComplexitySignals {
            requirement_count: 3,
            has_novel_technology: true,
            ..Default::default()
        };
        assert_eq!(select_research_level(&signals), ResearchLevel::Standard);
    }

    #[test]
    fn select_level_high_complexity_returns_deep_dive() {
        let signals = ComplexitySignals {
            requirement_count: 12,
            has_novel_technology: true,
            has_security_concerns: true,
            has_architectural_decisions: true,
            ..Default::default()
        };
        assert_eq!(select_research_level(&signals), ResearchLevel::DeepDive);
    }

    #[test]
    fn select_level_existing_patterns_reduce_score() {
        let signals = ComplexitySignals {
            requirement_count: 4,
            has_existing_patterns: true,
            ..Default::default()
        };
        assert_eq!(
            select_research_level(&signals),
            ResearchLevel::Skip,
            "existing patterns should offset requirement count"
        );
    }

    #[test]
    fn select_level_security_plus_migration_returns_standard() {
        let signals = ComplexitySignals {
            requirement_count: 3,
            has_security_concerns: true,
            has_data_migration: true,
            ..Default::default()
        };
        assert_eq!(select_research_level(&signals), ResearchLevel::Standard);
    }

    #[test]
    fn select_level_external_integrations_add_complexity() {
        let signals = ComplexitySignals {
            requirement_count: 4,
            has_external_integrations: true,
            ..Default::default()
        };
        assert_eq!(select_research_level(&signals), ResearchLevel::Standard);
    }

    // -- ResearchOutput --

    #[test]
    fn research_output_serde_roundtrip() {
        let output = merge_research(vec![ResearchFinding {
            domain: ResearchDomain::Stack,
            content: "Test content.".into(),
            status: FindingStatus::Complete,
        }]);
        let json = serde_json::to_string(&output).unwrap();
        let back: ResearchOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(back.findings.len(), 1);
        assert_eq!(back.markdown, output.markdown);
    }
}
