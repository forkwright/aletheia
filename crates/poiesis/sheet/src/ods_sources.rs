//! Sources-sheet provenance tracing for the workbook ODS renderer.
//!
//! Mirrors [`crate::sources::append_sources_sheet`] (B-007 §5) on
//! `spreadsheet-ods`'s `WorkBook`/`Sheet` API instead of `rust_xlsxwriter`.
//! The row content and the "provenance ledger, not a formatted presentation
//! cell" convention (every value rendered through `Display`, not `poiesis-sheet`'s
//! number-format machinery) are identical to the XLSX Sources sheet; only the
//! underlying cell-write calls differ per format.

use std::collections::HashSet;

use poiesis_core::bodies::{Workbook, WorkbookCell};
use poiesis_core::factbase::{Expr, Factbase, Source};
use poiesis_core::ids::FactId;
use poiesis_core::scalar::Scalar;
use spreadsheet_ods::{Sheet as OdsSheet, WorkBook as OdsWorkBook};

use crate::error::WorkbookError;
use crate::ods_workbook::OdsStyles;

/// Shorthand for sources-sheet results.
type Result<T> = std::result::Result<T, WorkbookError>;

/// Reserved worksheet name for the provenance sheet, matching the XLSX
/// Sources sheet's [`crate::sources`] convention.
const SHEET_NAME: &str = "Sources";

const HEADERS: [&str; 6] = ["Fact ID", "Value", "Unit", "Source", "Detail", "Captured"];

/// Append a "Sources" worksheet tracing every fact `wb` cites back to
/// `factbase`, in first-cited order.
///
/// # Errors
///
/// Returns [`WorkbookError::UnknownFact`] if a cited fact id is not present
/// in `factbase` (should not happen for a workbook already rendered from
/// this same factbase's resolution -- surfaced rather than silently
/// skipped).
pub(crate) fn append_ods_sources_sheet(
    ods_wb: &mut OdsWorkBook,
    wb: &Workbook,
    factbase: &Factbase,
    styles: &OdsStyles,
) -> Result<()> {
    let cited = cited_fact_ids(wb);
    if cited.is_empty() {
        return Ok(());
    }

    let mut sheet = OdsSheet::new(SHEET_NAME);

    for (col, label) in HEADERS.iter().enumerate() {
        let col_u32 = u32::try_from(col).unwrap_or(0); // INVARIANT: HEADERS.len() < u32::MAX
        sheet.set_styled_value(0, col_u32, *label, &styles.header);
    }

    for (row_idx, id) in cited.iter().enumerate() {
        let fact = factbase
            .facts
            .get(id)
            .ok_or_else(|| WorkbookError::UnknownFact {
                id: id.as_str().to_owned(),
            })?;
        let row = u32::try_from(row_idx).map_err(|e| WorkbookError::OdsWrite {
            message: format!("row index {row_idx} exceeds u32 max: {e}"),
        })? + 1;
        let (source_kind, detail) = describe_source(&fact.source);

        sheet.set_value(row, 0, fact.id.as_str());
        sheet.set_value(row, 1, scalar_display(&fact.value));
        sheet.set_value(row, 2, fact.unit.to_string());
        sheet.set_value(row, 3, source_kind);
        sheet.set_value(row, 4, detail);
        sheet.set_value(row, 5, fact.captured.to_string());
    }

    ods_wb.push_sheet(sheet);
    Ok(())
}

/// Collect the [`FactId`]s cited by `wb`'s cells, first-seen order,
/// deduplicated across every sheet/row/column.
fn cited_fact_ids(wb: &Workbook) -> Vec<FactId> {
    let mut seen = HashSet::new();
    let mut ordered = Vec::new();
    for sheet in &wb.sheets {
        for row in &sheet.rows {
            for cell in row {
                if let WorkbookCell::Cite { fact } = cell
                    && seen.insert(fact.clone())
                {
                    ordered.push(fact.clone());
                }
            }
        }
    }
    ordered
}

/// Render a [`Scalar`] as plain display text for the Sources sheet.
fn scalar_display(scalar: &Scalar) -> String {
    match scalar {
        Scalar::Count { value } => value.to_string(),
        Scalar::Money { value } => value.to_string(),
        Scalar::Ratio { value } => value.to_string(),
        Scalar::Text { value } => value.clone(),
        Scalar::Date { value } => value.to_string(),
    }
}

/// Classify a [`Source`] into a short kind tag and a human-readable detail
/// string for the Sources sheet's `Source` / `Detail` columns.
fn describe_source(source: &Source) -> (&'static str, String) {
    match source {
        Source::Sql {
            data_source,
            query,
            table,
        } => ("sql", format!("{data_source} :: table {table} :: {query}")),
        Source::Derived { formula, inputs } => (
            "derived",
            format!("{} <- [{}]", describe_expr(formula), join_fact_ids(inputs)),
        ),
        Source::Reference { fact } => ("reference", format!("-> {fact}")),
        Source::Manual { note, captured_by } => ("manual", format!("{note} (by {captured_by})")),
        Source::File { path, locator } => ("file", format!("{}#{locator}", path.display())),
    }
}

/// Render a [`Expr`] as a short infix formula string.
fn describe_expr(expr: &Expr) -> String {
    match expr {
        Expr::Add { a, b } => format!("{a} + {b}"),
        Expr::Sub { a, b } => format!("{a} - {b}"),
        Expr::Mul { a, b } => format!("{a} * {b}"),
        Expr::Div { a, b } => format!("{a} / {b}"),
        Expr::Sum { terms } => format!("sum({})", join_fact_ids(terms)),
    }
}

fn join_fact_ids(ids: &[FactId]) -> String {
    ids.iter()
        .map(FactId::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use poiesis_core::bodies::Sheet;
    use poiesis_core::ids::SheetName;
    use poiesis_core::scalar::ScalarKind;

    use super::*;
    use crate::ods_workbook::OdsStyles;

    fn fact(id: &str, source: &Source) -> poiesis_core::factbase::Fact {
        let json = serde_json::json!({
            "id": id,
            "value": {"kind": "count", "value": 42},
            "unit": "count",
            "source": serde_json::to_value(source).expect("serialize source"),
            "captured": "1970-01-01T00:00:00Z",
        });
        serde_json::from_value(json).expect("deserialize fact")
    }

    fn workbook_citing(ids: &[&str]) -> Workbook {
        let headers = vec!["Metric".to_owned()];
        let rows = ids
            .iter()
            .map(|id| {
                vec![WorkbookCell::Cite {
                    fact: FactId::new(*id).expect("id"),
                }]
            })
            .collect();
        Workbook {
            sheets: vec![Sheet {
                name: SheetName::new("data").expect("name"),
                headers,
                rows,
                column_types: vec![ScalarKind::Count],
            }],
        }
    }

    #[test]
    fn append_ods_sources_sheet_writes_one_row_per_cited_fact() {
        let wb = workbook_citing(&["a"]);
        let mut factbase = Factbase::new();
        factbase.add_fact(fact(
            "a",
            &Source::Manual {
                note: "n".to_owned(),
                captured_by: "op".to_owned(),
            },
        ));
        let mut ods_wb = OdsWorkBook::new_empty();
        let theme = poiesis_theme::protos();
        let styles = OdsStyles::register(&mut ods_wb, &theme);
        append_ods_sources_sheet(&mut ods_wb, &wb, &factbase, &styles).expect("appends sheet");
        let bytes = spreadsheet_ods::write_ods_buf(&mut ods_wb, Vec::new()).expect("save");
        assert!(bytes.starts_with(b"PK"), "ods must be a valid zip");
    }

    #[test]
    fn append_ods_sources_sheet_is_noop_for_a_workbook_with_no_cites() {
        let wb = Workbook {
            sheets: vec![Sheet {
                name: SheetName::new("data").expect("name"),
                headers: vec!["Metric".to_owned()],
                rows: vec![vec![WorkbookCell::Lit {
                    value: Scalar::Text {
                        value: "literal".to_owned(),
                    },
                }]],
                column_types: vec![ScalarKind::Text],
            }],
        };
        let factbase = Factbase::new();
        let mut ods_wb = OdsWorkBook::new_empty();
        let theme = poiesis_theme::protos();
        let styles = OdsStyles::register(&mut ods_wb, &theme);
        append_ods_sources_sheet(&mut ods_wb, &wb, &factbase, &styles).expect("no-op ok");
    }

    #[test]
    fn append_ods_sources_sheet_rejects_cite_missing_from_factbase() {
        let wb = workbook_citing(&["ghost"]);
        let factbase = Factbase::new();
        let mut ods_wb = OdsWorkBook::new_empty();
        let theme = poiesis_theme::protos();
        let styles = OdsStyles::register(&mut ods_wb, &theme);
        let err = append_ods_sources_sheet(&mut ods_wb, &wb, &factbase, &styles)
            .expect_err("unknown fact must reject");
        assert!(matches!(err, WorkbookError::UnknownFact { .. }));
    }
}
