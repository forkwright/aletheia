//! Format-independent Sources-sheet projection (B-007 §5).
//!
//! The XLSX and ODS workbook renderers each append a "Sources" worksheet
//! tracing every cited fact's provenance. The row content and the
//! "provenance ledger, not a formatted presentation cell" convention (every
//! value rendered through `Display`, not `poiesis-sheet`'s number-format
//! machinery) are identical between the two backends — only the underlying
//! cell-write calls differ (`rust_xlsxwriter` vs `spreadsheet-ods`). This
//! module owns everything up to that point: which facts are cited, in what
//! order, and how each one renders to display strings. The two backend
//! modules (`crate::sources`, `crate::ods_sources`) consume [`project_rows`]
//! and write cells; neither touches [`poiesis_core::scalar::Scalar`],
//! [`Source`], or [`Expr`] directly.

use std::collections::HashSet;

use poiesis_core::bodies::{Workbook, WorkbookCell};
use poiesis_core::factbase::{Expr, Factbase, Source};
use poiesis_core::ids::FactId;
use poiesis_core::scalar::Scalar;

use crate::error::WorkbookError;

/// Reserved worksheet name for the provenance sheet, shared by both backends.
pub(crate) const SHEET_NAME: &str = "Sources";

/// Column headers, in order, for the Sources sheet in both backends.
pub(crate) const HEADERS: [&str; 6] = ["Fact ID", "Value", "Unit", "Source", "Detail", "Captured"];

/// One projected Sources-sheet row, already rendered to display strings.
///
/// Every field is pre-rendered so a cell-writing adapter never needs to
/// import `Scalar`, `Source`, or `Expr` — only this row shape.
pub(crate) struct SourceRow {
    pub(crate) id: String,
    pub(crate) value: String,
    pub(crate) unit: String,
    pub(crate) source_kind: &'static str,
    pub(crate) detail: String,
    pub(crate) captured: String,
}

/// Collect the [`FactId`]s cited by `wb`'s cells, first-seen order,
/// deduplicated across every sheet/row/column.
pub(crate) fn cited_fact_ids(wb: &Workbook) -> Vec<FactId> {
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

/// Project every cited fact into a display-ready [`SourceRow`], preserving
/// `cited`'s order.
///
/// # Errors
///
/// Returns [`WorkbookError::UnknownFact`] if a cited fact id is not present
/// in `factbase` (should not happen for a workbook already rendered from
/// this same factbase's resolution — surfaced rather than silently
/// skipped).
pub(crate) fn project_rows(
    cited: &[FactId],
    factbase: &Factbase,
) -> Result<Vec<SourceRow>, WorkbookError> {
    cited
        .iter()
        .map(|id| {
            let fact = factbase
                .facts
                .get(id)
                .ok_or_else(|| WorkbookError::UnknownFact {
                    id: id.as_str().to_owned(),
                })?;
            let (source_kind, detail) = describe_source(&fact.source);
            Ok(SourceRow {
                id: fact.id.as_str().to_owned(),
                value: scalar_display(&fact.value),
                unit: fact.unit.to_string(),
                source_kind,
                detail,
                captured: fact.captured.to_string(),
            })
        })
        .collect()
}

/// Render a [`Scalar`] as plain display text for the Sources sheet — this
/// sheet is a provenance ledger, not a formatted presentation cell, so every
/// kind renders through its own `Display` rather than `poiesis-sheet`'s
/// number-format machinery.
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
    fn cited_fact_ids_dedupes_and_preserves_first_seen_order() {
        let wb = workbook_citing(&["b", "a", "b", "c"]);
        let ids: Vec<String> = cited_fact_ids(&wb)
            .iter()
            .map(|i| i.as_str().to_owned())
            .collect();
        assert_eq!(ids, vec!["b".to_owned(), "a".to_owned(), "c".to_owned()]);
    }

    #[test]
    fn describe_source_manual() {
        let (kind, detail) = describe_source(&Source::Manual {
            note: "sample deck".to_owned(),
            captured_by: "operator".to_owned(),
        });
        assert_eq!(kind, "manual");
        assert_eq!(detail, "sample deck (by operator)");
    }

    #[test]
    fn describe_source_derived_formula() {
        let (kind, detail) = describe_source(&Source::Derived {
            formula: Expr::Sub {
                a: FactId::new("revenue").expect("id"),
                b: FactId::new("cost").expect("id"),
            },
            inputs: vec![
                FactId::new("revenue").expect("id"),
                FactId::new("cost").expect("id"),
            ],
        });
        assert_eq!(kind, "derived");
        assert_eq!(detail, "revenue - cost <- [revenue, cost]");
    }
}
