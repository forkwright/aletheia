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

    /// ODT rendering via the clean-room backend failed.
    #[snafu(display("odt render failed: {detail}"))]
    OdtRenderFailed {
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

```rust
pub fn render_odt_from_doc (doc: &poiesis_core::Document) -> Result<Vec<u8>>
```

## `src/pandoc/ast.rs`

> Serialize a `Document` to Pandoc JSON AST bytes.
> 
> The emitted JSON matches Pandoc's `--from json` input format.
> `pandoc-api-version` is pinned to `[1, 24, 1]` (Pandoc 3.x series).
```rust
pub fn document_to_pandoc_json (doc: &Document) -> Vec<u8>
```

## `src/pandoc/error.rs`

```rust
pub enum PandocError {
    /// `pandoc` binary was not found on PATH.
    ///
    /// Install pandoc ≥ 3.0. On NixOS/nix: `nix develop` (pinned in flake).
    /// On Ubuntu/Debian: `apt install pandoc`. See <https://pandoc.org/installing.html>.
    #[snafu(display(
        "pandoc not found (searched: {}). Install pandoc \u{2265} 3.0 \
         — on NixOS run `nix develop` (pinned in flake.nix); \
         on Ubuntu/Debian `apt install pandoc`; see https://pandoc.org/installing.html",
        searched.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
    ))]
    NotInstalled {
        /// Directories and paths that were searched.
        searched: Vec<PathBuf>,
    },

    /// `pandoc` was found but the version is below the minimum requirement.
    #[snafu(display(
        "pandoc at {} is version {found_major}.{found_minor}.{found_patch}, \
         but \u{2265} 3.0 is required. Run `nix develop` or upgrade via https://pandoc.org/installing.html",
        path.display()
    ))]
    VersionTooOld {
        /// Path to the too-old binary.
        path: PathBuf,
        /// Found major version.
        found_major: u32,
        /// Found minor version.
        found_minor: u32,
        /// Found patch version.
        found_patch: u32,
    },

    /// The Pandoc writer returned a non-zero exit code.
    #[snafu(display("pandoc writer failed for format {fmt}: {stderr}"))]
    WriterFailed {
        /// The output format being written.
        fmt: String,
        /// stderr output from the Pandoc process.
        stderr: String,
    },

    /// Pandoc process could not be spawned (I/O error).
    #[snafu(display("failed to spawn pandoc: {source}"))]
    Spawn {
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// Failed to write the AST temp file.
    #[snafu(display("failed to write pandoc AST temp file: {source}"))]
    TempFile {
        /// Underlying I/O error.
        source: std::io::Error,
    },
}
```

## `src/pandoc/mod.rs`

```rust
pub enum OutputFormat {
    /// Microsoft Word DOCX.
    Docx,
    /// `OpenDocument` text (.odt).
    Odt,
    /// PDF — routed to Typst fast-lane by default; `LaTeX` when overridden.
    Pdf,
    /// GitHub-Flavoured Markdown.
    Markdown,
    /// Standalone `LaTeX` source.
    Latex,
    /// Standalone HTML5.
    Html,
    /// EPUB 3.
    Epub,
}
```

```rust
pub enum PdfEngine {
    /// Typst in-process compiler (default, fastest).
    Typst,
    /// `xelatex` — use for math-heavy documents.
    XeLaTeX,
    /// `lualatex` — alternative `LaTeX` engine.
    LuaLaTeX,
}
```

```rust
pub struct DocOpts {
    /// Requested output format.
    pub format: OutputFormat,
    /// Override PDF backend. `None` = Typst (default).
    pub pdf_engine: Option<PdfEngine>,
    /// Paths to Lua filter files to apply (in order).
    pub lua_filters: Vec<PathBuf>,
    /// Path to `--reference-doc` asset (docx/odt theming). `None` = Pandoc default.
    pub reference_doc: Option<PathBuf>,
}
```

```rust
impl DocOpts {
    pub fn default_pdf () -> Self;
    pub fn docx () -> Self;
    pub fn odt () -> Self;
    pub fn markdown () -> Self;
}
```

```rust
pub struct PandocRunner {
    /// Resolved path to the `pandoc` binary.
    bin: PathBuf,
    /// Detected Pandoc version (major.minor.patch).
    pub version: (u32, u32, u32),
}
```

```rust
impl PandocRunner {
    pub fn probe (bin_override: Option<&Path>) -> Result<Self, PandocError>;
    pub fn render (&self, ast_json: &[u8], opts: &DocOpts) -> Result<Vec<u8>, PandocError>;
}
```

```rust
pub fn render_doc (doc: &Document, opts: &DocOpts) -> Result<Vec<u8>, PandocError>
```

## `src/pandoc_probe.rs`

> Minimum Pandoc version required by `poiesis-doc`.
```rust
pub const REQUIRED_PANDOC_VERSION: PandocVersion = (3, 0, 0);
```

> Semantic version triple used by the probe.
```rust
pub type PandocVersion = (u32, u32, u32);
```

```rust
pub enum PandocProbe {
    /// `pandoc` was found and meets the minimum version requirement.
    Present {
        /// Absolute path to the binary that was selected.
        path: PathBuf,
        /// Version reported by `pandoc --version`.
        version: PandocVersion,
    },
    /// `pandoc` could not be found on PATH.
    Missing {
        /// Candidate paths that were searched.
        searched: Vec<PathBuf>,
    },
    /// `pandoc` was found but is too old for this crate.
    TooOld {
        /// Absolute path to the binary that was selected.
        path: PathBuf,
        /// Version reported by `pandoc --version`.
        found: PandocVersion,
        /// Minimum version required by the crate.
        required: PandocVersion,
    },
}
```

```rust
impl PandocProbe {
    pub fn check () -> Self;
    pub fn require (self) -> Result<(), PandocProbeError>;
}
```

```rust
pub enum PandocProbeError {
    /// `pandoc` binary not found on PATH.
    #[snafu(display(
        "pandoc not found (searched: {}). Install pandoc >= 3.0.0; on NixOS run `nix develop`, on Ubuntu/Debian run `apt install pandoc`, on macOS run `brew install pandoc`, or download from https://pandoc.org/installing.html",
        searched
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))]
    NotInstalled {
        /// Candidate paths that were searched.
        searched: Vec<PathBuf>,
    },

    /// `pandoc` was found but the version is below the minimum requirement.
    #[snafu(display(
        "pandoc at {} is version {}, but >= {} is required. Upgrade via https://pandoc.org/installing.html or `nix develop`",
        path.display(),
        format_version(found),
        format_version(required)
    ))]
    VersionTooOld {
        /// Path to the too-old binary.
        path: PathBuf,
        /// Found version.
        found: PandocVersion,
        /// Minimum version required.
        required: PandocVersion,
    },
}
```
