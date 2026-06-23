//! Typed values that replace naked numbers and strings at the model boundary.
//!
//! Every numeric value that enters a deliverable lives in a [`Scalar`] tagged
//! with a [`Unit`]. The render-side never sees a bare `f64`; the QA gate
//! reasons in typed units. Money is represented in integer micro-units to
//! avoid pulling a decimal arithmetic crate while preserving exact accounting
//! to four-decimal precision.

use std::fmt;
use std::str::FromStr;

use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use crate::error::{
    BadAspectSnafu, BadMoneySnafu, BadRatioSnafu, BadToleranceSnafu, ScalarError, UnknownUnitSnafu,
};

/// The kind tag describing the shape of a [`Scalar`].
///
/// `ScalarKind` is what the model carries when it needs to talk about a
/// scalar's *type* without an instance — workbook column types are the
/// motivating consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScalarKind {
    /// Whole-number counts (`Count(i64)`).
    Count,
    /// Monetary amounts (`Money` — `i64` micro-units).
    Money,
    /// Dimensionless ratios (`Ratio(f64)`); use [`Unit::Percent`] when display
    /// should multiply by 100.
    Ratio,
    /// Free-form text values.
    Text,
    /// Calendar dates.
    Date,
}

impl ScalarKind {
    /// Short canonical name used in error messages and serialized form.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::Money => "money",
            Self::Ratio => "ratio",
            Self::Text => "text",
            Self::Date => "date",
        }
    }
}

impl fmt::Display for ScalarKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Monetary amount stored as signed micro-units (1 unit = 1e-6 of the
/// presentation currency). The `i64` micro-unit range covers ±9.22 × 10¹²
/// major units, comfortably more than any deliverable will carry while
/// staying inside the native `serde_json` integer range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Money(i64);

impl Money {
    /// Construct a `Money` directly from micro-units.
    #[must_use]
    pub const fn from_micros(micros: i64) -> Self {
        Self(micros)
    }

    /// The amount in micro-units.
    #[must_use]
    pub const fn micros(self) -> i64 {
        self.0
    }

    /// Build a `Money` from a major-unit integer (e.g. `from_units(150)` = $150.00).
    ///
    /// # Errors
    ///
    /// Returns [`ScalarError::BadMoney`] if multiplication overflows the
    /// representable range (`±9.22e12` major units).
    pub fn from_units(units: i64) -> Result<Self, ScalarError> {
        units
            .checked_mul(1_000_000)
            .map(Self)
            .ok_or_else(|| ScalarError::BadMoney {
                input: units.to_string(),
            })
    }

    /// Build a `Money` from a `(major, micros_fraction)` pair.
    ///
    /// `fraction` is the micro-unit portion in `[0, 1_000_000)`; this lets
    /// callers express e.g. `$1.23` as `Money::from_units_and_fraction(1, 230_000)`
    /// without floating-point.
    ///
    /// # Errors
    ///
    /// Returns [`ScalarError::BadMoney`] if `fraction` is out of range or
    /// the magnitude overflows.
    pub fn from_units_and_fraction(units: i64, fraction: u32) -> Result<Self, ScalarError> {
        if fraction >= 1_000_000 {
            return BadMoneySnafu {
                input: format!("{units}.{fraction:06}"),
            }
            .fail();
        }
        let sign: i64 = if units < 0 { -1 } else { 1 };
        let abs_units = units.checked_abs().ok_or_else(|| ScalarError::BadMoney {
            input: format!("{units}.{fraction:06}"),
        })?;
        let scaled = abs_units
            .checked_mul(1_000_000)
            .ok_or_else(|| ScalarError::BadMoney {
                input: format!("{units}.{fraction:06}"),
            })?;
        let summed =
            scaled
                .checked_add(i64::from(fraction))
                .ok_or_else(|| ScalarError::BadMoney {
                    input: format!("{units}.{fraction:06}"),
                })?;
        Ok(Self(sign * summed))
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let micros = self.0;
        let sign = if micros < 0 { "-" } else { "" };
        let abs = micros.unsigned_abs();
        let units = abs / 1_000_000;
        let fraction = abs % 1_000_000;
        if fraction == 0 {
            write!(f, "{sign}{units}")
        } else {
            let mut frac = format!("{fraction:06}");
            while frac.ends_with('0') {
                frac.pop();
            }
            write!(f, "{sign}{units}.{frac}")
        }
    }
}

/// A typed scalar value.
///
/// `Scalar` is the model-side representation of an authored value. It is
/// always paired with a [`Unit`] at the [`crate::factbase::Fact`] level so
/// callers know *what* the number means in addition to its raw shape.
// kanon:ignore RUST/non-exhaustive-enum — exhaustive match is part of the
// stable API; new kinds are an explicit additive evolution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Scalar {
    /// Whole-number count.
    Count {
        /// The count.
        value: i64,
    },
    /// Monetary amount (see [`Money`]).
    Money {
        /// The amount.
        value: Money,
    },
    /// Dimensionless ratio (`0.5` = 50%); pair with [`Unit::Percent`] for display.
    Ratio {
        /// The ratio.
        value: f64,
    },
    /// Free-form text.
    Text {
        /// The text.
        value: String,
    },
    /// Calendar date.
    Date {
        /// The date.
        value: Date,
    },
}

impl Scalar {
    /// Construct a finite ratio scalar.
    ///
    /// # Errors
    ///
    /// Returns [`ScalarError::BadRatio`] if `value` is `NaN`, `Infinity`, or
    /// `-Infinity`.
    pub fn new_ratio(value: f64) -> Result<Self, ScalarError> {
        if !value.is_finite() {
            return BadRatioSnafu { value }.fail();
        }
        Ok(Self::Ratio { value })
    }

    /// The kind tag of this scalar.
    #[must_use]
    pub fn kind(&self) -> ScalarKind {
        match self {
            Self::Count { .. } => ScalarKind::Count,
            Self::Money { .. } => ScalarKind::Money,
            Self::Ratio { .. } => ScalarKind::Ratio,
            Self::Text { .. } => ScalarKind::Text,
            Self::Date { .. } => ScalarKind::Date,
        }
    }
}

/// The presentation unit attached to a [`Scalar`].
///
/// Units describe meaning, not magnitude — `Percent` does not multiply the
/// underlying ratio; it tells the renderer to format `0.42` as `42%`. The
/// QA gate uses units to reject arithmetic that crosses incompatible
/// dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    /// A dimensionless count (people, sessions, line items).
    Count,
    /// US dollars (presentation currency; see [`Money`] for value shape).
    Usd,
    /// Dimensionless percentage; display multiplies the underlying ratio by 100.
    Percent,
    /// Dimensionless ratio; display leaves the underlying ratio as-is.
    Ratio,
    /// A calendar date.
    Date,
    /// Free-form text (label, slug, name).
    Text,
}

impl Unit {
    /// The canonical name as serialized.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::Usd => "usd",
            Self::Percent => "percent",
            Self::Ratio => "ratio",
            Self::Date => "date",
            Self::Text => "text",
        }
    }

    /// Whether this unit is compatible with the given [`ScalarKind`].
    ///
    /// `Unit::Percent` and `Unit::Ratio` pair with `ScalarKind::Ratio`;
    /// `Unit::Usd` pairs with `ScalarKind::Money`; the rest pair 1:1 with
    /// their like-named kind.
    #[must_use]
    pub fn compatible_with(self, kind: ScalarKind) -> bool {
        matches!(
            (self, kind),
            (Self::Count, ScalarKind::Count)
                | (Self::Usd, ScalarKind::Money)
                | (Self::Percent | Self::Ratio, ScalarKind::Ratio)
                | (Self::Date, ScalarKind::Date)
                | (Self::Text, ScalarKind::Text)
        )
    }
}

impl fmt::Display for Unit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Unit {
    type Err = ScalarError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "count" => Ok(Self::Count),
            "usd" => Ok(Self::Usd),
            "percent" => Ok(Self::Percent),
            "ratio" => Ok(Self::Ratio),
            "date" => Ok(Self::Date),
            "text" => Ok(Self::Text),
            _ => UnknownUnitSnafu { input: s }.fail(),
        }
    }
}

/// A slide aspect ratio expressed as `width:height` integer pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AspectRatio {
    width: u16,
    height: u16,
}

impl AspectRatio {
    /// 16:9 widescreen.
    pub const WIDESCREEN_16_9: Self = Self {
        width: 16,
        height: 9,
    };
    /// 4:3 standard.
    pub const STANDARD_4_3: Self = Self {
        width: 4,
        height: 3,
    };

    /// Construct an aspect ratio from a `(width, height)` integer pair.
    ///
    /// # Errors
    ///
    /// Returns [`ScalarError::BadAspect`] if either dimension is zero.
    pub fn new(width: u16, height: u16) -> Result<Self, ScalarError> {
        if width == 0 || height == 0 {
            return BadAspectSnafu {
                input: format!("{width}:{height}"),
            }
            .fail();
        }
        Ok(Self { width, height })
    }

    /// The width component.
    #[must_use]
    pub fn width(self) -> u16 {
        self.width
    }

    /// The height component.
    #[must_use]
    pub fn height(self) -> u16 {
        self.height
    }
}

impl fmt::Display for AspectRatio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.width, self.height)
    }
}

impl FromStr for AspectRatio {
    type Err = ScalarError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split(':').collect();
        let [w, h] = match parts.as_slice() {
            [w, h] => [*w, *h],
            _ => return BadAspectSnafu { input: s }.fail(),
        };
        let Ok(width) = w.parse::<u16>() else {
            return BadAspectSnafu { input: s }.fail();
        };
        let Ok(height) = h.parse::<u16>() else {
            return BadAspectSnafu { input: s }.fail();
        };
        Self::new(width, height)
    }
}

impl TryFrom<String> for AspectRatio {
    type Error = ScalarError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::from_str(&s)
    }
}

impl From<AspectRatio> for String {
    fn from(value: AspectRatio) -> Self {
        value.to_string()
    }
}

/// A numeric tolerance used by the QA gate when comparing computed and
/// claimed values. `0.0` requires exact equality; `0.01` allows a 1%
/// relative difference.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "f64", into = "f64")]
pub struct Tolerance(f64);

impl Tolerance {
    /// Strict-equality tolerance; the default for claim verification.
    pub const STRICT: Self = Self(0.0);

    /// Construct a tolerance in the closed unit interval `[0.0, 1.0]`.
    ///
    /// # Errors
    ///
    /// Returns [`ScalarError::BadTolerance`] if the value is `NaN`, negative,
    /// or greater than `1.0`.
    pub fn new(value: f64) -> Result<Self, ScalarError> {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return BadToleranceSnafu { value }.fail();
        }
        Ok(Self(value))
    }

    /// The tolerance as an `f64`.
    #[must_use]
    pub fn as_f64(self) -> f64 {
        self.0
    }
}

impl TryFrom<f64> for Tolerance {
    type Error = ScalarError;
    fn try_from(value: f64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Tolerance> for f64 {
    fn from(value: Tolerance) -> Self {
        value.0
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn money_from_units_round_trips() {
        let m = Money::from_units(150).expect("in range");
        assert_eq!(m.micros(), 150_000_000);
        assert_eq!(m.to_string(), "150");
    }

    #[test]
    fn money_fractional_displays_trimmed() {
        let m = Money::from_units_and_fraction(1, 230_000).expect("valid");
        assert_eq!(m.to_string(), "1.23");
    }

    #[test]
    fn money_negative_displays_with_sign() {
        let m = Money::from_units(-42).expect("in range");
        assert_eq!(m.to_string(), "-42");
    }

    #[test]
    fn money_rejects_out_of_range_fraction() {
        let err = Money::from_units_and_fraction(1, 1_000_000).expect_err("rejected");
        assert!(matches!(err, ScalarError::BadMoney { .. }));
    }

    #[test]
    fn unit_round_trips_via_string() {
        for u in [
            Unit::Count,
            Unit::Usd,
            Unit::Percent,
            Unit::Ratio,
            Unit::Date,
            Unit::Text,
        ] {
            let back: Unit = u.as_str().parse().expect("parse");
            assert_eq!(back, u);
        }
    }

    #[test]
    fn unit_rejects_unknown() {
        let err: Result<Unit, _> = "furlong".parse();
        assert!(matches!(err, Err(ScalarError::UnknownUnit { .. })));
    }

    #[test]
    fn unit_compatibility_holds() {
        assert!(Unit::Percent.compatible_with(ScalarKind::Ratio));
        assert!(Unit::Ratio.compatible_with(ScalarKind::Ratio));
        assert!(Unit::Usd.compatible_with(ScalarKind::Money));
        assert!(!Unit::Count.compatible_with(ScalarKind::Money));
        assert!(!Unit::Date.compatible_with(ScalarKind::Count));
    }

    #[test]
    fn aspect_ratio_parses_and_round_trips() {
        let a: AspectRatio = "16:9".parse().expect("parse");
        assert_eq!(a, AspectRatio::WIDESCREEN_16_9);
        assert_eq!(a.to_string(), "16:9");
    }

    #[test]
    fn aspect_ratio_rejects_zero_height() {
        let err: Result<AspectRatio, _> = "16:0".parse();
        assert!(matches!(err, Err(ScalarError::BadAspect { .. })));
    }

    #[test]
    fn tolerance_rejects_negative_and_nan() {
        assert!(matches!(
            Tolerance::new(-0.1),
            Err(ScalarError::BadTolerance { .. })
        ));
        assert!(matches!(
            Tolerance::new(f64::NAN),
            Err(ScalarError::BadTolerance { .. })
        ));
        assert!(matches!(
            Tolerance::new(1.5),
            Err(ScalarError::BadTolerance { .. })
        ));
    }

    #[test]
    fn scalar_kind_matches_variant() {
        let s = Scalar::Count { value: 7 };
        assert_eq!(s.kind(), ScalarKind::Count);
    }

    #[test]
    fn scalar_round_trips_via_serde() {
        let s = Scalar::Money {
            value: Money::from_units(100).expect("in range"),
        };
        let json = serde_json::to_string(&s).expect("ser");
        let back: Scalar = serde_json::from_str(&json).expect("de");
        assert_eq!(back, s);
    }

    #[test]
    fn ratio_accepts_finite() {
        let got = Scalar::new_ratio(0.42).expect("finite");
        assert_eq!(got, Scalar::Ratio { value: 0.42 });
    }

    #[test]
    fn ratio_rejects_non_finite() {
        assert!(matches!(
            Scalar::new_ratio(f64::NAN),
            Err(ScalarError::BadRatio { .. })
        ));
        assert!(matches!(
            Scalar::new_ratio(f64::INFINITY),
            Err(ScalarError::BadRatio { .. })
        ));
        assert!(matches!(
            Scalar::new_ratio(f64::NEG_INFINITY),
            Err(ScalarError::BadRatio { .. })
        ));
    }
}
