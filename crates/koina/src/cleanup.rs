//! Setup-time cleanup registration via RAII guards.
//!
//! [`CleanupGuard`](crate::cleanup::CleanupGuard) executes a callback when dropped, ensuring resource cleanup
//! fires on normal return, early error exit, panic, and async cancellation.
//! Register the guard at the point of resource acquisition, not in a separate
//! `Drop` impl, so cleanup is tied to the acquisition scope.
//!
//! # Example
//!
//! ```
//! use aletheia_koina::cleanup::CleanupGuard;
//!
//! let temp_file = "/tmp/example.lock";
//! // Register cleanup immediately after acquiring the resource.
//! let guard = CleanupGuard::new(|| {
//!     let _ = std::fs::remove_file(temp_file);
//! });
//!
//! // ... use the resource ...
//!
//! // If this function returns early (error, panic), the guard still fires.
//! // To suppress cleanup on success:
//! guard.disarm();
//! ```

/// RAII guard that runs a callback on drop.
///
/// The callback fires exactly once: on drop if not disarmed, or never if
/// [`disarm`](CleanupGuard::disarm) was called. The guard is `Send` when
/// the callback is `Send`, making it safe to hold across `.await` points.
pub struct CleanupGuard<F: FnOnce()> {
    callback: Option<F>,
}

impl<F: FnOnce()> CleanupGuard<F> {
    /// Create a guard that will run `callback` on drop.
    ///
    /// Register the guard at the point of resource acquisition so cleanup
    /// is guaranteed even on early return or panic.
    #[must_use]
    pub fn new(callback: F) -> Self {
        Self {
            callback: Some(callback),
        }
    }

    /// Suppress the cleanup callback.
    ///
    /// Call this on the success path when cleanup is no longer needed
    /// (e.g., ownership was transferred to another component).
    pub fn disarm(mut self) {
        self.callback = None;
    }
}

impl<F: FnOnce()> Drop for CleanupGuard<F> {
    fn drop(&mut self) {
        if let Some(f) = self.callback.take() {
            f();
        }
    }
}

/// Registry that collects multiple cleanup callbacks and runs them all on drop.
///
/// Unlike [`CleanupGuard`] (single callback), `CleanupRegistry` accumulates
/// callbacks over time and runs them in reverse registration order (LIFO) on
/// drop, matching the natural resource acquisition/release pattern.
///
/// # Example
///
/// ```
/// use aletheia_koina::cleanup::CleanupRegistry;
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
    fn guard_fires_on_drop() {
        let counter = Arc::new(AtomicU32::new(0));
        let c = Arc::clone(&counter);
        {
            let _guard = CleanupGuard::new(move || {
                c.fetch_add(1, Ordering::Relaxed);
            });
        }
        assert_eq!(
            counter.load(Ordering::Relaxed),
            1,
            "callback must fire on drop"
        );
    }

    #[test]
    fn guard_disarm_suppresses_callback() {
        let counter = Arc::new(AtomicU32::new(0));
        let c = Arc::clone(&counter);
        let guard = CleanupGuard::new(move || {
            c.fetch_add(1, Ordering::Relaxed);
        });
        guard.disarm();
        assert_eq!(
            counter.load(Ordering::Relaxed),
            0,
            "disarmed guard must not fire"
        );
    }

    #[test]
    fn guard_fires_on_panic() {
        let counter = Arc::new(AtomicU32::new(0));
        let c = Arc::clone(&counter);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = CleanupGuard::new(move || {
                c.fetch_add(1, Ordering::Relaxed);
            });
            panic!("simulated panic");
        }));
        assert!(result.is_err(), "panic must propagate");
        assert_eq!(
            counter.load(Ordering::Relaxed),
            1,
            "guard must fire during panic unwind"
        );
    }

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
    fn guard_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<CleanupGuard<Box<dyn FnOnce() + Send>>>();
    }

    #[test]
    fn registry_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CleanupRegistry>();
    }
}
