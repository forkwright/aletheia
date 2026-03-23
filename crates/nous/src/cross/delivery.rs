//! Delivery audit log for cross-nous messages.

use std::collections::VecDeque;

use ulid::Ulid;

use super::DeliveryState;

#[expect(dead_code, reason = "audit fields for future cross-nous diagnostic tooling")]
/// A single delivery audit record.
#[derive(Debug, Clone)]
pub(crate) struct DeliveryEntry {
    /// ID of the delivered message.
    pub message_id: Ulid,
    /// Sender nous ID.
    pub from: String,
    /// Target nous ID.
    pub to: String,
    /// Delivery outcome at the time of recording.
    pub state: DeliveryState,
    /// When this delivery event was recorded.
    pub timestamp: jiff::Timestamp,
}

/// Ring-buffer delivery audit log.
pub(crate) struct DeliveryLog {
    pub(super) entries: VecDeque<DeliveryEntry>,
    max_entries: usize,
}

impl DeliveryLog {
    /// Create a delivery log with the given maximum capacity.
    #[must_use]
    pub(crate) fn new(max_entries: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(max_entries.min(1024)),
            max_entries,
        }
    }

    /// Append an entry, evicting the oldest if at capacity.
    pub(crate) fn record(&mut self, entry: DeliveryEntry) {
        if self.entries.len() >= self.max_entries {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    #[cfg_attr(not(test), expect(dead_code, reason = "query API for future cross-nous diagnostic tooling"))]
    /// Most recent entries, newest first, up to `limit`.
    #[must_use]
    pub(crate) fn recent(&self, limit: usize) -> Vec<&DeliveryEntry> {
        self.entries.iter().rev().take(limit).collect()
    }

    #[cfg_attr(not(test), expect(dead_code, reason = "query API for future cross-nous diagnostic tooling"))]
    /// Recent entries involving the given nous (as sender or receiver), newest first.
    #[must_use]
    pub(crate) fn for_nous(&self, nous_id: &str, limit: usize) -> Vec<&DeliveryEntry> {
        self.entries
            .iter()
            .rev()
            .filter(|e| e.from == nous_id || e.to == nous_id)
            .take(limit)
            .collect()
    }
}
