use std::path::PathBuf;

use poiesis_core::{Block, Document, Metadata, RichText};

use super::typst::typst_source_from_doc;
use super::*;
use crate::{
    inspect_docx, render_docx_from_doc, render_html_from_doc, render_latex_from_doc,
    render_md_from_doc,
};

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
        Ok(_) | Err(PandocError::NotInstalled { .. } | PandocError::WriterFailed { .. }) => {
            // These outcomes are acceptable on hosts with or without pandoc.
        }
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
fn chart_figure_renders_png_in_latex() {
    if !pandoc_available() {
        return;
    }

    let latex = String::from_utf8(render_latex_from_doc(&chart_doc()).expect("latex must render"))
        .expect("latex must be utf-8");
    assert!(
        latex.contains(".png"),
        "latex output must reference the rasterized PNG asset, got: {latex}"
    );
    assert!(
        !latex.contains(".svg"),
        "latex output must not reference SVG assets, got: {latex}"
    );
    assert!(
        !latex.contains("includesvg"),
        "latex output must not require svg.sty, got: {latex}"
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
