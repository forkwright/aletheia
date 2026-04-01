//! Team coordination: child agent spawning with concurrency limits.
//!
//! WHY: Daemon-mode operations that exceed a single agent's scope (e.g.,
//! multi-file refactors, parallel knowledge graph updates) need controlled
//! child agent spawning with backpressure.

/// Coordinator manages child agent lifecycle with concurrency limits.
#[derive(Debug)]
pub struct Coordinator {
    max_children: usize,
}

impl Coordinator {
    /// Create a new coordinator with the given concurrency limit.
    #[must_use]
    pub fn new(max_children: usize) -> Self {
        Self { max_children }
    }

    /// Maximum number of concurrent child agents.
    #[must_use]
    pub fn max_children(&self) -> usize {
        self.max_children
    }
}
