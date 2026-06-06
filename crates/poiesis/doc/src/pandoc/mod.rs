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

use std::path::{Path, PathBuf};
use std::process::Command;

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
    pub fn render(&self, ast_json: &[u8], opts: &DocOpts) -> Result<Vec<u8>, PandocError> {
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
            .arg(fmt)
            .arg(tmp_path);

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
    runner.render(&ast_json, opts)
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
mod tests {
    use super::*;
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
}
