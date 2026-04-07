//! Evaluation provider trait: pluggable scenario sources.
//!
//! [`EvalProvider`] decouples scenario registration from execution. The runner
//! no longer hardcodes `scenarios::all_scenarios()` — instead, callers provide
//! a `Box<dyn EvalProvider>` that supplies the scenario set.
//!
//! WHY: dokimion was CLI-only. Making scenario sources pluggable enables:
//! - Daemon probes running a subset of scenarios on a schedule
//! - Canary prompt suites (W-12) as a separate provider
//! - Phase gate checks composing multiple providers
//! - A/B model evaluation with custom scenario sets

use crate::scenario::Scenario;

// ---------------------------------------------------------------------------
// EvalProvider trait
// ---------------------------------------------------------------------------

/// Source of evaluation scenarios.
///
/// Implementations decide which scenarios to include. The runner calls
/// [`provide`] once at the start of a run and executes the returned set.
pub trait EvalProvider: Send + Sync {
    /// Return the scenarios this provider supplies.
    ///
    /// Called once per eval run. Implementations may filter, compose, or
    /// dynamically generate scenarios.
    fn provide(&self) -> Vec<Box<dyn Scenario>>;

    /// Human-readable name for display in reports.
    fn name(&self) -> &str;
}

// ---------------------------------------------------------------------------
// BuiltinProvider
// ---------------------------------------------------------------------------

/// Provider that returns all built-in dokimion scenarios.
///
/// This is the default when no custom provider is specified — it wraps
/// [`scenarios::all_scenarios()`](crate::scenarios::all_scenarios).
pub struct BuiltinProvider;

impl EvalProvider for BuiltinProvider {
    fn provide(&self) -> Vec<Box<dyn Scenario>> {
        crate::scenarios::all_scenarios()
    }

    fn name(&self) -> &str {
        "builtin"
    }
}

// ---------------------------------------------------------------------------
// CompositeProvider
// ---------------------------------------------------------------------------

/// Combines multiple providers into a single scenario set.
///
/// Scenarios are collected in provider order. Deduplication is the caller's
/// responsibility (scenario IDs are not enforced unique across providers).
pub struct CompositeProvider {
    providers: Vec<Box<dyn EvalProvider>>,
    name: String,
}

impl CompositeProvider {
    /// Create a composite from a list of providers.
    #[must_use]
    pub fn new(providers: Vec<Box<dyn EvalProvider>>) -> Self {
        let name = providers
            .iter()
            .map(|p| p.name())
            .collect::<Vec<_>>()
            .join("+");
        Self { providers, name }
    }
}

impl EvalProvider for CompositeProvider {
    fn provide(&self) -> Vec<Box<dyn Scenario>> {
        let mut scenarios = Vec::new();
        for provider in &self.providers {
            scenarios.extend(provider.provide());
        }
        scenarios
    }

    fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_provider_returns_scenarios() {
        let provider = BuiltinProvider;
        let scenarios = provider.provide();
        assert!(!scenarios.is_empty(), "builtin provider should return scenarios");
        assert_eq!(provider.name(), "builtin");
    }

    #[test]
    fn composite_provider_combines() {
        let composite = CompositeProvider::new(vec![
            Box::new(BuiltinProvider),
            Box::new(BuiltinProvider),
        ]);
        let scenarios = composite.provide();
        let single = BuiltinProvider.provide().len();
        assert_eq!(scenarios.len(), single * 2);
        assert_eq!(composite.name(), "builtin+builtin");
    }

    #[test]
    fn empty_composite() {
        let composite = CompositeProvider::new(vec![]);
        assert!(composite.provide().is_empty());
        assert!(composite.name().is_empty());
    }
}
