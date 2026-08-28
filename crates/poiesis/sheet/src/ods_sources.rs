//! Sources-sheet provenance tracing for the workbook ODS renderer.
//!
//! Mirrors [`crate::sources::append_sources_sheet`] (B-007 §5) on
//! `spreadsheet-ods`'s `WorkBook`/`Sheet` API instead of `rust_xlsxwriter`.
//! The fact-collection and per-row projection logic it shares with the XLSX
//! backend lives in [`crate::sources_row`]; this module only writes cells.

use poiesis_core::bodies::Workbook;
use poiesis_core::factbase::Factbase;
use spreadsheet_ods::{Sheet as OdsSheet, WorkBook as OdsWorkBook};

use crate::error::WorkbookError;
use crate::ods_workbook::OdsStyles;
use crate::sources_row::{HEADERS, SHEET_NAME, cited_fact_ids, project_rows};

/// Shorthand for sources-sheet results.
type Result<T> = std::result::Result<T, WorkbookError>;

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
    let rows = project_rows(&cited, factbase)?;

    let mut sheet = OdsSheet::new(SHEET_NAME);

    for (col, label) in HEADERS.iter().enumerate() {
        let col_u32 = u32::try_from(col).unwrap_or(0); // INVARIANT: HEADERS.len() < u32::MAX
        sheet.set_styled_value(0, col_u32, *label, &styles.header);
    }

    for (row_idx, row) in rows.iter().enumerate() {
        let row_num = u32::try_from(row_idx).map_err(|e| WorkbookError::OdsWrite {
            message: format!("row index {row_idx} exceeds u32 max: {e}"),
        })? + 1;

        sheet.set_value(row_num, 0, row.id.as_str());
        sheet.set_value(row_num, 1, row.value.as_str());
        sheet.set_value(row_num, 2, row.unit.as_str());
        sheet.set_value(row_num, 3, row.source_kind);
        sheet.set_value(row_num, 4, row.detail.as_str());
        sheet.set_value(row_num, 5, row.captured.as_str());
    }

    ods_wb.push_sheet(sheet);
    Ok(())
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use poiesis_core::bodies::{Sheet, WorkbookCell};
    use poiesis_core::factbase::Source;
    use poiesis_core::ids::{FactId, SheetName};
    use poiesis_core::scalar::{Scalar, ScalarKind};

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
