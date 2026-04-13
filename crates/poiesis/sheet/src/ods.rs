//! ODS rendering backend using the `spreadsheet-ods` crate.
//!
//! Maps the document's [`Table`] blocks to ODS sheets. Heading and Paragraph
//! blocks are written as single-row text entries. [`PageBreak`] advances to a
//! new sheet.
//!
//! [`Table`]: poiesis_core::block::Table
//! [`PageBreak`]: poiesis_core::Block::PageBreak

use poiesis_core::{Block, Document, Renderer};
use snafu::Snafu;
use spreadsheet_ods::{WorkBook, Sheet, write_ods_buf};

/// Errors produced by the ODS renderer.
#[derive(Debug, Snafu)]
pub enum OdsRendererError {
    /// `spreadsheet-ods` returned an error while writing the ODS file.
    #[snafu(display("ODS error: {message}"))]
    Ods {
        /// Human-readable error description.
        message: String,
    },
}

/// Renders a [`Document`] to an ODS byte vector.
///
/// The document title becomes the first sheet name. Tables map directly to
/// sheets. Non-table blocks are rendered as plain-text rows.
pub struct OdsRenderer;

impl OdsRenderer {
    /// Construct a new `OdsRenderer`.
    pub fn new() -> Self {
        Self
    }
}

impl Default for OdsRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer for OdsRenderer {
    type Error = OdsRendererError;

    fn format(&self) -> &'static str {
        "ods"
    }

    fn render(&self, doc: &Document) -> Result<Vec<u8>, Self::Error> {
        let mut workbook = WorkBook::new_empty();

        let first_sheet_name = truncate_sheet_name(&doc.metadata.title);
        let mut sheet = Sheet::new(first_sheet_name);
        let mut row: u32 = 0;
        let mut sheet_count: usize = 1;

        for block in &doc.content {
            match block {
                Block::Heading { level: _, text } => {
                    sheet.set_value(row, 0, text.plain_text());
                    row += 1;
                }
                Block::Paragraph(rt) => {
                    sheet.set_value(row, 0, rt.plain_text());
                    row += 1;
                }
                Block::Table(table) => {
                    // Header row
                    for (col, header) in table.headers.iter().enumerate() {
                        // WHY: column count is bounded by the document model;
                        // u32 holds up to 4 billion columns which no real table
                        // approaches. Saturate rather than panic on pathological input.
                        let col_u32 = u32::try_from(col).unwrap_or(u32::MAX);
                        sheet.set_value(row, col_u32, header.as_str());
                    }
                    row += 1;

                    // Data rows
                    for data_row in &table.rows {
                        for (col, cell) in data_row.iter().enumerate() {
                            let col_u32 = u32::try_from(col).unwrap_or(u32::MAX);
                            sheet.set_value(row, col_u32, cell.plain_text());
                        }
                        row += 1;
                    }
                    row += 1;
                }
                Block::List { ordered: _, items } => {
                    for item in items {
                        sheet.set_value(row, 0, item.content.plain_text());
                        row += 1;
                    }
                }
                Block::Image(img) => {
                    sheet.set_value(row, 0, format!("[Image: {}]", img.alt));
                    row += 1;
                }
                Block::PageBreak => {
                    workbook.push_sheet(sheet);
                    sheet_count += 1;
                    sheet = Sheet::new(format!("Sheet{sheet_count}"));
                    row = 0;
                }
            }
        }

        workbook.push_sheet(sheet);

        let buf = Vec::new();
        write_ods_buf(&mut workbook, buf).map_err(|e| OdsRendererError::Ods {
            message: e.to_string(),
        })
    }
}

/// Truncate sheet name to 31 characters (ODS limit matches XLSX).
fn truncate_sheet_name(name: &str) -> String {
    let truncated: String = name.chars().take(31).collect();
    if truncated.is_empty() {
        "Sheet1".to_owned()
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use poiesis_core::{Block, Document, Metadata, RichText, Span, block::Table};

    fn sample_doc() -> Document {
        Document {
            metadata: Metadata {
                title: "ODS Test".to_owned(),
                author: None,
                created: None,
            },
            content: vec![Block::Table(Table {
                headers: vec!["Item".to_owned(), "Qty".to_owned()],
                rows: vec![vec![
                    RichText { spans: vec![Span::Plain("Widget".to_owned())] },
                    RichText { spans: vec![Span::Plain("42".to_owned())] },
                ]],
            })],
        }
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertion")]
    fn ods_produces_nonempty_bytes() {
        let r = OdsRenderer::new();
        let bytes = r.render(&sample_doc()).expect("ODS render failed");
        assert!(!bytes.is_empty(), "rendered ODS must not be empty");
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertion")]
    #[expect(clippy::indexing_slicing, reason = "test assertions on known-good data")]
    fn ods_starts_with_pk_magic() {
        // WHY: ODS is a ZIP archive; valid files start with PK (0x50 0x4B).
        let r = OdsRenderer::new();
        let bytes = r.render(&sample_doc()).expect("ODS render failed");
        assert_eq!(&bytes[..2], b"PK", "ODS output should be a valid ZIP/ODS");
    }
}
