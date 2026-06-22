//! Per-column totals computation for the workbook XLSX renderer.

use std::collections::BTreeMap;

use poiesis_core::bodies::{Sheet, WorkbookCell};
use poiesis_core::factbase::ResolvedFact;
use poiesis_core::ids::FactId;
use poiesis_core::scalar::{Money, Scalar, ScalarKind};

/// Compute per-column totals for a sheet.
///
/// Returns one `Option<Scalar>` per column header. Numeric columns
/// (`Count`, `Money`, `Ratio`) are summed across all data rows; `Text` and
/// `Date` columns always return `None`. Columns with no resolvable numeric
/// values also return `None`.
pub fn compute_totals(
    sheet: &Sheet,
    facts: &BTreeMap<FactId, ResolvedFact>,
) -> Vec<Option<Scalar>> {
    let ncols = sheet.headers.len();
    let mut totals: Vec<Option<Scalar>> = vec![None; ncols];

    for (col_idx, &kind) in sheet.column_types.iter().enumerate() {
        if col_idx >= ncols {
            break;
        }
        if !matches!(
            kind,
            ScalarKind::Count | ScalarKind::Money | ScalarKind::Ratio
        ) {
            continue;
        }
        let mut acc: Option<Scalar> = None;
        for row in &sheet.rows {
            let Some(cell) = row.get(col_idx) else {
                continue;
            };
            let Some(scalar) = resolve_scalar(cell, facts) else {
                continue;
            };
            acc = Some(match acc {
                None => scalar,
                Some(a) => accumulate(a, scalar),
            });
        }
        if let Some(slot) = totals.get_mut(col_idx) {
            *slot = acc;
        }
    }
    totals
}

fn resolve_scalar(cell: &WorkbookCell, facts: &BTreeMap<FactId, ResolvedFact>) -> Option<Scalar> {
    match cell {
        WorkbookCell::Lit { value } => Some(value.clone()),
        WorkbookCell::Cite { fact } => facts.get(fact).map(|r| r.value.clone()),
        &_ => None,
    }
}

fn accumulate(a: Scalar, b: Scalar) -> Scalar {
    match (a, b) {
        (Scalar::Count { value: va }, Scalar::Count { value: vb }) => Scalar::Count {
            value: va.saturating_add(vb),
        },
        (Scalar::Money { value: va }, Scalar::Money { value: vb }) => Scalar::Money {
            value: Money::from_micros(va.micros().saturating_add(vb.micros())),
        },
        (Scalar::Ratio { value: va }, Scalar::Ratio { value: vb }) => {
            Scalar::Ratio { value: va + vb }
        }
        (a, _) => a,
    }
}
