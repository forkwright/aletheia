//! Meta-insights computation: anomaly detection and metric aggregation.

pub mod anomaly;

/// Convert `usize` to `f64` losslessly for values that fit in `u32`.
///
/// # Panics
///
/// Does not panic — saturates at `u32::MAX`.
pub(crate) fn usize_to_f64(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}
