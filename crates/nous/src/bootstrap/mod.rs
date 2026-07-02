// kanon:ignore RUST/file-too-long — bootstrap assembly pipeline; modularization tracked in #3750
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

/// Pre-injection scan for workspace bootstrap files.
pub mod preinject_scan;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::{Duration, Instant, SystemTime};

use sha2::{Digest as _, Sha256};

use serde::Deserialize;
use snafu::IntoError as _;
use tracing::{debug, info, warn};

use taxis::cascade;
use taxis::oikos::Oikos;
use thesauros::loader::PackSection;
use thesauros::manifest::Priority as PackPriority;

use crate::budget::{CharEstimator, TokenBudget};
use crate::error::{self, Result};
use crate::recipes::{RecipeFile, RecipeRegistry};

/// Default TTL for bootstrap file cache entries when no operator override is set.
///
/// 60s balances freshness (operator edits to SOUL.md/USER.md should
/// surface within about a minute) against the cost of re-reading every
/// workspace file on every turn. mtime-based invalidation catches edits
/// sooner when they happen.
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

    /// Recipe name in `_llm/recipes.toml` that implements this bootstrap mode.
    #[must_use]
    pub fn recipe_name(&self) -> Option<&'static str> {
        match self {
            Self::ColdStart => Some("cold_start"),
            Self::InSession => Some("in_session"),
            Self::Refactor => Some("cross_crate_refactor"),
            Self::None => None,
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

/// Role-axis classification for a workspace file. Orthogonal to [`SectionPriority`]
/// (which expresses importance) and [`LoadTier`] (which expresses load timing).
///
/// Precedence order (when assembled): `Identity` → `SoulPersona` → `OperatorProfile` →
/// `Prosoche` → `Team` → `Goals` → `SkillsAlways` → `SkillsLazyIndex` → `Tools` →
/// `Checklist` → `Memory` → `Context`.
///
/// External design prior: HKUDS/DeepTutor `BOOTSTRAP_FILES` order.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BootstrapSlot {
    /// Agent's identity record — name, emoji, avatar metadata. (IDENTITY.md)
    Identity = 0,
    /// Workspace-local persona — operator-curated, per-instance. (SOUL.md)
    SoulPersona = 1,
    /// Operator's profile — what the operator brings, attested. (USER.md)
    OperatorProfile = 2,
    /// Heartbeat / attention checklist. (PROSOCHE.md)
    Prosoche = 3,
    /// Team topology — who else is in the workspace. (AGENTS.md)
    Team = 4,
    /// Active / completed / deferred goals. (GOALS.md)
    Goals = 5,
    /// Always-injected skill bodies. (from knowledge store)
    SkillsAlways = 6,
    /// Lazy skill index — one-line summaries for on-demand loading.
    SkillsLazyIndex = 7,
    /// Registered tool surface. (TOOLS.md)
    Tools = 8,
    /// Work procedures / checklist. (CHECKLIST.md)
    Checklist = 9,
    /// Operational memory — accumulated knowledge over time. (MEMORY.md)
    Memory = 10,
    /// Runtime config / auto-generated context. (CONTEXT.md)
    Context = 11,
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
    /// Role slot — precedence axis orthogonal to priority.
    pub slot: BootstrapSlot,
}

impl BootstrapSection {
    /// Construct a section with all fields specified.
    pub fn new(
        name: String,
        priority: SectionPriority,
        content: String,
        tokens: u64,
        truncatable: bool,
        slot: BootstrapSlot,
    ) -> Self {
        Self {
            name,
            priority,
            content,
            tokens,
            truncatable,
            slot,
        }
    }
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

impl BootstrapResult {
    /// Construct a result with all fields specified.
    pub fn new(
        system_prompt: String,
        sections_included: Vec<String>,
        sections_truncated: Vec<String>,
        sections_dropped: Vec<String>,
        sections_filtered: Vec<String>,
        total_tokens: u64,
        task_hint: TaskHint,
    ) -> Self {
        Self {
            system_prompt,
            sections_included,
            sections_truncated,
            sections_dropped,
            sections_filtered,
            total_tokens,
            task_hint,
        }
    }
}

/// Workspace file specification for cascade resolution.
struct WorkspaceFileSpec {
    filename: &'static str,
    priority: SectionPriority,
    truncatable: bool,
    /// Whether this file loads unconditionally or based on task hint.
    load_tier: LoadTier,
    /// Role slot for precedence ordering.
    slot: BootstrapSlot,
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
        slot: BootstrapSlot::SoulPersona,
    },
    // WHY(#4109): voice-exemplar cueing — inject a sample of the operator's
    // communication style near the top of the context to anchor model output
    // style from the first token. VOICE.md is scaffolded by `aletheia init`
    // and may contain writing examples, anti-patterns, and style notes.
    // Optional: silently absent when the file does not exist.
    WorkspaceFileSpec {
        filename: "VOICE.md",
        priority: SectionPriority::Optional,
        truncatable: true,
        load_tier: LoadTier::Always,
        slot: BootstrapSlot::SoulPersona,
    },
    WorkspaceFileSpec {
        filename: "USER.md",
        priority: SectionPriority::Important,
        truncatable: false,
        load_tier: LoadTier::Always,
        slot: BootstrapSlot::OperatorProfile,
    },
    // --- Conditionally-loaded (operational tier) ---
    WorkspaceFileSpec {
        filename: "AGENTS.md",
        priority: SectionPriority::Important,
        truncatable: false,
        load_tier: LoadTier::Conditional,
        slot: BootstrapSlot::Team,
    },
    WorkspaceFileSpec {
        filename: "GOALS.md",
        priority: SectionPriority::Important,
        truncatable: true,
        load_tier: LoadTier::Conditional,
        slot: BootstrapSlot::Goals,
    },
    WorkspaceFileSpec {
        filename: "TOOLS.md",
        priority: SectionPriority::Important,
        truncatable: true,
        load_tier: LoadTier::Conditional,
        slot: BootstrapSlot::Tools,
    },
    WorkspaceFileSpec {
        filename: "CHECKLIST.md",
        priority: SectionPriority::Flexible,
        truncatable: true,
        load_tier: LoadTier::Conditional,
        slot: BootstrapSlot::Checklist,
    },
    WorkspaceFileSpec {
        filename: "MEMORY.md",
        priority: SectionPriority::Flexible,
        truncatable: true,
        load_tier: LoadTier::Conditional,
        slot: BootstrapSlot::Memory,
    },
    // --- Always-loaded (identity tier, continued) ---
    WorkspaceFileSpec {
        filename: "IDENTITY.md",
        priority: SectionPriority::Flexible,
        truncatable: false,
        load_tier: LoadTier::Always,
        slot: BootstrapSlot::Identity,
    },
    WorkspaceFileSpec {
        filename: "PROSOCHE.md",
        priority: SectionPriority::Flexible,
        truncatable: false,
        load_tier: LoadTier::Always,
        slot: BootstrapSlot::Prosoche,
    },
    // --- Conditionally-loaded (operational tier, continued) ---
    WorkspaceFileSpec {
        filename: "CONTEXT.md",
        priority: SectionPriority::Flexible,
        truncatable: true,
        load_tier: LoadTier::Conditional,
        slot: BootstrapSlot::Context,
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
struct LlmManifest {
    #[serde(default)]
    #[expect(
        dead_code,
        reason = "deserialized for schema completeness; not yet consumed"
    )]
    version: u32,
    #[serde(default)]
    levels: HashMap<String, LlmLevel>,
    #[serde(default)]
    crates: Vec<LlmManifestCrate>,
}

#[derive(Debug, Clone, Deserialize)]
struct LlmLevel {
    path: String,
    #[serde(default)]
    #[expect(
        dead_code,
        reason = "deserialized for schema completeness; not yet consumed"
    )]
    generator: String,
    #[serde(default)]
    #[expect(
        dead_code,
        reason = "deserialized for schema completeness; not yet consumed"
    )]
    source_hash_algorithm: String,
    #[serde(default)]
    #[expect(
        dead_code,
        reason = "deserialized for schema completeness; not yet consumed"
    )]
    source_hash_version: u32,
}

#[derive(Debug, Clone, Deserialize)]
struct LlmManifestCrate {
    name: String,
    /// Path to the crate source directory, relative to the workspace root (oikos root).
    path: String,
    /// SHA-256 of all `.rs` source files in sorted crate-relative POSIX path order,
    /// with each path prepended to its file bytes; used to detect stale generated context.
    source_hash: String,
    #[expect(
        dead_code,
        reason = "token estimate preserved for future budget-aware selection; not yet consumed"
    )]
    l3_token_estimate: u64,
}

/// Compute the SHA-256 source hash for a crate directory.
///
/// Walks all `.rs` files under `crate_dir`, excluding any `target/`
/// directory, sorted by crate-relative POSIX path. For each file the hasher is
/// updated with the UTF-8 bytes of the relative path followed by the raw file
/// bytes, matching the algorithm used by `scripts/llm-extract-l3.py` when it
/// writes `manifest.toml`. Returns the lowercase hex string, or `None` when the
/// directory cannot be read or contains no `.rs` files.
///
/// WHY: The manifest records this hash as a staleness guard. Comparing the
/// live hash against the manifest entry lets the bootstrap assembler skip L3
/// sections whose source has diverged from the generated content.
async fn compute_crate_source_hash(crate_dir: &Path) -> Option<String> {
    let mut rs_paths: Vec<PathBuf> = Vec::new();
    collect_rs_files(crate_dir, &mut rs_paths);
    if rs_paths.is_empty() {
        return None;
    }

    let mut rs_files: Vec<(String, PathBuf)> = Vec::with_capacity(rs_paths.len());
    for path in rs_paths {
        let rel_path = path.strip_prefix(crate_dir).ok()?;
        let rel_posix = rel_path
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        rs_files.push((rel_posix, path));
    }
    rs_files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = Sha256::new();
    for (rel_posix, path) in &rs_files {
        hasher.update(rel_posix.as_bytes());
        match tokio::fs::read(path).await {
            Ok(bytes) => hasher.update(&bytes),
            Err(e) => {
                warn!(path = %path.display(), error = %e, "failed to read source file for hash check");
                return None;
            }
        }
    }
    let digest = hasher.finalize();
    let hex: String = digest
        .iter()
        .flat_map(|b| {
            [
                char::from_digit(u32::from(b >> 4), 16).unwrap_or('0'),
                char::from_digit(u32::from(b & 0x0f), 16).unwrap_or('0'),
            ]
        })
        .collect();
    Some(hex)
}

/// Collect all `.rs` files under `dir` recursively into `out`.
///
/// Uses synchronous directory traversal; intended to be called once per
/// validation check. Non-readable directories are silently skipped.
/// `target/` directories are excluded to match `scripts/llm-extract-l3.py`.
fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().and_then(|name| name.to_str()) == Some("target") {
                continue;
            }
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
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
    /// When true, resolve only this nous's workspace files and skip shared/theke fallback.
    private_workspace: bool,
    /// When true, pre-injection scan failures are fatal. Defaults to the
    /// `KOINA_PREINJECT_SCAN_STRICT` env var (false when absent).
    preinject_strict: bool,
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
            private_workspace: false,
            preinject_strict: preinject_scan::strict_mode(),
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
            private_workspace: false,
            preinject_strict: preinject_scan::strict_mode(),
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

    /// Restrict workspace resolution to this nous's own directory.
    ///
    /// Private nouses still read their own bootstrap files, but do not borrow
    /// cross-nous discovery sources from `shared/` or `theke/`.
    #[must_use]
    pub fn with_private_workspace(mut self, private_workspace: bool) -> Self {
        self.private_workspace = private_workspace;
        self
    }

    /// Set whether pre-injection scan failures are fatal.
    ///
    /// When `true`, a workspace file that fails the invisible-Unicode or
    /// threat-pattern scan causes bootstrap assembly to return an error.
    /// When `false` (default), the file is logged and skipped.
    #[must_use]
    pub fn with_preinject_strict(mut self, strict: bool) -> Self {
        self.preinject_strict = strict;
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

        let llm_sections = self.resolve_llm_sections(recipe).await?;
        sections.extend(llm_sections);

        // WHY(#5829): Allocate budget by priority so Required sections are
        // protected even when their slot comes after Flexible sections.
        // Final prompt assembly re-sorts by slot below.
        sections.sort_by_key(|s| s.priority);

        let mut included: Vec<BootstrapSection> = Vec::new();
        let mut truncated_names: Vec<String> = Vec::new();
        let mut dropped_names: Vec<String> = Vec::new();

        for section in sections {
            if budget.can_fit(section.tokens) {
                budget.consume(section.tokens);
                included.push(section);
            } else if section.priority == SectionPriority::Required {
                // WHY(#4109): Required sections (SOUL.md) are the primary identity
                // channel. Including them even when the budget is exhausted prevents
                // persona drift when the system prompt is compressed under token
                // pressure. An over-budget Required section is always better than a
                // missing one — the model cannot know who it is without it.
                // WHY(#4623): force_consume tracks the over-budget debt so that
                // downstream stages (history, recall) see the accurate remaining budget
                // via adjusted_history_budget() rather than an artificially inflated one.
                budget.force_consume(section.tokens);
                warn!(
                    section = section.name,
                    tokens = section.tokens,
                    remaining = budget.remaining(),
                    "required section included despite budget exhaustion"
                );
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

        // WHY(#5829): Prompt output stays in slot order; only budget allocation
        // uses priority as the primary axis.
        included.sort_by_key(|s| (s.slot, s.priority));

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

        let system_prompt = organon::interp::expand_file_refs(&system_prompt, self.oikos.root())
            .map_err(|e| crate::error::InterpSnafu.into_error(e))?;

        // WHY(#4623): file-ref expansion can grow the prompt beyond the pre-expansion
        // token estimate. Re-estimate the actual prompt size and force-consume any
        // extra tokens so that downstream stages (history, recall) see the true
        // remaining budget via adjusted_history_budget().
        let expanded_tokens = self.estimator.estimate(&system_prompt);
        let pre_expansion_consumed = budget.consumed();
        if expanded_tokens > pre_expansion_consumed {
            let expansion_debt = expanded_tokens - pre_expansion_consumed;
            budget.force_consume(expansion_debt);
            warn!(
                expansion_debt,
                expanded_tokens,
                pre_expansion_consumed,
                "file-ref expansion exceeded pre-expansion token estimate; carrying debt forward"
            );
        }

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

            let Some(p) = self.resolve_workspace_path(nous_id, spec.filename) else {
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
                if let Err(e) = preinject_scan::scan_workspace_content(&content, &p) {
                    if self.preinject_strict {
                        return Err(error::ContextAssemblySnafu {
                            message: format!("pre-injection scan failed for {}: {e}", p.display()),
                        }
                        .build());
                    }
                    warn!(
                        file = spec.filename,
                        path = %p.display(),
                        error = %e,
                        "pre-injection scan rejected workspace file, skipping"
                    );
                    continue;
                }
                sections.push(BootstrapSection {
                    name: spec.filename.to_owned(),
                    priority: spec.priority,
                    content,
                    tokens,
                    truncatable: spec.truncatable,
                    slot: spec.slot,
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
                    if let Err(e) = preinject_scan::scan_workspace_content(&content, &p) {
                        if self.preinject_strict {
                            return Err(error::ContextAssemblySnafu {
                                message: format!(
                                    "pre-injection scan failed for {}: {e}",
                                    p.display()
                                ),
                            }
                            .build());
                        }
                        warn!(
                            file = spec.filename,
                            path = %p.display(),
                            error = %e,
                            "pre-injection scan rejected workspace file, skipping"
                        );
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
                        slot: spec.slot,
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
            slot: BootstrapSlot::Context,
        });
        debug!("injected output-style section ({style_tokens} tokens)");

        Ok((sections, filtered))
    }

    fn resolve_workspace_path(&self, nous_id: &str, filename: &str) -> Option<PathBuf> {
        if self.private_workspace {
            let path = self.oikos.nous_dir(nous_id).join(filename);
            return path.exists().then_some(path);
        }

        cascade::resolve(self.oikos, nous_id, filename, None)
    }

    /// Resolve `_llm/` content into bootstrap sections based on the recipe.
    ///
    /// When `_llm/recipes.toml` exists and declares a recipe matching the
    /// selected [`LlmRecipe`], the assembler loads only the exact files listed
    /// by that recipe, expanding directories to their immediate `.md`/`.toml`
    /// children. Otherwise it falls back to the legacy root sweep used before
    /// recipe wiring.
    ///
    /// `_llm/manifest.toml` is required in either mode: it declares the L3
    /// index path and supplies per-crate source hashes for the #5404 staleness
    /// guard. Each loaded file is run through the pre-injection scan (#5409).
    ///
    /// Returns an empty vec when:
    /// - the recipe is [`LlmRecipe::None`]
    /// - `_llm/manifest.toml` does not exist
    /// - any I/O or parse error occurs (logged, not propagated)
    async fn resolve_llm_sections(&self, recipe: LlmRecipe) -> Result<Vec<BootstrapSection>> {
        if recipe == LlmRecipe::None {
            return Ok(Vec::new());
        }

        let llm_root = self.oikos.root().join("_llm");
        let manifest_path = llm_root.join("manifest.toml");

        if !manifest_path.exists() {
            debug!(path = %manifest_path.display(), "_llm/manifest.toml not found, skipping");
            return Ok(Vec::new());
        }

        let manifest_raw = match tokio::fs::read_to_string(&manifest_path).await {
            Ok(r) => r,
            Err(e) => {
                warn!(
                    path = %manifest_path.display(),
                    error = %e,
                    "failed to read _llm/manifest.toml, skipping"
                );
                return Ok(Vec::new());
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
                return Ok(Vec::new());
            }
        };

        let crate_hash_index: HashMap<String, (String, String)> = manifest
            .crates
            .iter()
            .map(|c| (c.name.clone(), (c.path.clone(), c.source_hash.clone())))
            .collect();

        // NOTE: prefer exact recipe paths when the registry is available.
        let recipe_files: Option<Vec<RecipeFile>> = if let Some(name) = recipe.recipe_name() {
            let recipes_path = llm_root.join("recipes.toml");
            if recipes_path.exists() {
                match RecipeRegistry::load_from_file(&recipes_path) {
                    Ok(registry) => match registry.resolve_files(name, &HashMap::new()) {
                        Ok(files) => {
                            debug!(
                                recipe = name,
                                file_count = files.len(),
                                "loaded exact _llm recipe"
                            );
                            Some(files)
                        }
                        Err(e) => {
                            warn!(
                                recipe = name,
                                error = %e,
                                "failed to resolve recipe files, falling back to legacy sweep"
                            );
                            None
                        }
                    },
                    Err(e) => {
                        warn!(
                            path = %recipes_path.display(),
                            error = %e,
                            "failed to load recipes registry, falling back to legacy sweep"
                        );
                        None
                    }
                }
            } else {
                None
            }
        } else {
            None
        };

        if let Some(files) = recipe_files {
            self.resolve_llm_recipe_files(recipe, &llm_root, &manifest, &crate_hash_index, files)
                .await
        } else {
            self.resolve_llm_legacy_sweep(recipe, &llm_root, &manifest, &crate_hash_index)
                .await
        }
    }

    /// Load the exact file list produced by a recipe from `_llm/recipes.toml`.
    async fn resolve_llm_recipe_files(
        &self,
        recipe: LlmRecipe,
        llm_root: &Path,
        manifest: &LlmManifest,
        crate_hash_index: &HashMap<String, (String, String)>,
        files: Vec<RecipeFile>,
    ) -> Result<Vec<BootstrapSection>> {
        let mut sections = Vec::new();
        let l3_dir = manifest.levels.get("L3").map(|l| llm_root.join(&l.path));

        for file in files {
            // NOTE: instructions and L4 paths belong to the workspace cascade
            // or on-demand tooling, not the bootstrap system prompt.
            if !file.path.starts_with("_llm/") {
                debug!(path = %file.path, "skipping non-_llm recipe path");
                continue;
            }
            let basename = Path::new(&file.path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            if basename == "manifest.toml" || basename == "recipes.toml" {
                continue;
            }

            let resolved_files = {
                let full_path = self.oikos.root().join(&file.path);
                if full_path.is_dir() {
                    self.expand_recipe_directory(&file).await
                } else {
                    vec![file]
                }
            };

            for rf in resolved_files {
                let full_path = self.oikos.root().join(&rf.path);
                let content = match tokio::fs::read_to_string(&full_path).await {
                    Ok(raw) => {
                        let content = raw.trim().to_owned();
                        if content.is_empty() {
                            continue;
                        }
                        content
                    }
                    Err(e) => {
                        warn!(
                            path = %full_path.display(),
                            error = %e,
                            "failed to read recipe _llm file, skipping"
                        );
                        continue;
                    }
                };

                if let Err(e) = preinject_scan::scan_workspace_content(&content, &full_path) {
                    if self.preinject_strict {
                        return Err(error::ContextAssemblySnafu {
                            message: format!(
                                "pre-injection scan failed for {}: {e}",
                                full_path.display()
                            ),
                        }
                        .build());
                    }
                    warn!(
                        path = %full_path.display(),
                        error = %e,
                        "pre-injection scan rejected recipe _llm file, skipping"
                    );
                    continue;
                }

                // WHY(#5404): stale L3 files must not enter the prompt.
                if let Some(ref l3) = l3_dir
                    && full_path.parent() == Some(l3.as_path())
                {
                    let filename = full_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    let crate_name = filename.trim_end_matches(".md");
                    if let Some((crate_path, expected_hash)) = crate_hash_index.get(crate_name) {
                        let crate_dir = self.oikos.root().join(crate_path);
                        if let Some(actual_hash) = compute_crate_source_hash(&crate_dir).await
                            && actual_hash != *expected_hash
                        {
                            warn!(
                                section = %rf.path,
                                crate_name,
                                "stale L3 section skipped: source hash mismatch \
                                 (regenerate: uv run scripts/llm-extract-l3.py)"
                            );
                            continue;
                        }
                    }
                }

                let (priority, truncatable) = Self::priority_for_recipe_level(recipe, &rf.level);
                let tokens = self.estimator.estimate(&content);
                sections.push(BootstrapSection {
                    name: rf.path,
                    priority,
                    content,
                    tokens,
                    truncatable,
                    slot: BootstrapSlot::Context,
                });
            }
        }

        Ok(sections)
    }

    /// Expand a recipe directory entry to its immediate children.
    async fn expand_recipe_directory(&self, file: &RecipeFile) -> Vec<RecipeFile> {
        let full_path = self.oikos.root().join(&file.path);
        let mut out = Vec::new();
        let Ok(mut entries) = tokio::fs::read_dir(&full_path).await else {
            return out;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !matches!(ext, "md" | "toml") {
                continue;
            }
            let rel = path.strip_prefix(self.oikos.root()).map_or_else(
                |_| format!("{}/{name}", file.path),
                |p| p.to_string_lossy().to_string(),
            );
            out.push(RecipeFile {
                level: file.level.clone(),
                path: rel,
                note: file.note.clone(),
            });
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        out
    }

    /// Map a recipe/level pair to bootstrap priority and truncation settings.
    fn priority_for_recipe_level(recipe: LlmRecipe, level: &str) -> (SectionPriority, bool) {
        let is_l1 = level.eq_ignore_ascii_case("L1");
        let is_l3 = level.eq_ignore_ascii_case("L3");
        match recipe {
            LlmRecipe::ColdStart => {
                if is_l1 {
                    (SectionPriority::Required, false)
                } else if is_l3 {
                    (SectionPriority::Optional, true)
                } else {
                    (SectionPriority::Important, true)
                }
            }
            LlmRecipe::InSession | LlmRecipe::None => (SectionPriority::Optional, true),
            LlmRecipe::Refactor => (SectionPriority::Important, true),
        }
    }

    /// Legacy root sweep used when `_llm/recipes.toml` is missing or does not
    /// declare the selected recipe.
    #[expect(
        clippy::too_many_lines,
        reason = "preserves pre-#5406 sweep behavior as a fallback"
    )]
    async fn resolve_llm_legacy_sweep(
        &self,
        recipe: LlmRecipe,
        llm_root: &Path,
        manifest: &LlmManifest,
        crate_hash_index: &HashMap<String, (String, String)>,
    ) -> Result<Vec<BootstrapSection>> {
        let mut sections = Vec::new();
        let (l1_priority, l1_truncatable) = Self::priority_for_recipe_level(recipe, "L1");
        let (l3_priority, l3_truncatable) = Self::priority_for_recipe_level(recipe, "L3");

        // --- L1: workspace manifest files at _llm/ root ---
        if let Ok(mut entries) = tokio::fs::read_dir(llm_root).await {
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
                        if let Err(e) = preinject_scan::scan_workspace_content(&content, &path) {
                            if self.preinject_strict {
                                return Err(error::ContextAssemblySnafu {
                                    message: format!(
                                        "pre-injection scan failed for {}: {e}",
                                        path.display()
                                    ),
                                }
                                .build());
                            }
                            warn!(
                                path = %path.display(),
                                error = %e,
                                "pre-injection scan rejected _llm file, skipping"
                            );
                            continue;
                        }
                        let tokens = self.estimator.estimate(&content);
                        sections.push(BootstrapSection {
                            name: format!("_llm/{name}"),
                            priority: l1_priority,
                            content,
                            tokens,
                            truncatable: l1_truncatable,
                            slot: BootstrapSlot::Context,
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

                    let crate_name = name.trim_end_matches(".md");

                    if let Some((crate_path, expected_hash)) = crate_hash_index.get(crate_name) {
                        let crate_dir = self.oikos.root().join(crate_path);
                        if let Some(actual_hash) = compute_crate_source_hash(&crate_dir).await
                            && actual_hash != *expected_hash
                        {
                            warn!(
                                section = format!("_llm/{}/{name}", l3_level.path),
                                crate_name,
                                "stale L3 section skipped: source hash mismatch \
                                     (regenerate: uv run scripts/llm-extract-l3.py)"
                            );
                            continue;
                        }
                    }

                    match tokio::fs::read_to_string(&path).await {
                        Ok(raw) => {
                            let content = raw.trim().to_owned();
                            if content.is_empty() {
                                continue;
                            }
                            if let Err(e) = preinject_scan::scan_workspace_content(&content, &path)
                            {
                                if self.preinject_strict {
                                    return Err(error::ContextAssemblySnafu {
                                        message: format!(
                                            "pre-injection scan failed for {}: {e}",
                                            path.display()
                                        ),
                                    }
                                    .build());
                                }
                                warn!(
                                    path = %path.display(),
                                    error = %e,
                                    "pre-injection scan rejected L3 index file, skipping"
                                );
                                continue;
                            }
                            let tokens = self.estimator.estimate(&content);
                            sections.push(BootstrapSection {
                                name: format!("_llm/{}/{name}", l3_level.path),
                                priority: l3_priority,
                                content,
                                tokens,
                                truncatable: l3_truncatable,
                                slot: BootstrapSlot::Context,
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

        Ok(sections)
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
            slot: section.slot,
        }
    }

    /// Line-by-line truncation fallback.
    ///
    /// Keeps the **newest** (last) lines that fit within the budget, dropping
    /// the oldest. A truncation marker is prepended to indicate removed content.
    ///
    /// Line-by-line is coarser than header-based truncation but handles
    /// files without markdown structure. Still prefers newest content.
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
                slot: section.slot,
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
            slot: section.slot,
        }
    }
}

/// Extract the `## Communication` section from USER.md content.
///
/// Looks for a `## Communication` or `## Output` heading and returns
/// everything up to the next `## ` heading (or end of content). Returns
/// `None` if no such section exists.
fn extract_output_style(user_content: &str) -> Option<String> {
    let mut offset = 0;
    for line in user_content.split_inclusive('\n') {
        offset += line.len();
        if !is_output_style_heading(line_without_ending(line)) {
            continue;
        }

        let section_body_start = offset;
        let end = next_h2_heading_start(user_content, section_body_start);

        let body = user_content.get(section_body_start..end)?.trim();
        return if body.is_empty() {
            None
        } else {
            Some(body.to_owned())
        };
    }

    None
}

fn is_output_style_heading(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.starts_with("## communication") || lower.starts_with("## output")
}

fn line_without_ending(line: &str) -> &str {
    let line = line.strip_suffix('\n').unwrap_or(line);
    line.strip_suffix('\r').unwrap_or(line)
}

fn next_h2_heading_start(user_content: &str, start: usize) -> usize {
    let mut offset = start;
    let Some(after_start) = user_content.get(start..) else {
        return user_content.len();
    };

    for line in after_start.split_inclusive('\n') {
        if line_without_ending(line).starts_with("## ") {
            return offset;
        }
        offset += line.len();
    }

    user_content.len()
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
    let research = score_research_keywords(&lower);
    let planning = score_keywords(&lower, aletheia_lexica::keywords::PLANNING_KEYWORDS);

    let max = coding.max(research).max(planning);
    if max == 0 {
        return TaskHint::General;
    }
    if coding == 0
        && planning == 0
        && research == 1
        && lower.split_whitespace().next() == Some("what")
    {
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

fn score_research_keywords(text: &str) -> usize {
    let mut score = score_keywords(text, aletheia_lexica::keywords::RESEARCH_KEYWORDS);
    if score == 1 && text.contains("what") {
        score = 0;
    }
    score
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
                slot: BootstrapSlot::Context,
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "bootstrap_tests/mod.rs"]
mod tests;

#[cfg(test)]
mod preinject_tests;
