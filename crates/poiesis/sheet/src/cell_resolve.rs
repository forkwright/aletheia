//! Shared cell-resolution logic for [`Workbook`](poiesis_core::bodies::Workbook)
//! renderers.
//!
//! WHY: `render_workbook` (XLSX) and `render_ods_workbook` (ODS) both walk the
//! same [`Workbook`] body against the same pre-resolved factbase; only the
//! output-format write step differs. Resolving a [`WorkbookCell`] to its
//! scalar value and presentation unit is one rule, so it lives here once
//! rather than drifting between two format-specific copies.

use std::collections::BTreeMap;

use poiesis_core::bodies::{Sheet, WorkbookCell};
use poiesis_core::factbase::ResolvedFact;
use poiesis_core::ids::FactId;
use poiesis_core::scalar::{Scalar, ScalarKind, Unit};

use crate::error::WorkbookError;

type Result<T> = std::result::Result<T, WorkbookError>;

/// Resolve a single cell to its scalar value and presentation unit.
pub(crate) fn resolve_cell(
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
            None => Err(WorkbookError::UnknownFact {
                id: fact.as_str().to_owned(),
            }),
        },
        &_ => Err(WorkbookError::UnsupportedCellKind),
    }
}

/// Return the canonical unit for a scalar kind when no factbase context is
/// available (e.g. literal cells).
pub(crate) fn kind_default_unit(kind: ScalarKind) -> Unit {
    match kind {
        ScalarKind::Count => Unit::Count,
        ScalarKind::Money => Unit::Usd,
        ScalarKind::Ratio => Unit::Ratio,
        ScalarKind::Text => Unit::Text,
        ScalarKind::Date => Unit::Date,
    }
}

/// Determine the presentation unit for a totals column.
///
/// Walks the column looking for the first [`WorkbookCell::Cite`] and uses the
/// associated fact's unit; falls back to the kind-default if no cite is found.
pub(crate) fn unit_for_total(
    sheet: &Sheet,
    facts: &BTreeMap<FactId, ResolvedFact>,
    col_idx: usize,
    kind: ScalarKind,
) -> Unit {
    for row in &sheet.rows {
        if let Some(WorkbookCell::Cite { fact }) = row.get(col_idx)
            && let Some(resolved) = facts.get(fact)
        {
            return resolved.unit;
        }
    }
    kind_default_unit(kind)
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use poiesis_core::scalar::Money;

    use super::*;

    #[test]
    fn resolve_cell_lit_uses_kind_default_unit() {
        let cell = WorkbookCell::Lit {
            value: Scalar::Money {
                value: Money::from_micros(1_000_000),
            },
        };
        let (scalar, unit) = resolve_cell(&cell, &BTreeMap::new(), ScalarKind::Money)
            .expect("literal cells always resolve");
        assert_eq!(scalar, Scalar::Money {
            value: Money::from_micros(1_000_000)
        });
        assert_eq!(unit, Unit::Usd);
    }

    #[test]
    fn resolve_cell_cite_rejects_unknown_fact() {
        let cell = WorkbookCell::Cite {
            fact: FactId::new("ghost").expect("id"),
        };
        let err = resolve_cell(&cell, &BTreeMap::new(), ScalarKind::Count)
            .expect_err("unknown fact must reject");
        assert!(matches!(err, WorkbookError::UnknownFact { .. }));
    }
}
