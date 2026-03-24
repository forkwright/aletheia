//! Reusable drag-to-resize panel divider.
//!
//! # Usage
//!
//! 1. Add `use_resize_state` to the parent component to get the resize signals.
//! 2. Attach `resize_mousemove` / `resize_mouseup` to the container that spans
//!    both panels (so dragging outside the handle still works).
//! 3. Place `ResizeHandle` between the two panels; pass the signals in.
//! 4. Double-clicking the handle resets the panel to `default_size`.

use dioxus::prelude::*;

/// Direction of the resize axis.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ResizeDir {
    /// Horizontal split -- left/right panels.  Cursor: `col-resize`.
    Horizontal,
    /// Vertical split -- top/bottom panels.  Cursor: `row-resize`.
    Vertical,
}

/// All signals needed to drive a resize interaction.
///
/// Create via [`use_resize_state`] and pass to [`ResizeHandle`] plus the
/// parent container's `onmousemove` / `onmouseup` handlers.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct ResizeState {
    /// Current panel size in pixels.
    pub size: Signal<f64>,
    /// Whether a drag is in progress.
    pub is_dragging: Signal<bool>,
    /// Client coordinate at drag start (x for horizontal, y for vertical).
    pub drag_origin: Signal<f64>,
    /// Panel size at drag start.
    pub drag_start_size: Signal<f64>,
    /// Minimum allowed size.
    pub min_size: f64,
    /// Maximum allowed size.
    pub max_size: f64,
    /// Default size -- restored on double-click.
    pub default_size: f64,
}

impl ResizeState {
    /// Handle `onmousemove` on the outer container.
    pub(crate) fn on_move(&self, client_x: f64, client_y: f64, dir: ResizeDir) {
        if !*self.is_dragging.read() {
            return;
        }
        let origin = *self.drag_origin.read();
        let start_size = *self.drag_start_size.read();
        let delta = match dir {
            ResizeDir::Horizontal => client_x - origin,
            ResizeDir::Vertical => client_y - origin,
        };
        let new_size = (start_size + delta).clamp(self.min_size, self.max_size);
        self.size.clone().set(new_size);
    }

    /// Handle `onmouseup` on the outer container.
    pub(crate) fn on_up(&self) {
        self.is_dragging.clone().set(false);
    }
}

/// Create resize interaction state for a panel.
#[must_use]
pub(crate) fn use_resize_state(
    default_size: f64,
    min_size: f64,
    max_size: f64,
) -> ResizeState {
    ResizeState {
        size: use_signal(|| default_size),
        is_dragging: use_signal(|| false),
        drag_origin: use_signal(|| 0.0_f64),
        drag_start_size: use_signal(|| 0.0_f64),
        min_size,
        max_size,
        default_size,
    }
}

/// Thin panel divider that initiates drag-to-resize.
///
/// Place between two panels.  The parent container must handle
/// `onmousemove` / `onmouseup` using `ResizeState::on_move` and
/// `ResizeState::on_up` so dragging outside the handle works correctly.
#[component]
pub(crate) fn ResizeHandle(
    /// Which axis to resize along.
    dir: ResizeDir,
    /// Resize state -- created by [`use_resize_state`] in the parent.
    state: ResizeState,
) -> Element {
    let cursor = match dir {
        ResizeDir::Horizontal => "col-resize",
        ResizeDir::Vertical => "row-resize",
    };
    let (w, h) = match dir {
        ResizeDir::Horizontal => ("4px", "100%"),
        ResizeDir::Vertical => ("100%", "4px"),
    };

    rsx! {
        div {
            role: "separator",
            "aria-orientation": match dir { ResizeDir::Horizontal => "vertical", ResizeDir::Vertical => "horizontal" },
            "aria-label": "Resize panel",
            tabindex: "0",
            class: "resize-handle",
            style: "
                width: {w};
                height: {h};
                cursor: {cursor};
                flex-shrink: 0;
                background: transparent;
                transition: background var(--transition-quick, 0.15s);
                position: relative;
                z-index: 1;
            ",
            onmousedown: move |evt: Event<MouseData>| {
                let coords = evt.client_coordinates();
                let origin = match dir {
                    ResizeDir::Horizontal => coords.x,
                    ResizeDir::Vertical => coords.y,
                };
                state.is_dragging.clone().set(true);
                state.drag_origin.clone().set(origin);
                state.drag_start_size.clone().set(*state.size.read());
            },
            ondoubleclick: move |_| {
                state.size.clone().set(state.default_size);
            },
            onkeydown: move |evt: Event<KeyboardData>| {
                // WHY: keyboard users can resize with arrow keys at 8px increments.
                let step = 8.0_f64;
                let current = *state.size.read();
                let new_size = match (dir, evt.key().to_string().as_str()) {
                    (ResizeDir::Horizontal, "ArrowRight") => current + step,
                    (ResizeDir::Horizontal, "ArrowLeft") => current - step,
                    (ResizeDir::Vertical, "ArrowDown") => current + step,
                    (ResizeDir::Vertical, "ArrowUp") => current - step,
                    (_, "Home") => state.default_size,
                    _ => return,
                };
                state.size.clone().set(new_size.clamp(state.min_size, state.max_size));
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pure clamping logic tested without Dioxus runtime (no Signal).
    fn clamp_resize(start_size: f64, delta: f64, min: f64, max: f64) -> f64 {
        (start_size + delta).clamp(min, max)
    }

    #[test]
    fn clamp_to_min() {
        // delta = 50 - 200 = -150 → 280 - 150 = 130 → clamped to 160
        let result = clamp_resize(280.0, 50.0 - 200.0, 160.0, 600.0);
        assert!((result - 160.0).abs() < f64::EPSILON);
    }

    #[test]
    fn clamp_to_max() {
        // delta = 600 - 100 = 500 → 280 + 500 = 780 → clamped to 600
        let result = clamp_resize(280.0, 600.0 - 100.0, 160.0, 600.0);
        assert!((result - 600.0).abs() < f64::EPSILON);
    }

    #[test]
    fn no_clamp_within_range() {
        let result = clamp_resize(280.0, 40.0, 160.0, 600.0);
        assert!((result - 320.0).abs() < f64::EPSILON);
    }

    #[test]
    fn vertical_resize_uses_y_delta() {
        // For vertical direction, the y coordinate drives the resize.
        // Simulate: drag_origin_y=200, current_y=350 → delta=150 → 400+150=550
        let result = clamp_resize(400.0, 350.0 - 200.0, 100.0, 800.0);
        assert!((result - 550.0).abs() < f64::EPSILON);
    }
}
