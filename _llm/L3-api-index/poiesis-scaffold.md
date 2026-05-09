# L3 API Index: poiesis-scaffold

Crate path: `crates/poiesis/scaffold`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/lib.rs`

```rust
pub enum Error {
    /// The provided slug was empty.
    #[snafu(display("slug must not be empty"))]
    EmptySlug,

    /// Failed to generate the XLSX scaffold.
    #[snafu(display("failed to generate xlsx scaffold: {source}"))]
    XlsxRender {
        /// Underlying renderer error.
        source: poiesis_sheet::xlsx::XlsxRendererError,
    },
}
```

```rust
pub enum Format {
    /// Typst-only scaffold.
    Typst,
    /// XLSX-only scaffold.
    Xlsx,
    /// Both Typst and XLSX scaffolds.
    Both,
}
```

```rust
pub struct ScaffoldFile {
    /// Relative path within the project directory.
    pub path: PathBuf,
    /// File contents.
    pub contents: Vec<u8>,
}
```

```rust
impl ScaffoldFile {
    pub fn new (path: impl Into<PathBuf>, contents: impl Into<Vec<u8>>) -> Self;
}
```

```rust
pub fn scaffold_report (
    slug: &str,
    description: &str,
    format: Format,
    confidential: bool,
) -> Result<Vec<ScaffoldFile>, Error>
```
