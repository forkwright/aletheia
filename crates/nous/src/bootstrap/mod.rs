//! Context bootstrap assembly.
//!
//! Reads workspace files through the taxis cascade, estimates tokens,
//! and packs sections in priority order within the token budget.
//!
//! Workspace files are split into two load tiers (see issue #2041):
//!
//! - **Always-loaded (identity):** SOUL, USER, IDENTITY, PROSOCHE — define
//!   who the agent is and load unconditionally.
//! - **Conditionally-loaded (operational):** AGENTS, GOALS, TOOLS, CHECKLIST,
//!   MEMORY, CONTEXT — loaded only when relevant to the [`TaskHint`].

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

/// Task hint for conditional workspace file loading.
///
/// Classifies the kind of work a turn involves so the bootstrap assembler
/// loads only workspace files relevant to that work. Identity-tier files
/// (SOUL, USER, IDENTITY, PROSOCHE) load regardless of the hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum TaskHint {
    /// Load all workspace files. Default for backward compatibility.
    #[default]
    General,
    /// Coding task: loads TOOLS, CHECKLIST, MEMORY.
    Coding,
    /// Research or information gathering: loads GOALS, CONTEXT, MEMORY.
    Research,
    /// Planning or architecture: loads GOALS, AGENTS, CONTEXT.
    Planning,
    /// Quick question or casual conversation: identity files only.
    Conversation,
}

/// Load tier: whether a workspace file loads unconditionally or based on task hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoadTier {
    /// Identity files: always loaded regardless of task hint.
    Always,
    /// Operational files: loaded only when relevant to the current [`TaskHint`].
    Conditional,
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
    /// Workspace file names filtered out by the task hint (never loaded).
    pub sections_filtered: Vec<String>,
    /// Total estimated tokens consumed by the system prompt.
    pub total_tokens: u64,
    /// The task hint used for conditional loading.
    pub task_hint: TaskHint,
}

/// Workspace file specification for cascade resolution.
struct WorkspaceFileSpec {
    filename: &'static str,
    priority: SectionPriority,
    truncatable: bool,
    /// Whether this file loads unconditionally or based on task hint.
    load_tier: LoadTier,
}

/// Ordered list of workspace files resolved through the oikos cascade.
///
/// Split into two tiers (issue #2041):
///
/// **Always-loaded (identity tier):**
/// - SOUL.md: Required (core identity)
/// - USER.md: Important (operator profile, typically in theke/)
/// - IDENTITY.md: Flexible (name, emoji, avatar metadata)
/// - PROSOCHE.md: Flexible (heartbeat checklist)
///
/// **Conditionally-loaded (operational tier):**
/// - AGENTS.md: Important (team topology) — Planning
/// - GOALS.md: Important + truncatable (active/completed/deferred goals) — Research, Planning
/// - TOOLS.md: Important + truncatable (available commands, SSH, paths) — Coding
/// - CHECKLIST.md: Flexible + truncatable (work procedures) — Coding
/// - MEMORY.md: Flexible + truncatable (operational memory) — Coding, Research
/// - CONTEXT.md: Flexible + truncatable (runtime config, auto-generated) — Research, Planning
const WORKSPACE_FILES: &[WorkspaceFileSpec] = &[
    // --- Always-loaded (identity tier) ---
    WorkspaceFileSpec {
        filename: "SOUL.md",
        priority: SectionPriority::Required,
        truncatable: false,
        load_tier: LoadTier::Always,
    },
    WorkspaceFileSpec {
        filename: "USER.md",
        priority: SectionPriority::Important,
        truncatable: false,
        load_tier: LoadTier::Always,
    },
    // --- Conditionally-loaded (operational tier) ---
    WorkspaceFileSpec {
        filename: "AGENTS.md",
        priority: SectionPriority::Important,
        truncatable: false,
        load_tier: LoadTier::Conditional,
    },
    WorkspaceFileSpec {
        filename: "GOALS.md",
        priority: SectionPriority::Important,
        truncatable: true,
        load_tier: LoadTier::Conditional,
    },
    WorkspaceFileSpec {
        filename: "TOOLS.md",
        priority: SectionPriority::Important,
        truncatable: true,
        load_tier: LoadTier::Conditional,
    },
    WorkspaceFileSpec {
        filename: "CHECKLIST.md",
        priority: SectionPriority::Flexible,
        truncatable: true,
        load_tier: LoadTier::Conditional,
    },
    WorkspaceFileSpec {
        filename: "MEMORY.md",
        priority: SectionPriority::Flexible,
        truncatable: true,
        load_tier: LoadTier::Conditional,
    },
    // --- Always-loaded (identity tier, continued) ---
    WorkspaceFileSpec {
        filename: "IDENTITY.md",
        priority: SectionPriority::Flexible,
        truncatable: false,
        load_tier: LoadTier::Always,
    },
    WorkspaceFileSpec {
        filename: "PROSOCHE.md",
        priority: SectionPriority::Flexible,
        truncatable: false,
        load_tier: LoadTier::Always,
    },
    // --- Conditionally-loaded (operational tier, continued) ---
    WorkspaceFileSpec {
        filename: "CONTEXT.md",
        priority: SectionPriority::Flexible,
        truncatable: true,
        load_tier: LoadTier::Conditional,
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
    /// Loads all workspace files (identity + operational). Use
    /// [`assemble_conditional`](Self::assemble_conditional) for task-aware loading.
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
    /// Uses [`TaskHint::General`] which loads all workspace files, preserving
    /// backward-compatible behavior. Use [`assemble_conditional`](Self::assemble_conditional)
    /// for task-aware loading.
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
        // WHY: General hint loads all files for backward compatibility
        self.assemble_conditional(nous_id, budget, extra_sections, TaskHint::General)
            .await
    }

    /// Assemble the bootstrap system prompt with conditional file loading.
    ///
    /// Only workspace files relevant to the given [`TaskHint`] are loaded.
    /// Identity-tier files (SOUL, USER, IDENTITY, PROSOCHE) always load.
    /// Operational files (AGENTS, GOALS, TOOLS, CHECKLIST, MEMORY, CONTEXT)
    /// load only when relevant to the task hint.
    ///
    /// # Errors
    ///
    /// Returns [`error::Error::ContextAssembly`] if a Required file (SOUL.md) is
    /// missing or unreadable.
    pub async fn assemble_conditional(
        &self,
        nous_id: &str,
        budget: &mut TokenBudget,
        extra_sections: Vec<BootstrapSection>,
        hint: TaskHint,
    ) -> Result<BootstrapResult> {
        let (mut sections, filtered_names) = self.resolve_workspace_files(nous_id, hint).await?;
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
            ?hint,
            sections = section_names.len(),
            total_tokens,
            truncated = truncated_names.len(),
            dropped = dropped_names.len(),
            filtered = filtered_names.len(),
            "bootstrap assembled"
        );

        Ok(BootstrapResult {
            system_prompt,
            sections_included: section_names,
            sections_truncated: truncated_names,
            sections_dropped: dropped_names,
            sections_filtered: filtered_names,
            total_tokens,
            task_hint: hint,
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
    ///
    /// Returns `(sections, filtered_names)` where `filtered_names` lists
    /// workspace files that were skipped by the task hint filter.
    async fn resolve_workspace_files(
        &self,
        nous_id: &str,
        hint: TaskHint,
    ) -> Result<(Vec<BootstrapSection>, Vec<String>)> {
        let mut sections = Vec::new();
        let mut filtered = Vec::new();

        for spec in WORKSPACE_FILES {
            // NOTE: always-tier files load unconditionally; conditional files check relevance
            if spec.load_tier == LoadTier::Conditional && !is_file_relevant(spec.filename, hint) {
                debug!(file = spec.filename, ?hint, "skipped by task hint filter");
                filtered.push(spec.filename.to_owned());
                continue;
            }

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

        Ok((sections, filtered))
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

        #[expect(
            clippy::indexing_slicing,
            reason = "i comes from 0..formatted.len(), so index is always in bounds"
        )]
        for i in (0..formatted.len()).rev() {
            let part_tokens = self.estimator.estimate(&formatted[i]); // kanon:ignore RUST/indexing-slicing
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
        #[expect(
            clippy::indexing_slicing,
            reason = "kept only contains indices from 0..formatted.len(), so all are valid"
        )]
        for i in kept {
            result.push_str(&formatted[i]); // kanon:ignore RUST/indexing-slicing
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

/// Whether a conditional workspace file should be loaded for the given task hint.
///
/// Only called for [`LoadTier::Conditional`] files. Always-tier files bypass
/// this check entirely.
fn is_file_relevant(filename: &str, hint: TaskHint) -> bool {
    match hint {
        // WHY: General loads everything for backward compatibility
        TaskHint::General => true,
        TaskHint::Coding => matches!(filename, "TOOLS.md" | "CHECKLIST.md" | "MEMORY.md"),
        TaskHint::Research => matches!(filename, "GOALS.md" | "CONTEXT.md" | "MEMORY.md"),
        TaskHint::Planning => matches!(filename, "GOALS.md" | "AGENTS.md" | "CONTEXT.md"),
        // WHY: Conversation loads identity-tier files only; all conditional files skipped
        TaskHint::Conversation => false,
    }
}

/// Classify user message content into a task hint for conditional file loading.
///
/// Uses keyword scoring to detect task patterns. Returns [`TaskHint::General`]
/// when the message is ambiguous or matches no specific pattern.
#[must_use]
pub fn classify_task_hint(content: &str) -> TaskHint {
    let lower = content.to_lowercase();

    // WHY: short messages with greetings are casual conversation, not work tasks
    if content.split_whitespace().count() <= 5 && score_keywords(&lower, CONVERSATION_KEYWORDS) > 0
    {
        return TaskHint::Conversation;
    }

    let coding = score_keywords(&lower, CODING_KEYWORDS);
    let research = score_keywords(&lower, RESEARCH_KEYWORDS);
    let planning = score_keywords(&lower, PLANNING_KEYWORDS);

    let max = coding.max(research).max(planning);
    if max == 0 {
        return TaskHint::General;
    }

    // WHY: ties broken coding > research > planning since coding is the most common task
    if coding >= research && coding >= planning {
        TaskHint::Coding
    } else if research >= planning {
        TaskHint::Research
    } else {
        TaskHint::Planning
    }
}

const CODING_KEYWORDS: &[&str] = &[
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

const RESEARCH_KEYWORDS: &[&str] = &[
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
];

const PLANNING_KEYWORDS: &[&str] = &[
    "plan",
    "design",
    "architect",
    "strategy",
    "roadmap",
    "organize",
    "coordinate",
    "priority",
    "goal",
    "milestone",
];

const CONVERSATION_KEYWORDS: &[&str] = &[
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

fn score_keywords(text: &str, keywords: &[&str]) -> usize {
    keywords.iter().filter(|kw| text.contains(**kw)).count()
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
