//! Theme-driven cell format helpers for the workbook XLSX renderer.

use poiesis_core::scalar::{ScalarKind, Unit};
use poiesis_theme::resolved::ResolvedTheme;
use rust_xlsxwriter::{Color, Format};

/// Returns the data-cell format for a column given its kind, unit, and theme.
pub fn cell_format(kind: ScalarKind, unit: Unit, _theme: &ResolvedTheme) -> Format {
    let fmt = Format::new();

    match (kind, unit) {
        (ScalarKind::Count, Unit::Count) => fmt.set_num_format("#,##0"),
        (ScalarKind::Money, Unit::Usd) => fmt.set_num_format("\"$\"#,##0.00"),
        (ScalarKind::Ratio, Unit::Percent) => fmt.set_num_format("0.0%"),
        (ScalarKind::Ratio, Unit::Ratio) => fmt.set_num_format("0.0000"),
        (ScalarKind::Date, Unit::Date) => fmt.set_num_format("yyyy\\-mm\\-dd"),
        _ => fmt,
    }
}

/// Returns the header-row format: bold + optional theme colours.
pub fn header_format(theme: &ResolvedTheme) -> Format {
    let mut fmt = Format::new().set_bold();

    if let Some(ref_name) = &theme.table.header_fill {
        if let Some(hex) = theme.lookup_color(ref_name) {
            if let Ok(rgb) = u32::from_str_radix(hex.as_str().trim_start_matches('#'), 16) {
                fmt = fmt.set_background_color(Color::RGB(rgb));
            }
        }
    }

    if let Some(ref_name) = &theme.table.header_ink {
        if let Some(hex) = theme.lookup_color(ref_name) {
            if let Ok(rgb) = u32::from_str_radix(hex.as_str().trim_start_matches('#'), 16) {
                fmt = fmt.set_font_color(Color::RGB(rgb));
            }
        }
    }

    fmt
}

/// Returns the totals-row format: same number format as cell_format but bold.
pub fn totals_format(kind: ScalarKind, unit: Unit, theme: &ResolvedTheme) -> Format {
    cell_format(kind, unit, theme).set_bold()
}
