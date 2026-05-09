# L3 API Index: poiesis-diff

Crate path: `crates/poiesis/diff`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/error.rs`

> Result type for poiesis-diff operations.
```rust
pub type Result<T> = std::result::Result<T, DiffError>;
```

```rust
pub enum DiffError {
    /// Failed to parse ZIP archive (XLSX/PPTX format).
    #[snafu(display("failed to parse ZIP archive: {source}"))]
    ZipError {
        /// Source error from the zip crate.
        source: zip::result::ZipError,
    },

    /// Invalid file format (not a valid XLSX or PPTX).
    #[snafu(display("invalid file format: {detail}"))]
    InvalidFormat {
        /// Details about why the file is invalid.
        detail: String,
    },

    /// IO error while reading document.
    #[snafu(display("IO error: {source}"))]
    Io {
        /// Source IO error.
        source: std::io::Error,
    },
}
```

## `src/lib.rs`

```rust
pub struct CellDiff {
    /// Sheet name where the difference occurs.
    pub sheet: String,
    /// Row index (0-based).
    pub row: u32,
    /// Column index (0-based).
    pub col: u32,
    /// Content before the change (None if inserted).
    pub before: Option<String>,
    /// Content after the change (None if deleted).
    pub after: Option<String>,
}
```

```rust
pub struct SlideDiff {
    /// Slide index (0-based).
    pub slide_index: usize,
    /// Text content before the change.
    pub before: Option<String>,
    /// Text content after the change.
    pub after: Option<String>,
}
```

```rust
pub fn diff_workbooks (a: &[u8], b: &[u8]) -> Result<Vec<CellDiff>>
```

```rust
pub fn diff_presentations (a: &[u8], b: &[u8]) -> Result<Vec<SlideDiff>>
```
