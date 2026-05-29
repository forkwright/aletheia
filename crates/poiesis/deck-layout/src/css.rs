use crate::{Canvas, Zone};

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "zone coords are normalized [0,1] × canvas ≤ 1280, well within i64"
)]
fn f64_to_px(v: f64, canvas_px: u32) -> i64 {
    (v * f64::from(canvas_px)).round() as i64
}

/// Convert a normalized zone to absolute CSS pixel coordinates.
///
/// Returns a string like `"left: 64px; top: 36px; width: 1152px; height: 108px;"`.
#[must_use]
pub fn zone_to_css(zone: &Zone, canvas: &Canvas) -> String {
    let left = f64_to_px(zone.x, canvas.width_px);
    let top = f64_to_px(zone.y, canvas.height_px);
    let width = f64_to_px(zone.w, canvas.width_px);
    let height = f64_to_px(zone.h, canvas.height_px);
    format!("left: {left}px; top: {top}px; width: {width}px; height: {height}px;")
}
