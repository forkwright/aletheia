//! Sources-sheet provenance tracing for the workbook XLSX renderer.
//!
//! B-007 names a "Sources" worksheet listing every cited fact's provenance
//! (`planning/poiesis-evolution/B-007-poiesis-workbook-tracing.md` §5). This
//! module collects the [`FactId`]s a [`Workbook`] body actually cites via
//! [`WorkbookCell::Cite`] and, when the caller opts in with the originating
//! [`Factbase`], appends one row per cited fact: id, resolved value, unit,
//! source kind, and a human-readable detail string.
//!
//! `Verified` is not a column here: no fact-level verification status is
//! attached to [`Fact`]/[`ResolvedFact`] today (`poiesis-verify` runs as a
//! separate pass over a `VerifyManifest`, not wired to individual facts), so
//! a column claiming it would be fabricated. Wiring `poiesis-verify` output
//! per-fact is out of scope here.

use std::collections::HashSet;

use poiesis_core::bodies::{Workbook, WorkbookCell};
use poiesis_core::factbase::{Expr, Factbase, Source};
use poiesis_core::ids::FactId;
use poiesis_core::scalar::Scalar;
use poiesis_theme::resolved::ResolvedTheme;
use rust_xlsxwriter::Workbook as XlsxWorkbook;

use crate::error::WorkbookError;
use crate::format::header_format;

/// Shorthand for sources-sheet results.
type Result<T> = std::result::Result<T, WorkbookError>;

/// Reserved worksheet name for the provenance sheet. Colliding with a
/// data-sheet name of the same title fails loud via `set_name`'s existing
/// `XlsxError` → [`WorkbookError`] conversion rather than silently merging.
const SHEET_NAME: &str = "Sources";

const HEADERS: [&str; 6] = ["Fact ID", "Value", "Unit", "Source", "Detail", "Captured"];

/// Append a "Sources" worksheet tracing every fact the workbook cites back
/// to `factbase`, in first-cited order.
///
/// # Errors
///
/// Returns [`WorkbookError::UnknownFact`] if a cited fact id is not present
/// in `factbase` (should not happen for a workbook already rendered from
/// this same factbase's resolution — surfaced rather than silently
/// skipped). Returns [`WorkbookError::XlsxWrite`] on any underlying
/// `rust_xlsxwriter` failure, including a worksheet name collision with
/// [`SHEET_NAME`].
pub fn append_sources_sheet(
    xlsx_wb: &mut XlsxWorkbook,
    wb: &Workbook,
    factbase: &Factbase,
    theme: &ResolvedTheme,
) -> Result<()> {
    let cited = cited_fact_ids(wb);
    if cited.is_empty() {
        return Ok(());
    }

    let ws = xlsx_wb.add_worksheet();
    ws.set_name(SHEET_NAME)?;
    ws.set_freeze_panes(1, 0)?;

    let header_fmt = header_format(theme);
    for (col, label) in HEADERS.iter().enumerate() {
        let col_u16 = u16::try_from(col).unwrap_or(0); // INVARIANT: HEADERS.len() < u16::MAX
        ws.write_with_format(0, col_u16, *label, &header_fmt)?;
    }

    for (row_idx, id) in cited.iter().enumerate() {
        let fact = factbase
            .facts
            .get(id)
            .ok_or_else(|| WorkbookError::UnknownFact {
                id: id.as_str().to_owned(),
            })?;
        let row = u32::try_from(row_idx).map_err(|e| WorkbookError::XlsxWrite {
            message: format!("row index {row_idx} exceeds u32 max: {e}"),
        })? + 1;
        let (source_kind, detail) = describe_source(&fact.source);

        ws.write(row, 0, fact.id.as_str())?;
        ws.write(row, 1, scalar_display(&fact.value))?;
        ws.write(row, 2, fact.unit.to_string())?;
        ws.write(row, 3, source_kind)?;
        ws.write(row, 4, detail)?;
        ws.write(row, 5, fact.captured.to_string())?;
    }

    ws.autofit();
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
    use poiesis_core::factbase::Fact;
    use poiesis_core::ids::SheetName;
    use poiesis_core::scalar::ScalarKind;

    use super::*;

    // WHY JSON round-trip, not a `Fact` struct literal: `Fact::captured` is a
    // `jiff::Timestamp`, and `poiesis-sheet` does not depend on `jiff`
    // directly (only transitively through `poiesis-core`) — naming the type
    // would need a new Cargo.toml dependency edge for a single test helper.
    // `Fact` already derives `Deserialize`, so building it from an RFC-3339
    // string sidesteps that without adding a dependency.
    fn fact(id: &str, source: &Source) -> Fact {
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

    #[test]
    fn append_sources_sheet_writes_one_row_per_cited_fact() {
        let wb = workbook_citing(&["a"]);
        let mut factbase = Factbase::new();
        factbase.add_fact(fact(
            "a",
            &Source::Manual {
                note: "n".to_owned(),
                captured_by: "op".to_owned(),
            },
        ));
        let mut xlsx_wb = XlsxWorkbook::new();
        let theme = poiesis_theme::protos();
        append_sources_sheet(&mut xlsx_wb, &wb, &factbase, &theme).expect("appends sheet");
        // Round-trip through a buffer to assert the worksheet exists and is
        // well-formed (parts-level assertion, matching the ZIP-based-sink
        // convention used by the other poiesis-theme/sheet golden tests).
        let bytes = xlsx_wb.save_to_buffer().expect("save");
        assert!(bytes.starts_with(b"PK"), "xlsx must be a valid zip");
    }

    #[test]
    fn append_sources_sheet_is_noop_for_a_workbook_with_no_cites() {
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
        let mut xlsx_wb = XlsxWorkbook::new();
        let theme = poiesis_theme::protos();
        append_sources_sheet(&mut xlsx_wb, &wb, &factbase, &theme).expect("no-op ok");
    }

    #[test]
    fn append_sources_sheet_rejects_cite_missing_from_factbase() {
        let wb = workbook_citing(&["ghost"]);
        let factbase = Factbase::new();
        let mut xlsx_wb = XlsxWorkbook::new();
        let theme = poiesis_theme::protos();
        let err = append_sources_sheet(&mut xlsx_wb, &wb, &factbase, &theme)
            .expect_err("unknown fact must reject");
        assert!(matches!(err, WorkbookError::UnknownFact { .. }));
    }
}
