//! Context bootstrap assembly.
//!
//! Reads workspace files through the taxis cascade, estimates tokens,
//! and packs sections in priority order within the token budget.

/// Tool summary generation for inclusion in the bootstrap system prompt.
pub mod tools;

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
            estimator: CharEstimator,
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

        // Stable sort preserves declaration order within same priority
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
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");

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
                        return Err(error::ContextAssemblySnafu {
                            message: format!("required file {} unreadable: {e}", spec.filename),
                        }
                        .build());
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
    /// Strategy: split on `## ` markdown headers, include complete subsections
    /// until budget is reached. Falls back to line-by-line truncation if no
    /// headers are found.
    fn truncate_section(&self, section: &BootstrapSection, max_tokens: u64) -> BootstrapSection {
        let parts: Vec<&str> = section.content.split("\n## ").collect();

        if parts.len() <= 1 {
            return self.truncate_by_lines(section, max_tokens);
        }

        let mut result = String::new();
        let mut tokens: u64 = 0;

        for (i, part) in parts.iter().enumerate() {
            let text = if i == 0 {
                (*part).to_owned()
            } else {
                format!("\n## {part}")
            };
            let part_tokens = self.estimator.estimate(&text);

            if tokens + part_tokens > max_tokens {
                if result.is_empty() {
                    return self.truncate_by_lines(section, max_tokens);
                }
                result.push_str("\n\n... [truncated for token budget] ...");
                break;
            }
            result.push_str(&text);
            tokens += part_tokens;
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
    fn truncate_by_lines(&self, section: &BootstrapSection, max_tokens: u64) -> BootstrapSection {
        let mut result = String::new();
        let mut tokens: u64 = 0;

        for line in section.content.lines() {
            let line_with_newline = format!("{line}\n");
            let line_tokens = self.estimator.estimate(&line_with_newline);

            if tokens + line_tokens > max_tokens {
                result.push_str("\n... [truncated for token budget] ...");
                break;
            }
            result.push_str(&line_with_newline);
            tokens += line_tokens;
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
                PackPriority::Optional => SectionPriority::Optional,
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
mod tests {
    use super::*;
    use crate::budget::TokenBudget;
    use std::fs;
    use tempfile::TempDir;

    /// Create an oikos directory structure with the given files.
    /// Files are placed in `nous/{nous_id}/` unless the filename starts with `theke:`.
    fn setup_oikos(nous_id: &str, files: &[(&str, &str)]) -> (TempDir, Oikos) {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        // Create tier directories
        fs::create_dir_all(root.join(format!("nous/{nous_id}"))).unwrap();
        fs::create_dir_all(root.join("shared")).unwrap();
        fs::create_dir_all(root.join("theke")).unwrap();

        for (name, content) in files {
            if let Some(stripped) = name.strip_prefix("theke:") {
                fs::write(root.join("theke").join(stripped), content).unwrap();
            } else {
                fs::write(root.join(format!("nous/{nous_id}")).join(name), content).unwrap();
            }
        }

        let oikos = Oikos::from_root(root);
        (dir, oikos)
    }

    fn default_budget() -> TokenBudget {
        TokenBudget::new(200_000, 0.6, 16_384, 40_000)
    }

    // --- Assembly tests ---

    #[tokio::test]
    async fn assemble_with_required_only() {
        let (_dir, oikos) = setup_oikos("test", &[("SOUL.md", "I am a test agent.")]);
        let assembler = BootstrapAssembler::new(&oikos);
        let mut budget = default_budget();

        let result = assembler.assemble("test", &mut budget).await.unwrap();
        assert!(result.system_prompt.contains("I am a test agent."));
        assert_eq!(result.sections_included, vec!["SOUL.md"]);
        assert!(result.sections_dropped.is_empty());
    }

    #[tokio::test]
    async fn assemble_missing_required_errors() {
        let (_dir, oikos) = setup_oikos("test", &[("USER.md", "some user info")]);
        let assembler = BootstrapAssembler::new(&oikos);
        let mut budget = default_budget();

        let err = assembler.assemble("test", &mut budget).await.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("SOUL.md"),
            "error should mention SOUL.md: {msg}"
        );
    }

    #[tokio::test]
    async fn assemble_missing_optional_skips() {
        let (_dir, oikos) = setup_oikos("test", &[("SOUL.md", "identity")]);
        let assembler = BootstrapAssembler::new(&oikos);
        let mut budget = default_budget();

        let result = assembler.assemble("test", &mut budget).await.unwrap();
        // Only SOUL.md included, others silently skipped
        assert_eq!(result.sections_included, vec!["SOUL.md"]);
        assert!(result.sections_dropped.is_empty());
    }

    #[tokio::test]
    async fn assemble_priority_ordering() {
        let (_dir, oikos) = setup_oikos(
            "test",
            &[
                ("SOUL.md", "identity"),
                ("MEMORY.md", "memory notes"),
                ("GOALS.md", "goals"),
            ],
        );
        let assembler = BootstrapAssembler::new(&oikos);
        let mut budget = default_budget();

        let result = assembler.assemble("test", &mut budget).await.unwrap();
        // Required (SOUL) before Important (GOALS) before Flexible (MEMORY)
        let soul_pos = result
            .sections_included
            .iter()
            .position(|s| s == "SOUL.md")
            .unwrap();
        let goals_pos = result
            .sections_included
            .iter()
            .position(|s| s == "GOALS.md")
            .unwrap();
        let memory_pos = result
            .sections_included
            .iter()
            .position(|s| s == "MEMORY.md")
            .unwrap();
        assert!(soul_pos < goals_pos);
        assert!(goals_pos < memory_pos);
    }

    #[tokio::test]
    async fn assemble_all_files_present() {
        let (_dir, oikos) = setup_oikos(
            "test",
            &[
                ("SOUL.md", "identity"),
                ("USER.md", "user info"),
                ("AGENTS.md", "team topology"),
                ("GOALS.md", "goals"),
                ("TOOLS.md", "tool list"),
                ("MEMORY.md", "memory"),
                ("IDENTITY.md", "name and emoji"),
                ("PROSOCHE.md", "checklist"),
                ("CONTEXT.md", "runtime config"),
            ],
        );
        let assembler = BootstrapAssembler::new(&oikos);
        let mut budget = default_budget();

        let result = assembler.assemble("test", &mut budget).await.unwrap();
        assert_eq!(result.sections_included.len(), 9);
        assert!(result.total_tokens > 0);
    }

    #[tokio::test]
    async fn assemble_empty_file_skipped() {
        let (_dir, oikos) = setup_oikos(
            "test",
            &[
                ("SOUL.md", "identity"),
                ("AGENTS.md", ""),
                ("GOALS.md", "   \n  \n  "), // whitespace-only
            ],
        );
        let assembler = BootstrapAssembler::new(&oikos);
        let mut budget = default_budget();

        let result = assembler.assemble("test", &mut budget).await.unwrap();
        assert_eq!(result.sections_included, vec!["SOUL.md"]);
    }

    #[tokio::test]
    async fn assemble_memory_truncated() {
        // Create a large MEMORY.md that exceeds a small budget
        let large_memory = "## Recent\nNew stuff here.\n## Old\nOld stuff here that is much longer and should be truncated when the budget is tight. ".repeat(50);
        let (_dir, oikos) = setup_oikos(
            "test",
            &[("SOUL.md", "identity"), ("MEMORY.md", &large_memory)],
        );
        let assembler = BootstrapAssembler::new(&oikos);
        // Small budget: enough for SOUL.md but not full MNEME.md
        let mut budget = TokenBudget::new(100_000, 0.0, 0, 500);

        let result = assembler.assemble("test", &mut budget).await.unwrap();
        assert!(result.sections_included.contains(&"MEMORY.md".to_owned()));
        assert!(result.sections_truncated.contains(&"MEMORY.md".to_owned()));
        assert!(
            result
                .system_prompt
                .contains("[truncated for token budget]")
        );
    }

    #[tokio::test]
    async fn assemble_optional_dropped() {
        // SOUL.md fills the entire budget, MEMORY.md gets dropped
        let large_soul = "x".repeat(2000); // ~500 tokens at 4 chars/token
        let (_dir, oikos) = setup_oikos(
            "test",
            &[("SOUL.md", &large_soul), ("MEMORY.md", "memory notes")],
        );
        let assembler = BootstrapAssembler::new(&oikos);
        let mut budget = TokenBudget::new(100_000, 0.0, 0, 500);

        let result = assembler.assemble("test", &mut budget).await.unwrap();
        assert!(result.sections_included.contains(&"SOUL.md".to_owned()));
        assert!(result.sections_dropped.contains(&"MEMORY.md".to_owned()));
    }

    #[tokio::test]
    async fn assemble_budget_consumed_correctly() {
        let (_dir, oikos) =
            setup_oikos("test", &[("SOUL.md", "identity"), ("USER.md", "user info")]);
        let assembler = BootstrapAssembler::new(&oikos);
        let mut budget = default_budget();

        let result = assembler.assemble("test", &mut budget).await.unwrap();
        assert_eq!(budget.consumed(), result.total_tokens);
        assert!(result.total_tokens > 0);
    }

    #[tokio::test]
    async fn assemble_cascade_nous_tier() {
        // File only in nous tier
        let (_dir, oikos) = setup_oikos("syn", &[("SOUL.md", "I am Syn.")]);
        let assembler = BootstrapAssembler::new(&oikos);
        let mut budget = default_budget();

        let result = assembler.assemble("syn", &mut budget).await.unwrap();
        assert!(result.system_prompt.contains("I am Syn."));
    }

    #[tokio::test]
    async fn assemble_cascade_theke_fallback() {
        // USER.md only in theke (common pattern)
        let (_dir, oikos) = setup_oikos(
            "syn",
            &[("SOUL.md", "identity"), ("theke:USER.md", "Alice T.")],
        );
        let assembler = BootstrapAssembler::new(&oikos);
        let mut budget = default_budget();

        let result = assembler.assemble("syn", &mut budget).await.unwrap();
        assert!(result.system_prompt.contains("Alice T."));
        assert!(result.sections_included.contains(&"USER.md".to_owned()));
    }

    #[tokio::test]
    async fn assemble_nous_overrides_theke() {
        // SOUL.md in both tiers — nous wins
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("nous/syn")).unwrap();
        fs::create_dir_all(root.join("shared")).unwrap();
        fs::create_dir_all(root.join("theke")).unwrap();
        fs::write(root.join("nous/syn/SOUL.md"), "nous-specific soul").unwrap();
        fs::write(root.join("theke/SOUL.md"), "theke soul").unwrap();

        let oikos = Oikos::from_root(root);
        let assembler = BootstrapAssembler::new(&oikos);
        let mut budget = default_budget();

        let result = assembler.assemble("syn", &mut budget).await.unwrap();
        assert!(result.system_prompt.contains("nous-specific soul"));
        assert!(!result.system_prompt.contains("theke soul"));
    }

    // --- Truncation tests ---

    #[test]
    fn truncate_section_aware() {
        let oikos = Oikos::from_root("/tmp/unused");
        let assembler = BootstrapAssembler::new(&oikos);

        let section = BootstrapSection {
            name: "MEMORY.md".to_owned(),
            priority: SectionPriority::Flexible,
            content: "## Section A\nContent A.\n## Section B\nContent B.\n## Section C\nContent C."
                .to_owned(),
            tokens: 100,
            truncatable: true,
        };

        // Budget enough for first section only
        let truncated = assembler.truncate_section(&section, 10);
        assert!(truncated.content.contains("Section A"));
        assert!(truncated.content.contains("[truncated for token budget]"));
    }

    #[test]
    fn truncate_falls_back_to_lines() {
        let oikos = Oikos::from_root("/tmp/unused");
        let assembler = BootstrapAssembler::new(&oikos);

        let section = BootstrapSection {
            name: "MEMORY.md".to_owned(),
            priority: SectionPriority::Flexible,
            content: "Line one\nLine two\nLine three\nLine four\nLine five".to_owned(),
            tokens: 100,
            truncatable: true,
        };

        // Budget enough for ~2 lines
        let truncated = assembler.truncate_by_lines(&section, 5);
        assert!(truncated.content.contains("Line one"));
        assert!(truncated.content.contains("[truncated for token budget]"));
    }

    // --- Pack conversion tests ---

    #[test]
    fn pack_sections_to_bootstrap_converts_priorities() {
        let sections = [
            PackSection {
                name: "LOGIC.md".to_owned(),
                content: "Business logic content".to_owned(),
                priority: PackPriority::Required,
                truncatable: false,
                agents: vec![],
                pack_name: "test-pack".to_owned(),
            },
            PackSection {
                name: "GLOSSARY.md".to_owned(),
                content: "Term definitions".to_owned(),
                priority: PackPriority::Flexible,
                truncatable: true,
                agents: vec!["chiron".to_owned()],
                pack_name: "test-pack".to_owned(),
            },
        ];

        let refs: Vec<&PackSection> = sections.iter().collect();
        let result = pack_sections_to_bootstrap(&refs, &CharEstimator);

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "[test-pack] LOGIC.md");
        assert_eq!(result[0].priority, SectionPriority::Required);
        assert!(!result[0].truncatable);
        assert_eq!(result[0].content, "Business logic content");
        assert!(result[0].tokens > 0);

        assert_eq!(result[1].name, "[test-pack] GLOSSARY.md");
        assert_eq!(result[1].priority, SectionPriority::Flexible);
        assert!(result[1].truncatable);
    }

    #[test]
    fn pack_sections_to_bootstrap_empty_input() {
        let result = pack_sections_to_bootstrap(&[], &CharEstimator);
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn assemble_with_extra_includes_pack_sections() {
        let (_dir, oikos) = setup_oikos("test", &[("SOUL.md", "I am a test agent.")]);
        let assembler = BootstrapAssembler::new(&oikos);
        let mut budget = default_budget();

        let extra = vec![BootstrapSection {
            name: "[pack] LOGIC.md".to_owned(),
            priority: SectionPriority::Important,
            content: "Domain logic from pack.".to_owned(),
            tokens: 6,
            truncatable: false,
        }];

        let result = assembler
            .assemble_with_extra("test", &mut budget, extra)
            .await
            .unwrap();
        assert!(result.system_prompt.contains("Domain logic from pack."));
        assert!(
            result
                .sections_included
                .contains(&"[pack] LOGIC.md".to_owned())
        );
        assert_eq!(result.sections_included.len(), 2);
    }

    #[tokio::test]
    async fn assemble_with_extra_respects_priority_ordering() {
        let (_dir, oikos) = setup_oikos("test", &[("SOUL.md", "identity")]);
        let assembler = BootstrapAssembler::new(&oikos);
        let mut budget = default_budget();

        let extra = vec![
            BootstrapSection {
                name: "optional-pack".to_owned(),
                priority: SectionPriority::Optional,
                content: "optional".to_owned(),
                tokens: 2,
                truncatable: true,
            },
            BootstrapSection {
                name: "important-pack".to_owned(),
                priority: SectionPriority::Important,
                content: "important".to_owned(),
                tokens: 2,
                truncatable: false,
            },
        ];

        let result = assembler
            .assemble_with_extra("test", &mut budget, extra)
            .await
            .unwrap();

        // SOUL.md (Required) < important-pack (Important) < optional-pack (Optional)
        let soul_pos = result
            .sections_included
            .iter()
            .position(|s| s == "SOUL.md")
            .unwrap();
        let important_pos = result
            .sections_included
            .iter()
            .position(|s| s == "important-pack")
            .unwrap();
        let optional_pos = result
            .sections_included
            .iter()
            .position(|s| s == "optional-pack")
            .unwrap();
        assert!(soul_pos < important_pos);
        assert!(important_pos < optional_pos);
    }
}
