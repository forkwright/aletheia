//! Reserved child-agent coordination boundary.
//!
//! WHY: daemon-mode operations that exceed a single agent's scope may need
//! controlled child-agent spawning with backpressure. The current daemon
//! runtime does not spawn, join, kill, or track child agents yet.

/// Reserved coordinator configuration for future child-agent lifecycle wiring.
///
/// Today this type only stores the configured child concurrency limit. It does
/// not spawn child agents or track in-flight children.
#[derive(Debug)]
pub struct Coordinator {
    max_children: usize,
}

impl Coordinator {
    /// Create a reserved coordinator marker with the given concurrency limit.
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
