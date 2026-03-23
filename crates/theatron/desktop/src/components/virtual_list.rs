//! Virtual scrolling utilities for large lists.
//!
//! Provides a container component and a `visible_range` utility used by chat,
//! sessions, and memory views to render only the items inside the viewport
//! plus a configurable overscan buffer.

use dioxus::prelude::*;

/// Default overscan — extra items rendered above and below the visible window.
pub(crate) const DEFAULT_OVERSCAN: usize = 10;

/// Compute which item indices are visible given scroll position and item height.
///
/// Returns `(range_start, range_end)` — a half-open slice into the item list.
/// Both ends are clamped to `[0, total_items]`.
#[must_use]
pub(crate) fn visible_range(
    scroll_top: f64,
    container_height: f64,
    total_items: usize,
    item_height: f64,
    overscan: usize,
) -> (usize, usize) {
    if total_items == 0 || item_height <= 0.0 {
        return (0, 0);
    }
    let first = ((scroll_top / item_height) as usize).min(total_items);
    let count = ((container_height / item_height).ceil() as usize + 1).min(total_items);
    let start = first.saturating_sub(overscan);
    let end = (first + count + overscan).min(total_items);
    (start, end)
}

/// Compute spacer heights for virtual scroll from a visible range.
///
/// Returns `(pad_top_px, pad_bottom_px)`.
#[must_use]
pub(crate) fn spacer_heights(
    range_start: usize,
    range_end: usize,
    total_items: usize,
    item_height: f64,
) -> (f64, f64) {
    let pad_top = range_start as f64 * item_height;
    let pad_bottom = total_items.saturating_sub(range_end) as f64 * item_height;
    (pad_top, pad_bottom)
}

/// A scrollable container that manages virtual scroll state.
///
/// Renders a vertically scrollable div with spacer divs above and below
/// the visible items. The parent is responsible for:
/// - Computing visible range with [`visible_range`]
/// - Rendering only the visible items as `children`
/// - Passing the correct `pad_top` / `pad_bottom` values
///
/// The `on_scroll` callback receives `(scroll_top, container_height)` whenever
/// the user scrolls. Use these values to recompute the visible range.
#[component]
pub(crate) fn VirtualScrollContainer(
    pad_top: f64,
    pad_bottom: f64,
    on_scroll: EventHandler<(f64, f64)>,
    /// Optional `data-*` attribute used to target this element in eval.
    scroll_key: &'static str,
    children: Element,
) -> Element {
    let key = scroll_key;
    rsx! {
        div {
            style: "flex: 1; overflow-y: auto; position: relative;",
            role: "list",
            "data-vscroll": key,
            onscroll: move |_| {
                let js = format!(
                    r#"(function(){{
                        var el = document.querySelector('[data-vscroll="{key}"]');
                        if (el) return JSON.stringify({{top: el.scrollTop, h: el.clientHeight}});
                        return '{{}}'
                    }})()"#
                );
                spawn(async move {
                    if let Ok(val) = document::eval(&js).await {
                        let raw = val.to_string();
                        let cleaned = raw.trim_matches('"').replace("\\\"", "\"");
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&cleaned) {
                            let top = parsed.get("top").and_then(|v| v.as_f64()).unwrap_or(0.0);
                            let h = parsed.get("h").and_then(|v| v.as_f64()).unwrap_or(0.0);
                            if h > 0.0 {
                                on_scroll.call((top, h));
                            }
                        }
                    }
                });
            },

            // Top spacer — represents off-screen items above viewport
            div {
                style: "height: {pad_top}px; flex-shrink: 0;",
                "aria-hidden": "true",
            }

            {children}

            // Bottom spacer — represents off-screen items below viewport
            div {
                style: "height: {pad_bottom}px; flex-shrink: 0;",
                "aria-hidden": "true",
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_range_empty_list() {
        let (s, e) = visible_range(0.0, 600.0, 0, 80.0, 10);
        assert_eq!((s, e), (0, 0));
    }

    #[test]
    fn visible_range_at_top_no_scroll() {
        // Container 600px, item 80px → ceil(600/80)+1 = 9 visible; +10 overscan each side
        let (s, e) = visible_range(0.0, 600.0, 100, 80.0, 10);
        assert_eq!(s, 0); // clamped at 0
        // first=0, count=9, end=0+9+10=19
        assert_eq!(e, 19);
    }

    #[test]
    fn visible_range_scrolled_mid_list() {
        // scroll=800px → first=10, count=9, start=0, end=29
        let (s, e) = visible_range(800.0, 600.0, 100, 80.0, 10);
        assert_eq!(s, 0); // 10 - 10 = 0
        assert_eq!(e, 29); // 10 + 9 + 10 = 29
    }

    #[test]
    fn visible_range_clamped_at_end() {
        let (s, e) = visible_range(5000.0, 600.0, 20, 80.0, 10);
        assert_eq!(e, 20); // clamped to total
        assert!(s <= e);
    }

    #[test]
    fn visible_range_overscan_only_on_renderable_items() {
        // Only 5 items total
        let (s, e) = visible_range(0.0, 600.0, 5, 80.0, 10);
        assert_eq!(s, 0);
        assert_eq!(e, 5);
    }

    #[test]
    fn spacer_heights_basic() {
        let (top, bottom) = spacer_heights(5, 15, 30, 80.0);
        assert!((top - 400.0).abs() < f64::EPSILON);
        assert!((bottom - 1200.0).abs() < f64::EPSILON);
    }

    #[test]
    fn spacer_heights_at_boundaries() {
        let (top, bottom) = spacer_heights(0, 10, 10, 80.0);
        assert!((top - 0.0).abs() < f64::EPSILON);
        assert!((bottom - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn visible_range_zero_height_item() {
        let (s, e) = visible_range(0.0, 600.0, 100, 0.0, 10);
        assert_eq!((s, e), (0, 0));
    }
}
