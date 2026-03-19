use std::ops::Range;

/// Virtual scroll state for O(viewport) rendering of message lists.
///
/// Pre-computes item heights and maintains prefix sums so that visible range
/// determination is O(log n) via binary search. Only viewport + buffer items
/// are rendered per frame instead of the full message list.
#[derive(Debug, Clone)]
pub(crate) struct VirtualScroll {
    /// Estimated height (terminal rows) per item.
    item_heights: Vec<u16>,
    /// Prefix sums: `prefix_sums[i]` = sum of `item_heights[0..i]`.
    /// Length = `item_heights.len() + 1`, with `prefix_sums[0] = 0`.
    prefix_sums: Vec<u64>,
    /// Terminal inner width used to compute the cached heights.
    cached_width: u16,
    /// Number of extra items to render above/below the viewport.
    buffer: usize,
}

/// Result of computing which items are visible in the viewport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ViewportSlice {
    /// Range of item indices to render (includes buffer zone).
    pub(crate) range: Range<usize>,
    /// Line offset within the first rendered item (for smooth scrolling).
    pub(crate) line_offset: u16,
}

const DEFAULT_BUFFER: usize = 20;

impl Default for VirtualScroll {
    fn default() -> Self {
        Self {
            item_heights: Vec::new(),
            prefix_sums: vec![0],
            cached_width: 0,
            buffer: DEFAULT_BUFFER,
        }
    }
}

impl VirtualScroll {
    /// Create a new virtual scroll with the given buffer zone.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Total number of tracked items.
    pub(crate) fn len(&self) -> usize {
        self.item_heights.len()
    }

    /// Total content height in terminal rows.
    pub(crate) fn total_height(&self) -> u64 {
        self.prefix_sums.last().copied().unwrap_or(0)
    }

    /// Returns the cached width these heights were computed at.
    pub(crate) fn cached_width(&self) -> u16 {
        self.cached_width
    }

    /// Append a single item height (amortized O(1)).
    /// Called when a new message arrives.
    pub(crate) fn push_item(&mut self, height: u16) {
        let prev_sum = self.prefix_sums.last().copied().unwrap_or(0);
        self.item_heights.push(height);
        self.prefix_sums.push(prev_sum + u64::from(height));
    }

    /// Clear all cached heights (e.g. on session switch).
    pub(crate) fn clear(&mut self) {
        self.item_heights.clear();
        self.prefix_sums.clear();
        self.prefix_sums.push(0);
        self.cached_width = 0;
    }

    /// Rebuild all heights from a slice of pre-computed heights.
    /// Called on session load, terminal resize, or filter change.
    pub(crate) fn rebuild(&mut self, heights: &[u16], width: u16) {
        self.item_heights.clear();
        self.prefix_sums.clear();
        self.prefix_sums.push(0);
        self.cached_width = width;

        let mut running = 0u64;
        for &h in heights {
            self.item_heights.push(h);
            running += u64::from(h);
            self.prefix_sums.push(running);
        }
    }

    /// Compute the visible range of items given the current scroll state.
    ///
    /// `scroll_offset` is lines-from-bottom (0 = at bottom).
    /// `auto_scroll` overrides to pin to the bottom.
    /// `viewport_height` is the visible area in terminal rows.
    #[expect(
        clippy::indexing_slicing,
        reason = "item_at_line guarantees returned indices are within prefix_sums bounds (n+1 entries for n items)"
    )]
    pub(crate) fn visible_slice(
        &self,
        scroll_offset: usize,
        auto_scroll: bool,
        viewport_height: u16,
    ) -> ViewportSlice {
        let total = self.total_height();
        let vh = u64::from(viewport_height);

        if total == 0 || self.item_heights.is_empty() {
            return ViewportSlice {
                range: 0..0,
                line_offset: 0,
            };
        }

        let top_line = if auto_scroll {
            total.saturating_sub(vh)
        } else {
            total
                .saturating_sub(vh)
                .saturating_sub(scroll_offset as u64)
        };

        let bottom_line = top_line + vh;

        // NOTE: item_at_line binary-searches prefix_sums to find the item containing top_line.
        let first_item = self.item_at_line(top_line);
        let last_item = self.item_at_line(bottom_line.min(total));

        let first_item_start = self.prefix_sums[first_item];
        let line_offset =
            u16::try_from(top_line.saturating_sub(first_item_start)).unwrap_or(u16::MAX);

        let start = first_item.saturating_sub(self.buffer);
        let end = (last_item + 1 + self.buffer).min(self.item_heights.len());

        ViewportSlice {
            range: start..end,
            line_offset: if start < first_item {
                // NOTE: Buffer items above are rendered; add their height so the
                // line_offset is relative to the start of the rendered range, not first_item.
                let buffer_height: u64 = self.prefix_sums[first_item] - self.prefix_sums[start];
                u16::try_from(buffer_height)
                    .unwrap_or(u16::MAX)
                    .saturating_add(line_offset)
            } else {
                line_offset
            },
        }
    }

    /// Find the item index that contains the given absolute line position.
    /// Uses binary search on prefix sums: O(log n).
    fn item_at_line(&self, line: u64) -> usize {
        if line == 0 {
            return 0;
        }
        // NOTE: binary search: find first i where prefix_sums[i+1] > line,
        // i.e. the item whose cumulative range includes `line`.
        let n = self.item_heights.len();
        if n == 0 {
            return 0;
        }

        // NOTE: partition_point returns the first index where prefix_sums[idx] > line,
        // so the item containing `line` is idx - 1 (prefix_sums[idx-1] <= line < prefix_sums[idx]).
        let idx = self.prefix_sums.partition_point(|&s| s <= line);
        idx.saturating_sub(1).min(n.saturating_sub(1))
    }

    /// Scrollbar position as (offset_ratio, size_ratio) in [0.0, 1.0].
    /// Returns `None` if content fits in viewport.
    pub(crate) fn scrollbar_position(
        &self,
        scroll_offset: usize,
        auto_scroll: bool,
        viewport_height: u16,
    ) -> Option<(f64, f64)> {
        let total = self.total_height();
        let vh = u64::from(viewport_height);

        if total <= vh {
            return None;
        }

        let top_line = if auto_scroll {
            total.saturating_sub(vh)
        } else {
            total
                .saturating_sub(vh)
                .saturating_sub(scroll_offset as u64)
        };

        let size_ratio = vh as f64 / total as f64;
        let offset_ratio = top_line as f64 / total as f64;

        Some((offset_ratio, size_ratio))
    }
}

/// Estimate the height of a message in terminal rows.
///
/// This is a cheap approximation used to avoid full markdown rendering
/// for off-screen messages. It accounts for:
/// - 1 header line
/// - text content wrapped at `width`
/// - 1 trailing blank line
///
/// Tool calls are rendered exclusively in the ops pane and do not add
/// lines to the chat view, so they are not counted here.
pub(crate) fn estimate_message_height(text_len: usize, width: u16) -> u16 {
    let w = usize::from(width.max(1));
    let header = 1u16;
    let content = if text_len == 0 {
        0u16
    } else {
        u16::try_from(text_len / w + 1).unwrap_or(u16::MAX)
    };
    let blank = 1u16;

    header + content + blank
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    #[test]
    fn empty_scroll_returns_empty_range() {
        let vs = VirtualScroll::new();
        let slice = vs.visible_slice(0, true, 40);
        assert_eq!(slice.range, 0..0);
        assert_eq!(slice.line_offset, 0);
    }

    #[test]
    fn single_item_fits_in_viewport() {
        let mut vs = VirtualScroll::new();
        vs.push_item(5);
        let slice = vs.visible_slice(0, true, 40);
        assert_eq!(slice.range, 0..1);
        assert_eq!(slice.line_offset, 0);
    }

    #[test]
    fn total_height_tracks_items() {
        let mut vs = VirtualScroll::new();
        vs.push_item(3);
        vs.push_item(5);
        vs.push_item(2);
        assert_eq!(vs.total_height(), 10);
        assert_eq!(vs.len(), 3);
    }

    #[test]
    fn visible_range_at_bottom_auto_scroll() {
        let mut vs = VirtualScroll::new();
        // 100 items, each 3 lines tall = 300 total lines
        for _ in 0..100 {
            vs.push_item(3);
        }
        let slice = vs.visible_slice(0, true, 30);
        // Viewport shows 30 lines = 10 items at bottom.
        // With buffer=20, range extends further up.
        assert!(slice.range.start <= 70);
        assert_eq!(slice.range.end, 100);
    }

    #[test]
    fn visible_range_scrolled_up() {
        let mut vs = VirtualScroll::new();
        // 100 items, each 3 lines tall = 300 total lines
        for _ in 0..100 {
            vs.push_item(3);
        }
        // Scroll up 30 lines (10 items) from bottom.
        // Viewport = 30 lines. Total = 300.
        // top_line = 300 - 30 - 30 = 240 = item 80
        let slice = vs.visible_slice(30, false, 30);
        // Items 80..90 visible, with buffer extending both ways.
        assert!(slice.range.start <= 70);
        assert!(slice.range.end >= 90);
        assert!(slice.range.end <= 100);
    }

    #[test]
    fn visible_range_scrolled_to_top() {
        let mut vs = VirtualScroll::new();
        for _ in 0..100 {
            vs.push_item(3);
        }
        // Scroll all the way up: offset = 300 - 30 = 270
        let slice = vs.visible_slice(270, false, 30);
        assert_eq!(slice.range.start, 0);
    }

    #[test]
    fn variable_height_items() {
        let mut vs = VirtualScroll::new();
        vs.push_item(1); // item 0: lines 0..1
        vs.push_item(5); // item 1: lines 1..6
        vs.push_item(2); // item 2: lines 6..8
        vs.push_item(3); // item 3: lines 8..11
        vs.push_item(1); // item 4: lines 11..12

        // Viewport of 4 lines at bottom, auto_scroll
        // total=12, top_line=8 (item 3), bottom_line=12 (item 4)
        let vs_no_buf = {
            let mut v = VirtualScroll {
                buffer: 0,
                ..VirtualScroll::default()
            };
            v.push_item(1);
            v.push_item(5);
            v.push_item(2);
            v.push_item(3);
            v.push_item(1);
            v
        };
        let slice = vs_no_buf.visible_slice(0, true, 4);
        assert_eq!(slice.range, 3..5);
    }

    #[test]
    fn line_offset_within_first_item() {
        let mut vs = VirtualScroll {
            buffer: 0,
            ..VirtualScroll::default()
        };
        vs.push_item(10); // item 0: lines 0..10
        vs.push_item(10); // item 1: lines 10..20

        // Viewport 5 lines, scroll_offset=0, auto_scroll
        // total=20, top_line=15 (in middle of item 1)
        let slice = vs.visible_slice(0, true, 5);
        assert_eq!(slice.range, 1..2);
        // top_line=15, item 1 starts at 10, offset=5
        assert_eq!(slice.line_offset, 5);
    }

    #[test]
    fn push_item_amortized() {
        let mut vs = VirtualScroll::new();
        for i in 0..1000 {
            vs.push_item((i % 5 + 1) as u16);
        }
        assert_eq!(vs.len(), 1000);
        assert_eq!(
            vs.total_height(),
            (0..1000u64).map(|i| i % 5 + 1).sum::<u64>()
        );
    }

    #[test]
    fn clear_resets_state() {
        let mut vs = VirtualScroll::new();
        vs.push_item(5);
        vs.push_item(3);
        vs.clear();
        assert_eq!(vs.len(), 0);
        assert_eq!(vs.total_height(), 0);
        assert_eq!(vs.prefix_sums.len(), 1);
    }

    #[test]
    fn rebuild_replaces_all_heights() {
        let mut vs = VirtualScroll::new();
        vs.push_item(5);
        vs.push_item(3);

        vs.rebuild(&[2, 4, 6], 80);
        assert_eq!(vs.len(), 3);
        assert_eq!(vs.total_height(), 12);
        assert_eq!(vs.cached_width(), 80);
    }

    #[test]
    fn scrollbar_none_when_content_fits() {
        let mut vs = VirtualScroll::new();
        vs.push_item(5);
        assert!(vs.scrollbar_position(0, true, 40).is_none());
    }

    #[test]
    fn scrollbar_at_bottom() {
        let mut vs = VirtualScroll::new();
        for _ in 0..100 {
            vs.push_item(3);
        }
        let (offset, size) = vs.scrollbar_position(0, true, 30).unwrap();
        // At bottom: offset should be near 1.0 - size
        assert!(offset > 0.8, "offset={offset}");
        assert!(size > 0.0 && size < 1.0, "size={size}");
    }

    #[test]
    fn scrollbar_at_top() {
        let mut vs = VirtualScroll::new();
        for _ in 0..100 {
            vs.push_item(3);
        }
        // Scroll all the way up
        let (offset, _size) = vs.scrollbar_position(270, false, 30).unwrap();
        assert!(offset < 0.01, "offset={offset}");
    }

    #[test]
    fn estimate_message_height_empty() {
        assert_eq!(estimate_message_height(0, 80), 2); // header + blank
    }

    #[test]
    fn estimate_message_height_short() {
        // "hello" (5 chars) at width 80 = 1 content line
        assert_eq!(estimate_message_height(5, 80), 3); // header + 1 content + blank
    }

    #[test]
    fn estimate_message_height_wrapping() {
        // 200 chars at width 80 = 3 content lines
        assert_eq!(estimate_message_height(200, 80), 5); // header + 3 content + blank
    }

    #[test]
    fn item_at_line_binary_search() {
        let mut vs = VirtualScroll::new();
        vs.push_item(3); // 0..3
        vs.push_item(5); // 3..8
        vs.push_item(2); // 8..10

        assert_eq!(vs.item_at_line(0), 0);
        assert_eq!(vs.item_at_line(2), 0);
        assert_eq!(vs.item_at_line(3), 1);
        assert_eq!(vs.item_at_line(7), 1);
        assert_eq!(vs.item_at_line(8), 2);
        assert_eq!(vs.item_at_line(9), 2);
    }

    #[test]
    fn visible_slice_with_buffer_zone() {
        let mut vs = VirtualScroll::new();
        // 50 items of height 2 = 100 total lines
        for _ in 0..50 {
            vs.push_item(2);
        }
        // Viewport 10 lines at bottom, auto_scroll
        // Without buffer: items 45..50
        // With default buffer (20): items 25..50
        let slice = vs.visible_slice(0, true, 10);
        assert!(slice.range.start <= 25);
        assert_eq!(slice.range.end, 50);
    }

    #[test]
    fn benchmark_visible_slice_15k_items() {
        let mut vs = VirtualScroll::new();
        // Simulate 15K messages with variable heights
        for i in 0..15_000 {
            vs.push_item((i % 7 + 2) as u16); // heights 2..8
        }

        let start = std::time::Instant::now();
        let iterations = 10_000;
        for offset in 0..iterations {
            let _ = vs.visible_slice(offset % 1000, false, 40);
        }
        let elapsed = start.elapsed();
        let per_call = elapsed / iterations as u32;

        // Must be sub-microsecond per call (binary search on 15K items)
        assert!(
            per_call.as_micros() < 10,
            "visible_slice too slow: {per_call:?} per call"
        );
    }

    #[test]
    fn benchmark_push_item_amortized() {
        let mut vs = VirtualScroll::new();
        let start = std::time::Instant::now();
        for i in 0..15_000 {
            vs.push_item((i % 5 + 1) as u16);
        }
        let elapsed = start.elapsed();

        // 15K pushes should complete in well under 10ms
        assert!(
            elapsed.as_millis() < 10,
            "push_item too slow for 15K items: {elapsed:?}"
        );
    }
}
