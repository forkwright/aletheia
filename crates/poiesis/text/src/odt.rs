//! ODT rendering backend using a clean-room ZIP/XML implementation.
//!
//! `OpenDocument` Text is a ZIP archive containing several XML files:
//! - `mimetype` (stored, first entry)
//! - `META-INF/manifest.xml`
//! - `meta.xml`
//! - `styles.xml`
//! - `content.xml`

use std::io::Write as _;

use poiesis_core::{Block, Document, Renderer, RichText, Span};
use snafu::Snafu;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

/// Errors produced by the ODT renderer.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum OdtError {
    /// A ZIP I/O error occurred while building the ODT package.
    #[snafu(display("ODT zip error: {message}"))]
    Zip {
        /// Human-readable ZIP error description.
        message: String,
    },
}

/// Renders a [`Document`] to an ODT byte vector.
pub struct OdtRenderer;

impl OdtRenderer {
    /// Construct a new `OdtRenderer`.
    pub fn new() -> Self {
        Self
    }
}

impl Default for OdtRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer for OdtRenderer {
    type Error = OdtError;

    fn format(&self) -> &'static str {
        "odt"
    }

    fn render(&self, doc: &Document) -> Result<Vec<u8>, Self::Error> {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(cursor);

        zip.start_file(
            "mimetype",
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
        )
        .map_err(|e| OdtError::Zip {
            message: e.to_string(),
        })?;
        zip.write_all(b"application/vnd.oasis.opendocument.text")
            .map_err(|e| OdtError::Zip {
                message: e.to_string(),
            })?;

        write_entry(&mut zip, "META-INF/manifest.xml", &build_manifest())?;
        write_entry(&mut zip, "meta.xml", &build_meta(doc))?;
        write_entry(&mut zip, "styles.xml", build_styles())?;
        write_entry(&mut zip, "content.xml", &build_content(doc))?;

        let cursor = zip.finish().map_err(|e| OdtError::Zip {
            message: e.to_string(),
        })?;
        Ok(cursor.into_inner())
    }
}

fn write_entry(
    zip: &mut ZipWriter<std::io::Cursor<Vec<u8>>>,
    name: &str,
    content: &str,
) -> Result<(), OdtError> {
    zip.start_file(name, SimpleFileOptions::default())
        .map_err(|e| OdtError::Zip {
            message: e.to_string(),
        })?;
    zip.write_all(content.as_bytes())
        .map_err(|e| OdtError::Zip {
            message: e.to_string(),
        })
}

fn build_manifest() -> String {
    concat!(
        r#"<?xml version="1.0" encoding="UTF-8"?>"#,
        "\n",
        r#"<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.2">"#,
        "\n",
        r#"  <manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.text"/>"#,
        "\n",
        r#"  <manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>"#,
        "\n",
        r#"  <manifest:file-entry manifest:full-path="styles.xml" manifest:media-type="text/xml"/>"#,
        "\n",
        r#"  <manifest:file-entry manifest:full-path="meta.xml" manifest:media-type="text/xml"/>"#,
        "\n",
        r#"</manifest:manifest>"#,
    )
    .to_owned()
}

fn build_meta(doc: &Document) -> String {
    let title = xml_escape(&doc.metadata.title);
    let author = doc
        .metadata
        .author
        .as_deref()
        .map(xml_escape)
        .unwrap_or_default();

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-meta xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
  xmlns:dc="http://purl.org/dc/elements/1.1/"
  xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0"
  office:version="1.2">
  <office:meta>
    <dc:title>{title}</dc:title>
    <dc:creator>{author}</dc:creator>
  </office:meta>
</office:document-meta>"#
    )
}

fn build_styles() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-styles
  xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
  xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0"
  xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"
  xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0"
  office:version="1.2">
  <office:styles>
    <style:style style:name="Heading_1" style:family="paragraph" style:class="text">
      <style:paragraph-properties fo:margin-top="6pt" fo:margin-bottom="3pt"/>
      <style:text-properties fo:font-size="20pt" fo:font-weight="bold"/>
    </style:style>
    <style:style style:name="Heading_2" style:family="paragraph" style:class="text">
      <style:paragraph-properties fo:margin-top="5pt" fo:margin-bottom="2pt"/>
      <style:text-properties fo:font-size="16pt" fo:font-weight="bold"/>
    </style:style>
    <style:style style:name="Heading_3" style:family="paragraph" style:class="text">
      <style:paragraph-properties fo:margin-top="4pt" fo:margin-bottom="2pt"/>
      <style:text-properties fo:font-size="14pt" fo:font-weight="bold"/>
    </style:style>
    <style:style style:name="Text_Body" style:family="paragraph" style:class="text">
      <style:paragraph-properties fo:margin-bottom="4pt"/>
      <style:text-properties fo:font-size="11pt"/>
    </style:style>
    <style:style style:name="Bold" style:family="text">
      <style:text-properties fo:font-weight="bold"/>
    </style:style>
    <style:style style:name="Italic" style:family="text">
      <style:text-properties fo:font-style="italic"/>
    </style:style>
    <style:style style:name="Code" style:family="text">
      <style:text-properties fo:font-family="monospace" style:font-name="monospace"/>
    </style:style>
  </office:styles>
</office:document-styles>"#
}

fn build_content(doc: &Document) -> String {
    let mut body = String::new();
    for block in &doc.content {
        render_block(block, &mut body);
    }

    let mut content = String::new();
    push_line(&mut content, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    push_line(&mut content, r"<office:document-content");
    push_line(
        &mut content,
        r#"  xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0""#,
    );
    push_line(
        &mut content,
        r#"  xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0""#,
    );
    push_line(
        &mut content,
        r#"  xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0""#,
    );
    push_line(
        &mut content,
        r#"  xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0""#,
    );
    push_line(
        &mut content,
        r#"  xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0""#,
    );
    push_line(
        &mut content,
        r#"  xmlns:xlink="http://www.w3.org/1999/xlink""#,
    );
    push_line(&mut content, r#"  office:version="1.2">"#);
    push_line(&mut content, "  <office:body>");
    push_line(&mut content, "    <office:text>");
    content.push_str(&body);
    push_line(&mut content, "    </office:text>");
    push_line(&mut content, "  </office:body>");
    push_line(&mut content, "</office:document-content>");
    content
}

#[expect(
    clippy::too_many_lines,
    reason = "ODT/XML block mapping is intentionally centralized for readability"
)]
fn render_block(block: &Block, out: &mut String) {
    match block {
        Block::Heading { level, text } => {
            let style = heading_style(*level);
            let outline = (*level).clamp(1, 6);
            push_line(
                out,
                &format!(
                    "      <text:h text:style-name=\"{style}\" text:outline-level=\"{outline}\">{}</text:h>",
                    render_rich_text(text)
                ),
            );
        }
        Block::Paragraph(rt) => {
            push_line(
                out,
                &format!(
                    "      <text:p text:style-name=\"Text_Body\">{}</text:p>",
                    render_rich_text(rt)
                ),
            );
        }
        Block::Note(note) => {
            let text = format!(
                "{}: {}",
                xml_escape(note.kind.label()),
                render_rich_text(&note.body)
            );
            push_line(
                out,
                &format!("      <text:p text:style-name=\"Text_Body\">{text}</text:p>"),
            );
        }
        Block::DisplayMath(expr) => {
            push_line(
                out,
                &format!(
                    "      <text:p text:style-name=\"Text_Body\">{}</text:p>",
                    xml_escape(expr)
                ),
            );
        }
        Block::RawBlock { content, .. } => {
            push_line(
                out,
                &format!(
                    "      <text:p text:style-name=\"Text_Body\">{}</text:p>",
                    xml_escape(content)
                ),
            );
        }
        Block::Table(table) => {
            out.push_str(
                "      <table:table>\n        <table:table-header-rows>\n          <table:table-row>\n",
            );
            for header in &table.headers {
                push_line(
                    out,
                    &format!(
                        "            <table:table-cell><text:p>{}</text:p></table:table-cell>",
                        xml_escape(header)
                    ),
                );
            }
            out.push_str("          </table:table-row>\n        </table:table-header-rows>\n");
            for row in &table.rows {
                out.push_str("        <table:table-row>\n");
                for (col_idx, _) in table.headers.iter().enumerate() {
                    let cell_text = row.get(col_idx).map(render_rich_text).unwrap_or_default();
                    push_line(
                        out,
                        &format!(
                            "          <table:table-cell><text:p>{cell_text}</text:p></table:table-cell>"
                        ),
                    );
                }
                out.push_str("        </table:table-row>\n");
            }
            out.push_str("      </table:table>\n");
        }
        Block::List { ordered, items } => {
            let list_style = if *ordered { "OL" } else { "UL" };
            push_line(
                out,
                &format!("      <text:list text:style-name=\"{list_style}\">"),
            );
            for item in items {
                push_line(
                    out,
                    &format!(
                        "        <text:list-item><text:p>{}</text:p></text:list-item>",
                        render_rich_text(&item.content)
                    ),
                );
            }
            push_line(out, "      </text:list>");
        }
        Block::Image(img) => {
            push_line(
                out,
                &format!(
                    "      <text:p text:style-name=\"Text_Body\">[Image: {}]</text:p>",
                    xml_escape(&img.alt)
                ),
            );
        }
        Block::PageBreak => {
            push_line(out, "      <text:p><text:page-break/></text:p>");
        }
    }
}

fn render_rich_text(rt: &RichText) -> String {
    rt.spans.iter().map(render_span).collect()
}

fn render_span(span: &Span) -> String {
    match span {
        Span::Plain(s) | Span::Cite(s) => xml_escape(s),
        Span::Bold(s) => format!(
            "<text:span text:style-name=\"Bold\">{}</text:span>",
            xml_escape(s)
        ),
        Span::Italic(s) => format!(
            "<text:span text:style-name=\"Italic\">{}</text:span>",
            xml_escape(s)
        ),
        Span::Code(s) => format!(
            "<text:span text:style-name=\"Code\">{}</text:span>",
            xml_escape(s)
        ),
        Span::Link { text, url } => format!(
            "<text:a xlink:type=\"simple\" xlink:href=\"{}\">{}</text:a>",
            xml_escape(url),
            xml_escape(text)
        ),
    }
}

fn heading_style(level: u8) -> &'static str {
    match level {
        1 => "Heading_1",
        2 => "Heading_2",
        _ => "Heading_3",
    }
}

fn push_line(out: &mut String, line: &str) {
    out.push_str(line);
    out.push('\n');
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use poiesis_core::{Block, Document, Metadata, RichText, Span, block::Table};

    use super::*;

    fn sample_doc() -> Document {
        Document {
            metadata: Metadata {
                title: "ODT Test".to_owned(),
                author: Some("Bob".to_owned()),
                created: None,
            },
            content: vec![
                Block::Heading {
                    level: 1,
                    text: RichText {
                        spans: vec![Span::Plain("Section One".to_owned())],
                    },
                },
                Block::Paragraph(RichText {
                    spans: vec![
                        Span::Plain("Hello ".to_owned()),
                        Span::Bold("world".to_owned()),
                    ],
                }),
                Block::Table(Table {
                    headers: vec!["A".to_owned(), "B".to_owned()],
                    rows: vec![vec![
                        RichText {
                            spans: vec![Span::Plain("x".to_owned())],
                        },
                        RichText {
                            spans: vec![Span::Plain("y".to_owned())],
                        },
                    ]],
                }),
            ],
        }
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertion")]
    fn odt_produces_nonempty_bytes() {
        let renderer = OdtRenderer::new();
        let bytes = renderer.render(&sample_doc()).expect("ODT render failed");
        assert!(!bytes.is_empty());
    }

    #[test]
    #[expect(
        clippy::expect_used,
        clippy::indexing_slicing,
        reason = "test assertions on known-good data"
    )]
    fn odt_starts_with_pk_magic() {
        let renderer = OdtRenderer::new();
        let bytes = renderer.render(&sample_doc()).expect("ODT render failed");
        assert_eq!(&bytes[..2], b"PK", "output should be a valid ZIP/ODT");
    }

    #[test]
    fn xml_escape_handles_all_chars() {
        assert_eq!(
            xml_escape("a&b<c>d\"e'f"),
            "a&amp;b&lt;c&gt;d&quot;e&apos;f"
        );
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn odt_includes_span_styles_and_xlink_ns() {
        let doc = Document {
            metadata: Metadata {
                title: "Styles Test".to_owned(),
                author: None,
                created: None,
            },
            content: vec![Block::Paragraph(RichText {
                spans: vec![
                    Span::Bold("bold text".to_owned()),
                    Span::Italic("italic text".to_owned()),
                    Span::Code("code text".to_owned()),
                    Span::Cite("fact-42".to_owned()),
                    Span::Link {
                        text: "link".to_owned(),
                        url: "https://example.com".to_owned(),
                    },
                ],
            })],
        };
        let bytes = OdtRenderer::new().render(&doc).expect("render");
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).expect("zip");

        let mut styles_xml = String::new();
        archive
            .by_name("styles.xml")
            .expect("styles.xml")
            .read_to_string(&mut styles_xml)
            .expect("read");
        assert!(
            styles_xml.contains("style:name=\"Bold\""),
            "must declare Bold style"
        );
        assert!(
            styles_xml.contains("style:name=\"Italic\""),
            "must declare Italic style"
        );
        assert!(
            styles_xml.contains("style:name=\"Code\""),
            "must declare Code style"
        );

        let mut content_xml = String::new();
        archive
            .by_name("content.xml")
            .expect("content.xml")
            .read_to_string(&mut content_xml)
            .expect("read");
        assert!(
            content_xml.contains("xmlns:xlink=\"http://www.w3.org/1999/xlink\""),
            "must declare xlink namespace"
        );
    }
}
