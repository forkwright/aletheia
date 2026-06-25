//! Per-column totals computation for the workbook XLSX renderer.

use std::collections::BTreeMap;

use poiesis_core::bodies::{Sheet, WorkbookCell};
use poiesis_core::factbase::ResolvedFact;
use poiesis_core::ids::FactId;
use poiesis_core::scalar::{Money, Scalar, ScalarKind};

use crate::error::WorkbookError;

/// Compute per-column totals for a sheet.
///
/// Returns one `Option<Scalar>` per column header. Numeric columns
/// (`Count`, `Money`, `Ratio`) are summed across all data rows; `Text` and
/// `Date` columns always return `None`. Columns with no resolvable numeric
/// values also return `None`.
///
/// # Errors
///
/// Returns [`WorkbookError::NonFiniteRatio`] if a `Ratio` column contains a
/// non-finite `f64` value or the running total becomes non-finite.
pub fn compute_totals(
    sheet: &Sheet,
    facts: &BTreeMap<FactId, ResolvedFact>,
) -> Result<Vec<Option<Scalar>>, WorkbookError> {
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
                Some(a) => accumulate(a, scalar)?,
            });
        }
        if let Some(slot) = totals.get_mut(col_idx) {
            *slot = acc;
        }
    }
    Ok(totals)
}

fn resolve_scalar(cell: &WorkbookCell, facts: &BTreeMap<FactId, ResolvedFact>) -> Option<Scalar> {
    match cell {
        WorkbookCell::Lit { value } => Some(value.clone()),
        WorkbookCell::Cite { fact } => facts.get(fact).map(|r| r.value.clone()),
        &_ => None,
    }
}

fn accumulate(a: Scalar, b: Scalar) -> Result<Scalar, WorkbookError> {
    match (a, b) {
        (Scalar::Count { value: va }, Scalar::Count { value: vb }) => Ok(Scalar::Count {
            value: va.saturating_add(vb),
        }),
        (Scalar::Money { value: va }, Scalar::Money { value: vb }) => Ok(Scalar::Money {
            value: Money::from_micros(va.micros().saturating_add(vb.micros())),
        }),
        (Scalar::Ratio { value: va }, Scalar::Ratio { value: vb }) => {
            // WHY: NaN is toxic — NaN + anything = NaN — and would corrupt the
            // entire XLSX totals row without surfacing an error. Reject it here
            // so the caller gets an explicit failure instead of silent #NUM!.
            let sum = va + vb;
            Scalar::new_ratio(sum).map_err(|_e| WorkbookError::NonFiniteRatio { value: sum })
        }
        (a, _) => Ok(a),
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn accumulate_ratio_sums_finite() {
        let a = Scalar::Ratio { value: 0.5 };
        let b = Scalar::Ratio { value: 0.25 };
        let got = accumulate(a, b).expect("sum");
        assert_eq!(got, Scalar::Ratio { value: 0.75 });
    }

    #[test]
    fn accumulate_ratio_rejects_nan_operand() {
        let a = Scalar::Ratio { value: f64::NAN };
        let b = Scalar::Ratio { value: 1.0 };
        let err = accumulate(a, b).expect_err("nan fails");
        assert!(matches!(err, WorkbookError::NonFiniteRatio { .. }));
    }

    #[test]
    fn accumulate_ratio_rejects_infinite_total() {
        let a = Scalar::Ratio { value: f64::MAX };
        let b = Scalar::Ratio { value: f64::MAX };
        let err = accumulate(a, b).expect_err("inf total fails");
        assert!(matches!(err, WorkbookError::NonFiniteRatio { .. }));
    }
}
