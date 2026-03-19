//! Multi-output pipeline stages via the `OutputBuffer` pattern.
//!
//! Inspired by Vector's topology: stages declare multiple named outputs and
//! the `OutputBuffer` routes events to downstream consumers by output name.
//! Supports fan-out (one event → N outputs), conditional routing (one event →
//! one of N outputs), and a dead-letter output for events that fail processing.
//!
//! # Usage
//!
//! ```
//! use aletheia_koina::output_buffer::OutputBuffer;
//!
//! let mut buf: OutputBuffer<String> = OutputBuffer::new();
//! buf.register_output("main");
//! buf.register_output("audit");
//! buf.register_dead_letter();
//!
//! // Fan-out: one event to both outputs
//! buf.fan_out("event-A".to_owned(), &["main", "audit"]);
//!
//! // Conditional routing: one event to one output based on predicate
//! buf.route("event-B".to_owned(), |e| {
//!     if e.contains('B') { "audit" } else { "main" }
//! });
//!
//! assert_eq!(buf.drain("main").len(), 1);
//! assert_eq!(buf.drain("audit").len(), 2);
//! ```

use std::collections::HashMap;

/// Dead-letter output name constant.
pub const DEAD_LETTER: &str = "__dead_letter";

/// A buffer that routes events to multiple named outputs.
///
/// Each output is an independent queue. Events are pushed via
/// [`fan_out`](OutputBuffer::fan_out), [`route`](OutputBuffer::route),
/// or [`push`](OutputBuffer::push). Failed events go to the
/// dead-letter output if registered.
#[derive(Debug, Clone)]
pub struct OutputBuffer<T> {
    /// Named output queues.
    outputs: HashMap<String, Vec<T>>,
}

impl<T: Clone> Default for OutputBuffer<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone> OutputBuffer<T> {
    /// Create an empty output buffer with no registered outputs.
    #[must_use]
    pub fn new() -> Self {
        Self {
            outputs: HashMap::new(),
        }
    }

    /// Register a named output.
    ///
    /// If the output already exists, this is a no-op.
    pub fn register_output(&mut self, name: &str) {
        self.outputs.entry(name.to_owned()).or_default();
    }

    /// Register the dead-letter output for failed events.
    ///
    /// Equivalent to `register_output(DEAD_LETTER)`.
    pub fn register_dead_letter(&mut self) {
        self.register_output(DEAD_LETTER);
    }

    /// Push an event to a single named output.
    ///
    /// If the output does not exist and a dead-letter output is registered,
    /// the event is routed there instead. Returns `false` if the event
    /// was dropped (no matching output and no dead-letter).
    pub fn push(&mut self, event: T, output: &str) -> bool {
        if let Some(queue) = self.outputs.get_mut(output) {
            queue.push(event);
            true
        } else if let Some(dl) = self.outputs.get_mut(DEAD_LETTER) {
            dl.push(event);
            false
        } else {
            false
        }
    }

    /// Fan-out: send clones of one event to multiple named outputs.
    ///
    /// Each target output receives a clone. Unknown output names are
    /// silently skipped. If no targets matched and a dead-letter output
    /// is registered, the event lands there. Returns the number of
    /// outputs that received the event.
    pub fn fan_out(&mut self, event: T, targets: &[&str]) -> usize {
        let mut delivered = 0;
        for &target in targets {
            if let Some(queue) = self.outputs.get_mut(target) {
                queue.push(event.clone());
                delivered += 1;
            }
        }
        if delivered == 0
            && let Some(dl) = self.outputs.get_mut(DEAD_LETTER)
        {
            dl.push(event);
        }
        delivered
    }

    /// Conditional routing: send one event to one of N outputs based on a predicate.
    ///
    /// The routing function receives a reference to the event and returns the
    /// output name. If the output does not exist, the event goes to the
    /// dead-letter output (if registered).
    pub fn route(&mut self, event: T, router: impl FnOnce(&T) -> &str) -> bool {
        let target = router(&event).to_owned();
        self.push(event, &target)
    }

    /// Drain all events from a named output, returning them.
    ///
    /// Returns an empty `Vec` if the output does not exist.
    #[must_use]
    pub fn drain(&mut self, output: &str) -> Vec<T> {
        self.outputs
            .get_mut(output)
            .map(std::mem::take)
            .unwrap_or_default()
    }

    /// Drain the dead-letter output.
    #[must_use]
    pub fn drain_dead_letter(&mut self) -> Vec<T> {
        self.drain(DEAD_LETTER)
    }

    /// Peek at events in a named output without draining.
    #[must_use]
    pub fn peek(&self, output: &str) -> &[T] {
        self.outputs.get(output).map_or(&[], Vec::as_slice)
    }

    /// Number of events in a named output.
    #[must_use]
    pub fn len(&self, output: &str) -> usize {
        self.outputs.get(output).map_or(0, Vec::len)
    }

    /// Whether a named output is empty or does not exist.
    #[must_use]
    pub fn is_empty(&self, output: &str) -> bool {
        self.len(output) == 0
    }

    /// Total events across all outputs.
    #[must_use]
    pub fn total_events(&self) -> usize {
        self.outputs.values().map(Vec::len).sum()
    }

    /// Names of all registered outputs (including dead-letter if registered).
    #[must_use]
    pub fn output_names(&self) -> Vec<&str> {
        self.outputs.keys().map(String::as_str).collect()
    }

    /// Whether a named output is registered.
    #[must_use]
    pub fn has_output(&self, name: &str) -> bool {
        self.outputs.contains_key(name)
    }

    /// Clear all events from all outputs without removing the registrations.
    pub fn clear(&mut self) {
        for queue in self.outputs.values_mut() {
            queue.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_push() {
        let mut buf: OutputBuffer<String> = OutputBuffer::new();
        buf.register_output("main");
        assert!(
            buf.push("hello".to_owned(), "main"),
            "push to existing output succeeds"
        );
        assert_eq!(buf.len("main"), 1);
    }

    #[test]
    fn push_to_unknown_output_drops() {
        let mut buf: OutputBuffer<String> = OutputBuffer::new();
        assert!(
            !buf.push("lost".to_owned(), "nowhere"),
            "push to unknown returns false"
        );
    }

    #[test]
    fn push_to_unknown_routes_to_dead_letter() {
        let mut buf: OutputBuffer<String> = OutputBuffer::new();
        buf.register_dead_letter();
        assert!(
            !buf.push("lost".to_owned(), "nowhere"),
            "push to unknown returns false even with dead-letter"
        );
        assert_eq!(buf.len(DEAD_LETTER), 1, "event landed in dead-letter");
    }

    #[test]
    fn fan_out_delivers_to_all_targets() {
        let mut buf: OutputBuffer<String> = OutputBuffer::new();
        buf.register_output("a");
        buf.register_output("b");
        buf.register_output("c");
        let count = buf.fan_out("event".to_owned(), &["a", "b", "c"]);
        assert_eq!(count, 3, "delivered to all three");
        assert_eq!(buf.len("a"), 1);
        assert_eq!(buf.len("b"), 1);
        assert_eq!(buf.len("c"), 1);
    }

    #[test]
    fn fan_out_partial_targets() {
        let mut buf: OutputBuffer<String> = OutputBuffer::new();
        buf.register_output("a");
        buf.register_dead_letter();
        let count = buf.fan_out("event".to_owned(), &["a", "missing"]);
        assert_eq!(count, 1, "only 'a' received it");
        assert_eq!(buf.len("a"), 1);
    }

    #[test]
    fn conditional_routing() {
        let mut buf: OutputBuffer<i32> = OutputBuffer::new();
        buf.register_output("even");
        buf.register_output("odd");
        buf.route(4, |n| if n % 2 == 0 { "even" } else { "odd" });
        buf.route(7, |n| if n % 2 == 0 { "even" } else { "odd" });
        assert_eq!(buf.len("even"), 1);
        assert_eq!(buf.len("odd"), 1);
    }

    #[test]
    fn conditional_routing_dead_letter() {
        let mut buf: OutputBuffer<String> = OutputBuffer::new();
        buf.register_output("known");
        buf.register_dead_letter();
        buf.route("event".to_owned(), |_| "unknown");
        assert_eq!(buf.len(DEAD_LETTER), 1, "routed to dead-letter");
    }

    #[test]
    fn drain_returns_events_and_empties() {
        let mut buf: OutputBuffer<String> = OutputBuffer::new();
        buf.register_output("main");
        buf.push("a".to_owned(), "main");
        buf.push("b".to_owned(), "main");
        let events = buf.drain("main");
        assert_eq!(events, vec!["a", "b"]);
        assert!(buf.is_empty("main"), "empty after drain");
    }

    #[test]
    fn drain_nonexistent_returns_empty() {
        let mut buf: OutputBuffer<String> = OutputBuffer::new();
        let events = buf.drain("nope");
        assert!(events.is_empty());
    }

    #[test]
    fn peek_does_not_drain() {
        let mut buf: OutputBuffer<String> = OutputBuffer::new();
        buf.register_output("main");
        buf.push("a".to_owned(), "main");
        assert_eq!(buf.peek("main"), &["a"]);
        assert_eq!(buf.len("main"), 1, "not drained");
    }

    #[test]
    fn total_events_across_outputs() {
        let mut buf: OutputBuffer<String> = OutputBuffer::new();
        buf.register_output("a");
        buf.register_output("b");
        buf.push("x".to_owned(), "a");
        buf.push("y".to_owned(), "b");
        buf.push("z".to_owned(), "b");
        assert_eq!(buf.total_events(), 3);
    }

    #[test]
    fn clear_empties_all_outputs() {
        let mut buf: OutputBuffer<String> = OutputBuffer::new();
        buf.register_output("a");
        buf.register_output("b");
        buf.push("x".to_owned(), "a");
        buf.push("y".to_owned(), "b");
        buf.clear();
        assert_eq!(buf.total_events(), 0);
        assert!(buf.has_output("a"), "registrations preserved");
        assert!(buf.has_output("b"), "registrations preserved");
    }

    #[test]
    fn output_names_includes_all() {
        let mut buf: OutputBuffer<String> = OutputBuffer::new();
        buf.register_output("alpha");
        buf.register_output("beta");
        buf.register_dead_letter();
        let mut names = buf.output_names();
        names.sort_unstable();
        assert_eq!(names, vec!["__dead_letter", "alpha", "beta"]);
    }

    #[test]
    fn default_is_empty() {
        let buf: OutputBuffer<String> = OutputBuffer::default();
        assert_eq!(buf.total_events(), 0);
        assert!(buf.output_names().is_empty());
    }

    #[test]
    fn register_same_output_twice_is_noop() {
        let mut buf: OutputBuffer<String> = OutputBuffer::new();
        buf.register_output("main");
        buf.push("a".to_owned(), "main");
        buf.register_output("main");
        assert_eq!(buf.len("main"), 1, "data preserved after double register");
    }

    #[test]
    fn fan_out_to_empty_targets_with_dead_letter() {
        let mut buf: OutputBuffer<String> = OutputBuffer::new();
        buf.register_dead_letter();
        let count = buf.fan_out("lost".to_owned(), &["x", "y"]);
        assert_eq!(count, 0, "no targets matched");
        assert_eq!(buf.len(DEAD_LETTER), 1, "event in dead-letter");
    }

    #[test]
    fn fan_out_single_target() {
        let mut buf: OutputBuffer<String> = OutputBuffer::new();
        buf.register_output("only");
        let count = buf.fan_out("event".to_owned(), &["only"]);
        assert_eq!(count, 1);
        assert_eq!(buf.peek("only"), &["event"]);
    }

    #[test]
    fn dead_letter_drains_correctly() {
        let mut buf: OutputBuffer<String> = OutputBuffer::new();
        buf.register_dead_letter();
        buf.push("a".to_owned(), "unknown");
        buf.push("b".to_owned(), "also_unknown");
        let dead = buf.drain_dead_letter();
        assert_eq!(dead, vec!["a", "b"]);
    }
}
