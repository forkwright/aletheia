# L3 API Index: poiesis-lint

Crate path: `crates/poiesis/lint`

Public API signatures extracted from source. Doc comments shown above each signature.
For implementation context, read the source directly (`L4`).

## `src/error.rs`

```rust
pub enum LintError {
    /// Failed to read the input file.
    #[snafu(display("failed to read file {path:?}: {source}"))]
    ReadFile {
        /// Path that could not be read.
        path: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// Failed to write fixed content back to the file.
    #[snafu(display("failed to write file {path:?}: {source}"))]
    WriteFile {
        /// Path that could not be written.
        path: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// Failed to serialize findings to JSON.
    #[snafu(display("failed to serialize findings: {source}"))]
    Serialize {
        /// Underlying serde error.
        source: serde_json::Error,
    },
}
```

## `src/lib.rs`

```rust
pub struct Finding {
    /// 1-indexed first line of the finding.
    pub line_start: usize,
    /// 1-indexed last line of the finding (same as `line_start` for single-line findings).
    pub line_end: usize,
    /// Human-readable description of the issue.
    pub message: String,
    /// Category of this finding.
    pub kind: FindingKind,
    /// Auto-fix data, if this finding can be fixed automatically.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<LineFix>,
}
```

```rust
pub enum FindingKind {
    /// A banned word or phrase was found.
    BannedWord,
    /// A table or data display lacks a nearby citation.
    MissingCitation,
    /// An AI structural tell (e.g. transition density) was detected.
    StructuralPattern,
    /// A required section (lead or closing) is absent.
    RequiredSectionMissing,
    /// A heading exceeds the allowed length.
    HeaderLength,
}
```

```rust
pub struct LineFix {
    /// 1-indexed line number where the match occurred.
    pub line_number: usize,
    /// The original matched text (may differ in case from the pattern).
    pub matched: String,
    /// Replacement text to write back.
    pub replacement: String,
}
```

```rust
pub struct LintConfig {
    /// Enable banned word checks.
    pub check_banned_words: bool,
    /// Enable citation presence checks.
    pub check_citations: bool,
    /// Enable structural pattern checks.
    pub check_structure: bool,
    /// Enable required section checks.
    pub check_sections: bool,
    /// Enable header length checks.
    pub check_header_length: bool,
    /// Maximum H2 heading length in characters.
    pub max_header_length: usize,
    /// Number of lines before/after a table to search for citations.
    pub citation_window: usize,
}
```

> Stateless report linter. Construct once; call `check` for each document.
```rust
pub struct Linter {
    config: LintConfig,
}
```

```rust
impl Linter {
    pub fn new (config: LintConfig) -> Self;
    pub fn check (&self, text: &str) -> Vec<Finding>;
    pub fn apply_fixes (&self, text: &str, findings: &[Finding]) -> String;
    pub fn check_file (
        &self,
        path: &std::path::Path,
        apply_fix: bool,
    ) -> Result<Vec<Finding>, LintError>;
    pub fn to_json (findings: &[Finding]) -> Result<String, LintError>;
}
```
