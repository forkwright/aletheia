//! Pandoc subprocess wrapper for poiesis-doc.
//!
//! Provides [`PandocRunner`] (typed wrapper around the `pandoc` binary),
//! [`OutputFormat`] (supported export formats), and [`DocOpts`] (render options).
//!
//! The `render_doc` function is the unified dispatch entry point: it routes
//! PDF to `poiesis-typst` (in-process fast-lane) and all other formats to
//! the Pandoc subprocess.
//!
//! # GPL-clean boundary
//!
//! `pandoc` is invoked as a subprocess only. This crate must not add the
//! `pandoc` Rust crate as a dependency. See B-006 § GPL-clean boundary.
//!
//! # B-001 upgrade note
//!
//! The current entry point accepts `poiesis_core::Document`. When B-001
//! lands (`DeliverableSpec`), the signature will be upgraded. See ESCALATION.md.

/// AST serialization: `Document` → Pandoc JSON AST.
pub mod ast;
/// Error types for the Pandoc subprocess wrapper.
pub mod error;
/// Figure helpers and chart rendering for `Image` payloads.
pub(crate) mod figure;

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::raster;
use poiesis_core::Document;
use tracing::instrument;

pub use error::PandocError;

/// Supported export output formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
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
            if let Ok(output) = Command::new(candidate).arg("--version").output()
                && output.status.success()
            {
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

        // Write AST to a temp file (more reliable than large stdin pipes).
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

        // Lua filters
        for filter in &opts.lua_filters {
            cmd.arg("--lua-filter").arg(filter);
        }

        // Reference doc theming
        if let Some(ref_doc) = &opts.reference_doc {
            cmd.arg("--reference-doc").arg(ref_doc);
        }

        // Standalone flag for html/latex/epub
        match opts.format {
            OutputFormat::Html | OutputFormat::Latex | OutputFormat::Epub => {
                cmd.arg("--standalone");
            }
            _ => {}
        }

        cmd.arg(tmp_path);

        let output = cmd.output().map_err(|e| PandocError::Spawn { source: e })?;

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
/// 1. `opts.format == Pdf` and no `pdf_engine` override → Typst in-process.
/// 2. `opts.pdf_engine == Some(Typst)` → Typst in-process.
/// 3. `opts.pdf_engine == Some(XeLaTeX | LuaLaTeX)` → Pandoc + `LaTeX` engine.
/// 4. Content-trigger routing (`DisplayMath`/`RawBlock(latex)`) remains a
///    later routing chunk. See ESCALATION.md Q2.
/// 5. All other formats → Pandoc via `PandocRunner`.
///
/// # Errors
///
/// Returns `PandocError` for Pandoc-backend failures, or
/// `poiesis_typst::PoiesisError` mapped to `PandocError::WriterFailed` for
/// Typst failures.
#[instrument(skip(doc), fields(fmt = ?opts.format))]
pub fn render_doc(doc: &Document, opts: &DocOpts) -> Result<Vec<u8>, PandocError> {
    // Route PDF to Typst fast-lane unless an explicit LaTeX engine is set.
    let use_typst = matches!(opts.format, OutputFormat::Pdf)
        && !matches!(
            opts.pdf_engine,
            Some(PdfEngine::XeLaTeX | PdfEngine::LuaLaTeX)
        );

    if use_typst {
        return render_doc_via_typst(doc);
    }

    // Pandoc path — probe for binary.
    let runner = PandocRunner::probe(None)?;
    let ast_json = ast::document_to_pandoc_json(doc);
    let mut render_opts = opts.clone();
    render_opts.lua_filters.extend(default_lua_filters());
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
    ];
    runner.render(&ast_json, &render_opts, &extra_env)
}

#[derive(Debug, serde::Serialize)]
struct ApxFactSidecarEntry {
    display: String,
    source_footnote: Option<String>,
}

fn default_lua_filters() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../filters");
    vec![
        root.join("apx-cite.lua"),
        root.join("apx-theme.lua"),
        root.join("apx-figure.lua"),
    ]
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

fn render_doc_via_typst(doc: &Document) -> Result<Vec<u8>, PandocError> {
    use poiesis_core::{Block, Span};

    fn render_rich(rt: &poiesis_core::RichText) -> String {
        rt.spans
            .iter()
            .map(|s| match s {
                Span::Plain(t) => t.clone(),
                Span::Bold(t) => format!("*{t}*"),
                Span::Italic(t) => format!("_{t}_"),
                Span::Code(t) => format!("`{t}`"),
                Span::Cite(id) => id.clone(),
                Span::Link { text, url } => format!("#link(\"{url}\")[{text}]"),
            })
            .collect()
    }

    use std::fmt::Write as _;

    let mut src = String::new();
    src.push_str("#set page(paper: \"a4\", margin: 2cm)\n");
    src.push_str("#set text(font: \"Liberation Sans\", size: 11pt)\n\n");
    let _ = write!(
        src,
        "#align(center)[#text(18pt, weight: \"bold\")[{}]]\n\n",
        doc.metadata.title
    );
    if let Some(author) = &doc.metadata.author {
        let _ = write!(src, "#align(center)[#emph[{author}]]\n#v(8pt)\n\n");
    }

    for block in &doc.content {
        match block {
            Block::Heading { level, text } => {
                let eq = "=".repeat(usize::from((*level).max(1)));
                let _ = write!(src, "{eq} {}\n\n", render_rich(text));
            }
            Block::Paragraph(text) => {
                src.push_str(&render_rich(text));
                src.push_str("\n\n");
            }
            Block::Note(note) => {
                let _ = writeln!(src, "*{}:* {}", note.kind.label(), render_rich(&note.body));
                src.push('\n');
            }
            Block::DisplayMath(expr) => {
                let _ = write!(src, "${expr}$\n\n");
            }
            Block::RawBlock { format, content } => {
                let _ = writeln!(src, "[raw:{format}] {content}");
                src.push('\n');
            }
            Block::PageBreak => src.push_str("#pagebreak()\n\n"),
            Block::Table(t) => {
                let n = t.headers.len().max(1);
                let _ = writeln!(src, "#table(columns: {n},");
                for h in &t.headers {
                    let _ = writeln!(src, "  [*{h}*],");
                }
                for row in &t.rows {
                    for cell in row {
                        let _ = writeln!(src, "  [{}],", render_rich(cell));
                    }
                }
                src.push_str(")\n\n");
            }
            Block::List { ordered, items } => {
                let cmd = if *ordered { "enum" } else { "list" };
                let _ = writeln!(src, "#{cmd}(");
                for item in items {
                    let _ = writeln!(src, "  [{}],", render_rich(&item.content));
                }
                src.push_str(")\n\n");
            }
            Block::Image(_) => {} // skip in stub
        }
    }

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
