//! export-matrix integration tests for poiesis-doc.

#![expect(
    clippy::string_slice,
    clippy::default_trait_access,
    reason = "integration test: assertions parse ASCII citation markers and use explicit defaults"
)]

use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use poiesis_charts::model::{
    AxisSide, Chart, ChartKind, FactCite, FactId, LegendSpec, Point, Series, SeriesStyle, ToneRef,
    Unit,
};
use poiesis_core::{Block, Document, Image, Metadata, RichText, Span, Table};
use poiesis_doc::{
    PandocProbe, inspect_docx, pandoc::DocOpts, pandoc::PandocRunner, pandoc::render_doc,
    render_odt_from_doc, render_pdf_from_doc,
};
use tempfile::tempdir;

const MISSING_PANDOC_DIR: &str = "/tmp/poiesis-doc-export-matrix-missing-pandoc";
const TOO_OLD_PANDOC_DIR: &str = "/tmp/poiesis-doc-export-matrix-too-old-pandoc";

type ExportMatrixResult<T> = Result<T, ExportMatrixError>;

#[derive(Debug)]
enum ExportMatrixError {
    Io(std::io::Error),
    Doc(Box<poiesis_doc::Error>),
    Pandoc(Box<poiesis_doc::pandoc::PandocError>),
    Utf8(std::string::FromUtf8Error),
    Unexpected(&'static str),
}

impl fmt::Display for ExportMatrixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(source) => write!(f, "I/O error: {source}"),
            Self::Doc(source) => write!(f, "poiesis-doc error: {source}"),
            Self::Pandoc(source) => write!(f, "pandoc error: {source}"),
            Self::Utf8(source) => write!(f, "UTF-8 error: {source}"),
            Self::Unexpected(message) => f.write_str(message),
        }
    }
}

impl From<std::io::Error> for ExportMatrixError {
    fn from(source: std::io::Error) -> Self {
        Self::Io(source)
    }
}

impl From<poiesis_doc::Error> for ExportMatrixError {
    fn from(source: poiesis_doc::Error) -> Self {
        Self::Doc(Box::new(source))
    }
}

impl From<poiesis_doc::pandoc::PandocError> for ExportMatrixError {
    fn from(source: poiesis_doc::pandoc::PandocError) -> Self {
        Self::Pandoc(Box::new(source))
    }
}

impl From<std::string::FromUtf8Error> for ExportMatrixError {
    fn from(source: std::string::FromUtf8Error) -> Self {
        Self::Utf8(source)
    }
}

#[test]
fn export_matrix() -> ExportMatrixResult<()> {
    let document = canonical_document();
    let math_document = math_document();

    let missing_probe_error = {
        let _guard = PathGuard::set(OsStr::new(MISSING_PANDOC_DIR));
        match PandocProbe::check().require() {
            Ok(()) => {
                return Err(ExportMatrixError::Unexpected(
                    "missing pandoc probe unexpectedly succeeded",
                ));
            }
            Err(err) => err.to_string(),
        }
    };
    insta::assert_snapshot!("pandoc_probe_missing", missing_probe_error);

    let too_old_probe_error = {
        let probe_dir = PathBuf::from(TOO_OLD_PANDOC_DIR);
        fs::create_dir_all(&probe_dir)?;
        let fake_pandoc = probe_dir.join("pandoc");
        fs::write(
            &fake_pandoc,
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  printf 'pandoc 2.19.2\\n'\n  exit 0\nfi\nprintf 'old pandoc\\n'\n",
        )?;
        make_executable(&fake_pandoc)?;

        let _guard = PathGuard::set(OsStr::new(TOO_OLD_PANDOC_DIR));
        match PandocProbe::check().require() {
            Ok(()) => {
                return Err(ExportMatrixError::Unexpected(
                    "too-old pandoc probe unexpectedly succeeded",
                ));
            }
            Err(err) => err.to_string(),
        }
    };
    insta::assert_snapshot!("pandoc_probe_too_old", too_old_probe_error);

    let fake_pandoc_dir = tempdir()?;
    let fake_pandoc = fake_pandoc_dir.path().join("pandoc");
    let fake_pandoc_log = fake_pandoc_dir.path().join("pandoc.log");
    fs::write(
        &fake_pandoc,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  printf 'pandoc 3.7.0.2\\n'\n  exit 0\nfi\nif [ -n \"${PANDOC_RUN_LOG:-}\" ]; then\n  printf '%s\\n' \"$*\" >> \"$PANDOC_RUN_LOG\"\nfi\nprintf 'subprocess-ok'\n",
    )?;
    make_executable(&fake_pandoc)?;

    let runner = PandocRunner::probe(Some(&fake_pandoc))?;
    let rendered = runner.render(
        b"{}",
        &DocOpts::markdown(),
        &[(
            "PANDOC_RUN_LOG".to_owned(),
            fake_pandoc_log.display().to_string(),
        )],
    )?;
    assert_eq!(rendered, b"subprocess-ok");
    let log = fs::read_to_string(&fake_pandoc_log)?;
    assert!(log.contains("-f json -t gfm"), "pandoc must be spawned");

    if !pandoc_present() {
        return Ok(());
    }

    let docx = render_doc(&document, &DocOpts::docx())?;
    assert_pk_magic(&docx);
    let docx_text = inspect_docx(&docx)?.paragraphs.join("\n");

    let odt = render_odt_from_doc(&document)?;
    assert_pk_magic(&odt);

    let typst_pdf = render_pdf_from_doc(&document)?;
    assert_pdf_magic(&typst_pdf);

    let html = String::from_utf8(render_doc(&document, &DocOpts::html())?)?;
    assert!(!html.is_empty(), "HTML output must not be empty");

    let md = String::from_utf8(render_doc(&document, &DocOpts::markdown())?)?;
    assert!(!md.is_empty(), "Markdown output must not be empty");

    let latex = String::from_utf8(render_doc(&document, &DocOpts::latex())?)?;
    assert!(!latex.is_empty(), "LaTeX output must not be empty");

    let epub = render_doc(&document, &DocOpts::epub())?;
    assert_pk_magic(&epub);

    let docx_cite = citation_marker(&docx_text)
        .ok_or_else(|| std::io::Error::other("DOCX output missing citation number"))?;
    let html_cite = citation_marker(&html)
        .ok_or_else(|| std::io::Error::other("HTML output missing citation number"))?;
    let md_cite = citation_marker(&md)
        .ok_or_else(|| std::io::Error::other("Markdown output missing citation number"))?;
    assert_eq!(docx_cite, html_cite);
    assert_eq!(html_cite, md_cite);

    if which::which("xelatex").is_ok() || which::which("lualatex").is_ok() {
        let math_pdf = render_pdf_from_doc(&math_document)?;
        assert_pdf_magic(&math_pdf);

        let math_latex = String::from_utf8(render_doc(&math_document, &DocOpts::latex())?)?;
        assert!(math_latex.contains("\\begin{document}"));
        assert!(math_latex.contains("x^2 + y^2 = z^2"));
    }

    Ok(())
}

fn canonical_document() -> Document {
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
                        value: 10.0,
                        unit: Unit::Number,
                    },
                },
                Point {
                    label: Some(poiesis_charts::CiteOrText::Text("FEB".to_owned())),
                    x: None,
                    y: FactCite {
                        id: FactId("rev-2".to_owned()),
                        value: 12.0,
                        unit: Unit::Number,
                    },
                },
            ],
            tone: ToneRef::Indexed(0),
            axis: AxisSide::Left,
            style: SeriesStyle::Default,
        }],
        axes: Default::default(),
        legend: LegendSpec::Auto,
        data_labels: false,
        caption: None,
    };

    let chart_bytes = serde_json::to_vec(&chart).unwrap_or_else(|e| panic!("chart json: {e}"));

    Document {
        metadata: Metadata {
            title: "Export Matrix".to_owned(),
            author: Some("alice".to_owned()),
            created: None,
        },
        content: vec![
            Block::Heading {
                level: 1,
                text: RichText::from("Export Matrix"),
            },
            Block::Paragraph(RichText {
                spans: vec![
                    Span::Plain("See ".to_owned()),
                    Span::Cite("fact-42".to_owned()),
                    Span::Plain(" for details.".to_owned()),
                ],
            }),
            Block::Table(Table {
                headers: vec!["Quarter".to_owned(), "Value".to_owned()],
                rows: vec![vec![RichText::from("North"), RichText::from("10")]],
            }),
            Block::Image(Image {
                data: chart_bytes,
                mime: "application/vnd.poiesis.chart+json".to_owned(),
                alt: "Revenue trend".to_owned(),
            }),
        ],
    }
}

fn math_document() -> Document {
    // Math-only (no chart figure): this exercises the DisplayMath -> LaTeX-engine
    // routing. A chart SVG on the LaTeX-PDF path would need rsvg-convert + svg.sty
    // (external TeX tooling); figure embedding is covered by the docx/html exports.
    Document {
        metadata: Metadata {
            title: "Math".to_owned(),
            author: Some("alice".to_owned()),
            created: None,
        },
        content: vec![
            Block::Heading {
                level: 1,
                text: RichText::from("Math"),
            },
            Block::Paragraph(RichText::from("The Pythagorean identity:")),
            Block::DisplayMath("x^2 + y^2 = z^2".to_owned()),
        ],
    }
}

fn pandoc_present() -> bool {
    which::which("pandoc").is_ok()
}

fn assert_pk_magic(bytes: &[u8]) {
    assert!(
        bytes.starts_with(b"PK"),
        "archive output must start with PK"
    );
}

fn assert_pdf_magic(bytes: &[u8]) {
    assert!(
        bytes.starts_with(b"%PDF"),
        "PDF output must start with %PDF"
    );
}

fn citation_marker(text: &str) -> Option<String> {
    for (start, _) in text.match_indices('[') {
        // Markdown escapes display brackets as "\[1\]" and footnote refs as "[^N]";
        // rich formats use "[N]". All carry the same citation NUMBER — normalise to "[N]".
        let mut after = start + 1;
        if text[after..].starts_with('^') {
            after += 1;
        }
        let digits: String = text[after..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if digits.is_empty() {
            continue;
        }
        let end = after + digits.len();
        if text[end..].starts_with(']') || text[end..].starts_with("\\]") {
            return Some(format!("[{digits}]"));
        }
    }
    None
}

fn make_executable(path: &Path) -> ExportMatrixResult<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms)?;
    }

    Ok(())
}

struct PathGuard {
    original: Option<OsString>,
}

impl PathGuard {
    fn set(value: &OsStr) -> Self {
        let original = env::var_os("PATH");
        #[expect(
            unsafe_code,
            reason = "test-only PATH mutation is serialised by the single integration test"
        )]
        unsafe {
            env::set_var("PATH", value);
        }
        Self { original }
    }
}

impl Drop for PathGuard {
    fn drop(&mut self) {
        match &self.original {
            Some(value) => {
                #[expect(
                    unsafe_code,
                    reason = "test-only PATH restoration follows the same guarded mutation"
                )]
                unsafe {
                    env::set_var("PATH", value);
                }
            }
            None => {
                #[expect(
                    unsafe_code,
                    reason = "test-only PATH restoration follows the same guarded mutation"
                )]
                unsafe {
                    env::remove_var("PATH");
                }
            }
        }
    }
}
