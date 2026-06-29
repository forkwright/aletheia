//! Pandoc subprocess wrapper for poiesis-doc.
//!
//! Provides [`PandocRunner`] (typed wrapper around the `pandoc` binary),
//! [`OutputFormat`] (supported export formats), and [`DocOpts`] (render options).
//!
//! The `render_doc` function is the unified dispatch entry point: it routes
//! PDF to `poiesis-typst` (in-process fast-lane) unless the document content
//! requires `LaTeX`, in which case it probes a system engine and uses Pandoc.
//!
//! # GPL-clean boundary
//!
//! `pandoc` is invoked as a subprocess only. This crate must not add the
//! `pandoc` Rust crate as a dependency. See B-006 § GPL-clean boundary.

/// AST serialization: `Document` → Pandoc JSON AST.
pub(crate) mod ast;
/// Error types for the Pandoc subprocess wrapper.
pub mod error;
/// Figure helpers and chart rendering for `Image` payloads.
pub(crate) mod figure;
mod typst;

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests;

use std::path::{Path, PathBuf};
use std::time::Duration;

use poiesis_core::Document;
use tracing::instrument;

use crate::process::{CommandOutputError, command, output_with_timeout};
use crate::raster;

pub use error::PandocError;

const PANDOC_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const PANDOC_RENDER_TIMEOUT: Duration = Duration::from_mins(2);
const APX_CITE_LUA: &str = include_str!("../../../filters/apx-cite.lua");
const APX_THEME_LUA: &str = include_str!("../../../filters/apx-theme.lua");
const APX_FIGURE_LUA: &str = include_str!("../../../filters/apx-figure.lua");

/// Supported export output formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
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

impl OutputFormat {
    /// Pandoc writer flag name for `-t <fmt>`.
    fn pandoc_flag(self) -> &'static str {
        match self {
            Self::Docx => "docx",
            Self::Odt => "odt",
            Self::Pdf => "pdf",
            Self::Markdown => "gfm",
            Self::Latex => "latex",
            Self::Html => "html5",
            Self::Epub => "epub3",
        }
    }
}

/// PDF backend selection for `DocOpts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PdfEngine {
    /// Typst in-process compiler (default, fastest).
    Typst,
    /// `xelatex` — use for math-heavy documents.
    XeLaTeX,
    /// `lualatex` — alternative `LaTeX` engine.
    LuaLaTeX,
}

impl PdfEngine {
    /// Return the Pandoc `--pdf-engine` flag value for system engines.
    fn pandoc_arg(self) -> Option<&'static str> {
        match self {
            Self::Typst => None,
            Self::XeLaTeX => Some("xelatex"),
            Self::LuaLaTeX => Some("lualatex"),
        }
    }
}

/// Options controlling the render call.
#[derive(Debug, Clone)]
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

impl DocOpts {
    /// Default PDF options: Typst engine, no filters.
    pub fn default_pdf() -> Self {
        Self {
            format: OutputFormat::Pdf,
            pdf_engine: None,
            lua_filters: Vec::new(),
            reference_doc: None,
        }
    }

    /// Default DOCX options.
    pub fn docx() -> Self {
        Self {
            format: OutputFormat::Docx,
            ..Self::default_pdf()
        }
    }

    /// Default HTML options.
    pub fn html() -> Self {
        Self {
            format: OutputFormat::Html,
            ..Self::default_pdf()
        }
    }

    /// Default `LaTeX` options.
    pub fn latex() -> Self {
        Self {
            format: OutputFormat::Latex,
            ..Self::default_pdf()
        }
    }

    /// Default EPUB options.
    pub fn epub() -> Self {
        Self {
            format: OutputFormat::Epub,
            ..Self::default_pdf()
        }
    }

    /// Default ODT options.
    pub fn odt() -> Self {
        Self {
            format: OutputFormat::Odt,
            ..Self::default_pdf()
        }
    }

    /// Default Markdown options.
    pub fn markdown() -> Self {
        Self {
            format: OutputFormat::Markdown,
            ..Self::default_pdf()
        }
    }
}

/// Typed wrapper around the `pandoc` binary.
///
/// Construct via [`PandocRunner::probe`]. Call [`PandocRunner::render`] to
/// convert a Pandoc JSON AST blob to the requested output format.
#[derive(Debug, Clone)]
pub struct PandocRunner {
    /// Resolved path to the `pandoc` binary.
    bin: PathBuf,
    /// Detected Pandoc version (major.minor.patch).
    pub version: (u32, u32, u32),
}

impl PandocRunner {
    /// Probe for `pandoc` at `bin_override` or on PATH.
    ///
    /// Returns `Err(PandocError::NotInstalled)` if not found,
    /// `Err(PandocError::VersionTooOld)` if below 3.0.
    ///
    /// # Errors
    ///
    /// See [`PandocError`].
    pub fn probe(bin_override: Option<&Path>) -> Result<Self, PandocError> {
        let mut searched: Vec<PathBuf> = Vec::new();
        let mut candidates: Vec<PathBuf> = Vec::new();

        if let Some(p) = bin_override {
            searched.push(p.to_path_buf());
            candidates.push(p.to_path_buf());
        }

        if let Some(path_var) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&path_var) {
                let candidate = dir.join("pandoc");
                searched.push(dir);
                candidates.push(candidate);
            }
        }

        for candidate in &candidates {
            if !candidate.exists() {
                continue;
            }
            let mut cmd = command(candidate);
            cmd.arg("--version");
            match output_with_timeout(&mut cmd, PANDOC_PROBE_TIMEOUT) {
                Ok(output) if output.status.success() => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if let Some(ver) = parse_pandoc_version(&stdout) {
                        if ver.0 >= 3 {
                            return Ok(PandocRunner {
                                bin: candidate.clone(),
                                version: ver,
                            });
                        }
                        return Err(PandocError::VersionTooOld {
                            path: candidate.clone(),
                            found_major: ver.0,
                            found_minor: ver.1,
                            found_patch: ver.2,
                        });
                    }
                }
                Err(error @ CommandOutputError::Timeout { .. }) => {
                    return Err(pandoc_process_error(error, "probe"));
                }
                Ok(_) | Err(_) => {
                    // Keep probing later candidates after bad version output or I/O failure.
                }
            }
        }

        Err(PandocError::NotInstalled { searched })
    }

    /// Render `ast_json` bytes (Pandoc JSON AST) to the format specified in `opts`.
    ///
    /// Writes the AST to a temp file, spawns `pandoc -f json -t <fmt>`,
    /// and returns stdout as the artifact.
    ///
    /// # Errors
    ///
    /// Returns [`PandocError::WriterFailed`] on non-zero exit or
    /// [`PandocError::TempFile`] / [`PandocError::Spawn`] on I/O failures.
    #[instrument(skip(self, ast_json), fields(fmt = ?opts.format))]
    pub fn render(
        &self,
        ast_json: &[u8],
        opts: &DocOpts,
        extra_env: &[(String, String)],
    ) -> Result<Vec<u8>, PandocError> {
        use std::io::Write;

        // WHY: a temp file is more reliable than piping a large AST through stdin.
        let mut tmp = tempfile::Builder::new()
            .suffix(".json")
            .tempfile()
            .map_err(|e| PandocError::TempFile { source: e })?;
        tmp.write_all(ast_json)
            .map_err(|e| PandocError::TempFile { source: e })?;
        let tmp_path = tmp.path().to_path_buf();

        let fmt = opts.format.pandoc_flag();
        let mut cmd = command(&self.bin);
        cmd.env("LC_ALL", "C")
            .arg("-f")
            .arg("json")
            .arg("-t")
            .arg(fmt);

        for (key, value) in extra_env {
            cmd.env(key, value);
        }

        for filter in &opts.lua_filters {
            cmd.arg("--lua-filter").arg(filter);
        }

        if let Some(ref_doc) = &opts.reference_doc {
            cmd.arg("--reference-doc").arg(ref_doc);
        }

        if matches!(
            opts.format,
            OutputFormat::Html | OutputFormat::Latex | OutputFormat::Epub
        ) {
            cmd.arg("--standalone");
        }

        if matches!(opts.format, OutputFormat::Html) {
            cmd.arg("--mathml");
        }

        if matches!(opts.format, OutputFormat::Pdf)
            && let Some(engine) = opts.pdf_engine
            && let Some(engine_arg) = engine.pandoc_arg()
        {
            cmd.arg("--pdf-engine").arg(engine_arg);
        }

        cmd.arg(tmp_path);

        let output = output_with_timeout(&mut cmd, PANDOC_RENDER_TIMEOUT)
            .map_err(|e| pandoc_process_error(e, "render"))?;

        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(PandocError::WriterFailed {
                fmt: fmt.to_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            })
        }
    }
}

fn parse_pandoc_version(output: &str) -> Option<(u32, u32, u32)> {
    let first = output.lines().next()?;
    let ver = first.strip_prefix("pandoc ")?.trim();
    let mut parts = ver.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    Some((major, minor, patch))
}

/// Unified dispatch: render a [`Document`] to bytes using the appropriate backend.
///
/// Routing rules (per B-006 § 3):
/// 1. `opts.format == Pdf` and no `pdf_engine` override plus no `LaTeX`-only
///    content → Typst in-process.
/// 2. `opts.pdf_engine == Some(Typst)` → Typst in-process.
/// 3. `opts.pdf_engine == Some(XeLaTeX | LuaLaTeX)` or content-triggered PDF
///    (`DisplayMath` / `RawBlock(LaTeX)`) → Pandoc + `LaTeX` engine.
/// 4. All other formats → Pandoc via `PandocRunner`.
///
/// # Errors
///
/// Returns `PandocError` for Pandoc-backend failures, or
/// `poiesis_typst::PoiesisError` mapped to `PandocError::WriterFailed` for
/// Typst failures.
#[instrument(skip(doc), fields(fmt = ?opts.format))]
pub fn render_doc(doc: &Document, opts: &DocOpts) -> Result<Vec<u8>, PandocError> {
    let pdf_engine = select_pdf_engine(doc, opts)?;
    let use_typst = matches!(opts.format, OutputFormat::Pdf) && pdf_engine.is_none();

    if use_typst {
        return typst::render_doc_via_typst(doc);
    }

    let runner = PandocRunner::probe(None)?;
    let ast_json = ast::document_to_pandoc_json(doc)?;
    let mut render_opts = opts.clone();
    if let Some(engine) = pdf_engine {
        render_opts.pdf_engine = Some(engine);
    }
    let render_tmp = tempfile::tempdir().map_err(|source| PandocError::TempFile { source })?;
    render_opts
        .lua_filters
        .extend(materialize_default_lua_filters(render_tmp.path())?);
    let facts_sidecar = write_apx_facts_sidecar(doc)?;
    let figures_sidecar = write_apx_figures_sidecar(doc)?;
    let extra_env = vec![
        (
            "APX_FACTS".to_owned(),
            facts_sidecar.path().display().to_string(),
        ),
        (
            "APX_FIGURES".to_owned(),
            figures_sidecar.sidecar.path().display().to_string(),
        ),
        (
            "APX_TMPDIR".to_owned(),
            render_tmp.path().display().to_string(),
        ),
    ];
    runner.render(&ast_json, &render_opts, &extra_env)
}

fn select_pdf_engine(doc: &Document, opts: &DocOpts) -> Result<Option<PdfEngine>, PandocError> {
    if !matches!(opts.format, OutputFormat::Pdf) {
        return Ok(None);
    }

    if let Some(engine @ (PdfEngine::XeLaTeX | PdfEngine::LuaLaTeX)) = opts.pdf_engine {
        return Ok(Some(engine));
    }

    if matches!(opts.pdf_engine, Some(PdfEngine::Typst)) {
        return Ok(None);
    }

    if opts.pdf_engine.is_none() && document_needs_latex_pdf(doc) {
        return Ok(Some(crate::LatexProbe::check().engine()?));
    }

    Ok(None)
}

fn document_needs_latex_pdf(doc: &Document) -> bool {
    use poiesis_core::Block;

    doc.content.iter().any(|block| match block {
        Block::DisplayMath(_) => true,
        Block::RawBlock { format, .. } => format.eq_ignore_ascii_case("latex"),
        _ => false,
    })
}

#[derive(Debug, serde::Serialize)]
struct ApxFactSidecarEntry {
    display: String,
    source_footnote: Option<String>,
}

fn pandoc_process_error(error: CommandOutputError, operation: &str) -> PandocError {
    match error {
        CommandOutputError::Spawn { source } => PandocError::Spawn { source },
        CommandOutputError::TempFile { source } | CommandOutputError::Wait { source } => {
            PandocError::SubprocessIo {
                operation: operation.to_owned(),
                source,
            }
        }
        CommandOutputError::Timeout {
            timeout,
            kill_error,
            wait_error,
        } => PandocError::Timeout {
            operation: operation.to_owned(),
            timeout_secs: timeout.as_secs(),
            kill_error,
            wait_error,
        },
    }
}

fn materialize_default_lua_filters(dir: &Path) -> Result<Vec<PathBuf>, PandocError> {
    let filters = [
        ("apx-cite.lua", APX_CITE_LUA),
        ("apx-theme.lua", APX_THEME_LUA),
        ("apx-figure.lua", APX_FIGURE_LUA),
    ];

    let mut paths = Vec::with_capacity(filters.len());
    for (name, source) in filters {
        let path = dir.join(name);
        std::fs::write(&path, source).map_err(|source| PandocError::TempFile { source })?;
        paths.push(path);
    }

    Ok(paths)
}

struct FigureSidecarArtifacts {
    sidecar: tempfile::NamedTempFile,
    _png_files: Vec<tempfile::NamedTempFile>,
}

fn write_apx_facts_sidecar(doc: &Document) -> Result<tempfile::NamedTempFile, PandocError> {
    use std::collections::{BTreeMap, HashSet};
    use std::io::Write;

    let mut seen = HashSet::new();
    let mut ordered_fact_ids = Vec::new();

    for block in &doc.content {
        collect_fact_ids_from_block(block, &mut seen, &mut ordered_fact_ids);
    }

    let mut sidecar = BTreeMap::new();
    for (index, fact_id) in ordered_fact_ids.into_iter().enumerate() {
        sidecar.insert(
            fact_id,
            ApxFactSidecarEntry {
                display: format!("[{}]", index + 1),
                source_footnote: None,
            },
        );
    }

    let serialized = serde_json::to_vec(&sidecar).map_err(|source| PandocError::TempFile {
        source: std::io::Error::other(source),
    })?;

    let mut tmp = tempfile::Builder::new()
        .suffix(".json")
        .tempfile()
        .map_err(|source| PandocError::TempFile { source })?;
    tmp.write_all(&serialized)
        .map_err(|source| PandocError::TempFile { source })?;
    Ok(tmp)
}

fn write_apx_figures_sidecar(doc: &Document) -> Result<FigureSidecarArtifacts, PandocError> {
    use std::io::Write;

    let mut sidecar = String::from("return {\n");
    let mut png_files = Vec::new();
    let mut figure_index = 0usize;

    for block in &doc.content {
        let poiesis_core::Block::Image(image) = block else {
            continue;
        };

        let figure_id = figure::figure_id(figure_index);
        let svg =
            figure::svg_from_image(image).map_err(|source| PandocError::FigureRenderFailed {
                figure_id: figure_id.clone(),
                source,
            })?;
        let png = raster::svg_to_png(&svg, raster::DEFAULT_DPI).map_err(|source| {
            PandocError::FigureRasterizeFailed {
                figure_id: figure_id.clone(),
                source,
            }
        })?;

        let mut png_file = tempfile::Builder::new()
            .suffix(".png")
            .tempfile()
            .map_err(|source| PandocError::TempFile { source })?;
        png_file
            .write_all(&png)
            .map_err(|source| PandocError::TempFile { source })?;

        sidecar.push_str("  [");
        sidecar.push_str(&lua_string_literal(&figure_id));
        sidecar.push_str("] = {\n");
        sidecar.push_str("    svg = ");
        sidecar.push_str(&lua_string_literal(&svg));
        sidecar.push_str(",\n");
        sidecar.push_str("    png_path = ");
        sidecar.push_str(&lua_string_literal(&png_file.path().display().to_string()));
        sidecar.push_str(",\n");
        sidecar.push_str("  },\n");
        png_files.push(png_file);
        figure_index += 1;
    }

    sidecar.push_str("}\n");

    let mut tmp = tempfile::Builder::new()
        .suffix(".lua")
        .tempfile()
        .map_err(|source| PandocError::TempFile { source })?;
    tmp.write_all(sidecar.as_bytes())
        .map_err(|source| PandocError::TempFile { source })?;

    Ok(FigureSidecarArtifacts {
        sidecar: tmp,
        _png_files: png_files,
    })
}

fn lua_string_literal(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            '\u{0008}' => escaped.push_str("\\b"),
            '\u{000C}' => escaped.push_str("\\f"),
            ch if ch.is_control() => push_lua_hex_escape(&mut escaped, ch),
            ch => escaped.push(ch),
        }
    }
    escaped.push('"');
    escaped
}

fn push_lua_hex_escape(escaped: &mut String, ch: char) {
    use std::fmt::Write as _;

    if let Err(error) = write!(escaped, "\\x{:02X}", u32::from(ch)) {
        unreachable!("writing to a String must not fail: {error}");
    }
}

fn collect_fact_ids_from_block(
    block: &poiesis_core::Block,
    seen: &mut std::collections::HashSet<String>,
    ordered_fact_ids: &mut Vec<String>,
) {
    use poiesis_core::Block;

    match block {
        Block::Heading { text, .. } | Block::Paragraph(text) => {
            collect_fact_ids_from_rich_text(text, seen, ordered_fact_ids);
        }
        Block::Note(note) => {
            collect_fact_ids_from_rich_text(&note.body, seen, ordered_fact_ids);
        }
        Block::Table(table) => {
            for row in &table.rows {
                for cell in row {
                    collect_fact_ids_from_rich_text(cell, seen, ordered_fact_ids);
                }
            }
        }
        Block::List { items, .. } => {
            for item in items {
                collect_fact_ids_from_rich_text(&item.content, seen, ordered_fact_ids);
            }
        }
        Block::DisplayMath(_) | Block::RawBlock { .. } | Block::Image(_) | Block::PageBreak => {
            // These block kinds do not contain rich-text citation spans.
        }
    }
}

fn collect_fact_ids_from_rich_text(
    text: &poiesis_core::RichText,
    seen: &mut std::collections::HashSet<String>,
    ordered_fact_ids: &mut Vec<String>,
) {
    use poiesis_core::Span;

    for span in &text.spans {
        if let Span::Cite(fact_id) = span
            && seen.insert(fact_id.clone())
        {
            ordered_fact_ids.push(fact_id.clone());
        }
    }
}
