# L3 API Index: basanos

Crate path: `crates/basanos`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/commands/audit_component.rs`

```rust
pub struct AuditReport {
    /// Crate name under audit.
    pub crate_name: String,
    /// Overall pass/fail score (0-8).
    pub overall_score: u8,
    /// The 8 checks.
    pub checks: Vec<AuditCheck>,
}
```

```rust
pub struct AuditCheck {
    /// Check identifier: `generic-vs-specific`, `map-vs-angle`, etc.
    pub id: String,
    /// Result: `PASS`, `FAIL`, or `NEEDS_HUMAN`.
    pub result: CheckResult,
    /// Evidence or explanation.
    pub evidence: String,
}
```

```rust
pub enum CheckResult {
    /// Check passed.
    Pass,
    /// Check failed.
    Fail,
    /// Human review needed.
    NeedsHuman,
}
```

> Run the audit component subcommand.
>
> # Arguments
>
> - `crate_name`: the crate to audit (e.g., "eidos")
> - `project_root`: the workspace root (usually ".")
> - `format`: output format ("json" or "markdown")
```rust
pub fn run_audit_component (crate_name: &str, project_root: &str, format: &str) -> Result<String>
```

## `src/commands/mod.rs`

> Run the lint subcommand (the original behavior).
```rust
pub fn run_lint (project_root: &str) -> Result<()>
```

## `src/error.rs`

```rust
pub enum Error {
    /// Failed to read a file.
    #[snafu(display("failed to read file {}", path.display()))]
    ReadFile {
        /// Path to the file.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Failed to scan directory.
    #[snafu(display("failed to read directory {}", path.display()))]
    ReadDir {
        /// Path to the directory.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Lint violations found.
    #[snafu(display("lint violations found"))]
    LintViolations,

    /// Unknown crate.
    #[snafu(display("crate not found: {crate_name}"))]
    UnknownCrate {
        /// The crate name that was not found.
        crate_name: String,
    },

    /// Failed to serialize JSON.
    #[snafu(display("failed to serialize audit report: {source}"))]
    SerializeJson {
        /// The underlying JSON serialization error.
        source: serde_json::Error,
    },
}
```

## `src/rules/api_consistency.rs`

> Rule: API/field-casing
>
> Detect when types in the same crate use both `snake_case` and camelCase serde aliases.
> Example: one struct uses `#[serde(rename = "userId")]` while another uses `#[serde(rename = "user_id")]`.
```rust
pub struct FieldCasingRule;
```

> Rule: API/error-variant-naming
>
> Detect inconsistent error variant naming patterns within the same error enum.
> Example: an error enum that uses both `NotFound` and `ItemDoesNotExist` for similar semantics.
```rust
pub struct ErrorVariantNamingRule;
```

## `src/rules/architecture/fact_required.rs`

> Rule: ARCHITECTURE/fact-required.
>
> Scan crate source files for architectural seams (lib.rs, pub mod
> declarations) and warn when no architecture fact is present for them.
```rust
pub struct FactRequiredRule {
    config: FactRequiredConfig,
}
```

```rust
pub struct FactRequiredConfig {
    /// Whether the policy is active. Defaults to `true`.
    pub enabled: bool,
    /// Directory containing flat JSON architecture facts.
    pub facts_dir: PathBuf,
    /// Prefix used when deriving expected fact IDs.
    pub project_prefix: String,
}
```

```rust
impl FactRequiredRule {
    pub fn with_config (config: FactRequiredConfig) -> Self;
}
```

## `src/rules/derive_vs_declare.rs`

> Rule: STANDARDS/declare-without-derive.
>
> Detects two patterns:
> 1. Health endpoints that return only `"status": "..."` without per-check details.
> 2. Version endpoints that return only `"version": "..."` without build metadata (git sha, timestamp, etc.).
```rust
pub struct DeriveVsDeclareRule;
```

## `src/rules/mod.rs`

```rust
pub struct Violation {
    /// Rule identifier, e.g. `PLANNING/missing-falsifier`.
    pub rule: String,
    /// File path where the violation was found.
    pub path: String,
    /// Approximate line number (1-based).
    pub line: usize,
    /// Human-readable message.
    pub message: String,
}
```

> A lint rule that can be applied to a project tree.
```rust
pub trait Rule {
    fn id (&self) -> &'static str;
    fn check (&self, project_root: &str) -> Result<Vec<Violation>>;
}
```

> All registered rules.
```rust
pub fn all_rules () -> Vec<Box<dyn Rule>>
```

## `src/rules/planning.rs`

> Rule: PLANNING/missing-falsifier.
>
> Ensures every phase PLAN.md has a Falsification section that covers
> all success criteria, and that vision.md / ROADMAP.md do not contain
> unfalsifiable adjectives without measurement.
```rust
pub struct MissingFalsifierRule;
```

## `src/rules/vocabulary.rs`

```rust
pub struct HubWordDisciplineRule {
    hub_words_path: PathBuf,
    disabled_words: HashSet<String>,
}
```

```rust
impl HubWordDisciplineRule {
    pub fn new () -> Self;
    pub fn with_config (path: impl Into<PathBuf>, disabled_words: Vec<String>) -> Self;
}
```

## `src/rules/writing.rs`

> Rule: WRITING/purpose-in-technical-doc.
>
> Detects purpose/vision language in technical documentation that should be
> capability descriptions instead. Exempt files: vision.md, ROADMAP.md.
```rust
pub struct PurposeInTechnicalDocRule;
```

> Rule: WRITING/reference-must-compress.
>
> Detects references that fail the compression test: bare issue numbers,
> bare standards references, and citations without context. Allows references
> that include a brief inline description or appear in reference sections.
```rust
pub struct ReferenceMustCompressRule;
```
