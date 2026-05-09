//! Setup-time cleanup registration via a callback registry.

/// Registry that collects multiple cleanup callbacks and runs them all on drop.
///
/// `CleanupRegistry` accumulates callbacks over time and runs them in reverse
/// registration order (LIFO) on drop -- matching the natural resource
/// acquisition/release pattern.
///
/// # Example
///
/// ```
/// use koina::cleanup::CleanupRegistry;
/// use std::sync::Arc;
/// use std::sync::atomic::{AtomicU32, Ordering};
///
/// let counter = Arc::new(AtomicU32::new(0));
/// let mut registry = CleanupRegistry::new();
///
/// let c = Arc::clone(&counter);
/// registry.register(move || { c.fetch_add(1, Ordering::Relaxed); });
///
/// let c = Arc::clone(&counter);
/// registry.register(move || { c.fetch_add(10, Ordering::Relaxed); });
///
/// drop(registry);
/// assert_eq!(counter.load(Ordering::Relaxed), 11, "both callbacks must fire");
/// ```
pub struct CleanupRegistry {
    callbacks: Vec<Box<dyn FnOnce() + Send + Sync>>,
}

impl CleanupRegistry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            callbacks: Vec::new(),
        }
    }

    /// Register a cleanup callback that will run on drop.
    ///
    /// Callbacks execute in reverse registration order (LIFO).
    pub fn register(&mut self, callback: impl FnOnce() + Send + Sync + 'static) {
        self.callbacks.push(Box::new(callback));
    }

    /// Disarm all registered callbacks without running them.
    pub fn disarm(&mut self) {
        self.callbacks.clear();
    }
}

impl Default for CleanupRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for CleanupRegistry {
    fn drop(&mut self) {
        // Run in reverse order (LIFO) so later acquisitions are cleaned up first.
        for callback in self.callbacks.drain(..).rev() {
            callback();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    #[test]
    fn registry_fires_all_in_reverse_order() {
        let order = Arc::new(std::sync::Mutex::new(Vec::new()));
        {
            let mut registry = CleanupRegistry::new();
            let o = Arc::clone(&order);
            registry.register(move || {
                let _ = o.lock().map(|mut v| v.push(1));
            });
            let o = Arc::clone(&order);
            registry.register(move || {
                let _ = o.lock().map(|mut v| v.push(2));
            });
            let o = Arc::clone(&order);
            registry.register(move || {
                let _ = o.lock().map(|mut v| v.push(3));
            });
        }
        #[expect(clippy::expect_used, reason = "test assertion")]
        let recorded = order.lock().expect("lock not poisoned");
        assert_eq!(
            &*recorded,
            &[3, 2, 1],
            "callbacks must fire in reverse order"
        );
    }

    #[test]
    fn registry_disarm_clears_all() {
        let counter = Arc::new(AtomicU32::new(0));
        {
            let mut registry = CleanupRegistry::new();
            let c = Arc::clone(&counter);
            registry.register(move || {
                c.fetch_add(1, Ordering::Relaxed);
            });
            registry.disarm();
        }
        assert_eq!(
            counter.load(Ordering::Relaxed),
            0,
            "disarmed registry must not fire callbacks"
        );
    }

    #[test]
    fn registry_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CleanupRegistry>();
    }
}
