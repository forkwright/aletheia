//! XLSX rendering backend using the `rust_xlsxwriter` crate.
//!
//! Each [`Table`] block in the document becomes one worksheet. Heading and
//! Paragraph blocks are written as single-row text entries above the first
//! table on the active sheet. [`PageBreak`] advances to a new worksheet.
//!
//! [`Table`]: poiesis_core::block::Table
//! [`PageBreak`]: poiesis_core::Block::PageBreak

use poiesis_core::{Block, Document, Renderer};
use rust_xlsxwriter::{Workbook, XlsxError};
use snafu::Snafu;

/// Errors produced by the XLSX renderer.
#[derive(Debug, Snafu)]
pub enum XlsxRendererError {
    /// `rust_xlsxwriter` returned an error.
    #[snafu(display("XLSX error: {message}"))]
    Xlsx {
        /// Human-readable error description.
        message: String,
    },
}

impl From<XlsxError> for XlsxRendererError {
    fn from(e: XlsxError) -> Self {
        Self::Xlsx {
            message: e.to_string(),
        }
    }
}

/// Renders a [`Document`] to an Excel XLSX byte vector.
///
/// The document title becomes the first sheet name. Each [`Block::Table`]
/// produces one worksheet. Non-table blocks are written as plain-text rows
/// at the top of the current sheet.
pub struct XlsxRenderer;

impl XlsxRenderer {
    /// Construct a new `XlsxRenderer`.
    pub fn new() -> Self {
        Self
    }
}

impl Default for XlsxRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer for XlsxRenderer {
    type Error = XlsxRendererError;

    fn format(&self) -> &'static str {
        "xlsx"
    }

    fn render(&self, doc: &Document) -> Result<Vec<u8>, Self::Error> {
        let mut workbook = Workbook::new();

        // Ensure at least one worksheet exists.
        let default_sheet_name = sanitize_sheet_name(&doc.metadata.title);
        let mut sheet_index: usize = 0;
        let mut current_row: u32 = 0;

        // Lazy-create the first worksheet when we see the first block.
        let mut worksheet = workbook.add_worksheet();
        worksheet
            .set_name(&default_sheet_name)
            .map_err(XlsxRendererError::from)?;

        for block in &doc.content {
            match block {
                Block::Heading { level: _, text } => {
                    worksheet
                        .write(current_row, 0, text.plain_text().as_str())
                        .map_err(XlsxRendererError::from)?;
                    current_row += 1;
                }
                Block::Paragraph(rt) => {
                    worksheet
                        .write(current_row, 0, rt.plain_text().as_str())
                        .map_err(XlsxRendererError::from)?;
                    current_row += 1;
                }
                Block::Table(table) => {
                    // Header row
                    for (col, header) in table.headers.iter().enumerate() {
                        let col_u16 = u16::try_from(col).map_err(|e| XlsxRendererError::Xlsx {
                            message: format!("column index {col} exceeds u16 max: {e}"),
                        })?;
                        worksheet
                            .write(current_row, col_u16, header.as_str())
                            .map_err(XlsxRendererError::from)?;
                    }
                    current_row += 1;

                    // Data rows
                    for row in &table.rows {
                        for (col, cell) in row.iter().enumerate() {
                            let col_u16 = u16::try_from(col).map_err(|e| XlsxRendererError::Xlsx {
                                message: format!("column index {col} exceeds u16 max: {e}"),
                            })?;
                            worksheet
                                .write(current_row, col_u16, cell.plain_text().as_str())
                                .map_err(XlsxRendererError::from)?;
                        }
                        current_row += 1;
                    }
                    current_row += 1; // blank row after table
                }
                Block::List { ordered: _, items } => {
                    for item in items {
                        worksheet
                            .write(current_row, 0, item.content.plain_text().as_str())
                            .map_err(XlsxRendererError::from)?;
                        current_row += 1;
                    }
                }
                Block::Image(img) => {
                    let alt = format!("[Image: {}]", img.alt);
                    worksheet
                        .write(current_row, 0, alt.as_str())
                        .map_err(XlsxRendererError::from)?;
                    current_row += 1;
                }
                Block::PageBreak => {
                    sheet_index += 1;
                    let sheet_name = format!("Sheet{}", sheet_index + 1);
                    worksheet = workbook.add_worksheet();
                    worksheet
                        .set_name(&sheet_name)
                        .map_err(XlsxRendererError::from)?;
                    current_row = 0;
                }
            }
        }

        workbook
            .save_to_buffer()
            .map_err(XlsxRendererError::from)
    }
}

/// Sanitize a string for use as an Excel sheet name (max 31 chars, no `[]:*?/\`).
fn sanitize_sheet_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .filter(|c| !matches!(c, '[' | ']' | ':' | '*' | '?' | '/' | '\\'))
        .take(31)
        .collect();

    if sanitized.is_empty() {
        "Sheet1".to_owned()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use poiesis_core::{Block, Document, Metadata, RichText, Span, block::Table};

    fn sample_doc() -> Document {
        Document {
            metadata: Metadata {
                title: "XLSX Test".to_owned(),
                author: None,
                created: None,
            },
            content: vec![
                Block::Heading {
                    level: 1,
                    text: RichText {
                        spans: vec![Span::Plain("Report Title".to_owned())],
                    },
                },
                Block::Table(Table {
                    headers: vec!["Name".to_owned(), "Score".to_owned()],
                    rows: vec![
                        vec![
                            RichText { spans: vec![Span::Plain("Alice".to_owned())] },
                            RichText { spans: vec![Span::Plain("95".to_owned())] },
                        ],
                        vec![
                            RichText { spans: vec![Span::Plain("Bob".to_owned())] },
                            RichText { spans: vec![Span::Plain("87".to_owned())] },
                        ],
                    ],
                }),
            ],
        }
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertion")]
    fn xlsx_produces_nonempty_bytes() {
        let r = XlsxRenderer::new();
        let bytes = r.render(&sample_doc()).expect("XLSX render failed");
        assert!(!bytes.is_empty(), "rendered XLSX must not be empty");
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertion")]
    #[expect(clippy::indexing_slicing, reason = "test assertions on known-good data")]
    fn xlsx_starts_with_pk_magic() {
        // WHY: XLSX is a ZIP archive; valid files start with PK (0x50 0x4B).
        let r = XlsxRenderer::new();
        let bytes = r.render(&sample_doc()).expect("XLSX render failed");
        assert_eq!(&bytes[..2], b"PK", "XLSX output should be a valid ZIP/XLSX");
    }

    #[test]
    fn sanitize_sheet_name_truncates() {
        let long = "a".repeat(50);
        let sanitized = sanitize_sheet_name(&long);
        assert!(sanitized.len() <= 31);
    }

    #[test]
    fn sanitize_sheet_name_strips_illegal_chars() {
        assert_eq!(sanitize_sheet_name("foo[bar]"), "foobar");
    }

    #[test]
    fn sanitize_sheet_name_empty_fallback() {
        assert_eq!(sanitize_sheet_name(""), "Sheet1");
    }
}
