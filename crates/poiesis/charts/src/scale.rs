//! Linear scale + `nice()` domain extension + 1-2-5 tick generator.
//!
//! A [`Scale`] maps a value from data space (`domain: (min, max)`) to
//! pixel space (`range: (px0, px1)`). It is the smallest reusable
//! geometric primitive the per-kind emitters share.
//!
//! Two derived constructions live here because they are part of the
//! determinism contract:
//!
//! - [`nice`] extends a raw data extent to the next 1-2-5 ×10ⁿ boundary so
//!   `axes: { domain: auto }` produces the same nice-rounded extent on
//!   every machine.
//! - [`ticks`] generates ~5 ticks at 1-2-5 ×10ⁿ spacings so `Ticks::Auto`
//!   is reproducible.

/// Linear mapping from data space to pixel space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scale {
    /// Data-space extent (`min`, `max`).
    pub domain: (f64, f64),
    /// Pixel-space extent (`px0`, `px1`).
    pub range: (f64, f64),
}

impl Scale {
    /// Build a new scale; both extents may be `(min, max)` or `(max, min)`
    /// (the inverted-y convention is common for SVG, where px grows down).
    #[must_use]
    pub const fn new(domain: (f64, f64), range: (f64, f64)) -> Self {
        Self { domain, range }
    }

    /// Map a data value to its pixel coordinate.
    ///
    /// Returns the range midpoint when the domain is degenerate (`min == max`).
    /// A degenerate domain only happens when every datum in a series is the
    /// same value, which is a curated edge case rather than an error.
    #[must_use]
    pub fn map(&self, value: f64) -> f64 {
        let (d0, d1) = self.domain;
        let (r0, r1) = self.range;
        let span = d1 - d0;
        if span.abs() < f64::EPSILON {
            return f64::midpoint(r0, r1);
        }
        let t = (value - d0) / span;
        r0 + t * (r1 - r0)
    }
}

/// Extend a raw `(min, max)` extent to the next 1-2-5 ×10ⁿ boundary.
///
/// The nice extent is always a superset of the raw extent, with both endpoints
/// at "round" multiples of a step chosen so 5 ticks fit comfortably. Used
/// when `AxisSpec::domain == Domain::Auto`.
#[must_use]
pub fn nice(min: f64, max: f64) -> (f64, f64) {
    let (min, max) = if min <= max { (min, max) } else { (max, min) };
    if (max - min).abs() < f64::EPSILON {
        let pad = if min.abs() < f64::EPSILON {
            1.0
        } else {
            min.abs() * 0.1
        };
        return (min - pad, max + pad);
    }

    let span = max - min;
    let step = nice_step(span / 5.0);
    let nice_min = (min / step).floor() * step;
    let nice_max = (max / step).ceil() * step;
    (nice_min, nice_max)
}

/// Generate ~`target_count` tick values in `(min, max)` at 1-2-5 ×10ⁿ spacings.
///
/// The first tick equals `min`; the last tick equals `max`; spacing is
/// uniform. `target_count` is clamped to `[2, 12]` so a single chart cannot
/// emit thousands of ticks even with a hostile spec.
#[must_use]
pub fn ticks(min: f64, max: f64, target_count: u8) -> Vec<f64> {
    let target = target_count.clamp(2, 12);
    if (max - min).abs() < f64::EPSILON {
        return vec![min];
    }
    let raw_step = (max - min) / f64::from(target);
    let step = nice_step(raw_step);
    let mut out = Vec::new();
    let count = tick_count(min, max, step);
    for i in 0..=count {
        let value = min + step * idx_to_f64(i);
        if value > max + step * 0.5 {
            break;
        }
        out.push(value);
    }
    out
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::as_conversions,
    reason = "count is bounded by target tick budget (≤12), well below u32 range"
)]
fn tick_count(min: f64, max: f64, step: f64) -> u32 {
    ((max - min) / step).round().max(0.0) as u32
}

fn idx_to_f64(i: u32) -> f64 {
    f64::from(i)
}

fn nice_step(raw: f64) -> f64 {
    if raw <= 0.0 {
        return 1.0;
    }
    let exponent = raw.log10().floor();
    let fraction = raw / 10_f64.powf(exponent);
    let nice_fraction = if fraction <= 1.0 {
        1.0
    } else if fraction <= 2.0 {
        2.0
    } else if fraction <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice_fraction * 10_f64.powf(exponent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_maps_endpoints() {
        let s = Scale::new((0.0, 100.0), (0.0, 200.0));
        assert!((s.map(0.0)).abs() < 1e-9);
        assert!((s.map(100.0) - 200.0).abs() < 1e-9);
        assert!((s.map(50.0) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn scale_handles_inverted_range() {
        let s = Scale::new((0.0, 100.0), (500.0, 100.0));
        assert!((s.map(0.0) - 500.0).abs() < 1e-9);
        assert!((s.map(100.0) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn scale_degenerate_domain_returns_midpoint() {
        let s = Scale::new((42.0, 42.0), (0.0, 100.0));
        assert!((s.map(42.0) - 50.0).abs() < 1e-9);
    }

    #[test]
    fn nice_rounds_offsite_axis() {
        let (lo, hi) = nice(0.0, 28.0);
        assert!((lo - 0.0).abs() < 1e-9);
        assert!((hi - 30.0).abs() < 1e-9);
    }

    #[test]
    fn nice_handles_degenerate_extent() {
        let (lo, hi) = nice(10.0, 10.0);
        assert!(lo < 10.0);
        assert!(hi > 10.0);
    }

    #[test]
    fn ticks_match_5_at_1_2_5_step() {
        // raw step = 30/5 = 6 → nice step = 10 (1-2-5 ladder snaps 6 up to 10).
        // Result: 4 ticks at the nearest 1-2-5×10ⁿ spacing.
        let t = ticks(0.0, 30.0, 5);
        assert_eq!(t, vec![0.0, 10.0, 20.0, 30.0]);
    }

    #[test]
    fn ticks_higher_target_recovers_offsite_5_step() {
        // For the offsite slide-3 y_left (0..30 at step 5), target ~7 ticks:
        // raw step = 30/7 ≈ 4.3 → nice step = 5.
        let t = ticks(0.0, 30.0, 7);
        assert_eq!(t, vec![0.0, 5.0, 10.0, 15.0, 20.0, 25.0, 30.0]);
    }

    #[test]
    fn ticks_for_y_right_offsite_200() {
        let t = ticks(0.0, 200.0, 5);
        assert_eq!(t, vec![0.0, 50.0, 100.0, 150.0, 200.0]);
    }
}
