//! Sources-sheet provenance tracing for the workbook XLSX renderer.
//!
//! B-007 names a "Sources" worksheet listing every cited fact's provenance
//! (`planning/poiesis-evolution/B-007-poiesis-workbook-tracing.md` §5). This
//! module writes XLSX cells; the fact-collection and per-row projection
//! logic it shares with the ODS backend lives in [`crate::sources_row`].
//!
//! `Verified` is not a column here: no fact-level verification status is
//! attached to `Fact`/`ResolvedFact` today (`poiesis-verify` runs as a
//! separate pass over a `VerifyManifest`, not wired to individual facts), so
//! a column claiming it would be fabricated. Wiring `poiesis-verify` output
//! per-fact is out of scope here.

use poiesis_core::bodies::Workbook;
use poiesis_core::factbase::Factbase;
use poiesis_theme::resolved::ResolvedTheme;
use rust_xlsxwriter::Workbook as XlsxWorkbook;

use crate::error::WorkbookError;
use crate::format::header_format;
use crate::sources_row::{HEADERS, SHEET_NAME, cited_fact_ids, project_rows};

/// Shorthand for sources-sheet results.
type Result<T> = std::result::Result<T, WorkbookError>;

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
    let rows = project_rows(&cited, factbase)?;

    let ws = xlsx_wb.add_worksheet();
    ws.set_name(SHEET_NAME)?;
    ws.set_freeze_panes(1, 0)?;

    let header_fmt = header_format(theme);
    for (col, label) in HEADERS.iter().enumerate() {
        let col_u16 = u16::try_from(col).unwrap_or(0); // INVARIANT: HEADERS.len() < u16::MAX
        ws.write_with_format(0, col_u16, *label, &header_fmt)?;
    }

    for (row_idx, row) in rows.iter().enumerate() {
        let row_num = u32::try_from(row_idx).map_err(|e| WorkbookError::XlsxWrite {
            message: format!("row index {row_idx} exceeds u32 max: {e}"),
        })? + 1;

        ws.write(row_num, 0, row.id.as_str())?;
        ws.write(row_num, 1, row.value.as_str())?;
        ws.write(row_num, 2, row.unit.as_str())?;
        ws.write(row_num, 3, row.source_kind)?;
        ws.write(row_num, 4, row.detail.as_str())?;
        ws.write(row_num, 5, row.captured.as_str())?;
    }

    ws.autofit();
    Ok(())
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use poiesis_core::bodies::{Sheet, WorkbookCell};
    use poiesis_core::factbase::{Fact, Source};
    use poiesis_core::ids::{FactId, SheetName};
    use poiesis_core::scalar::{Scalar, ScalarKind};

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
