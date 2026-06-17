//! Fixed-precision number formatting for chart text.
//!
//! Determinism requires that the only path from `f64` to chart `<text>`
//! goes through this module. No code outside `format` may call
//! `format!("{}", value)` on a raw float that ends up in an SVG `<text>`.
//!
//! The contract:
//!
//! - Decimal separator: `.` (no locale-dependence).
//! - Thousands grouping: only for `Compact`; absent otherwise.
//! - Rounding: half-away-from-zero, applied at the precision boundary.
//! - Negative zero: collapsed to `0` at the printable precision.
//!
//! `Unit` chooses the precision when `NumFormat::FromUnit` is in effect:
//! `Number` → 0 dp for |x| ≥ 1000, else up to 2 dp; `Money` → integer with
//! currency prefix; `Percent` → 1 dp with `%` suffix; `Seconds` → integer
//! with `s` suffix.

use crate::model::{NumFormat, Unit};

/// Format a number for axis ticks / data labels.
///
/// `unit` is only consulted when `format == NumFormat::FromUnit`.
///
/// # Examples
///
/// ```
/// use poiesis_charts::format::format_number;
/// use poiesis_charts::{NumFormat, Unit};
///
/// assert_eq!(format_number(1500.0, NumFormat::Compact, Unit::Number), "1.5k");
/// assert_eq!(format_number(99.0, NumFormat::Int, Unit::Number), "99");
/// assert_eq!(format_number(12.5, NumFormat::Percent, Unit::Percent), "12.5%");
/// ```
#[must_use]
pub fn format_number(value: f64, format: NumFormat, unit: Unit) -> String {
    assert!(
        value.is_finite(),
        "format_number received non-finite value: {value}"
    );
    let effective = if matches!(format, NumFormat::FromUnit) {
        format_for_unit(unit)
    } else {
        format
    };
    match effective {
        NumFormat::Int => format_int(value),
        NumFormat::Money => format_money(value),
        NumFormat::Percent => format_percent(value),
        NumFormat::Compact => format_compact(value),
        NumFormat::FromUnit => format_default(value),
    }
}

const fn format_for_unit(unit: Unit) -> NumFormat {
    match unit {
        Unit::Number | Unit::Seconds => NumFormat::Int,
        Unit::Money => NumFormat::Money,
        Unit::Percent => NumFormat::Percent,
    }
}

fn format_int(v: f64) -> String {
    let rounded = v.round();
    if rounded == 0.0 {
        return "0".to_owned();
    }
    format!("{rounded:.0}")
}

fn format_money(v: f64) -> String {
    let rounded = v.round();
    if rounded == 0.0 {
        return "$0".to_owned();
    }
    format!("${rounded:.0}")
}

fn format_percent(v: f64) -> String {
    let rounded = (v * 10.0).round() / 10.0;
    if rounded == 0.0 {
        return "0.0%".to_owned();
    }
    format!("{rounded:.1}%")
}

fn format_compact(v: f64) -> String {
    let abs = v.abs();
    let (scaled, suffix) = if abs >= 1e9 {
        (v / 1e9, "B")
    } else if abs >= 1e6 {
        (v / 1e6, "M")
    } else if abs >= 1e3 {
        (v / 1e3, "k")
    } else {
        (v, "")
    };
    let rounded = (scaled * 10.0).round() / 10.0;
    if rounded == 0.0 {
        return "0".to_owned();
    }
    if suffix.is_empty() {
        format!("{rounded:.0}")
    } else {
        format!("{rounded:.1}{suffix}")
    }
}

fn format_default(v: f64) -> String {
    if v.abs() >= 1000.0 {
        format_int(v)
    } else {
        let rounded = (v * 100.0).round() / 100.0;
        if rounded == 0.0 {
            return "0".to_owned();
        }
        let s = format!("{rounded:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

/// Format an SVG coordinate to two decimal places.
///
/// The single chokepoint for `f64` → SVG attribute conversion. Every
/// coordinate the emitter writes passes through here, which is what makes
/// the byte-identical golden-snapshot contract hold.
#[must_use]
pub fn coord(v: f64) -> String {
    assert!(v.is_finite(), "coord received non-finite value: {v}");
    let rounded = (v * 100.0).round() / 100.0;
    if rounded == 0.0 {
        return "0".to_owned();
    }
    let s = format!("{rounded:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_rounds_to_one_dp() {
        assert_eq!(
            format_number(12.345, NumFormat::Percent, Unit::Percent),
            "12.3%"
        );
        assert_eq!(
            format_number(0.0, NumFormat::Percent, Unit::Percent),
            "0.0%"
        );
    }

    #[test]
    fn money_is_integer_with_dollar_prefix() {
        assert_eq!(
            format_number(1234.56, NumFormat::Money, Unit::Money),
            "$1235"
        );
        assert_eq!(format_number(0.0, NumFormat::Money, Unit::Money), "$0");
    }

    #[test]
    fn compact_uses_si_suffixes() {
        assert_eq!(
            format_number(950.0, NumFormat::Compact, Unit::Number),
            "950"
        );
        assert_eq!(
            format_number(1500.0, NumFormat::Compact, Unit::Number),
            "1.5k"
        );
        assert_eq!(
            format_number(2_000_000.0, NumFormat::Compact, Unit::Number),
            "2.0M"
        );
        assert_eq!(
            format_number(3_500_000_000.0, NumFormat::Compact, Unit::Number),
            "3.5B"
        );
    }

    #[test]
    fn from_unit_routes_through_unit() {
        // Number → int when large, decimal when small.
        assert_eq!(
            format_number(1500.6, NumFormat::FromUnit, Unit::Number),
            "1501"
        );
        // Money → integer with prefix.
        assert_eq!(format_number(99.0, NumFormat::FromUnit, Unit::Money), "$99");
        // Percent → 1 dp with suffix.
        assert_eq!(
            format_number(12.5, NumFormat::FromUnit, Unit::Percent),
            "12.5%"
        );
        // Seconds → int.
        assert_eq!(
            format_number(60.4, NumFormat::FromUnit, Unit::Seconds),
            "60"
        );
    }

    #[test]
    fn coord_rounds_to_two_dp() {
        assert_eq!(coord(123.456), "123.46");
        assert_eq!(coord(0.0), "0");
        assert_eq!(coord(-0.001), "0");
        assert_eq!(coord(10.0), "10");
    }

    #[test]
    #[should_panic(expected = "coord received non-finite value: NaN")]
    fn coord_panics_on_nan() {
        let _ = coord(f64::NAN);
    }

    #[test]
    #[should_panic(expected = "format_number received non-finite value: inf")]
    fn format_number_panics_on_infinity() {
        let _ = format_number(f64::INFINITY, NumFormat::Int, Unit::Number);
    }
}
