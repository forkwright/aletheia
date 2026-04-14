//! ODT rendering backend — clean-room ZIP/XML implementation.
//!
//! `OpenDocument` Text is a ZIP archive containing several XML files:
//! - `mimetype` (uncompressed, first entry)
//! - `META-INF/manifest.xml`
//! - `meta.xml`
//! - `styles.xml`
//! - `content.xml`
//!
//! This renderer produces a conformant ODF 1.2 document using only the `zip`
//! crate for packaging and hand-built XML strings for content.

use std::fmt::Write as FmtWrite;
use std::io::Write as IoWrite;

use poiesis_core::{Block, Document, RichText, Renderer, Span};
use snafu::Snafu;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

/// Errors produced by the ODT renderer.
#[derive(Debug, Snafu)]
pub enum OdtError {
    /// A ZIP I/O error occurred while building the ODT package.
    #[snafu(display("ODT zip error: {message}"))]
    Zip {
        /// Human-readable ZIP error description.
        message: String,
    },
}

/// Renders a [`Document`] to an ODT byte vector.
///
/// The output is a self-contained `OpenDocument` Text file (ODF 1.2) that can be
/// opened in `LibreOffice`, Apache `OpenOffice`, or any conformant editor.
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
        let buf = Vec::new();
        let cursor = std::io::Cursor::new(buf);
        let mut zip = ZipWriter::new(cursor);

        // `mimetype` MUST be the first entry and MUST NOT be compressed.
        zip.start_file(
            "mimetype",
            SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
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
    zip.write_all(content.as_bytes()).map_err(|e| OdtError::Zip {
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
        r"</manifest:manifest>",
    ).to_owned()
}

fn build_meta(doc: &Document) -> String {
    let title = xml_escape(&doc.metadata.title);
    let author = doc
        .metadata
        .author
        .as_deref()
        .map(xml_escape)
        .unwrap_or_default();
    // XML namespace URIs below (http://purl.org/dc/...) are identifiers, not network endpoints
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
    // Minimal ODF 1.2 style sheet defining Heading_1 … Heading_3 and Text_Body.
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
  </office:styles>
</office:document-styles>"#
}

fn build_content(doc: &Document) -> String {
    let mut body = String::new();
    for block in &doc.content {
        render_block(block, &mut body);
    }

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content
  xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
  xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"
  xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0"
  xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0"
  xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0"
  office:version="1.2">
  <office:body>
    <office:text>
{body}
    </office:text>
  </office:body>
</office:document-content>"#
    )
}

fn render_block(block: &Block, out: &mut String) {
    match block {
        Block::Heading { level, text } => {
            let style = heading_style(*level);
            let outline = level.min(&6);
            let _ = writeln!(
                out,
                "      <text:h text:style-name=\"{style}\" text:outline-level=\"{outline}\">{}</text:h>",
                render_rich_text(text)
            );
        }
        Block::Paragraph(rt) => {
            let _ = writeln!(
                out,
                "      <text:p text:style-name=\"Text_Body\">{}</text:p>",
                render_rich_text(rt)
            );
        }
        Block::List { ordered, items } => {
            let list_style = if *ordered { "OL" } else { "UL" };
            let _ = writeln!(out, "      <text:list text:style-name=\"{list_style}\">");
            for item in items {
                let _ = writeln!(
                    out,
                    "        <text:list-item><text:p>{}</text:p></text:list-item>",
                    render_rich_text(&item.content)
                );
            }
            out.push_str("      </text:list>\n");
        }
        Block::Table(table) => {
            let col_count = table.headers.len();
            out.push_str(
                "      <table:table>\n        <table:table-header-rows>\n          <table:table-row>\n",
            );
            for header in &table.headers {
                let _ = writeln!(
                    out,
                    "            <table:table-cell><text:p>{}</text:p></table:table-cell>",
                    xml_escape(header)
                );
            }
            out.push_str(
                "          </table:table-row>\n        </table:table-header-rows>\n",
            );
            for row in &table.rows {
                out.push_str("        <table:table-row>\n");
                for col_idx in 0..col_count {
                    let cell_text = row
                        .get(col_idx)
                        .map(render_rich_text)
                        .unwrap_or_default();
                    let _ = writeln!(
                        out,
                        "          <table:table-cell><text:p>{cell_text}</text:p></table:table-cell>"
                    );
                }
                out.push_str("        </table:table-row>\n");
            }
            out.push_str("      </table:table>\n");
        }
        Block::Image(img) => {
            // Images embedded as draw:image require an image binary in the ZIP.
            // For now emit the alt text as a paragraph — img.data is available
            // for a future extension that embeds the binary.
            let _ = writeln!(
                out,
                "      <text:p text:style-name=\"Text_Body\">[Image: {}]</text:p>",
                xml_escape(&img.alt)
            );
        }
        Block::PageBreak => {
            out.push_str("      <text:p><text:page-break/></text:p>\n");
        }
    }
}

fn render_rich_text(rt: &RichText) -> String {
    rt.spans.iter().map(render_span).collect()
}

fn render_span(span: &Span) -> String {
    match span {
        Span::Plain(s) => xml_escape(s),
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

/// Escape XML special characters.
fn xml_escape(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            '&' => "&amp;".chars().collect::<Vec<_>>(),
            '<' => "&lt;".chars().collect(),
            '>' => "&gt;".chars().collect(),
            '"' => "&quot;".chars().collect(),
            '\'' => "&apos;".chars().collect(),
            other => vec![other],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use poiesis_core::{Block, Document, Metadata, RichText, Span, block::Table};

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
                        RichText { spans: vec![Span::Plain("x".to_owned())] },
                        RichText { spans: vec![Span::Plain("y".to_owned())] },
                    ]],
                }),
            ],
        }
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertion")]
    fn odt_produces_nonempty_bytes() {
        let r = OdtRenderer::new();
        let bytes = r.render(&sample_doc()).expect("ODT render failed");
        assert!(!bytes.is_empty());
    }

    #[test]
    #[expect(clippy::expect_used, clippy::indexing_slicing, reason = "test assertions on known-good data")]
    fn odt_starts_with_pk_magic() {
        // WHY: ZIP files start with PK (0x50 0x4B).
        let r = OdtRenderer::new();
        let bytes = r.render(&sample_doc()).expect("ODT render failed");
        assert_eq!(&bytes[..2], b"PK", "output should be a valid ZIP/ODT");
    }

    #[test]
    fn xml_escape_handles_all_chars() {
        assert_eq!(xml_escape("a&b<c>d\"e'f"), "a&amp;b&lt;c&gt;d&quot;e&apos;f");
    }
}
