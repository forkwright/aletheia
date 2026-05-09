# L3 API Index: poiesis-inspect

Crate path: `crates/poiesis/inspect`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/error.rs`

> Result type for poiesis-inspect operations.
```rust
pub type Result<T> = std::result::Result<T, InspectError>;
```

```rust
pub enum InspectError {
    /// Failed to parse ZIP archive (XLSX/PPTX format).
    #[snafu(display("failed to parse ZIP archive: {source}"))]
    ZipError {
        /// Source error from the zip crate.
        source: zip::result::ZipError,
    },

    /// Failed to extract text from PDF.
    #[snafu(display("failed to extract PDF text: {detail}"))]
    PdfExtractionError {
        /// Details about the PDF extraction error.
        detail: String,
    },

    /// Invalid file format (not a valid PDF, XLSX, or PPTX).
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
pub struct PdfSummary {
    /// Number of pages in the PDF.
    pub pages: usize,
    /// Extracted text snippets from each page.
    pub text_snippets: Vec<String>,
}
```

```rust
pub struct WorkbookSummary {
    /// Sheet names and their extracted text content, in workbook order.
    pub sheets: indexmap::IndexMap<String, String>,
}
```

```rust
pub struct PresentationSummary {
    /// Slide text content (indexed by slide number).
    pub slides: Vec<String>,
}
```

```rust
pub fn inspect_pdf (bytes: &[u8]) -> Result<PdfSummary>
```

```rust
pub fn inspect_xlsx (bytes: &[u8]) -> Result<WorkbookSummary>
```

```rust
pub fn inspect_pptx (bytes: &[u8]) -> Result<PresentationSummary>
```
