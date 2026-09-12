//! Outbound message queue for the chat composer.
//!
//! WHY(#7299): the composer previously disabled submission outright while a
//! turn was streaming (`InputBar`'s textarea and send button both gated on
//! `is_streaming`), so anything typed at the send boundary was simply
//! discarded. Queuing lets an operator keep composing follow-ups without
//! waiting for the current turn to end; `views/chat.rs` dispatches the
//! front of the queue once the turn's terminal event lands.

use std::collections::VecDeque;

/// FIFO queue of messages submitted while a turn is in flight.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ComposerQueue(VecDeque<String>);

impl ComposerQueue {
    /// Queue a message behind the in-flight turn.
    pub(crate) fn push(&mut self, text: String) {
        self.0.push_back(text);
    }

    /// Take the next queued message, in submission order.
    pub(crate) fn pop_front(&mut self) -> Option<String> {
        self.0.pop_front()
    }

    /// Whether any messages are waiting to be dispatched.
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Number of messages waiting to be dispatched.
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    /// Queued messages in dispatch order, for rendering the visible queue.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &String> {
        self.0.iter()
    }

    /// Remove every queued entry with this exact text.
    ///
    /// WHY: the composer's queue indicator lets an operator cancel a
    /// queued follow-up before it dispatches; content-based removal is
    /// enough since a queued entry has no index-stable handle of its own.
    pub(crate) fn remove(&mut self, text: &str) {
        self.0.retain(|queued| queued != text);
    }
}

/// Decide whether a submitted message should queue behind an in-flight turn
/// or dispatch immediately.
///
/// Returns `true` when the message was queued (the caller must not dispatch
/// it now); `false` when the message was left untouched and the caller
/// should dispatch it immediately.
pub(crate) fn enqueue_if_streaming(queue: &mut ComposerQueue, is_streaming: bool, text: String) -> bool {
    if is_streaming {
        queue.push(text);
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_submitted_while_streaming_is_queued_not_dispatched() {
        let mut queue = ComposerQueue::default();

        let queued = enqueue_if_streaming(&mut queue, true, "follow-up".to_string());

        assert!(queued, "a mid-turn submission must queue, not dispatch");
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.iter().collect::<Vec<_>>(), vec!["follow-up"]);
    }

    #[test]
    fn message_submitted_while_idle_is_left_for_immediate_dispatch() {
        let mut queue = ComposerQueue::default();

        let queued = enqueue_if_streaming(&mut queue, false, "go now".to_string());

        assert!(!queued, "an idle-turn submission must dispatch immediately");
        assert!(queue.is_empty());
    }

    #[test]
    fn remove_drops_the_matching_queued_entry() {
        let mut queue = ComposerQueue::default();
        queue.push("keep".to_string());
        queue.push("drop me".to_string());

        queue.remove("drop me");

        assert_eq!(queue.len(), 1);
        assert_eq!(queue.iter().collect::<Vec<_>>(), vec!["keep"]);
    }

    #[test]
    fn queue_drains_front_to_back_in_submission_order() {
        let mut queue = ComposerQueue::default();
        queue.push("first".to_string());
        queue.push("second".to_string());

        assert_eq!(queue.pop_front(), Some("first".to_string()));
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.pop_front(), Some("second".to_string()));
        assert!(queue.is_empty());
        assert_eq!(queue.pop_front(), None);
    }
}
