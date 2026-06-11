# L3 API Index: poiesis-typst

Crate path: `crates/poiesis/typst`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/error.rs`

```rust
pub enum PoiesisError {
    /// Data injection produced invalid JSON.
    #[snafu(display("failed to serialize injected data: {detail}"))]
    SerializeData {
        /// Human-readable description of the serialization failure.
        detail: String,
    },

    /// Typst compilation produced errors.
    #[snafu(display("typst compilation failed:\n{diagnostics}"))]
    Compile {
        /// Formatted diagnostics including source locations.
        diagnostics: String,
    },

    /// PDF export failed after successful compilation.
    #[snafu(display("typst PDF export failed:\n{diagnostics}"))]
    PdfExport {
        /// Formatted diagnostics from the PDF exporter.
        diagnostics: String,
    },

    /// A built-in template slug was not recognized.
    #[snafu(display("unknown template slug: {slug:?} (known: {known})"))]
    UnknownTemplate {
        /// The slug that was requested.
        slug: String,
        /// Comma-separated list of known template slugs.
        known: String,
    },
}
```

## `src/lib.rs`

> Result alias for poiesis-typst operations.
```rust
pub type Result<T, E = PoiesisError> = std::result::Result<T, E>;
```

```rust
pub fn render_typst (source: &str, data: &serde_json::Value) -> Result<Vec<u8>>
```

```rust
pub fn render_template (slug: &str, data: &serde_json::Value) -> Result<Vec<u8>>
```

## `src/templates.rs`

> Slug for the default one-page report template.
```rust
pub const DEFAULT: &str = "default";
```

> Slug for the eval-report template.
```rust
pub const EVAL_REPORT: &str = "eval-report";
```

> Slug for the graph-audit template.
```rust
pub const GRAPH_AUDIT: &str = "graph-audit";
```

> List of all known template slugs.
```rust
pub const SLUGS: &[&str] = &[DEFAULT, EVAL_REPORT, GRAPH_AUDIT];
```
