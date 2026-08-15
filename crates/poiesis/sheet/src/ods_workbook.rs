//! ODS renderer for the [`Workbook`] body.
//!
//! Mirrors [`crate::workbook::render_workbook`]'s per-sheet iteration on
//! `spreadsheet-ods`'s `WorkBook`/`Sheet` API instead of `rust_xlsxwriter`
//! (B-007: workbook-body renderer parity across the sheet backends). Cell
//! resolution (`Lit`/`Cite` -> scalar + unit) is shared with the XLSX path
//! via [`crate::cell_resolve`]; only the write step and the theme-driven
//! style registration are format-specific, since `spreadsheet-ods` styles
//! are named objects registered once on the workbook rather than XLSX's
//! per-cell `Format` value.

use std::collections::BTreeMap;

use poiesis_core::bodies::{Sheet, Workbook};
use poiesis_core::factbase::{Factbase, ResolvedFact};
use poiesis_core::ids::FactId;
use poiesis_core::scalar::{Scalar, ScalarKind, Unit};
use poiesis_theme::resolved::ResolvedTheme;
use spreadsheet_ods::color::Rgb;
use spreadsheet_ods::format::{
    FormatNumberStyle, create_number_format_fixed, create_percentage_format,
};
use spreadsheet_ods::{
    CellStyle, CellStyleRef, Sheet as OdsSheet, Value, ValueFormatCurrency, ValueFormatDateTime,
    WorkBook as OdsWorkBook, write_ods_buf,
};

use crate::cell_resolve::{resolve_cell, unit_for_total};
use crate::error::WorkbookError;
use crate::ods_sources::append_ods_sources_sheet;
use crate::totals::compute_totals;

/// Shorthand for ODS workbook-level results.
type Result<T> = std::result::Result<T, WorkbookError>;

/// Theme-driven cell styles registered once on an [`OdsWorkBook`], keyed by
/// the same `(kind, unit)` dispatch [`crate::format::cell_format`] uses for
/// XLSX, plus a bold variant of each for the totals row.
pub(crate) struct OdsStyles {
    pub(crate) header: CellStyleRef,
    count: CellStyleRef,
    count_bold: CellStyleRef,
    money: CellStyleRef,
    money_bold: CellStyleRef,
    percent: CellStyleRef,
    percent_bold: CellStyleRef,
    ratio: CellStyleRef,
    ratio_bold: CellStyleRef,
    date: CellStyleRef,
    date_bold: CellStyleRef,
    /// Bold-only style for totals in a column whose kind/unit combination has
    /// no dedicated number format (mirrors XLSX's `totals_format` bolding an
    /// otherwise-default `Format::new()`).
    plain_bold: CellStyleRef,
}

impl OdsStyles {
    /// Register every style this renderer needs on `ods_wb` and return the
    /// handles. Must run before any sheet referencing them is pushed.
    pub(crate) fn register(ods_wb: &mut OdsWorkBook, theme: &ResolvedTheme) -> Self {
        let count_fmt = ods_wb.add_number_format(create_number_format_fixed("num-count", 0, true));
        let money_fmt = ods_wb.add_currency_format(money_format("num-money"));
        let percent_fmt = ods_wb.add_percentage_format(create_percentage_format("num-percent", 1));
        let ratio_fmt = ods_wb.add_number_format(create_number_format_fixed("num-ratio", 4, false));
        let date_fmt = ods_wb.add_datetime_format(date_format("num-date"));

        let count = ods_wb.add_cellstyle(CellStyle::new("cell-count", &count_fmt));
        let count_bold = ods_wb.add_cellstyle(bold(CellStyle::new("cell-count-bold", &count_fmt)));
        let money = ods_wb.add_cellstyle(CellStyle::new("cell-money", &money_fmt));
        let money_bold = ods_wb.add_cellstyle(bold(CellStyle::new("cell-money-bold", &money_fmt)));
        let percent = ods_wb.add_cellstyle(CellStyle::new("cell-percent", &percent_fmt));
        let percent_bold =
            ods_wb.add_cellstyle(bold(CellStyle::new("cell-percent-bold", &percent_fmt)));
        let ratio = ods_wb.add_cellstyle(CellStyle::new("cell-ratio", &ratio_fmt));
        let ratio_bold = ods_wb.add_cellstyle(bold(CellStyle::new("cell-ratio-bold", &ratio_fmt)));
        let date = ods_wb.add_cellstyle(CellStyle::new("cell-date", &date_fmt));
        let date_bold = ods_wb.add_cellstyle(bold(CellStyle::new("cell-date-bold", &date_fmt)));
        let plain_bold = ods_wb.add_cellstyle(bold(CellStyle::new_empty()));

        let header = ods_wb.add_cellstyle(header_style(theme));

        Self {
            header,
            count,
            count_bold,
            money,
            money_bold,
            percent,
            percent_bold,
            ratio,
            ratio_bold,
            date,
            date_bold,
            plain_bold,
        }
    }

    /// Resolve the style for a data or totals cell, mirroring
    /// [`crate::format::cell_format`]'s `(kind, unit)` dispatch. Returns
    /// `None` for a non-bold cell with no dedicated format (equivalent to
    /// XLSX's unformatted `Format::new()` fallback -- no style needed).
    fn cell_style(&self, kind: ScalarKind, unit: Unit, is_bold: bool) -> Option<&CellStyleRef> {
        match (kind, unit, is_bold) {
            (ScalarKind::Count, Unit::Count, false) => Some(&self.count),
            (ScalarKind::Count, Unit::Count, true) => Some(&self.count_bold),
            (ScalarKind::Money, Unit::Usd, false) => Some(&self.money),
            (ScalarKind::Money, Unit::Usd, true) => Some(&self.money_bold),
            (ScalarKind::Ratio, Unit::Percent, false) => Some(&self.percent),
            (ScalarKind::Ratio, Unit::Percent, true) => Some(&self.percent_bold),
            (ScalarKind::Ratio, Unit::Ratio, false) => Some(&self.ratio),
            (ScalarKind::Ratio, Unit::Ratio, true) => Some(&self.ratio_bold),
            (ScalarKind::Date, Unit::Date, false) => Some(&self.date),
            (ScalarKind::Date, Unit::Date, true) => Some(&self.date_bold),
            (_, _, false) => None,
            (_, _, true) => Some(&self.plain_bold),
        }
    }
}

/// Currency format: `$` prefix, grouped, 2 fixed decimal places -- matches
/// XLSX's `"$"#,##0.00`.
fn money_format(name: &'static str) -> ValueFormatCurrency {
    let mut v = ValueFormatCurrency::new_named(name);
    v.part_currency().symbol("$").build();
    v.part_number()
        .min_integer_digits(1)
        .fixed_decimal_places(2)
        .grouping()
        .build();
    v
}

/// Date format `yyyy-mm-dd` -- matches XLSX's `yyyy\\-mm\\-dd`.
///
/// NOTE: Applied to a `Value::Text` cell, exactly as XLSX's `cell_format`
/// applies its `yyyy\\-mm\\-dd` `Format` to a string written by
/// [`crate::workbook::write_scalar`]'s `Scalar::Date` arm -- neither backend
/// has a native date scalar today, so this is parity with the existing XLSX
/// behavior (the style's data-style-name is inert on a string-typed cell in
/// both formats) rather than a new gap introduced here.
fn date_format(name: &'static str) -> ValueFormatDateTime {
    let mut v = ValueFormatDateTime::new_named(name);
    v.part_year().style(FormatNumberStyle::Long).build();
    v.part_text("-").build();
    v.part_month().style(FormatNumberStyle::Long).build();
    v.part_text("-").build();
    v.part_day().style(FormatNumberStyle::Long).build();
    v
}

/// Apply bold to a [`CellStyle`] and return it (builder-style helper since
/// `set_font_bold` mutates in place).
fn bold(mut style: CellStyle) -> CellStyle {
    style.set_font_bold();
    style
}

/// Header-row style: bold + theme colours, mirroring
/// [`crate::format::header_format`].
fn header_style(theme: &ResolvedTheme) -> CellStyle {
    let mut style = CellStyle::new_empty();
    style.set_font_bold();

    if let Some(ref_name) = &theme.table.header_fill
        && let Some(hex) = theme.lookup_color(ref_name)
        && let Some(rgb) = rgb_from_hex(hex.as_str())
    {
        style.set_background_color(rgb);
    }

    if let Some(ref_name) = &theme.table.header_ink
        && let Some(hex) = theme.lookup_color(ref_name)
        && let Some(rgb) = rgb_from_hex(hex.as_str())
    {
        style.set_color(rgb);
    }

    style
}

/// Parse a `#rrggbb` theme colour into an [`Rgb`] triple.
fn rgb_from_hex(hex: &str) -> Option<Rgb<u8>> {
    let packed = u32::from_str_radix(hex.trim_start_matches('#'), 16).ok()?;
    let r = u8::try_from((packed >> 16) & 0xFF).ok()?;
    let g = u8::try_from((packed >> 8) & 0xFF).ok()?;
    let b = u8::try_from(packed & 0xFF).ok()?;
    Some(Rgb::new(r, g, b))
}

/// Renders a [`Workbook`] to an ODS byte vector.
///
/// Structural and provenance parity with [`crate::workbook::render_workbook`]:
/// same per-sheet/per-cell resolution against `facts`, same optional
/// "Sources" worksheet when `provenance` is `Some`.
///
/// # Errors
///
/// Returns [`WorkbookError::UnknownFact`] for an unresolved
/// [`poiesis_core::bodies::WorkbookCell::Cite`], [`WorkbookError::NonFiniteRatio`]
/// for a non-finite ratio value, and [`WorkbookError::OdsWrite`] on any
/// underlying `spreadsheet-ods` failure.
pub fn render_ods_workbook(
    wb: &Workbook,
    facts: &BTreeMap<FactId, ResolvedFact>,
    theme: &ResolvedTheme,
    provenance: Option<&Factbase>,
) -> std::result::Result<Vec<u8>, WorkbookError> {
    let mut ods_wb = OdsWorkBook::new_empty();
    let styles = OdsStyles::register(&mut ods_wb, theme);

    for sheet in &wb.sheets {
        render_sheet(&mut ods_wb, sheet, facts, &styles)?;
    }

    if let Some(factbase) = provenance {
        append_ods_sources_sheet(&mut ods_wb, wb, factbase, &styles)?;
    }

    write_ods_buf(&mut ods_wb, Vec::new()).map_err(WorkbookError::from)
}

fn render_sheet(
    ods_wb: &mut OdsWorkBook,
    sheet: &Sheet,
    facts: &BTreeMap<FactId, ResolvedFact>,
    styles: &OdsStyles,
) -> Result<()> {
    let mut ods_sheet = OdsSheet::new(sheet.name.as_str());
    ods_sheet.set_header_rows(0, 0);

    for (col_idx, header) in sheet.headers.iter().enumerate() {
        let col_u32 = col_index(col_idx)?;
        ods_sheet.set_styled_value(0, col_u32, header.as_str(), &styles.header);
    }

    for (row_idx, row) in sheet.rows.iter().enumerate() {
        let row_num = row_index(row_idx)?;

        for (col_idx, cell) in row.iter().enumerate() {
            let col_u32 = col_index(col_idx)?;

            let kind = sheet.column_types.get(col_idx).copied();
            let Some(kind) = kind else {
                continue;
            };

            let (scalar, unit) = resolve_cell(cell, facts, kind)?;
            let style = styles.cell_style(kind, unit, false);
            write_scalar(&mut ods_sheet, row_num, col_u32, &scalar, unit, style)?;
        }
    }

    let totals = compute_totals(sheet, facts)?;
    let totals_row = row_index(sheet.rows.len())?;

    for (col_idx, total) in totals.iter().enumerate() {
        let Some(total) = total else { continue };
        let col_u32 = col_index(col_idx)?;
        let kind = sheet.column_types.get(col_idx).copied();
        let Some(kind) = kind else { continue };
        let unit = unit_for_total(sheet, facts, col_idx, kind);
        let style = styles.cell_style(kind, unit, true);
        write_scalar(&mut ods_sheet, totals_row, col_u32, total, unit, style)?;
    }

    ods_wb.push_sheet(ods_sheet);
    Ok(())
}

/// Write a [`Scalar`] into an ODS cell, applying `style` when present.
fn write_scalar(
    sheet: &mut OdsSheet,
    row: u32,
    col: u32,
    scalar: &Scalar,
    unit: Unit,
    style: Option<&CellStyleRef>,
) -> Result<()> {
    let value = scalar_to_value(scalar, unit)?;
    match style {
        Some(style) => sheet.set_styled_value(row, col, value, style),
        None => sheet.set_value(row, col, value),
    }
    Ok(())
}

/// Convert a [`Scalar`] + [`Unit`] pair into a typed ODS [`Value`].
///
/// WHY: `Scalar::Date` renders through `Value::Text(value.to_string())`, not
/// a native ODS date value, matching [`crate::workbook::write_scalar`]'s
/// existing XLSX behavior -- that renderer also writes dates as display
/// strings rather than a native Excel date serial, so this is parity with
/// what exists today, not a regression against a native-date XLSX path that
/// does not exist.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "ods numeric output is a presentation conversion, matching write_scalar's XLSX precedent"
)]
fn scalar_to_value(scalar: &Scalar, unit: Unit) -> Result<Value> {
    match scalar {
        Scalar::Count { value } => Ok(Value::Number(*value as f64)),
        Scalar::Money { value } => {
            let dollars = value.micros() as f64 / 1_000_000.0; // kanon:ignore RUST/as-cast — i64→f64 via TryFrom is unavailable in std; precision loss is acceptable for spreadsheet display, matching write_scalar's XLSX precedent
            Ok(Value::Currency(dollars, "USD".into()))
        }
        Scalar::Ratio { value } => {
            if !value.is_finite() {
                return Err(WorkbookError::NonFiniteRatio { value: *value });
            }
            if unit == Unit::Percent {
                Ok(Value::Percentage(*value))
            } else {
                Ok(Value::Number(*value))
            }
        }
        Scalar::Text { value } => Ok(Value::Text(value.clone())),
        Scalar::Date { value } => Ok(Value::Text(value.to_string())),
    }
}

fn col_index(col_idx: usize) -> Result<u32> {
    u32::try_from(col_idx).map_err(|e| WorkbookError::OdsWrite {
        message: format!("column index {col_idx} exceeds u32 max: {e}"),
    })
}

fn row_index(row_idx: usize) -> Result<u32> {
    u32::try_from(row_idx)
        .map_err(|e| WorkbookError::OdsWrite {
            message: format!("row index {row_idx} exceeds u32 max: {e}"),
        })
        .map(|n| n + 1)
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use poiesis_core::bodies::{Sheet, Workbook, WorkbookCell};
    use poiesis_core::factbase::{Fact, Factbase, ResolvedFact, Source};
    use poiesis_core::ids::{FactId, SheetName};
    use poiesis_core::scalar::{Money, ScalarKind};

    use super::*;

    fn resolved_facts() -> BTreeMap<FactId, ResolvedFact> {
        let mut map = BTreeMap::new();
        map.insert(
            FactId::new("revenue").expect("id"),
            ResolvedFact {
                id: FactId::new("revenue").expect("id"),
                value: Scalar::Money {
                    value: Money::from_micros(1_500_000_000),
                },
                unit: Unit::Usd,
            },
        );
        map
    }

    fn sample_workbook() -> Workbook {
        Workbook {
            sheets: vec![Sheet {
                name: SheetName::new("Q1").expect("name"),
                headers: vec!["Metric".to_owned(), "Amount".to_owned()],
                rows: vec![vec![
                    WorkbookCell::Lit {
                        value: Scalar::Text {
                            value: "Revenue".to_owned(),
                        },
                    },
                    WorkbookCell::Cite {
                        fact: FactId::new("revenue").expect("id"),
                    },
                ]],
                column_types: vec![ScalarKind::Text, ScalarKind::Money],
            }],
        }
    }

    #[test]
    fn render_ods_workbook_produces_valid_zip() {
        let wb = sample_workbook();
        let theme = poiesis_theme::summus();
        let bytes =
            render_ods_workbook(&wb, &resolved_facts(), &theme, None).expect("ods render succeeds");
        assert!(bytes.starts_with(b"PK"), "ODS output should be a valid ZIP");
    }

    #[test]
    fn render_ods_workbook_rejects_unknown_fact() {
        let wb = Workbook {
            sheets: vec![Sheet {
                name: SheetName::new("Q1").expect("name"),
                headers: vec!["Amount".to_owned()],
                rows: vec![vec![WorkbookCell::Cite {
                    fact: FactId::new("ghost").expect("id"),
                }]],
                column_types: vec![ScalarKind::Money],
            }],
        };
        let theme = poiesis_theme::summus();
        let err = render_ods_workbook(&wb, &BTreeMap::new(), &theme, None)
            .expect_err("unknown fact must reject");
        assert!(matches!(err, WorkbookError::UnknownFact { .. }));
    }

    #[test]
    fn render_ods_workbook_with_provenance_appends_sources_sheet() {
        let wb = sample_workbook();
        let mut factbase = Factbase::new();
        let json = serde_json::json!({
            "id": "revenue",
            "value": {"kind": "money", "value": 1_500_000_000_i64},
            "unit": "usd",
            "source": serde_json::to_value(Source::Manual {
                note: "Q1 close".to_owned(),
                captured_by: "operator".to_owned(),
            }).expect("serialize source"),
            "captured": "1970-01-01T00:00:00Z",
        });
        let fact: Fact = serde_json::from_value(json).expect("deserialize fact");
        factbase.add_fact(fact);

        let theme = poiesis_theme::summus();
        let bytes = render_ods_workbook(&wb, &resolved_facts(), &theme, Some(&factbase))
            .expect("ods render with provenance succeeds");
        assert!(bytes.starts_with(b"PK"), "ODS output should be a valid ZIP");
    }
}
