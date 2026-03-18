//! Context bootstrap assembly.
//!
//! Reads workspace files through the taxis cascade, estimates tokens,
//! and packs sections in priority order within the token budget.

/// Tool summary generation for inclusion in the bootstrap system prompt.
pub mod tools;

use snafu::IntoError as _;
use tracing::{debug, info, warn};

use aletheia_taxis::cascade;
use aletheia_taxis::oikos::Oikos;
use aletheia_thesauros::loader::PackSection;
use aletheia_thesauros::manifest::Priority as PackPriority;

use crate::budget::{CharEstimator, TokenBudget, TokenEstimator};
use crate::error::{self, Result};

/// Priority level for bootstrap sections.
///
/// Determines inclusion order and drop/truncation behavior under budget pressure.
/// Derives `Ord` so sections sort Required-first.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum SectionPriority {
    /// Must be included. Missing = error.
    Required = 0,
    /// Should be included if present. Missing = skip silently.
    Important = 1,
    /// Can be truncated (oldest content removed first).
    Flexible = 2,
    /// Dropped first under budget pressure.
    Optional = 3,
}

/// A section of the bootstrap system prompt.
#[derive(Debug, Clone)]
pub struct BootstrapSection {
    /// Section name (e.g. "SOUL.md", "tools-summary").
    pub name: String,
    /// Priority level.
    pub priority: SectionPriority,
    /// The text content.
    pub content: String,
    /// Estimated token count.
    pub tokens: u64,
    /// Whether this section can be truncated (vs dropped entirely).
    pub truncatable: bool,
}

/// Result of bootstrap assembly.
#[derive(Debug, Clone)]
pub struct BootstrapResult {
    /// The assembled system prompt text.
    pub system_prompt: String,
    /// Section names that were included (in order).
    pub sections_included: Vec<String>,
    /// Section names that were truncated.
    pub sections_truncated: Vec<String>,
    /// Section names that were dropped entirely.
    pub sections_dropped: Vec<String>,
    /// Total estimated tokens consumed by the system prompt.
    pub total_tokens: u64,
}

/// Workspace file specification for cascade resolution.
struct WorkspaceFileSpec {
    filename: &'static str,
    priority: SectionPriority,
    truncatable: bool,
}

/// Ordered list of workspace files resolved through the oikos cascade.
///
/// Priority ordering:
/// - SOUL.md: Required (core identity)
/// - USER.md: Important (operator profile, typically in theke/)
/// - AGENTS.md: Important (operating instructions, typically in theke/)
/// - GOALS.md: Important + truncatable (active/completed/deferred goals)
/// - TOOLS.md: Important + truncatable (available commands, SSH, paths)
/// - MEMORY.md: Flexible + truncatable (operational memory, oldest entries dropped first)
/// - IDENTITY.md: Flexible (name, emoji, avatar metadata)
/// - PROSOCHE.md: Flexible (heartbeat checklist)
/// - CONTEXT.md: Flexible + truncatable (runtime config, auto-generated)
const WORKSPACE_FILES: &[WorkspaceFileSpec] = &[
    WorkspaceFileSpec {
        filename: "SOUL.md",
        priority: SectionPriority::Required,
        truncatable: false,
    },
    WorkspaceFileSpec {
        filename: "USER.md",
        priority: SectionPriority::Important,
        truncatable: false,
    },
    WorkspaceFileSpec {
        filename: "AGENTS.md",
        priority: SectionPriority::Important,
        truncatable: false,
    },
    WorkspaceFileSpec {
        filename: "GOALS.md",
        priority: SectionPriority::Important,
        truncatable: true,
    },
    WorkspaceFileSpec {
        filename: "TOOLS.md",
        priority: SectionPriority::Important,
        truncatable: true,
    },
    WorkspaceFileSpec {
        filename: "MEMORY.md",
        priority: SectionPriority::Flexible,
        truncatable: true,
    },
    WorkspaceFileSpec {
        filename: "IDENTITY.md",
        priority: SectionPriority::Flexible,
        truncatable: false,
    },
    WorkspaceFileSpec {
        filename: "PROSOCHE.md",
        priority: SectionPriority::Flexible,
        truncatable: false,
    },
    WorkspaceFileSpec {
        filename: "CONTEXT.md",
        priority: SectionPriority::Flexible,
        truncatable: true,
    },
];

/// Minimum tokens remaining before attempting truncation (below this, just drop).
const MIN_TRUNCATION_BUDGET: u64 = 200;

/// Assembles the bootstrap system prompt from oikos workspace files.
///
/// Resolves files through the three-tier cascade (`nous/{id}/` → `shared/` → `theke/`),
/// reads contents, estimates tokens, and packs sections in priority order.
pub struct BootstrapAssembler<'a, E: TokenEstimator = CharEstimator> {
    oikos: &'a Oikos,
    estimator: E,
}

impl<'a> BootstrapAssembler<'a, CharEstimator> {
    /// Create an assembler with the default character-based estimator.
    #[must_use]
    pub fn new(oikos: &'a Oikos) -> Self {
        Self {
            oikos,
            estimator: CharEstimator::default(),
        }
    }

    /// Create an assembler with a configurable characters-per-token divisor.
    ///
    /// Wires the operator-configured `chars_per_token` value from
    /// `agents.defaults.chars_per_token` into the bootstrap estimator.
    #[must_use]
    pub fn new_with_chars_per_token(oikos: &'a Oikos, chars_per_token: u64) -> Self {
        Self {
            oikos,
            estimator: CharEstimator::new(chars_per_token),
        }
    }
}

impl<'a, E: TokenEstimator> BootstrapAssembler<'a, E> {
    /// Create an assembler with a custom token estimator.
    #[must_use]
    pub fn with_estimator(oikos: &'a Oikos, estimator: E) -> Self {
        Self { oikos, estimator }
    }

    /// Assemble the bootstrap system prompt for the given nous.
    ///
    /// Resolves workspace files through the cascade, packs them in priority order
    /// within the token budget, truncates flexible sections, and drops optional
    /// sections if budget is exhausted.
    ///
    /// # Errors
    ///
    /// Returns [`error::Error::ContextAssembly`] if a Required file (SOUL.md) is
    /// missing or unreadable.
    pub async fn assemble(
        &self,
        nous_id: &str,
        budget: &mut TokenBudget,
    ) -> Result<BootstrapResult> {
        self.assemble_with_extra(nous_id, budget, Vec::new()).await
    }

    /// Assemble the bootstrap system prompt with additional sections from domain packs.
    ///
    /// Extra sections participate in the same priority sorting and token budget
    /// as workspace files. They are appended after workspace files before sorting,
    /// so sections with the same priority level interleave naturally.
    ///
    /// # Errors
    ///
    /// Returns [`error::Error::ContextAssembly`] if a Required file (SOUL.md) is
    /// missing or unreadable.
    pub async fn assemble_with_extra(
        &self,
        nous_id: &str,
        budget: &mut TokenBudget,
        extra_sections: Vec<BootstrapSection>,
    ) -> Result<BootstrapResult> {
        let mut sections = self.resolve_workspace_files(nous_id).await?;
        sections.extend(extra_sections);

        // NOTE: stable sort preserves declaration order within same priority
        sections.sort_by_key(|s| s.priority);

        let mut included: Vec<BootstrapSection> = Vec::new();
        let mut truncated_names: Vec<String> = Vec::new();
        let mut dropped_names: Vec<String> = Vec::new();

        for section in sections {
            if budget.can_fit(section.tokens) {
                budget.consume(section.tokens);
                included.push(section);
            } else if section.truncatable && budget.remaining() > MIN_TRUNCATION_BUDGET {
                let truncated = self.truncate_section(&section, budget.remaining());
                budget.consume(truncated.tokens);
                warn!(
                    section = section.name,
                    original_tokens = section.tokens,
                    truncated_tokens = truncated.tokens,
                    "section truncated for token budget"
                );
                truncated_names.push(truncated.name.clone());
                included.push(truncated);
            } else {
                warn!(
                    section = section.name,
                    tokens = section.tokens,
                    remaining = budget.remaining(),
                    "section dropped — budget exhausted"
                );
                dropped_names.push(section.name);
            }
        }

        let system_prompt = included
            .iter()
            .map(|s| format!("## {}\n\n{}", s.name, s.content))
            .fold(String::new(), |mut acc, s| {
                if !acc.is_empty() {
                    acc.push_str("\n\n---\n\n");
                }
                acc.push_str(&s);
                acc
            });

        let section_names: Vec<String> = included.iter().map(|s| s.name.clone()).collect();
        let total_tokens = budget.consumed();

        info!(
            nous_id,
            sections = section_names.len(),
            total_tokens,
            truncated = truncated_names.len(),
            dropped = dropped_names.len(),
            "bootstrap assembled"
        );

        Ok(BootstrapResult {
            system_prompt,
            sections_included: section_names,
            sections_truncated: truncated_names,
            sections_dropped: dropped_names,
            total_tokens,
        })
    }

    /// Estimate tokens for a piece of text using this assembler's estimator.
    ///
    /// Useful for callers building [`BootstrapSection`] values externally
    /// (e.g. from domain pack content).
    pub fn estimate_tokens(&self, text: &str) -> u64 {
        self.estimator.estimate(text)
    }

    /// Resolve workspace files through the cascade and read their contents.
    async fn resolve_workspace_files(&self, nous_id: &str) -> Result<Vec<BootstrapSection>> {
        let mut sections = Vec::new();

        for spec in WORKSPACE_FILES {
            let Some(p) = cascade::resolve(self.oikos, nous_id, spec.filename, None) else {
                if spec.priority == SectionPriority::Required {
                    return Err(error::ContextAssemblySnafu {
                        message: format!("required file {} not found in cascade", spec.filename),
                    }
                    .build());
                }
                debug!(file = spec.filename, "not found in cascade, skipping");
                continue;
            };

            match tokio::fs::read_to_string(&p).await {
                Ok(content) => {
                    let content = content.trim().to_owned();
                    if content.is_empty() {
                        debug!(file = spec.filename, "skipping empty file");
                        continue;
                    }
                    let tokens = self.estimator.estimate(&content);
                    sections.push(BootstrapSection {
                        name: spec.filename.to_owned(),
                        priority: spec.priority,
                        content,
                        tokens,
                        truncatable: spec.truncatable,
                    });
                }
                Err(e) => {
                    if spec.priority == SectionPriority::Required {
                        return Err(error::ContextAssemblyIoSnafu {
                            file: spec.filename.to_owned(),
                        }
                        .into_error(e));
                    }
                    warn!(
                        file = spec.filename,
                        path = %p.display(),
                        error = %e,
                        "failed to read workspace file, skipping"
                    );
                }
            }
        }

        Ok(sections)
    }

    /// Truncate a section to fit within the given token limit.
    ///
    /// Strategy: split on `## ` markdown headers and keep the **newest** (last)
    /// subsections that fit within the budget, dropping the oldest. A truncation
    /// marker is prepended so the reader knows content was removed. Falls back to
    /// line-by-line truncation if no headers are found or no single section fits.
    fn truncate_section(&self, section: &BootstrapSection, max_tokens: u64) -> BootstrapSection {
        let parts: Vec<&str> = section.content.split("\n## ").collect();

        if parts.len() <= 1 {
            return self.truncate_by_lines(section, max_tokens);
        }

        let formatted: Vec<String> = parts
            .iter()
            .enumerate()
            .map(|(i, part)| {
                if i == 0 {
                    (*part).to_owned()
                } else {
                    format!("\n## {part}")
                }
            })
            .collect();

        let mut tokens_used: u64 = 0;
        let mut kept: Vec<usize> = Vec::new();

        for i in (0..formatted.len()).rev() {
            let part_tokens = self.estimator.estimate(&formatted[i]);
            if tokens_used + part_tokens > max_tokens {
                break;
            }
            tokens_used += part_tokens;
            kept.push(i);
        }

        if kept.is_empty() {
            // NOTE: no single section fits in budget: fall back to line-by-line truncation
            return self.truncate_by_lines(section, max_tokens);
        }

        // NOTE: reverse restores chronological order: kept is newest-first from the backwards iteration
        kept.reverse();

        // WHY: prepend marker so callers can see that oldest sections were dropped
        let mut result = String::from("... [truncated for token budget] ...");
        for i in kept {
            result.push_str(&formatted[i]);
        }

        let final_tokens = self.estimator.estimate(&result);
        BootstrapSection {
            name: section.name.clone(),
            priority: section.priority,
            content: result,
            tokens: final_tokens,
            truncatable: section.truncatable,
        }
    }

    /// Line-by-line truncation fallback.
    ///
    /// Keeps the **newest** (last) lines that fit within the budget, dropping
    /// the oldest. A truncation marker is prepended to indicate removed content.
    fn truncate_by_lines(&self, section: &BootstrapSection, max_tokens: u64) -> BootstrapSection {
        let lines: Vec<&str> = section.content.lines().collect();

        let mut tokens_used: u64 = 0;
        let mut kept: Vec<&str> = Vec::new();

        for line in lines.iter().rev() {
            let line_with_newline = format!("{line}\n");
            let line_tokens = self.estimator.estimate(&line_with_newline);

            if tokens_used + line_tokens > max_tokens {
                break;
            }
            tokens_used += line_tokens;
            kept.push(line);
        }

        if kept.is_empty() {
            // NOTE: even one line exceeds budget: return only the truncation marker
            let content = "... [truncated for token budget] ...".to_owned();
            let final_tokens = self.estimator.estimate(&content);
            return BootstrapSection {
                name: section.name.clone(),
                priority: section.priority,
                content,
                tokens: final_tokens,
                truncatable: section.truncatable,
            };
        }

        // NOTE: reverse restores chronological order: kept is newest-first from the backwards iteration
        kept.reverse();

        // WHY: prepend marker so callers can see that oldest lines were dropped
        let mut result = String::from("... [truncated for token budget] ...\n");
        for line in kept {
            result.push_str(line);
            result.push('\n');
        }

        let final_tokens = self.estimator.estimate(&result);
        BootstrapSection {
            name: section.name.clone(),
            priority: section.priority,
            content: result,
            tokens: final_tokens,
            truncatable: section.truncatable,
        }
    }
}

/// Convert domain pack sections into bootstrap sections.
///
/// Maps thesauros [`PackSection`] values to [`BootstrapSection`] values,
/// computing token estimates for each section's content. Section names
/// are prefixed with the pack name for traceability.
pub fn pack_sections_to_bootstrap<E: TokenEstimator>(
    sections: &[&PackSection],
    estimator: &E,
) -> Vec<BootstrapSection> {
    sections
        .iter()
        .map(|s| {
            let priority = match s.priority {
                PackPriority::Required => SectionPriority::Required,
                PackPriority::Important => SectionPriority::Important,
                PackPriority::Flexible => SectionPriority::Flexible,
                // WHY: non_exhaustive fallback -- unknown priorities treated as optional
                PackPriority::Optional | _ => SectionPriority::Optional,
            };
            BootstrapSection {
                name: format!("[{}] {}", s.pack_name, s.name),
                priority,
                content: s.content.clone(),
                tokens: estimator.estimate(&s.content),
                truncatable: s.truncatable,
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "bootstrap_tests.rs"]
mod tests;
