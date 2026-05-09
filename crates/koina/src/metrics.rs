//! Shared Prometheus registry for the Aletheia workspace.
//!
//! `prometheus-client` does not expose a process-wide global registry, so
//! every crate that emits metrics registers its metric families against a
//! single shared [`Registry`] at startup. The [`MetricsRegistry`] wrapper here
//! is stored in `pylon`'s application state and shared with the `/metrics`
//! exposition handler.
//!
//! Each metrics-emitting crate follows this pattern:
//!
//! ```rust,ignore
//! // In the crate's `metrics` module:
//! use std::sync::LazyLock;
//! use prometheus_client::metrics::counter::Counter;
//! use prometheus_client::metrics::family::Family;
//! use prometheus_client::registry::Registry;
//!
//! pub(crate) static MY_COUNTER: LazyLock<Family<MyLabels, Counter>> =
//!     LazyLock::new(Family::default);
//!
//! /// Register this crate's metrics with the shared registry.
//! pub fn register(registry: &mut Registry) {
//!     registry.register(
//!         "aletheia_my_counter", // no `_total` suffix: the encoder appends it
//!         "counter description",
//!         MY_COUNTER.clone(),
//!     );
//! }
//! ```
//!
//! Callers then do:
//!
//! ```rust,ignore
//! let registry = koina::metrics::MetricsRegistry::new();
//! my_crate::metrics::register(&mut registry.write());
//! ```
//!
//! # Naming
//!
//! `prometheus-client` automatically appends `_total` to counter names during
//! exposition. Register counter families **without** the `_total` suffix so
//! the exposed name is `aletheia_foo_total` (matching the previous
//! `prometheus` crate output). Histograms and gauges keep their full name.

use std::sync::{Arc, Mutex};

use prometheus_client::registry::Registry;

/// Shared metrics registry.
///
/// Wraps [`prometheus_client::registry::Registry`] behind an `Arc<Mutex<_>>`
/// so it can be cloned cheaply into handler state, middleware, and startup
/// code. Registration happens once during startup; encoding happens on every
/// `/metrics` scrape.
#[derive(Clone, Default)]
pub struct MetricsRegistry {
    inner: Arc<Mutex<Registry>>,
}

impl MetricsRegistry {
    /// Create a new empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Run a closure with exclusive access to the underlying registry.
    ///
    /// Used at startup to register metric families. Not intended for hot paths
    /// — cloning metric Families (cheap, uses `Arc` internally) is preferred
    /// for recording.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned, which indicates a prior
    /// panic while another caller held the lock. Startup registration is the
    /// only caller, so poisoning is a programmer error.
    pub fn with_registry<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Registry) -> R,
    {
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&mut guard)
    }

    /// Encode the registry as `OpenMetrics` text format into `buffer`.
    ///
    /// The output is compatible with Prometheus scrapers (Prometheus accepts
    /// `OpenMetrics` text natively).
    ///
    /// # Errors
    ///
    /// Returns [`std::fmt::Error`] only if the underlying writer fails.
    /// Writing into a `String` never fails, so call sites that use a `String`
    /// buffer can safely unwrap.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned (see [`Self::with_registry`]).
    pub fn encode(&self, buffer: &mut String) -> Result<(), std::fmt::Error> {
        let guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        prometheus_client::encoding::text::encode(buffer, &guard)
    }
}

impl std::fmt::Debug for MetricsRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetricsRegistry").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use prometheus_client::encoding::EncodeLabelSet;
    use prometheus_client::metrics::counter::Counter;
    use prometheus_client::metrics::family::Family;

    use super::*;

    #[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
    struct TestLabels {
        kind: String,
    }

    #[test]
    fn registry_encodes_registered_metric() {
        let registry = MetricsRegistry::new();
        let counter: Family<TestLabels, Counter> = Family::default();
        registry.with_registry(|r| {
            r.register("aletheia_probe", "probe counter", counter.clone());
        });
        counter
            .get_or_create(&TestLabels { kind: "ok".into() })
            .inc();
        let mut buffer = String::new();
        #[expect(
            clippy::expect_used,
            reason = "encoding into String is infallible; test assertion"
        )]
        registry.encode(&mut buffer).expect("encode");
        assert!(buffer.contains("aletheia_probe_total"), "got: {buffer}");
        assert!(buffer.contains("kind=\"ok\""), "got: {buffer}");
    }

    #[test]
    fn registry_is_clone_send_sync() {
        fn assert_bounds<T: Clone + Send + Sync>() {}
        assert_bounds::<MetricsRegistry>();
    }
}
