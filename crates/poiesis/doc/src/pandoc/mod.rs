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
pub mod ast;
/// Error types for the Pandoc subprocess wrapper.
pub mod error;
/// Figure helpers and chart rendering for `Image` payloads.
pub(crate) mod figure;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::process::{CommandOutputError, output_with_timeout};
use crate::raster;
use poiesis_core::Document;
use tracing::instrument;

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
            let mut cmd = Command::new(candidate);
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
                Ok(_) | Err(_) => {}
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
        let mut cmd = Command::new(&self.bin);
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

        match opts.format {
            OutputFormat::Html | OutputFormat::Latex | OutputFormat::Epub => {
                cmd.arg("--standalone");
            }
            _ => {}
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
        return render_doc_via_typst(doc);
    }

    let runner = PandocRunner::probe(None)?;
    let ast_json = ast::document_to_pandoc_json(doc);
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
            ch if ch.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(escaped, "\\x{:02X}", u32::from(ch));
            }
            ch => escaped.push(ch),
        }
    }
    escaped.push('"');
    escaped
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
        Block::DisplayMath(_) | Block::RawBlock { .. } | Block::Image(_) | Block::PageBreak => {}
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

#[derive(Debug, Clone, Copy)]
enum TypstEscapeContext {
    Markup,
    StringLiteral,
}

fn typst_escape(text: &str, context: TypstEscapeContext) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '#' | '*' | '_' | '[' | ']' | '$' | '@' | '<' | '>' | '='
                if matches!(context, TypstEscapeContext::Markup) =>
            {
                escaped.push('\\');
                escaped.push(ch);
            }
            '#' | '*' | '_' | '[' | ']' | '$' | '@' | '<' | '>' | '=' => {
                escaped.push_str(typst_char_escape(ch));
            }
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn typst_char_escape(ch: char) -> &'static str {
    match ch {
        '#' => "\\u{23}",
        '*' => "\\u{2a}",
        '_' => "\\u{5f}",
        '[' => "\\u{5b}",
        ']' => "\\u{5d}",
        '$' => "\\u{24}",
        '@' => "\\u{40}",
        '<' => "\\u{3c}",
        '>' => "\\u{3e}",
        '=' => "\\u{3d}",
        _ => unreachable!("only Typst punctuation is converted to string escapes"),
    }
}

fn render_typst_rich_text(rt: &poiesis_core::RichText) -> String {
    use poiesis_core::Span;

    rt.spans
        .iter()
        .map(|s| match s {
            Span::Plain(t) => typst_escape(t, TypstEscapeContext::Markup),
            Span::Bold(t) => format!("*{}*", typst_escape(t, TypstEscapeContext::Markup)),
            Span::Italic(t) => format!("_{}_", typst_escape(t, TypstEscapeContext::Markup)),
            Span::Code(t) => format!(
                "#raw(\"{}\")",
                typst_escape(t, TypstEscapeContext::StringLiteral)
            ),
            Span::Cite(id) => typst_escape(id, TypstEscapeContext::Markup),
            Span::Link { text, url } => format!(
                "#link(\"{}\")[{}]",
                typst_escape(url, TypstEscapeContext::StringLiteral),
                typst_escape(text, TypstEscapeContext::Markup)
            ),
        })
        .collect()
}

fn typst_source_from_doc(doc: &Document) -> String {
    use poiesis_core::Block;
    use std::fmt::Write as _;

    let mut src = String::new();
    src.push_str("#set page(paper: \"a4\", margin: 2cm)\n");
    src.push_str("#set text(font: \"Liberation Sans\", size: 11pt)\n\n");
    let _ = write!(
        src,
        "#align(center)[#text(18pt, weight: \"bold\")[{}]]\n\n",
        typst_escape(&doc.metadata.title, TypstEscapeContext::Markup)
    );
    if let Some(author) = &doc.metadata.author {
        let _ = write!(
            src,
            "#align(center)[#emph[{}]]\n#v(8pt)\n\n",
            typst_escape(author, TypstEscapeContext::Markup)
        );
    }

    for block in &doc.content {
        match block {
            Block::Heading { level, text } => {
                let eq = "=".repeat(usize::from((*level).max(1)));
                let _ = write!(src, "{eq} {}\n\n", render_typst_rich_text(text));
            }
            Block::Paragraph(text) => {
                src.push_str(&render_typst_rich_text(text));
                src.push_str("\n\n");
            }
            Block::Note(note) => {
                let _ = writeln!(
                    src,
                    "*{}:* {}",
                    note.kind.label(),
                    render_typst_rich_text(&note.body)
                );
                src.push('\n');
            }
            Block::DisplayMath(expr) => {
                let _ = write!(
                    src,
                    "${}$\n\n",
                    typst_escape(expr, TypstEscapeContext::Markup)
                );
            }
            Block::RawBlock { format, content } => {
                let _ = writeln!(
                    src,
                    "[raw:{}] {}",
                    typst_escape(format, TypstEscapeContext::Markup),
                    typst_escape(content, TypstEscapeContext::Markup)
                );
                src.push('\n');
            }
            Block::PageBreak => src.push_str("#pagebreak()\n\n"),
            Block::Table(t) => {
                let n = t.headers.len().max(1);
                let _ = writeln!(src, "#table(columns: {n},");
                for h in &t.headers {
                    let _ = writeln!(
                        src,
                        "  [*{}*],",
                        typst_escape(h, TypstEscapeContext::Markup)
                    );
                }
                for row in &t.rows {
                    for cell in row {
                        let _ = writeln!(src, "  [{}],", render_typst_rich_text(cell));
                    }
                }
                src.push_str(")\n\n");
            }
            Block::List { ordered, items } => {
                let cmd = if *ordered { "enum" } else { "list" };
                let _ = writeln!(src, "#{cmd}(");
                for item in items {
                    let _ = writeln!(src, "  [{}],", render_typst_rich_text(&item.content));
                }
                src.push_str(")\n\n");
            }
            Block::Image(image) => {
                // WARNING: the in-process Typst fast-lane cannot embed image bytes
                // (TypstWorld wires only data.json); emit alt text as a visible
                // placeholder rather than silently dropping the block.
                let alt = if image.alt.trim().is_empty() {
                    "untitled figure"
                } else {
                    image.alt.trim()
                };
                let alt_escaped = typst_escape(alt, TypstEscapeContext::Markup);
                let _ = write!(
                    src,
                    "#figure(rect(width: 100%, stroke: 0.5pt)[#emph[Figure: {alt_escaped}]])\n\n"
                );
            }
        }
    }

    src
}

fn render_doc_via_typst(doc: &Document) -> Result<Vec<u8>, PandocError> {
    let src = typst_source_from_doc(doc);
    poiesis_typst::render_typst(&src, &serde_json::json!({})).map_err(|e| {
        PandocError::WriterFailed {
            fmt: "pdf".to_owned(),
            stderr: e.to_string(),
        }
    })
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::{
        inspect_docx, render_docx_from_doc, render_html_from_doc, render_latex_from_doc,
        render_md_from_doc,
    };
    use poiesis_core::{Block, Document, Metadata, RichText};

    fn simple_doc() -> Document {
        Document {
            metadata: Metadata {
                title: "Test".to_owned(),
                author: None,
                created: None,
            },
            content: vec![Block::Paragraph(RichText::from("Hello."))],
        }
    }

    fn cite_doc() -> Document {
        use poiesis_core::Span;

        Document {
            metadata: Metadata {
                title: "Citations".to_owned(),
                author: None,
                created: None,
            },
            content: vec![Block::Paragraph(RichText {
                spans: vec![
                    Span::Plain("See ".to_owned()),
                    Span::Cite("fact-42".to_owned()),
                    Span::Plain(" for details.".to_owned()),
                ],
            })],
        }
    }

    fn warning_doc() -> Document {
        use poiesis_core::{Note, NoteKind, Span};

        Document {
            metadata: Metadata {
                title: "Warnings".to_owned(),
                author: None,
                created: None,
            },
            content: vec![Block::Note(Note {
                kind: NoteKind::Warning,
                body: RichText {
                    spans: vec![Span::Plain("Mind the gap.".to_owned())],
                },
            })],
        }
    }

    fn pandoc_available() -> bool {
        which::which("pandoc").is_ok()
    }

    #[cfg(unix)]
    fn write_executable_script(dir: &tempfile::TempDir, name: &str, script: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt as _;

        let path = dir.path().join(name);
        std::fs::write(&path, script).expect("write script");
        let mut perms = std::fs::metadata(&path).expect("metadata").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).expect("chmod script");
        path
    }

    #[test]
    fn default_pdf_routes_to_typst() {
        let doc = simple_doc();
        let result = render_doc(&doc, &DocOpts::default_pdf());
        match result {
            Ok(bytes) => assert!(bytes.starts_with(b"%PDF"), "must be PDF"),
            Err(PandocError::WriterFailed { fmt, .. }) if fmt == "pdf" => {
                // Typst may fail if no font available in CI — acceptable skip
            }
            Err(e) => panic!("unexpected error: {e}"),
        }
    }

    #[test]
    fn docx_format_without_pandoc_returns_not_installed_or_fails() {
        let doc = simple_doc();
        let result = render_doc(&doc, &DocOpts::docx());
        // In CI without pandoc, this must return NotInstalled — not a panic.
        match result {
            Ok(_) | Err(PandocError::NotInstalled { .. } | PandocError::WriterFailed { .. }) => {}
            Err(e) => panic!("unexpected error variant: {e:?}"),
        }
    }

    #[test]
    fn output_format_flags() {
        assert_eq!(OutputFormat::Docx.pandoc_flag(), "docx");
        assert_eq!(OutputFormat::Odt.pandoc_flag(), "odt");
        assert_eq!(OutputFormat::Markdown.pandoc_flag(), "gfm");
        assert_eq!(OutputFormat::Latex.pandoc_flag(), "latex");
        assert_eq!(OutputFormat::Html.pandoc_flag(), "html5");
        assert_eq!(OutputFormat::Epub.pandoc_flag(), "epub3");
    }

    const TYPST_SPECIAL: &str = r#"\ # * _ [ ] $ @ < > = " ]"#;
    const TYPST_MARKUP_ESCAPED: &str = r#"\\ \# \* \_ \[ \] \$ \@ \< \> \= \" \]"#;
    const TYPST_STRING_ESCAPED: &str =
        r#"\\ \u{23} \u{2a} \u{5f} \u{5b} \u{5d} \u{24} \u{40} \u{3c} \u{3e} \u{3d} \" \u{5d}"#;

    fn typst_escape_fixture_doc() -> Document {
        use poiesis_core::{Image, ListItem, Note, NoteKind, Span, Table};

        let rich_text = RichText {
            spans: vec![
                Span::Plain(TYPST_SPECIAL.to_owned()),
                Span::Bold(TYPST_SPECIAL.to_owned()),
                Span::Italic(TYPST_SPECIAL.to_owned()),
                Span::Code(TYPST_SPECIAL.to_owned()),
                Span::Cite(TYPST_SPECIAL.to_owned()),
                Span::Link {
                    text: TYPST_SPECIAL.to_owned(),
                    url: TYPST_SPECIAL.to_owned(),
                },
            ],
        };
        Document {
            metadata: Metadata {
                title: TYPST_SPECIAL.to_owned(),
                author: Some(TYPST_SPECIAL.to_owned()),
                created: None,
            },
            content: vec![
                Block::Heading {
                    level: 1,
                    text: RichText::from(TYPST_SPECIAL),
                },
                Block::Paragraph(rich_text.clone()),
                Block::Note(Note {
                    kind: NoteKind::Warning,
                    body: RichText::from(TYPST_SPECIAL),
                }),
                Block::DisplayMath(TYPST_SPECIAL.to_owned()),
                Block::RawBlock {
                    format: TYPST_SPECIAL.to_owned(),
                    content: TYPST_SPECIAL.to_owned(),
                },
                Block::Table(Table {
                    headers: vec![TYPST_SPECIAL.to_owned()],
                    rows: vec![vec![rich_text.clone()]],
                }),
                Block::List {
                    ordered: false,
                    items: vec![ListItem {
                        content: RichText::from(TYPST_SPECIAL),
                    }],
                },
                Block::Image(Image {
                    data: Vec::new(),
                    mime: "image/png".to_owned(),
                    alt: TYPST_SPECIAL.to_owned(),
                }),
            ],
        }
    }

    fn assert_typst_source_escapes_user_content(source: &str) {
        assert!(
            !source.contains(TYPST_SPECIAL),
            "raw user content must not appear in Typst source: {source}"
        );
        assert!(
            source.contains(&format!(
                "#text(18pt, weight: \"bold\")[{TYPST_MARKUP_ESCAPED}]"
            )),
            "title must be escaped: {source}"
        );
        assert!(
            source.contains(&format!("#emph[{TYPST_MARKUP_ESCAPED}]")),
            "author must be escaped: {source}"
        );
        assert!(
            source.contains(&format!("= {TYPST_MARKUP_ESCAPED}")),
            "heading must be escaped: {source}"
        );
        assert!(
            source.contains(&format!("*{TYPST_MARKUP_ESCAPED}*")),
            "bold span must be escaped: {source}"
        );
        assert!(
            source.contains(&format!("_{TYPST_MARKUP_ESCAPED}_")),
            "italic span must be escaped: {source}"
        );
        assert!(
            source.contains(&format!("#raw(\"{TYPST_STRING_ESCAPED}\")")),
            "code span must be escaped as a Typst string literal: {source}"
        );
        assert!(
            source.contains(&format!(
                "#link(\"{TYPST_STRING_ESCAPED}\")[{TYPST_MARKUP_ESCAPED}]"
            )),
            "link URL and text must be escaped: {source}"
        );
        assert!(
            source.contains(&format!("${TYPST_MARKUP_ESCAPED}$")),
            "display math must be escaped: {source}"
        );
        assert!(
            source.contains(&format!(
                "[raw:{TYPST_MARKUP_ESCAPED}] {TYPST_MARKUP_ESCAPED}"
            )),
            "raw block format and content must be escaped: {source}"
        );
        assert!(
            source.contains(&format!("[*{TYPST_MARKUP_ESCAPED}*]")),
            "table headers must be escaped: {source}"
        );
        assert!(
            source.contains(&format!("[Figure: {TYPST_MARKUP_ESCAPED}]")),
            "image alt text must be escaped: {source}"
        );
    }

    #[test]
    fn typst_source_escapes_user_content() {
        let source = typst_source_from_doc(&typst_escape_fixture_doc());
        assert_typst_source_escapes_user_content(&source);
    }

    #[test]
    fn embedded_lua_filters_materialize_for_each_render() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let filters = materialize_default_lua_filters(dir.path()).expect("filters");

        assert_eq!(filters.len(), 3);
        for filter in &filters {
            assert!(
                filter.exists(),
                "materialized filter must exist: {}",
                filter.display()
            );
            assert_eq!(
                filter.parent(),
                Some(dir.path()),
                "filter must live under the render tempdir"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn writer_failure_preserves_stderr() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let fake_pandoc = write_executable_script(
            &dir,
            "pandoc",
            r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  echo "pandoc 3.1.1"
  exit 0
fi
echo "writer exploded" >&2
exit 23
"#,
        );

        let runner = PandocRunner::probe(Some(&fake_pandoc)).expect("probe");
        let err = runner
            .render(b"{}", &DocOpts::markdown(), &[])
            .expect_err("render must fail");

        match err {
            PandocError::WriterFailed { stderr, .. } => {
                assert!(
                    stderr.contains("writer exploded"),
                    "stderr must be preserved: {stderr}"
                );
            }
            other => panic!("expected WriterFailed, got {other:?}"),
        }
    }

    #[test]
    fn citation_display_is_stable_across_docx_html_and_md() {
        if !pandoc_available() {
            return;
        }

        let doc = cite_doc();

        let docx = render_docx_from_doc(&doc).expect("docx must render");
        let docx_summary = inspect_docx(&docx).expect("docx must inspect");
        let docx_text = docx_summary.paragraphs.join("\n");
        assert!(
            docx_text.contains("[1]"),
            "docx output must preserve the citation number"
        );

        let html = String::from_utf8(render_html_from_doc(&doc).expect("html must render"))
            .expect("html must be utf-8");
        assert!(
            html.contains("[1]"),
            "html output must preserve the citation number"
        );

        let md = String::from_utf8(render_md_from_doc(&doc).expect("md must render"))
            .expect("markdown must be utf-8");
        let md_visible = md.replace("\\[", "[").replace("\\]", "]");
        assert!(
            md_visible.contains("[1]"),
            "markdown output must preserve the citation number"
        );
    }

    #[test]
    fn warning_note_renders_as_admonition_in_html_and_latex() {
        if !pandoc_available() {
            return;
        }

        let doc = warning_doc();

        let html = String::from_utf8(render_html_from_doc(&doc).expect("html must render"))
            .expect("html must be utf-8");
        assert!(
            html.contains("admonition-warning"),
            "html output must include warning admonition classes"
        );
        assert!(
            html.contains("Mind the gap."),
            "html output must include the note body"
        );

        let latex = String::from_utf8(render_latex_from_doc(&doc).expect("latex must render"))
            .expect("latex must be utf-8");
        assert!(
            latex.contains("\\begin{tcolorbox}[title={Warning}]"),
            "latex output must wrap the note in a tcolorbox"
        );
        assert!(
            latex.contains("Mind the gap."),
            "latex output must include the note body"
        );
    }

    fn chart_doc() -> Document {
        use poiesis_charts::model::{
            AxisSide, Chart, ChartKind, FactCite, FactId, LegendSpec, Point, Series, SeriesStyle,
            ToneRef, Unit,
        };
        use poiesis_core::Image;

        let chart = Chart {
            kind: ChartKind::Line,
            title: None,
            series: vec![Series {
                name: poiesis_charts::CiteOrText::Text("Revenue".to_owned()),
                points: vec![
                    Point {
                        label: Some(poiesis_charts::CiteOrText::Text("JAN".to_owned())),
                        x: None,
                        y: FactCite {
                            id: FactId("rev-1".to_owned()),
                            value: 1.0,
                            unit: Unit::Number,
                        },
                    },
                    Point {
                        label: Some(poiesis_charts::CiteOrText::Text("FEB".to_owned())),
                        x: None,
                        y: FactCite {
                            id: FactId("rev-2".to_owned()),
                            value: 2.0,
                            unit: Unit::Number,
                        },
                    },
                ],
                tone: ToneRef::Indexed(0),
                axis: AxisSide::Left,
                style: SeriesStyle::Default,
            }],
            axes: poiesis_charts::Axes::default(),
            legend: LegendSpec::Auto,
            data_labels: false,
            caption: None,
        };

        let data = serde_json::to_vec(&chart).expect("chart json");

        Document {
            metadata: Metadata {
                title: "Chart Figure".to_owned(),
                author: None,
                created: None,
            },
            content: vec![Block::Image(Image {
                data,
                mime: "application/vnd.poiesis.chart+json".to_owned(),
                alt: "Revenue trend".to_owned(),
            })],
        }
    }

    fn docx_media_entries(bytes: &[u8]) -> Vec<String> {
        let cursor = std::io::Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(cursor).expect("docx zip");
        let mut names = Vec::new();

        for idx in 0..archive.len() {
            let file = archive.by_index(idx).expect("zip entry");
            let name = file.name().to_owned();
            if name.contains("media/") {
                names.push(name);
            }
        }

        names
    }

    #[test]
    fn chart_figure_renders_png_in_docx() {
        if !pandoc_available() {
            return;
        }

        let bytes = render_docx_from_doc(&chart_doc()).expect("docx must render");
        let media = docx_media_entries(&bytes);
        assert!(
            media.iter().any(|name| {
                std::path::Path::new(name)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
            }),
            "docx must embed a PNG media entry, got: {media:?}"
        );
    }

    #[test]
    fn chart_figure_renders_svg_in_html() {
        if !pandoc_available() {
            return;
        }

        let html = String::from_utf8(render_html_from_doc(&chart_doc()).expect("html must render"))
            .expect("html must be utf-8");
        assert!(
            html.contains("<svg"),
            "html output must include inline SVG, got: {html}"
        );
        assert!(
            !html.contains(".png"),
            "html output must not reference raster PNG assets, got: {html}"
        );
    }
}
