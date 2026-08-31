//! Bounded inbound-message dedupe filter.
//!
//! Providers can redeliver a message after restarts, retries, or cursor
//! resets — that is how at-least-once transports recover, not an error
//! path. The dispatcher records each message's dedupe key here and drops
//! repeat sightings before any agent work is scheduled.

use std::collections::{HashSet, VecDeque};

use crate::types::InboundMessage;

/// Default dedupe window size, in remembered messages.
///
/// WHY: deliberately independent of `MessagingConfig::buffer_capacity`,
/// which sizes only Signal's disconnected outbound retry buffer and would
/// be the wrong knob for an inbound replay window.
pub const DEFAULT_DEDUPE_CAPACITY: usize = 4096;

/// A bounded remember-set of recently seen inbound messages.
///
/// Oldest keys are evicted past `capacity`, bounding memory while covering
/// the realistic replay window (provider redelivery happens within seconds
/// to a restart boundary, not days later).
#[derive(Debug)]
pub struct DedupeFilter {
    capacity: usize,
    order: VecDeque<String>,
    seen: HashSet<String>,
}

impl DedupeFilter {
    /// Create a filter holding at most `capacity` keys (clamped to ≥ 1).
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            order: VecDeque::new(),
            seen: HashSet::new(),
        }
    }

    /// Record a sighting of `msg`.
    ///
    /// Returns `true` when the message is new and should be dispatched.
    /// A repeat sighting is counted in the `aletheia_ingress_duplicates`
    /// metric and returns `false`.
    pub fn check_and_record(&mut self, msg: &InboundMessage) -> bool {
        let key = msg.dedupe_key();
        if !self.seen.insert(key.clone()) {
            crate::metrics::record_ingress_duplicate(&msg.channel);
            return false;
        }
        self.order.push_back(key);
        while self.order.len() > self.capacity {
            if let Some(evicted) = self.order.pop_front() {
                self.seen.remove(&evicted);
            }
        }
        true
    }

    /// Number of keys currently remembered.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.seen.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(text: &str, timestamp: u64) -> InboundMessage {
        InboundMessage {
            channel: "signal".to_owned(),
            sender: "+15550100".to_owned(),
            sender_name: None,
            group_id: None,
            message_id: None,
            text: text.to_owned(),
            timestamp,
            attachments: vec![],
            raw: None,
        }
    }

    #[test]
    fn first_sighting_is_new_repeat_is_duplicate() {
        let mut filter = DedupeFilter::with_capacity(8);
        assert!(filter.check_and_record(&msg("hello", 1)));
        assert!(!filter.check_and_record(&msg("hello", 1)));
        assert!(filter.check_and_record(&msg("hello", 2)));
    }

    #[test]
    fn capacity_bounds_memory_and_evicts_oldest() {
        let mut filter = DedupeFilter::with_capacity(2);
        assert!(filter.check_and_record(&msg("a", 1)));
        assert!(filter.check_and_record(&msg("b", 2)));
        assert!(filter.check_and_record(&msg("c", 3)));
        assert_eq!(filter.len(), 2);
        // WHY: "a" was evicted, so a late redelivery is (correctly, under a
        // bounded window) treated as new rather than pinned forever. Under a
        // capacity of 2 this re-insertion itself evicts the oldest surviving
        // key ("b"), so only "c" — not "b" — is still in the window.
        assert!(filter.check_and_record(&msg("a", 1)));
        assert!(!filter.check_and_record(&msg("c", 3)));
        assert!(filter.check_and_record(&msg("b", 2)));
    }

    #[test]
    fn zero_capacity_is_clamped_to_one() {
        let mut filter = DedupeFilter::with_capacity(0);
        assert!(filter.check_and_record(&msg("a", 1)));
        assert!(!filter.check_and_record(&msg("a", 1)));
        assert!(filter.check_and_record(&msg("b", 2)));
        assert!(filter.check_and_record(&msg("a", 1)));
    }

    #[test]
    fn provider_message_id_dedupes_across_content() {
        let mut with_id = msg("hello", 1);
        with_id.channel = "matrix".to_owned();
        with_id.message_id = Some("$event1".to_owned());
        let mut filter = DedupeFilter::with_capacity(8);
        assert!(filter.check_and_record(&with_id));
        // WHY: same provider identity, different rendering — a redelivery
        // may carry an edited body or refreshed timestamp.
        let mut redelivered = msg("hello (edited)", 2);
        redelivered.channel = "matrix".to_owned();
        redelivered.message_id = Some("$event1".to_owned());
        assert!(!filter.check_and_record(&redelivered));
    }
}
