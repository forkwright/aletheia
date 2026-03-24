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
mod tests;
