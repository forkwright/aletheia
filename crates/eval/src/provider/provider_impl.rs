//! `EvalProvider` trait implementations.

use crate::provider::EvalProvider;
use crate::scenario::Scenario;

// ── BuiltinProvider implementation ───────────────────────────────────────────

/// Provider that returns all built-in dokimion scenarios.
///
/// This is the default when no custom provider is specified — it wraps
/// [`scenarios::all_scenarios()`](crate::scenarios::all_scenarios).
pub struct BuiltinProvider;

impl EvalProvider for BuiltinProvider {
    fn provide(&self) -> Vec<Box<dyn Scenario>> {
        crate::scenarios::all_scenarios()
    }

    // WHY: trait signature is `fn name(&self) -> &str`. CompositeProvider
    // returns a borrowed self.name field, so the trait cannot use 'static.
    // Allowed locally on impls that happen to return literals.
    #[allow(
        clippy::unnecessary_literal_bound,
        reason = "trait signature returns &str (borrowed), not &'static str"
    )]
    fn name(&self) -> &str {
        "builtin"
    }
}

// ── CompositeProvider implementation ─────────────────────────────────────────

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
