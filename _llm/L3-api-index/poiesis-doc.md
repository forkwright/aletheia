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

    /// PDF rendering needed a system `LaTeX` engine but none was available.
    #[snafu(display("pdf LaTeX engine unavailable: {source}"))]
    PdfLatexEngineUnavailable {
        /// Detailed probing error from the Pandoc `LaTeX` route.
        source: crate::pandoc::PandocError,
    },

    /// ODT rendering via the clean-room backend failed.
    #[snafu(display("odt render failed: {detail}"))]
    OdtRenderFailed {
        /// Human-readable description.
        detail: String,
    },

    /// A Pandoc-backed format could not be rendered.
    #[snafu(display(
        "{format} output requires Pandoc; install pandoc >= 3.0 or use pdf/xlsx for now"
    ))]
    PandocRequired {
        /// The requested format name (e.g. "docx").
        format: String,
    },
}
```

## `src/latex_probe.rs`

```rust
pub enum LatexProbe {
    /// A usable system engine was found.
    Present {
        /// Selected PDF engine.
        engine: PdfEngine,
        /// Absolute path to the chosen binary.
        path: PathBuf,
        /// First line reported by `--version`.
        version: String,
    },
    /// Neither `xelatex` nor `lualatex` could be used.
    Missing {
        /// Candidate paths that were searched.
        searched: Vec<PathBuf>,
    },
}
```

```rust
impl LatexProbe {
    pub fn check () -> Self;
    pub fn engine (self) -> Result<PdfEngine, LatexProbeError>;
}
```

```rust
pub enum LatexProbeError {
    /// Neither `xelatex` nor `lualatex` was usable on PATH.
    #[snafu(display(
        "latex engine not found (searched: {}). Install xelatex or lualatex via a TeX distribution such as TeX Live or MacTeX; on NixOS run `nix develop`",
        format_paths(searched)
    ))]
    NotInstalled {
        /// Candidate paths that were searched.
        searched: Vec<PathBuf>,
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
pub fn render_docx_from_doc (doc: &poiesis_core::Document) -> Result<Vec<u8>>
```

```rust
pub fn render_html_from_doc (doc: &poiesis_core::Document) -> Result<Vec<u8>>
```

```rust
pub fn render_md_from_doc (doc: &poiesis_core::Document) -> Result<Vec<u8>>
```

```rust
pub fn render_latex_from_doc (doc: &poiesis_core::Document) -> Result<Vec<u8>>
```

```rust
pub fn render_epub_from_doc (doc: &poiesis_core::Document) -> Result<Vec<u8>>
```

```rust
pub fn render_odt_from_doc (doc: &poiesis_core::Document) -> Result<Vec<u8>>
```

## `src/pandoc/ast.rs`

> Serialize a `Document` to Pandoc JSON AST bytes.
> 
> The emitted JSON matches Pandoc's `--from json` input format.
> `pandoc-api-version` is pinned to `[1, 23, 1, 1]` for Pandoc 3.7.x.
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

    /// A LaTeX-backed PDF export needed `xelatex` or `lualatex`, but neither was usable.
    #[snafu(display(
        "latex engine not found (searched: {}). Install xelatex or lualatex via a TeX distribution such as TeX Live or MacTeX; on NixOS run `nix develop`",
        searched.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
    ))]
    LatexEngineNotInstalled {
        /// Candidate paths that were searched.
        searched: Vec<PathBuf>,
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

    /// A chart figure could not be rendered into SVG.
    #[snafu(display("failed to render figure {figure_id}: {source}"))]
    FigureRenderFailed {
        /// Stable figure identifier.
        figure_id: String,
        /// Underlying figure rendering error.
        source: super::figure::FigureError,
    },

    /// A chart figure SVG could not be rasterized into PNG.
    #[snafu(display("failed to rasterize figure {figure_id}: {source}"))]
    FigureRasterizeFailed {
        /// Stable figure identifier.
        figure_id: String,
        /// Underlying SVG rasterization error.
        source: crate::raster::RasterError,
    },
}
```

## `src/pandoc/figure.rs`

```rust
pub enum FigureError {
    /// The embedded figure bytes were not valid UTF-8.
    #[snafu(display("figure SVG is not valid UTF-8: {source}"))]
    Utf8 {
        /// Underlying UTF-8 error.
        source: std::str::Utf8Error,
    },

    /// The embedded figure bytes were not valid JSON.
    #[snafu(display("figure chart JSON is invalid: {source}"))]
    Json {
        /// Underlying JSON parse error.
        source: serde_json::Error,
    },

    /// The figure MIME type is not supported by the chart path.
    #[snafu(display("unsupported figure MIME type: {mime}"))]
    UnsupportedMime {
        /// Observed MIME type.
        mime: String,
    },

    /// Chart rendering failed.
    #[snafu(display("chart render failed: {source}"))]
    ChartRender {
        /// Underlying chart renderer error.
        source: poiesis_charts::Error,
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
    /// PDF — routed to Typst fast-lane by default; `LaTeX` when overridden or content-triggered.
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
    pub fn html () -> Self;
    pub fn latex () -> Self;
    pub fn epub () -> Self;
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
    pub fn render (
        &self,
        ast_json: &[u8],
        opts: &DocOpts,
        extra_env: &[(String, String)],
    ) -> Result<Vec<u8>, PandocError>;
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

## `src/raster.rs`

```rust
pub enum RasterError {
    /// SVG parsing failed.
    #[snafu(display("invalid SVG: {source}"))]
    ParseSvg {
        /// Underlying SVG parse error.
        source: usvg::Error,
    },

    /// Pixmap allocation failed for the rendered size.
    #[snafu(display("failed to allocate pixmap {width}x{height}"))]
    PixmapAlloc {
        /// Output width in pixels.
        width: u32,
        /// Output height in pixels.
        height: u32,
    },

    /// PNG encoding failed after rendering.
    #[snafu(display("PNG encoding failed: {message}"))]
    EncodePng {
        /// Human-readable PNG encoding error.
        message: String,
    },
}
```
