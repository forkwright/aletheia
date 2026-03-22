//! KPI summary and category statistics for the ops pane.

use super::types::ToolCategory;

/// Per-category success/fail tallies and duration samples for percentile computation.
#[derive(Debug, Clone, Default)]
pub(crate) struct CategoryStats {
    pub(crate) success: u32,
    pub(crate) fail: u32,
    /// Sorted durations in milliseconds for percentile computation.
    durations: Vec<u64>,
}

impl CategoryStats {
    /// Record a completed tool call.
    pub(crate) fn record(&mut self, is_error: bool, duration_ms: u64) {
        if is_error {
            self.fail += 1;
        } else {
            self.success += 1;
        }
        // Insert in sorted order for percentile lookups.
        let pos = self.durations.partition_point(|&d| d < duration_ms);
        self.durations.insert(pos, duration_ms);
    }

    /// Total calls (success + fail).
    #[cfg(test)]
    pub(crate) fn total(&self) -> u32 {
        self.success + self.fail
    }

    /// Compute a percentile (0-100) from the sorted durations.
    /// Returns `None` if no durations have been recorded.
    pub(crate) fn percentile(&self, p: u8) -> Option<u64> {
        if self.durations.is_empty() {
            return None;
        }
        let idx = (usize::from(p) * self.durations.len() / 100).min(self.durations.len() - 1);
        self.durations.get(idx).copied()
    }
}

/// Summary KPIs for the ops pane header row.
#[derive(Debug, Clone, Default)]
pub(crate) struct OpsSummary {
    pub(crate) total_calls: u32,
    pub(crate) total_errors: u32,
    /// Per-category statistics.
    pub(crate) categories: std::collections::HashMap<ToolCategory, CategoryStats>,
}

impl OpsSummary {
    /// Record a completed tool call into the summary.
    pub(crate) fn record(&mut self, category: ToolCategory, is_error: bool, duration_ms: u64) {
        self.total_calls += 1;
        if is_error {
            self.total_errors += 1;
        }
        self.categories
            .entry(category)
            .or_default()
            .record(is_error, duration_ms);
    }
}
