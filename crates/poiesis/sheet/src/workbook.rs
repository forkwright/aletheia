//! XLSX renderer for the B-001 [`Workbook`] body.

use std::collections::BTreeMap;

use poiesis_core::bodies::{Sheet, Workbook, WorkbookCell};
use poiesis_core::factbase::ResolvedFact;
use poiesis_core::ids::FactId;
use poiesis_core::scalar::{Money, Scalar, ScalarKind, Unit};
use poiesis_theme::resolved::ResolvedTheme;
use rust_xlsxwriter::{Format, Workbook as XlsxWorkbook, XlsxError};

use crate::format::{cell_format, header_format, totals_format};
use crate::totals::compute_totals;

/// Shorthand for workbook-level results.
type Result<T> = std::result::Result<T, crate::error::WorkbookError>;

impl From<XlsxError> for crate::error::WorkbookError {
    fn from(e: XlsxError) -> Self {
        Self::XlsxWrite {
            message: e.to_string(),
        }
    }
}

/// Renders a [`Workbook`] to an XLSX byte vector.
///
/// `facts` is the pre-resolved factbase (call `factbase.resolve(&registry)` before
/// passing here). Every [`WorkbookCell::Cite`] must resolve in `facts`; an unknown
/// fact id returns [`WorkbookError::UnknownFact`].
///
/// `theme` drives header formatting via [`crate::format::header_format`] and
/// cell number formats via [`crate::format::cell_format`].
pub fn render_workbook(
    wb: &Workbook,
    facts: &BTreeMap<FactId, ResolvedFact>,
    theme: &ResolvedTheme,
) -> std::result::Result<Vec<u8>, crate::error::WorkbookError> {
    let mut xlsx_wb = XlsxWorkbook::new();

    for sheet in &wb.sheets {
        render_sheet(&mut xlsx_wb, sheet, facts, theme)?;
    }

    xlsx_wb
        .save_to_buffer()
        .map_err(crate::error::WorkbookError::from)
}

fn render_sheet(
    xlsx_wb: &mut XlsxWorkbook,
    sheet: &Sheet,
    facts: &BTreeMap<FactId, ResolvedFact>,
    theme: &ResolvedTheme,
) -> Result<()> {
    let ws = xlsx_wb.add_worksheet();
    ws.set_name(sheet.name.as_ref())?;
    ws.set_freeze_panes(1, 0)?;

    let ncols = sheet.headers.len();
    let ncols_u16 = u16::try_from(ncols).map_err(|e| crate::error::WorkbookError::XlsxWrite {
        message: format!("column count exceeds u16: {e}"),
    })?;

    // Header row
    for (col_idx, header) in sheet.headers.iter().enumerate() {
        let col_u16 =
            u16::try_from(col_idx).map_err(|e| crate::error::WorkbookError::XlsxWrite {
                message: format!("column index {col_idx} exceeds u16 max: {e}"),
            })?;
        let fmt = header_format(theme);
        ws.write_with_format(0, col_u16, header.as_str(), &fmt)?;
    }

    // Autofilter on header row
    if ncols > 0 {
        ws.autofilter(0, 0, 0, ncols_u16.saturating_sub(1))?;
    }

    // Data rows
    for (row_idx, row) in sheet.rows.iter().enumerate() {
        let row_num =
            u32::try_from(row_idx).map_err(|e| crate::error::WorkbookError::XlsxWrite {
                message: format!("row index {row_idx} exceeds u32 max: {e}"),
            })? + 1;

        for (col_idx, cell) in row.iter().enumerate() {
            let col_u16 =
                u16::try_from(col_idx).map_err(|e| crate::error::WorkbookError::XlsxWrite {
                    message: format!("column index {col_idx} exceeds u16 max: {e}"),
                })?;

            let kind = sheet.column_types.get(col_idx).copied();
            let Some(kind) = kind else {
                continue;
            };

            let (scalar, unit) = resolve_cell(cell, facts, kind)?;
            let fmt = cell_format(kind, unit, theme);
            write_scalar(ws, row_num, col_u16, &scalar, &fmt)?;
        }
    }

    // Totals row
    let totals = compute_totals(sheet, facts);
    let totals_row =
        u32::try_from(sheet.rows.len()).map_err(|e| crate::error::WorkbookError::XlsxWrite {
            message: format!("row count exceeds u32 max: {e}"),
        })? + 1;

    for (col_idx, total) in totals.iter().enumerate() {
        let Some(total) = total else { continue };
        let col_u16 =
            u16::try_from(col_idx).map_err(|e| crate::error::WorkbookError::XlsxWrite {
                message: format!("column index {col_idx} exceeds u16 max: {e}"),
            })?;
        let kind = sheet.column_types.get(col_idx).copied();
        let Some(kind) = kind else { continue };
        let unit = unit_for_total(sheet, facts, col_idx, kind);
        let fmt = totals_format(kind, unit, theme);
        write_scalar(ws, totals_row, col_u16, total, &fmt)?;
    }

    Ok(())
}

/// Resolve a single cell to its scalar value and presentation unit.
fn resolve_cell(
    cell: &WorkbookCell,
    facts: &BTreeMap<FactId, ResolvedFact>,
    kind: ScalarKind,
) -> Result<(Scalar, Unit)> {
    match cell {
        WorkbookCell::Lit { value } => {
            let unit = kind_default_unit(kind);
            Ok((value.clone(), unit))
        }
        WorkbookCell::Cite { fact } => match facts.get(fact) {
            Some(resolved) => Ok((resolved.value.clone(), resolved.unit)),
            None => Err(crate::error::WorkbookError::UnknownFact {
                id: fact.as_str().to_owned(),
            }),
        },
    }
}

/// Return the canonical unit for a scalar kind when no factbase context is
/// available (e.g. literal cells).
fn kind_default_unit(kind: ScalarKind) -> Unit {
    match kind {
        ScalarKind::Count => Unit::Count,
        ScalarKind::Money => Unit::Usd,
        ScalarKind::Ratio => Unit::Ratio,
        ScalarKind::Text => Unit::Text,
        ScalarKind::Date => Unit::Date,
    }
}

/// Write a [`Scalar`] into a worksheet cell with the supplied [`Format`].
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "xlsx numeric output is a presentation conversion"
)]
fn write_scalar(
    ws: &mut rust_xlsxwriter::Worksheet,
    row: u32,
    col: u16,
    scalar: &Scalar,
    fmt: &Format,
) -> Result<()> {
    match scalar {
        Scalar::Count { value } => {
            ws.write_with_format(row, col, *value as f64, fmt)?;
        }
        Scalar::Money { value } => {
            let micros = value.micros();
            ws.write_with_format(row, col, micros as f64 / 1_000_000.0, fmt)?;
        }
        Scalar::Ratio { value } => {
            ws.write_with_format(row, col, *value, fmt)?;
        }
        Scalar::Text { value } => {
            ws.write_with_format(row, col, value.as_str(), fmt)?;
        }
        Scalar::Date { value } => {
            ws.write_with_format(row, col, value.to_string().as_str(), fmt)?;
        }
    }
    Ok(())
}

/// Determine the presentation unit for a totals column.
///
/// Walks the column looking for the first [`WorkbookCell::Cite`] and uses the
/// associated fact's unit; falls back to the kind-default if no cite is found.
fn unit_for_total(
    sheet: &Sheet,
    facts: &BTreeMap<FactId, ResolvedFact>,
    col_idx: usize,
    kind: ScalarKind,
) -> Unit {
    for row in &sheet.rows {
        if let Some(cell) = row.get(col_idx) {
            if let WorkbookCell::Cite { fact } = cell {
                if let Some(resolved) = facts.get(fact) {
                    return resolved.unit;
                }
            }
        }
    }
    kind_default_unit(kind)
}
