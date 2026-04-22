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
//!
//! An `output-style` section is always injected, derived from the
//! `## Communication` section in USER.md (or defaults if absent).
//! This shapes response formatting to match the operator's cognitive
//! preferences from the first turn.

/// Tool summary generation for inclusion in the bootstrap system prompt.
pub mod tools;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::{Duration, Instant, SystemTime};

use serde::Deserialize;
use snafu::IntoError as _;
use tracing::{debug, info, warn};

use taxis::cascade;
use taxis::oikos::Oikos;
use thesauros::loader::PackSection;
use thesauros::manifest::Priority as PackPriority;

use crate::budget::{CharEstimator, TokenBudget};
use crate::error::{self, Result};

/// Default TTL for bootstrap file cache entries when no operator override is set.
///
/// // WHY: 60s balances freshness (operator edits to SOUL.md/USER.md should
/// // surface within about a minute) against the cost of re-reading every
/// // workspace file on every turn. mtime-based invalidation catches edits
/// // sooner when they happen.
pub const DEFAULT_BOOTSTRAP_CACHE_TTL_SECS: u64 = 60;

/// Cached content of a bootstrap workspace file.
///
/// Stores the trimmed content along with the mtime at read time and the
/// pre-computed token estimate. The token estimate is keyed to a specific
/// [`CharEstimator`] configuration (the assembler's `chars_per_token`), so
/// the cache records that value and invalidates on mismatch.
#[derive(Debug, Clone)]
struct CachedFile {
    /// Trimmed file content at read time.
    content: String,
    /// File mtime at read time: any on-disk change invalidates the entry.
    mtime: SystemTime,
    /// Pre-computed token estimate for `content`.
    tokens: u64,
    /// The `chars_per_token` value used to compute `tokens`: mismatch forces recompute.
    chars_per_token: u64,
    /// Wall-clock instant at insertion: drives TTL expiry.
    read_at: Instant,
}

/// TTL + mtime cache for bootstrap workspace file reads.
///
/// Bootstrap assembles the system prompt on every turn by reading the same
/// handful of workspace files (SOUL.md, USER.md, etc.) from disk. Before
/// this cache, each turn paid the cost of re-reading and re-tokenising
/// identical content (issue #3388). The cache keys on the resolved cascade
/// path and validates against the on-disk mtime, so:
///
/// - A hit within `ttl` with unchanged mtime avoids both I/O and tokenisation.
/// - An mtime change invalidates the entry immediately, without waiting for
///   the TTL to elapse: operator edits take effect on the next turn.
/// - TTL expiry forces a re-stat + re-read so silently swapped files
///   (e.g. via filesystem snapshots) are eventually observed.
///
/// The cache is `Send + Sync` and is designed to be shared across actor
/// pipeline turns via `Arc`.
#[derive(Debug)]
pub struct BootstrapFileCache {
    entries: RwLock<HashMap<PathBuf, CachedFile>>,
    ttl: Duration,
}

impl BootstrapFileCache {
    /// Create a new cache with the given TTL.
    #[must_use]
    pub fn new(ttl: Duration) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            ttl,
        }
    }

    /// Create a cache with TTL from `ttl_secs`. A value of `0` disables caching.
    #[must_use]
    pub fn with_ttl_secs(ttl_secs: u64) -> Self {
        Self::new(Duration::from_secs(ttl_secs))
    }

    /// Returns `true` when caching is disabled (TTL == 0).
    #[must_use]
    fn is_disabled(&self) -> bool {
        self.ttl.is_zero()
    }

    /// Clear all cached entries. Intended for tests and explicit invalidation.
    pub fn clear(&self) {
        if let Ok(mut entries) = self.entries.write() {
            entries.clear();
        }
    }

    /// Number of entries currently held in the cache (for tests/metrics).
    #[must_use]
    pub fn len(&self) -> usize {
        // WHY: poisoned lock treated as empty; metrics are never load-bearing.
        self.entries.read().map_or(0, |e| e.len())
    }

    /// Returns `true` if the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Fast-path read: return a cached entry when it is fresh and matches mtime.
    ///
    /// Returns `Some((content, tokens))` on a hit, or `None` on miss (entry
    /// absent, TTL expired, mtime changed, estimator mismatch, or stat failed).
    /// Never performs disk I/O except the mtime stat.
    fn get_fresh(&self, path: &Path, chars_per_token: u64) -> Option<(String, u64)> {
        if self.is_disabled() {
            return None;
        }

        let entries = self.entries.read().ok()?;
        let entry = entries.get(path)?;

        if entry.chars_per_token != chars_per_token {
            debug!(
                path = %path.display(),
                cached = entry.chars_per_token,
                wanted = chars_per_token,
                "bootstrap cache miss: chars_per_token mismatch"
            );
            return None;
        }

        if entry.read_at.elapsed() > self.ttl {
            debug!(
                path = %path.display(),
                ttl_secs = self.ttl.as_secs(),
                "bootstrap cache miss: TTL expired"
            );
            return None;
        }

        // WHY: stat after TTL check so cheap cases short-circuit first.
        let Ok(meta) = std::fs::metadata(path) else {
            debug!(
                path = %path.display(),
                "bootstrap cache miss: stat failed"
            );
            return None;
        };
        let Ok(mtime) = meta.modified() else {
            return None;
        };
        if mtime != entry.mtime {
            debug!(
                path = %path.display(),
                "bootstrap cache miss: mtime changed"
            );
            return None;
        }

        debug!(
            path = %path.display(),
            tokens = entry.tokens,
            "bootstrap cache hit"
        );
        Some((entry.content.clone(), entry.tokens))
    }

    /// Insert a freshly-read entry into the cache.
    fn insert(
        &self,
        path: PathBuf,
        content: String,
        mtime: SystemTime,
        tokens: u64,
        chars_per_token: u64,
    ) {
        if self.is_disabled() {
            return;
        }
        if let Ok(mut entries) = self.entries.write() {
            entries.insert(
                path,
                CachedFile {
                    content,
                    mtime,
                    tokens,
                    chars_per_token,
                    read_at: Instant::now(),
                },
            );
        }
    }
}

impl Default for BootstrapFileCache {
    fn default() -> Self {
        Self::with_ttl_secs(DEFAULT_BOOTSTRAP_CACHE_TTL_SECS)
    }
}

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

/// Recipe for loading `_llm/` content into the bootstrap system prompt.
///
/// Maps _llm/ levels to bootstrap priorities based on session state and
/// task type. Selected automatically from [`TaskHint`] or overridden via
/// dispatch metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum LlmRecipe {
    /// Load L1 as Required, L3 as Optional. Used on first turn (cold start).
    #[default]
    ColdStart,
    /// Load L1 as Optional, L3 as Optional. Used for general in-session turns.
    InSession,
    /// Load L1 as Important, L3 as Important. Used for planning and refactoring.
    Refactor,
    /// Skip all `_llm/` content.
    None,
}

impl LlmRecipe {
    /// Select a recipe based on task hint and whether this is the first turn.
    #[must_use]
    pub fn from_task_hint(task_hint: TaskHint, is_cold_start: bool) -> Self {
        if is_cold_start {
            return Self::ColdStart;
        }
        match task_hint {
            TaskHint::Planning => Self::Refactor,
            TaskHint::Conversation => Self::None,
            _ => Self::InSession,
        }
    }
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

/// Default output-style directives when USER.md has no `## Communication` section.
///
/// These encode the project operator's cognitive preferences so that the
/// system prompt shapes output formatting from the first turn.
const DEFAULT_OUTPUT_STYLE: &str = "\
- Direct, not performative. Answer-first.
- Structure over prose. Trust processing capacity.
- No forced linearity, no basics when the bigger picture is visible.
- If you can say it in one sentence, don't use three.";

/// Parsed `_llm/manifest.toml` describing available levels.
///
/// WHY: The manifest is the single source of truth for which _llm/ levels
/// exist and where they live. Parsing it lets the bootstrap assembler
/// discover levels dynamically rather than hardcoding paths.
#[derive(Debug, Clone, Deserialize)]
#[expect(
    dead_code,
    reason = "fields deserialized from manifest for future use but not yet consumed"
)]
struct LlmManifest {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    levels: HashMap<String, LlmLevel>,
    #[serde(default)]
    crates: Vec<LlmManifestCrate>,
}

#[derive(Debug, Clone, Deserialize)]
#[expect(
    dead_code,
    reason = "fields deserialized from manifest for future use but not yet consumed"
)]
struct LlmLevel {
    path: String,
    #[serde(default)]
    generator: String,
}

#[derive(Debug, Clone, Deserialize)]
#[expect(
    dead_code,
    reason = "fields deserialized from manifest for future use but not yet consumed"
)]
struct LlmManifestCrate {
    name: String,
    path: String,
    #[serde(rename = "source_hash")]
    _source_hash: String,
    #[serde(rename = "l3_token_estimate")]
    _l3_token_estimate: u64,
}

/// Assembles the bootstrap system prompt from oikos workspace files.
///
/// Resolves files through the three-tier cascade (`nous/{id}/` → `shared/` → `theke/`),
/// reads contents, estimates tokens, and packs sections in priority order.
///
/// Workspace file reads are served from an optional [`BootstrapFileCache`]
/// when one is attached via [`new_with_cache`](Self::new_with_cache). Without
/// a cache, every call re-reads every file from disk (legacy behaviour).
pub struct BootstrapAssembler<'a> {
    oikos: &'a Oikos,
    estimator: CharEstimator,
    /// Minimum tokens remaining before attempting truncation (below this, just drop).
    /// Default read from [`taxis::config::AgentBehaviorDefaults::bootstrap_min_truncation_budget`].
    min_truncation_budget: u64,
    /// Shared file cache: `None` disables caching (legacy path, used by tests
    /// that want guaranteed fresh reads).
    cache: Option<&'a BootstrapFileCache>,
    /// Recipe for loading `_llm/` content. `None` skips _llm/ entirely.
    llm_recipe: LlmRecipe,
}

impl<'a> BootstrapAssembler<'a> {
    /// Create an assembler with the default character-based estimator and no cache.
    #[must_use]
    pub fn new(oikos: &'a Oikos) -> Self {
        let min_truncation_budget =
            taxis::config::AgentBehaviorDefaults::default().bootstrap_min_truncation_budget;
        Self {
            oikos,
            estimator: CharEstimator::default(),
            min_truncation_budget,
            cache: None,
            llm_recipe: LlmRecipe::default(),
        }
    }

    /// Create an assembler with a configurable characters-per-token divisor.
    ///
    /// Wires the operator-configured `chars_per_token` value from
    /// `agents.defaults.chars_per_token` into the bootstrap estimator.
    #[must_use]
    pub fn new_with_chars_per_token(oikos: &'a Oikos, chars_per_token: u64) -> Self {
        let min_truncation_budget =
            taxis::config::AgentBehaviorDefaults::default().bootstrap_min_truncation_budget;
        Self {
            oikos,
            estimator: CharEstimator::new(chars_per_token),
            min_truncation_budget,
            cache: None,
            llm_recipe: LlmRecipe::default(),
        }
    }

    /// Attach a shared [`BootstrapFileCache`] to this assembler.
    ///
    /// With a cache attached, workspace file reads within the TTL window skip
    /// both the disk read and token estimation when the on-disk `mtime` is
    /// unchanged. Without a cache, every call re-reads every file.
    #[must_use]
    pub fn with_cache(mut self, cache: &'a BootstrapFileCache) -> Self {
        self.cache = Some(cache);
        self
    }

    /// Set the [`LlmRecipe`] for loading `_llm/` content.
    ///
    /// Determines which _llm/ levels are loaded and at what priority tier.
    /// The default is [`LlmRecipe::ColdStart`].
    #[must_use]
    pub fn with_llm_recipe(mut self, recipe: LlmRecipe) -> Self {
        self.llm_recipe = recipe;
        self
    }

    /// Assemble the bootstrap system prompt for the given nous.
    ///
    /// Loads all workspace files (identity + operational). Use
    /// [`assemble_conditional`](Self::assemble_conditional) for task-aware loading.
    ///
    /// # Complexity
    ///
    /// O(f) where f is the number of workspace files. Each file requires
    /// a cascade resolution and potentially a disk read.
    ///
    /// # Errors
    ///
    /// Returns [`error::Error::ContextAssembly`] if a Required file (SOUL.md) is
    /// missing or unreadable.
    ///
    /// # Cancel safety
    ///
    /// Not cancel-safe. If cancelled after partial file loading, the
    /// returned `BootstrapResult` may be incomplete. Callers should not
    /// use this in `select!` branches.
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
    ///
    /// # Cancel safety
    ///
    /// Not cancel-safe. Delegates to [`assemble_conditional`](Self::assemble_conditional).
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
    /// Uses [`LlmRecipe::from_task_hint`] with `is_cold_start = false` to
    /// select the _llm/ loading recipe. Use
    /// [`assemble_conditional_with_recipe`](Self::assemble_conditional_with_recipe)
    /// for explicit recipe control (e.g. cold-start detection).
    ///
    /// # Complexity
    ///
    /// O(f + s log s) where f is the number of workspace files and s is the
    /// number of sections after filtering. Sorting sections by priority dominates.
    ///
    /// # Errors
    ///
    /// Returns [`error::Error::ContextAssembly`] if a Required file (SOUL.md) is
    /// missing or unreadable.
    ///
    /// # Cancel safety
    ///
    /// Not cancel-safe. If cancelled during file I/O, partial sections may
    /// be loaded and token budget calculations incomplete. Do not use in
    /// `select!` branches.
    pub async fn assemble_conditional(
        &self,
        nous_id: &str,
        budget: &mut TokenBudget,
        extra_sections: Vec<BootstrapSection>,
        hint: TaskHint,
    ) -> Result<BootstrapResult> {
        self.assemble_conditional_with_recipe(
            nous_id,
            budget,
            extra_sections,
            hint,
            self.llm_recipe,
        )
        .await
    }

    /// Assemble the bootstrap system prompt with conditional file loading and
    /// explicit _llm/ recipe control.
    ///
    /// Same as [`assemble_conditional`](Self::assemble_conditional) but accepts
    /// an explicit [`LlmRecipe`] so callers can override automatic selection
    /// (e.g. to load L1 as Required on a cold start).
    ///
    /// # Errors
    ///
    /// Returns [`error::Error::ContextAssembly`] if a Required file (SOUL.md) is
    /// missing or unreadable.
    ///
    /// # Cancel safety
    ///
    /// Not cancel-safe. Delegates to the same I/O path as `assemble_conditional`.
    pub async fn assemble_conditional_with_recipe(
        &self,
        nous_id: &str,
        budget: &mut TokenBudget,
        extra_sections: Vec<BootstrapSection>,
        hint: TaskHint,
        recipe: LlmRecipe,
    ) -> Result<BootstrapResult> {
        let (mut sections, filtered_names) = self.resolve_workspace_files(nous_id, hint).await?;
        sections.extend(extra_sections);

        let llm_sections = self.resolve_llm_sections(recipe).await;
        sections.extend(llm_sections);

        // NOTE: stable sort preserves declaration order within same priority
        sections.sort_by_key(|s| s.priority);

        let mut included: Vec<BootstrapSection> = Vec::new();
        let mut truncated_names: Vec<String> = Vec::new();
        let mut dropped_names: Vec<String> = Vec::new();

        for section in sections {
            if budget.can_fit(section.tokens) {
                budget.consume(section.tokens);
                included.push(section);
            } else if section.truncatable && budget.remaining() > self.min_truncation_budget {
                debug!(
                    section = section.name,
                    remaining = budget.remaining(),
                    min_truncation_budget = self.min_truncation_budget,
                    "truncating section to fit token budget"
                );
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
    ///
    /// # Complexity
    ///
    /// O(f) where f is the number of workspace files. Each file requires
    /// a cascade resolution and potentially a disk read.
    #[expect(
        clippy::too_many_lines,
        reason = "cache-aware read path keeps cold/warm/insert flows colocated — splitting obscures the caching logic"
    )]
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

            // WHY: try the cache first to avoid re-reading and re-tokenising
            // workspace files that change rarely relative to turn frequency (#3388).
            if let Some(cache) = self.cache
                && let Some((content, tokens)) =
                    cache.get_fresh(&p, self.estimator.chars_per_token())
            {
                if content.is_empty() {
                    debug!(file = spec.filename, "skipping empty file (cached)");
                    continue;
                }
                sections.push(BootstrapSection {
                    name: spec.filename.to_owned(),
                    priority: spec.priority,
                    content,
                    tokens,
                    truncatable: spec.truncatable,
                });
                continue;
            }

            match tokio::fs::read_to_string(&p).await {
                Ok(raw) => {
                    let content = raw.trim().to_owned();
                    if content.is_empty() {
                        debug!(file = spec.filename, "skipping empty file");
                        // WHY: still cache the empty read so repeated skips don't re-hit disk.
                        if let Some(cache) = self.cache
                            && let Ok(meta) = tokio::fs::metadata(&p).await
                            && let Ok(mtime) = meta.modified()
                        {
                            cache.insert(
                                p.clone(),
                                String::new(),
                                mtime,
                                0,
                                self.estimator.chars_per_token(),
                            );
                        }
                        continue;
                    }
                    let tokens = self.estimator.estimate(&content);
                    if let Some(cache) = self.cache
                        && let Ok(meta) = tokio::fs::metadata(&p).await
                        && let Ok(mtime) = meta.modified()
                    {
                        cache.insert(
                            p.clone(),
                            content.clone(),
                            mtime,
                            tokens,
                            self.estimator.chars_per_token(),
                        );
                    }
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

        // WHY: inject output-style directives derived from USER.md's Communication
        // section (or defaults). This shapes formatting to match the operator's
        // cognitive preferences from the first turn. Always-tier because it applies
        // to every response regardless of task type.
        let style_content = sections
            .iter()
            .find(|s| s.name == "USER.md")
            .and_then(|s| extract_output_style(&s.content))
            .unwrap_or_else(|| DEFAULT_OUTPUT_STYLE.to_owned());

        let style_section = format!(
            "Format all output according to the operator's communication preferences:\n\n{style_content}"
        );
        let style_tokens = self.estimator.estimate(&style_section);
        sections.push(BootstrapSection {
            name: "output-style".to_owned(),
            priority: SectionPriority::Flexible,
            content: style_section,
            tokens: style_tokens,
            truncatable: false,
        });
        debug!("injected output-style section ({style_tokens} tokens)");

        Ok((sections, filtered))
    }

    /// Resolve `_llm/` content into bootstrap sections based on the recipe.
    ///
    /// Reads `_llm/manifest.toml` when present to discover available levels.
    /// Loads L1 (workspace manifest files at `_llm/` root) and L3 (cross-crate
    /// index from the path declared in the manifest). L2 is reserved for
    /// per-crate summaries and skipped when absent.
    ///
    /// Returns an empty vec when:
    /// - the recipe is [`LlmRecipe::None`]
    /// - `_llm/manifest.toml` does not exist
    /// - any I/O or parse error occurs (logged, not propagated)
    ///
    /// # Cancel safety
    ///
    /// Not cancel-safe. Performs async file I/O.
    #[expect(
        clippy::too_many_lines,
        reason = "L1 + L3 loading in one method keeps the recipe→levels mapping colocated"
    )]
    async fn resolve_llm_sections(&self, recipe: LlmRecipe) -> Vec<BootstrapSection> {
        if recipe == LlmRecipe::None {
            return Vec::new();
        }

        let llm_root = self.oikos.root().join("_llm");
        let manifest_path = llm_root.join("manifest.toml");

        if !manifest_path.exists() {
            debug!(path = %manifest_path.display(), "_llm/manifest.toml not found, skipping");
            return Vec::new();
        }

        let manifest_raw = match tokio::fs::read_to_string(&manifest_path).await {
            Ok(r) => r,
            Err(e) => {
                warn!(
                    path = %manifest_path.display(),
                    error = %e,
                    "failed to read _llm/manifest.toml, skipping"
                );
                return Vec::new();
            }
        };

        let manifest: LlmManifest = match toml::from_str(&manifest_raw) {
            Ok(m) => m,
            Err(e) => {
                warn!(
                    path = %manifest_path.display(),
                    error = %e,
                    "failed to parse _llm/manifest.toml, skipping"
                );
                return Vec::new();
            }
        };

        let mut sections = Vec::new();

        // --- L1: workspace manifest files at _llm/ root ---
        let (l1_priority, l1_truncatable) = match recipe {
            LlmRecipe::ColdStart => (SectionPriority::Required, false),
            LlmRecipe::InSession => (SectionPriority::Optional, true),
            LlmRecipe::Refactor => (SectionPriority::Important, true),
            LlmRecipe::None => return Vec::new(),
        };

        if let Ok(mut entries) = tokio::fs::read_dir(&llm_root).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };

                if name == "manifest.toml" || name == "recipes.toml" {
                    continue;
                }
                if path.is_dir() {
                    continue;
                }
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if !matches!(ext, "md" | "toml") {
                    continue;
                }

                match tokio::fs::read_to_string(&path).await {
                    Ok(raw) => {
                        let content = raw.trim().to_owned();
                        if content.is_empty() {
                            continue;
                        }
                        let tokens = self.estimator.estimate(&content);
                        sections.push(BootstrapSection {
                            name: format!("_llm/{name}"),
                            priority: l1_priority,
                            content,
                            tokens,
                            truncatable: l1_truncatable,
                        });
                    }
                    Err(e) => {
                        warn!(
                            path = %path.display(),
                            error = %e,
                            "failed to read _llm file, skipping"
                        );
                    }
                }
            }
        }

        // --- L3: cross-crate index ---
        let (l3_priority, l3_truncatable) = match recipe {
            LlmRecipe::ColdStart | LlmRecipe::InSession => (SectionPriority::Optional, true),
            LlmRecipe::Refactor => (SectionPriority::Important, true),
            LlmRecipe::None => return Vec::new(),
        };

        if let Some(l3_level) = manifest.levels.get("L3") {
            let l3_dir = llm_root.join(&l3_level.path);
            if l3_dir.is_dir()
                && let Ok(mut entries) = tokio::fs::read_dir(&l3_dir).await
            {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let path = entry.path();
                    if !path.is_file() {
                        continue;
                    }
                    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                        continue;
                    };
                    if !std::path::Path::new(name)
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
                    {
                        continue;
                    }

                    match tokio::fs::read_to_string(&path).await {
                        Ok(raw) => {
                            let content = raw.trim().to_owned();
                            if content.is_empty() {
                                continue;
                            }
                            let tokens = self.estimator.estimate(&content);
                            sections.push(BootstrapSection {
                                name: format!("_llm/{}/{name}", l3_level.path),
                                priority: l3_priority,
                                content,
                                tokens,
                                truncatable: l3_truncatable,
                            });
                        }
                        Err(e) => {
                            warn!(
                                path = %path.display(),
                                error = %e,
                                "failed to read L3 index file, skipping"
                            );
                        }
                    }
                }
            }
        }

        sections
    }

    /// Truncate a section to fit within the given token limit.
    ///
    /// Strategy: split on `## ` markdown headers and keep the **newest** (last)
    /// subsections that fit within the budget, dropping the oldest. A truncation
    /// marker is prepended so the reader knows content was removed. Falls back to
    /// line-by-line truncation if no headers are found or no single section fits.
    ///
    /// # Why
    ///
    /// Newest-first truncation preserves recent context (current goals,
    /// active tasks) which is more relevant than older history. The LLM
    /// attends more strongly to content near the end of long contexts.
    ///
    /// # Complexity
    ///
    /// O(p) where p is the number of markdown sections in the content.
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
    ///
    /// // WHY: Line-by-line is coarser than header-based truncation but handles
    /// // files without markdown structure. Still prefers newest content.
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

/// Extract the `## Communication` section from USER.md content.
///
/// Looks for a `## Communication` or `## Output` heading and returns
/// everything up to the next `## ` heading (or end of content). Returns
/// `None` if no such section exists.
fn extract_output_style(user_content: &str) -> Option<String> {
    // WHY: headings are case-insensitive to match operator variation
    let lower = user_content.to_lowercase();

    let start = lower
        .find("\n## communication")
        .or_else(|| lower.find("\n## output"));

    let start = match start {
        Some(pos) => pos + 1, // skip the leading newline
        None => {
            // NOTE: check if the content starts with the heading (no leading newline)
            if lower.starts_with("## communication") || lower.starts_with("## output") {
                0
            } else {
                return None;
            }
        }
    };

    // NOTE: find the end of this section (next ## heading or end of content).
    // WHY: `start` came from `.find()` on the same string, so it is on a UTF-8
    // boundary; `.get(start..)` is safe and None would indicate a logic bug.
    let after_start = user_content.get(start..)?;
    let section_body_start = after_start
        .find('\n')
        .map_or(user_content.len(), |nl| start + nl + 1);

    let after_body_start = user_content.get(section_body_start..)?;
    let end = after_body_start
        .find("\n## ")
        .map_or(user_content.len(), |pos| section_body_start + pos);

    let body = user_content.get(section_body_start..end)?.trim();
    if body.is_empty() {
        None
    } else {
        Some(body.to_owned())
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
    if content.split_whitespace().count() <= 5
        && score_keywords(&lower, aletheia_lexica::keywords::CONVERSATION_KEYWORDS) > 0
    {
        return TaskHint::Conversation;
    }

    let coding = score_keywords(&lower, aletheia_lexica::keywords::CODING_KEYWORDS);
    let research = score_keywords(&lower, aletheia_lexica::keywords::RESEARCH_KEYWORDS);
    let planning = score_keywords(&lower, aletheia_lexica::keywords::PLANNING_KEYWORDS);

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

fn score_keywords(text: &str, keywords: &[&str]) -> usize {
    keywords.iter().filter(|kw| text.contains(**kw)).count()
}

/// Convert domain pack sections into bootstrap sections.
///
/// Maps thesauros [`PackSection`] values to [`BootstrapSection`] values,
/// computing token estimates for each section's content. Section names
/// are prefixed with the pack name for traceability.
pub fn pack_sections_to_bootstrap(
    sections: &[&PackSection],
    estimator: &CharEstimator,
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
#[path = "bootstrap_tests/mod.rs"]
mod tests;
