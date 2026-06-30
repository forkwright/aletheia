#![deny(missing_docs)]
//! poiesis-sheet: XLSX and ODS spreadsheet rendering backends.
//!
//! Feature flags:
//! - `xlsx` (default): Excel XLSX output via `rust_xlsxwriter`.
//! - `ods` (default): `OpenDocument` Spreadsheet output via `spreadsheet-ods`.
//! - `workbook` (default): themed workbook assembly; implies `xlsx`.

#[cfg(feature = "xlsx")]
pub mod error;

#[cfg(feature = "xlsx")]
pub use error::Error;

#[cfg(feature = "xlsx")]
pub mod xlsx;

#[cfg(feature = "ods")]
pub mod ods;

#[cfg(feature = "workbook")]
mod format;

#[cfg(feature = "workbook")]
mod totals;

#[cfg(feature = "workbook")]
pub mod workbook;

#[cfg(feature = "workbook")]
pub use workbook::render_workbook;

#[cfg(feature = "workbook")]
pub use error::WorkbookError;

#[cfg(feature = "xlsx")]
pub use xlsx::XlsxRenderer;

#[cfg(feature = "ods")]
pub use ods::OdsRenderer;

#[cfg(feature = "xlsx")]
use rust_xlsxwriter::{Format, Workbook};

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
        render_one_sheet(&mut workbook, sheet_idx, sheet_val)?;
    }

    workbook.save_to_buffer().map_err(|e| Error::XlsxWrite {
        message: e.to_string(),
    })
}

#[cfg(feature = "xlsx")]
fn render_one_sheet(
    workbook: &mut Workbook,
    sheet_idx: usize,
    sheet_val: &serde_json::Value,
) -> Result<(), Error> {
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

    for (row_idx, row_val) in rows.iter().enumerate() {
        let cells = row_val.as_array().ok_or_else(|| Error::InvalidSchema {
            detail: format!("sheet {sheet_idx}, row {row_idx}: each row must be an array"),
        })?;

        for (col_idx, cell) in cells.iter().enumerate() {
            write_cell(worksheet, sheet_idx, row_idx, col_idx, cell)?;
        }
    }

    Ok(())
}

#[cfg(feature = "xlsx")]
fn write_cell(
    worksheet: &mut rust_xlsxwriter::Worksheet,
    _sheet_idx: usize,
    row_idx: usize,
    col_idx: usize,
    cell: &serde_json::Value,
) -> Result<(), Error> {
    let col_u16 = u16::try_from(col_idx).map_err(|e| Error::XlsxWrite {
        message: format!("column index {col_idx} exceeds u16 max: {e}"),
    })?;

    let row_num = u32::try_from(row_idx).map_err(|e| Error::XlsxWrite {
        message: format!("row index {row_idx} exceeds u32 max: {e}"),
    })? + 1;

    match cell {
        serde_json::Value::String(s) => {
            worksheet
                .write(row_num, col_u16, s.as_str())
                .map_err(|e| Error::XlsxWrite {
                    message: e.to_string(),
                })?;
        }
        serde_json::Value::Number(n) => {
            let val = n.as_f64().ok_or_else(|| Error::XlsxWrite {
                message: format!("invalid numeric cell at row {row_idx}, col {col_idx}"),
            })?;
            worksheet
                .write_number(row_num, col_u16, val)
                .map_err(|e| Error::XlsxWrite {
                    message: e.to_string(),
                })?;
        }
        serde_json::Value::Bool(b) => {
            worksheet
                .write_boolean(row_num, col_u16, *b)
                .map_err(|e| Error::XlsxWrite {
                    message: e.to_string(),
                })?;
        }
        serde_json::Value::Null => {
            worksheet
                .write_blank(row_num, col_u16, &Format::default())
                .map_err(|e| Error::XlsxWrite {
                    message: e.to_string(),
                })?;
        }
        other => {
            worksheet
                .write(row_num, col_u16, other.to_string().as_str())
                .map_err(|e| Error::XlsxWrite {
                    message: e.to_string(),
                })?;
        }
    }
    Ok(())
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
    use calamine::Reader;

    use super::*;

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

        let bytes = render_xlsx(&data).expect("render must succeed"); // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal

        let cursor = std::io::Cursor::new(&bytes);
        let mut workbook = calamine::Xlsx::new(cursor).expect("calamine must open xlsx"); // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
        let sheet_names = workbook.sheet_names();
        assert_eq!(sheet_names.len(), 1, "must have exactly one sheet");
        // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
        assert_eq!(
            sheet_names.first().expect("one sheet"),
            "Scores",
            "sheet name mismatch"
        );

        let range = workbook
            .worksheet_range("Scores")
            .expect("sheet must exist"); // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal

        // Header row + 3 data rows
        assert_eq!(range.height(), 4, "height mismatch");
        assert_eq!(range.width(), 2, "width mismatch");

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

        // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
        assert_eq!(
            rows.first().expect("header row"),
            &vec!["Name", "Score"],
            "header row mismatch"
        );
        // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
        assert_eq!(
            rows.get(1).expect("row 1"),
            &vec!["Alice", "95"],
            "row 1 mismatch"
        );
        // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
        assert_eq!(
            rows.get(2).expect("row 2"),
            &vec!["Bob", "87"],
            "row 2 mismatch"
        );
        // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
        assert_eq!(
            rows.get(3).expect("row 3"),
            &vec!["Carol", "92"],
            "row 3 mismatch"
        );
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

        let bytes = render_xlsx(&data).expect("render must succeed"); // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
        let summary = inspect_xlsx(&bytes).expect("inspect must succeed"); // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal

        assert_eq!(summary.sheets.len(), 2, "sheet count mismatch");
        // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
        assert_eq!(
            summary.sheets.first().expect("sheet 0").name,
            "Q1",
            "first sheet name mismatch"
        );
        // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
        assert_eq!(
            summary.sheets.get(1).expect("sheet 1").name,
            "Q2",
            "second sheet name mismatch"
        );
        assert!(summary.cell_count > 0); // kanon:ignore RUST/bare-assert — non-equality boolean assertion; no additional message needed
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

        let bytes = render_xlsx(&data).expect("render must succeed"); // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
        let summary = inspect_xlsx(&bytes).expect("inspect must succeed"); // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal

        assert_eq!(summary.sheets.len(), 1, "sheet count mismatch");
        let sheet = summary.sheets.first().expect("one sheet"); // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
        assert_eq!(sheet.name, "Inventory", "sheet name mismatch"); // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
        // Header + 2 data rows = 3 rows, 2 columns, 6 cells
        assert_eq!(sheet.row_count, 3, "row count mismatch");
        assert_eq!(sheet.column_count, 2, "column count mismatch");
        assert_eq!(summary.cell_count, 6, "cell count mismatch");
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

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn render_xlsx_numeric_and_boolean_cells_round_trip() {
        let data = serde_json::json!({
            "sheets": [
                {
                    "name": "Data",
                    "columns": [
                        { "header": "Number" },
                        { "header": "Bool" },
                        { "header": "Text" }
                    ],
                    "rows": [
                        [42, true, "hello"],
                        [2.5, false, "world"]
                    ]
                }
            ]
        });

        let bytes = render_xlsx(&data).expect("render must succeed"); // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal

        let cursor = std::io::Cursor::new(&bytes);
        let mut workbook = calamine::Xlsx::new(cursor).expect("calamine must open xlsx"); // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
        let range = workbook.worksheet_range("Data").expect("sheet must exist"); // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal

        // Row 0 is header; rows 1 and 2 are data.
        let cell_1_0 = range.get((1, 0)).expect("cell (1,0)"); // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
        assert!(
            matches!(cell_1_0, calamine::Data::Float(f) if (*f - 42.0).abs() < f64::EPSILON),
            "expected numeric 42, got {cell_1_0:?}"
        );

        let cell_1_1 = range.get((1, 1)).expect("cell (1,1)"); // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
        assert!(
            matches!(cell_1_1, calamine::Data::Bool(true)),
            "expected bool true, got {cell_1_1:?}"
        );

        let cell_2_0 = range.get((2, 0)).expect("cell (2,0)"); // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
        assert!(
            matches!(cell_2_0, calamine::Data::Float(f) if (*f - 2.5).abs() < f64::EPSILON),
            "expected numeric 2.5, got {cell_2_0:?}"
        );

        let cell_2_1 = range.get((2, 1)).expect("cell (2,1)"); // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
        assert!(
            matches!(cell_2_1, calamine::Data::Bool(false)),
            "expected bool false, got {cell_2_1:?}"
        );
    }
}
