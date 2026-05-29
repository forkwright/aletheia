use crate::{Canvas, Zone};

/// One pixel in EMU (English Metric Units).
const PX_TO_EMU: f64 = 9144.0;

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "zone coords are normalized [0,1] × canvas × 9144 ≤ 9e6, well within i64"
)]
fn f64_to_emu(v: f64, canvas_px: u32) -> i64 {
    (v * f64::from(canvas_px) * PX_TO_EMU).round() as i64
}

/// Convert a normalized zone to OOXML EMU coordinates.
///
/// Returns `(x_emu, y_emu, cx_emu, cy_emu)`.
#[must_use]
pub fn zone_to_emu(zone: &Zone, canvas: &Canvas) -> (i64, i64, i64, i64) {
    let x = f64_to_emu(zone.x, canvas.width_px);
    let y = f64_to_emu(zone.y, canvas.height_px);
    let cx = f64_to_emu(zone.w, canvas.width_px);
    let cy = f64_to_emu(zone.h, canvas.height_px);
    (x, y, cx, cy)
}
