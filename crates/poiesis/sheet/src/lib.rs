#![deny(missing_docs)]
//! poiesis-sheet: XLSX and ODS spreadsheet rendering backends.
//!
//! Feature flags:
//! - `xlsx` (default): Excel XLSX output via `rust_xlsxwriter`.
//! - `ods` (default): `OpenDocument` Spreadsheet output via `spreadsheet-ods`.

#[cfg(feature = "xlsx")]
pub mod error;

#[cfg(feature = "xlsx")]
pub use error::Error;

#[cfg(feature = "xlsx")]
pub mod xlsx;

#[cfg(feature = "ods")]
pub mod ods;

#[cfg(feature = "xlsx")]
pub use xlsx::XlsxRenderer;

#[cfg(feature = "ods")]
pub use ods::OdsRenderer;

#[cfg(feature = "xlsx")]
use rust_xlsxwriter::Workbook;

#[cfg(feature = "xlsx")]
use tracing::instrument;

/// Summary of a workbook returned by [`inspect_xlsx`].
#[cfg(feature = "xlsx")]
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct WorkbookSummary {
    /// Per-sheet summaries.
    pub sheets: Vec<SheetSummary>,
    /// Total number of non-empty cells across all sheets.
    pub cell_count: usize,
}

/// Summary of a single sheet returned by [`inspect_xlsx`].
#[cfg(feature = "xlsx")]
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SheetSummary {
    /// Sheet name.
    pub name: String,
    /// Number of rows with data.
    pub row_count: usize,
    /// Number of columns with data.
    pub column_count: usize,
}

/// Render a JSON workbook descriptor to XLSX bytes.
///
/// # JSON Schema
///
/// ```json
/// {
///   "sheets": [
///     {
///       "name": "Sheet1",
///       "columns": [
///         { "header": "Name", "width": 20.0 },
///         { "header": "Score" }
///       ],
///       "rows": [
///         ["Alice", 95],
///         ["Bob", 87]
///       ]
///     }
///   ]
/// }
/// ```
///
/// # Errors
///
/// Returns [`Error::InvalidSchema`] if the JSON is missing required fields,
/// or [`Error::XlsxWrite`] if `rust_xlsxwriter` fails.
#[cfg(feature = "xlsx")]
#[instrument(skip(data))]
pub fn render_xlsx(data: &serde_json::Value) -> Result<Vec<u8>, Error> {
    let sheets = data
        .get("sheets")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| Error::InvalidSchema {
            detail: "missing required field: sheets (array)".to_owned(),
        })?;

    let mut workbook = Workbook::new();

    for (sheet_idx, sheet_val) in sheets.iter().enumerate() {
        let name = sheet_val
            .get("name")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| Error::InvalidSchema {
                detail: format!("sheet {sheet_idx}: missing required field: name"),
            })?;

        let columns = sheet_val
            .get("columns")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| Error::InvalidSchema {
                detail: format!("sheet {sheet_idx}: missing required field: columns (array)"),
            })?;

        let rows = sheet_val
            .get("rows")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| Error::InvalidSchema {
                detail: format!("sheet {sheet_idx}: missing required field: rows (array)"),
            })?;

        let worksheet = workbook.add_worksheet();
        worksheet.set_name(name).map_err(|e| Error::XlsxWrite {
            message: e.to_string(),
        })?;

        // Header row
        for (col_idx, col_val) in columns.iter().enumerate() {
            let header = col_val
                .get("header")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| Error::InvalidSchema {
                    detail: format!(
                        "sheet {sheet_idx}, column {col_idx}: missing required field: header"
                    ),
                })?;

            let col_u16 = u16::try_from(col_idx).map_err(|e| Error::XlsxWrite {
                message: format!("column index {col_idx} exceeds u16 max: {e}"),
            })?;

            worksheet
                .write(0, col_u16, header)
                .map_err(|e| Error::XlsxWrite {
                    message: e.to_string(),
                })?;

            if let Some(width) = col_val.get("width").and_then(serde_json::Value::as_f64) {
                worksheet
                    .set_column_width(col_u16, width)
                    .map_err(|e| Error::XlsxWrite {
                        message: e.to_string(),
                    })?;
            }
        }

        // Data rows
        for (row_idx, row_val) in rows.iter().enumerate() {
            let cells = row_val.as_array().ok_or_else(|| Error::InvalidSchema {
                detail: format!("sheet {sheet_idx}, row {row_idx}: each row must be an array"),
            })?;

            for (col_idx, cell) in cells.iter().enumerate() {
                let col_u16 = u16::try_from(col_idx).map_err(|e| Error::XlsxWrite {
                    message: format!("column index {col_idx} exceeds u16 max: {e}"),
                })?;

                let cell_text = match cell {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::Bool(b) => b.to_string(),
                    serde_json::Value::Null => String::new(),
                    other => other.to_string(),
                };

                worksheet
                    .write(
                        u32::try_from(row_idx).map_err(|e| Error::XlsxWrite {
                            message: format!("row index {row_idx} exceeds u32 max: {e}"),
                        })? + 1,
                        col_u16,
                        cell_text.as_str(),
                    )
                    .map_err(|e| Error::XlsxWrite {
                        message: e.to_string(),
                    })?;
            }
        }
    }

    workbook.save_to_buffer().map_err(|e| Error::XlsxWrite {
        message: e.to_string(),
    })
}

/// Inspect an XLSX byte slice and return a structural summary.
///
/// # Errors
///
/// Returns [`Error::XlsxRead`] if `calamine` cannot parse the input.
#[cfg(feature = "xlsx")]
pub fn inspect_xlsx(bytes: &[u8]) -> Result<WorkbookSummary, Error> {
    use calamine::{Reader, Xlsx};

    let cursor = std::io::Cursor::new(bytes);
    let mut workbook = Xlsx::new(cursor).map_err(|e| Error::XlsxRead {
        message: e.to_string(),
    })?;

    let sheet_names = workbook.sheet_names().clone();
    let mut sheets = Vec::with_capacity(sheet_names.len());
    let mut cell_count: usize = 0;

    for name in &sheet_names {
        let range = workbook
            .worksheet_range(name)
            .map_err(|e| Error::XlsxRead {
                message: e.to_string(),
            })?;

        let row_count = range.height();
        let column_count = range.width();
        cell_count += row_count.saturating_mul(column_count);

        sheets.push(SheetSummary {
            name: name.clone(),
            row_count,
            column_count,
        });
    }

    Ok(WorkbookSummary { sheets, cell_count })
}

#[cfg(all(test, feature = "xlsx"))]
#[expect(clippy::expect_used, reason = "test assertions")]
mod json_api_tests {
    use super::*;
    use calamine::Reader;

    #[test]
    fn render_xlsx_simple_json_round_trips_through_calamine() {
        let data = serde_json::json!({
            "sheets": [
                {
                    "name": "Scores",
                    "columns": [
                        { "header": "Name", "width": 20.0 },
                        { "header": "Score" }
                    ],
                    "rows": [
                        ["Alice", 95],
                        ["Bob", 87],
                        ["Carol", 92]
                    ]
                }
            ]
        });

        let bytes = render_xlsx(&data).expect("render must succeed");

        // Round-trip through calamine
        let cursor = std::io::Cursor::new(&bytes);
        let mut workbook = calamine::Xlsx::new(cursor).expect("calamine must open xlsx");
        let sheet_names = workbook.sheet_names();
        assert_eq!(sheet_names.len(), 1);
        assert_eq!(sheet_names.first().expect("one sheet"), "Scores");

        let range = workbook
            .worksheet_range("Scores")
            .expect("sheet must exist");

        // Header row + 3 data rows
        assert_eq!(range.height(), 4);
        assert_eq!(range.width(), 2);

        let rows: Vec<Vec<String>> = range
            .rows()
            .map(|row| {
                row.iter()
                    .map(|cell| match cell {
                        calamine::Data::String(s)
                        | calamine::Data::DateTimeIso(s)
                        | calamine::Data::DurationIso(s) => s.clone(),
                        calamine::Data::Float(f) => f.to_string(),
                        calamine::Data::Int(i) => i.to_string(),
                        calamine::Data::Bool(b) => b.to_string(),
                        calamine::Data::DateTime(d) => d.to_string(),
                        calamine::Data::Error(e) => e.to_string(),
                        calamine::Data::Empty => String::new(),
                    })
                    .collect()
            })
            .collect();

        assert_eq!(rows.first().expect("header row"), &vec!["Name", "Score"]);
        assert_eq!(rows.get(1).expect("row 1"), &vec!["Alice", "95"]);
        assert_eq!(rows.get(2).expect("row 2"), &vec!["Bob", "87"]);
        assert_eq!(rows.get(3).expect("row 3"), &vec!["Carol", "92"]);
    }

    #[test]
    fn render_xlsx_multi_sheet() {
        let data = serde_json::json!({
            "sheets": [
                {
                    "name": "Q1",
                    "columns": [{ "header": "Revenue" }],
                    "rows": [[100]]
                },
                {
                    "name": "Q2",
                    "columns": [{ "header": "Revenue" }],
                    "rows": [[120]]
                }
            ]
        });

        let bytes = render_xlsx(&data).expect("render must succeed");
        let summary = inspect_xlsx(&bytes).expect("inspect must succeed");

        assert_eq!(summary.sheets.len(), 2);
        assert_eq!(summary.sheets.first().expect("sheet 0").name, "Q1");
        assert_eq!(summary.sheets.get(1).expect("sheet 1").name, "Q2");
        assert!(summary.cell_count > 0);
    }

    #[test]
    fn inspect_xlsx_returns_summary() {
        let data = serde_json::json!({
            "sheets": [
                {
                    "name": "Inventory",
                    "columns": [
                        { "header": "Item" },
                        { "header": "Qty" }
                    ],
                    "rows": [
                        ["Widget", 42],
                        ["Gadget", 7]
                    ]
                }
            ]
        });

        let bytes = render_xlsx(&data).expect("render must succeed");
        let summary = inspect_xlsx(&bytes).expect("inspect must succeed");

        assert_eq!(summary.sheets.len(), 1);
        let sheet = summary.sheets.first().expect("one sheet");
        assert_eq!(sheet.name, "Inventory");
        // Header + 2 data rows = 3 rows, 2 columns, 6 cells
        assert_eq!(sheet.row_count, 3);
        assert_eq!(sheet.column_count, 2);
        assert_eq!(summary.cell_count, 6);
    }

    #[test]
    fn render_xlsx_malformed_json_errors() {
        let data = serde_json::json!({ "title": "No sheets here" });
        let err = render_xlsx(&data).expect_err("must error");
        assert!(
            matches!(err, Error::InvalidSchema { .. }),
            "expected InvalidSchema, got: {err:?}"
        );
    }
}
