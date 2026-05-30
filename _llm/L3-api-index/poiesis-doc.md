# L3 API Index: poiesis-doc

Crate path: `crates/poiesis/doc`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/error.rs`

```rust
pub enum Error {
    /// The input JSON did not match the expected render schema.
    #[snafu(display("malformed input: {detail}"))]
    MalformedInput {
        /// Human-readable description of the schema violation.
        detail: String,
    },

    /// DOCX generation failed.
    #[snafu(display("docx build failed: {detail}"))]
    BuildDocx {
        /// Human-readable description of the build failure.
        detail: String,
    },

    /// ZIP inspection failed.
    #[snafu(display("zip read failed: {source}"))]
    ReadZip {
        /// Underlying ZIP error.
        source: zip::result::ZipError,
    },

    /// XML parsing during inspection failed.
    #[snafu(display("xml parse failed: {source}"))]
    ParseXml {
        /// Underlying quick-xml error.
        source: quick_xml::Error,
    },

    /// PDF rendering via Typst failed.
    #[snafu(display("pdf render failed: {detail}"))]
    PdfRenderFailed {
        /// Human-readable description.
        detail: String,
    },

    /// The requested format requires Pandoc, which is not yet available.
    #[snafu(display("{format} output requires Pandoc (coming in B-012); use pdf or xlsx for now"))]
    PandocRequired {
        /// The requested format name (e.g. "odt").
        format: String,
    },
}
```

## `src/lib.rs`

> Result alias for poiesis-doc operations.
```rust
pub type Result<T, E = Error> = std::result::Result<T, E>;
```

```rust
pub struct DocxSummary {
    /// Plain-text content of each paragraph, in document order.
    pub paragraphs: Vec<String>,
}
```

```rust
impl DocxSummary {
    pub fn new (paragraphs: Vec<String>) -> Self;
}
```

```rust
pub fn render_docx (data: &Value) -> Result<Vec<u8>>
```

> Inspect an in-memory DOCX file and extract paragraph text.
> 
> Reads `word/document.xml` from the ZIP archive and collects the
> concatenated text inside each `<w:t>` element, grouping by `<w:p>`.
> 
> # Errors
> 
> Returns [`Error::ReadZip`] if the bytes are not a valid ZIP archive,
> or [`Error::ParseXml`] if `document.xml` cannot be parsed.
```rust
pub fn inspect_docx (bytes: &[u8]) -> Result<DocxSummary>
```

```rust
pub fn render_pdf_from_doc (doc: &poiesis_core::Document) -> Result<Vec<u8>>
```

> Render a [`poiesis_core::Document`] to ODT bytes.
> 
> ODT output requires the Pandoc backend (B-012) which has not landed yet.
> Returns [`Error::PandocRequired`] until B-012 ships.
> 
> # Errors
> 
> Always returns [`Error::PandocRequired`] in this stub implementation.
```rust
pub fn render_odt_from_doc (_doc: &poiesis_core::Document) -> Result<Vec<u8>>
```
