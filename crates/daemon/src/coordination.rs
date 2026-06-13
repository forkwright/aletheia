//! Child-agent coordination boundary with enforced concurrency limits.
//!
//! WHY: daemon-mode operations that exceed a single agent's scope may need
//! controlled child-agent spawning with backpressure. The `Coordinator` tracks
//! live child count and enforces the configured `max_children` cap before any
//! spawn proceeds. Direct nous spawn paths are intentionally NOT wired here;
//! callers check `can_spawn()` / `record_spawn()` at the dispatch site.

/// Coordinator for child-agent lifecycle with enforced concurrency limits.
///
/// `Coordinator` tracks the current number of live child agents against the
/// configured cap. Callers are responsible for calling [`record_spawn`] and
/// [`record_exit`] symmetrically. The coordinator provides no implicit cleanup;
/// if a caller forgets to call `record_exit`, the count will remain elevated.
///
/// [`record_spawn`]: Self::record_spawn
/// [`record_exit`]: Self::record_exit
#[derive(Debug)]
pub struct Coordinator {
    max_children: usize,
    current_children: usize,
}

impl Coordinator {
    /// Create a coordinator with the given concurrency limit.
    #[must_use]
    pub fn new(max_children: usize) -> Self {
        Self {
            max_children,
            current_children: 0,
        }
    }

    /// Maximum number of concurrent child agents.
    #[must_use]
    pub fn max_children(&self) -> usize {
        self.max_children
    }

    /// Number of currently tracked live child agents.
    #[must_use]
    pub fn current_children(&self) -> usize {
        self.current_children
    }

    /// Returns `true` if a new child agent may be spawned within the configured limit.
    #[must_use]
    pub fn can_spawn(&self) -> bool {
        self.current_children < self.max_children
    }

    /// Remaining spawn capacity (`max_children - current_children`).
    #[must_use]
    pub fn remaining_capacity(&self) -> usize {
        self.max_children.saturating_sub(self.current_children)
    }

    /// Record that a child agent has been spawned. Increments the live count.
    ///
    /// Callers MUST pair each `record_spawn` with a corresponding [`record_exit`]
    /// when the child exits or is cancelled.
    ///
    /// [`record_exit`]: Self::record_exit
    pub fn record_spawn(&mut self) {
        self.current_children = self.current_children.saturating_add(1);
    }

    /// Record that a child agent has exited. Decrements the live count (saturating at 0).
    pub fn record_exit(&mut self) {
        self.current_children = self.current_children.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinator_starts_below_capacity() {
        let c = Coordinator::new(3);
        assert!(c.can_spawn());
        assert_eq!(c.remaining_capacity(), 3);
        assert_eq!(c.current_children(), 0);
    }

    #[test]
    fn coordinator_at_capacity_cannot_spawn() {
        let mut c = Coordinator::new(2);
        c.record_spawn();
        c.record_spawn();
        assert!(
            !c.can_spawn(),
            "at max_children, can_spawn must return false"
        );
        assert_eq!(c.remaining_capacity(), 0);
    }

    #[test]
    fn coordinator_spawn_exit_cycle_stays_within_bounds() {
        let mut c = Coordinator::new(3);
        c.record_spawn();
        assert_eq!(c.current_children(), 1);
        assert!(c.can_spawn());

        c.record_spawn();
        c.record_spawn();
        assert!(!c.can_spawn());

        c.record_exit();
        assert!(c.can_spawn());
        assert_eq!(c.current_children(), 2);
    }

    #[test]
    fn coordinator_exit_saturates_at_zero() {
        let mut c = Coordinator::new(3);
        c.record_exit(); // underflow-safe
        assert_eq!(c.current_children(), 0);
    }

    #[test]
    fn coordinator_max_children_accessor() {
        let c = Coordinator::new(5);
        assert_eq!(c.max_children(), 5);
    }
}
