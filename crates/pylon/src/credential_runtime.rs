//! Runtime credential-management state for pylon-managed provider credentials.
//!
//! The `/api/v1/system/credentials` endpoints mutate encrypted credential files
//! under the Oikos credential root. This module tracks whether those mutations
//! can be consumed by the live provider registry without a process restart, and
//! exposes that effect state to health/capability output (#4872).

use std::sync::Arc;
use std::time::Instant;

use tokio::sync::Mutex;

use hermeneus::provider::ProviderRegistry;
use serde::Serialize;
use snafu::Snafu;
use utoipa::ToSchema;

/// Effect of a credential-management mutation on the running harness.
///
/// WHY: callers must never see a plain success that implies the running harness
/// changed when only on-disk state changed. Every mutation returns a typed
/// effect so the UI can warn or block until the required action is taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CredentialMutationEffect {
    /// The mutation was applied to the live provider chain without restart.
    Applied,
    /// A process restart is required before the running harness will use the
    /// new credential state.
    RestartRequired,
    /// The on-disk state changed; the file-backed credential chain will pick it
    /// up on its next reload interval, but in-memory cached tokens may still win
    /// until then.
    PendingReload,
    /// The provider is registered, but its runtime credential source is not
    /// managed by these endpoints (e.g. env-var auth or a local subprocess).
    NotSupportedByRuntime,
}

/// Snapshot of the most recent credential mutation effect.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct LastCredentialEffect {
    /// Provider that was mutated.
    pub provider: String,
    /// Computed runtime effect.
    pub effect: CredentialMutationEffect,
    /// Seconds since the effect was recorded.
    pub elapsed_secs: u64,
}

/// Manager that owns the runtime view of pylon-managed credentials.
///
/// It knows the current provider registry, the set of providers whose
/// credentials pylon can manage, and the effect of the last mutation.
pub struct CredentialRuntimeManager {
    /// Registry of available LLM providers.
    provider_registry: Arc<ProviderRegistry>,
    /// Last mutation effect recorded for health/capability output.
    last_effect: Mutex<Option<RecordedEffect>>,
}

struct RecordedEffect {
    provider: String,
    effect: CredentialMutationEffect,
    at: Instant,
}

impl CredentialRuntimeManager {
    /// Create a manager bound to an instance layout and provider registry.
    #[must_use]
    pub fn new(provider_registry: Arc<ProviderRegistry>) -> Self {
        Self {
            provider_registry,
            last_effect: Mutex::new(None),
        }
    }

    /// Canonical providers that pylon-managed credential files can feed.
    ///
    /// WHY: the runtime consumption path is currently the Anthropic file chain
    /// (`oikos.credentials().join("anthropic.json")`). These names are accepted
    /// even when the registry is degraded at startup so that operators can add
    /// a credential after a no-credential start.
    const MANAGED_PROVIDER_NAMES: &'static [&'static str] = &["anthropic", "claude"];

    /// Return all provider names that API consumers may reference.
    ///
    /// This is the union of registered LLM providers and the canonical managed
    /// provider names, deduplicated and sorted for stable output.
    #[must_use]
    pub fn supported_providers(&self) -> Vec<String> {
        let mut names: std::collections::BTreeSet<String> = self
            .provider_registry
            .providers()
            .into_iter()
            .map(|p| p.name().to_owned())
            .collect();
        for name in Self::MANAGED_PROVIDER_NAMES {
            names.insert((*name).to_owned());
        }
        names.into_iter().collect()
    }

    /// Return `true` if `provider` names a registered or canonical provider.
    #[must_use]
    pub fn is_supported_provider(&self, provider: &str) -> bool {
        let normalized = provider.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return false;
        }
        Self::is_managed_provider_name(&normalized)
            || self
                .provider_registry
                .providers()
                .into_iter()
                .any(|p| p.name().to_ascii_lowercase() == normalized)
    }

    /// Validate that `provider` is supported by the runtime.
    ///
    /// Returns `Ok(())` when the provider is registered or is a canonical
    /// managed provider. Otherwise returns an error listing supported names.
    pub fn validate_provider(&self, provider: &str) -> Result<(), CredentialRuntimeError> {
        if self.is_supported_provider(provider) {
            Ok(())
        } else {
            UnsupportedProviderSnafu {
                provider: provider.to_owned(),
                supported: self.supported_providers(),
            }
            .fail()
        }
    }

    /// Compute the runtime effect of mutating `provider`'s credentials.
    ///
    /// Callers must validate the provider first. Canonical managed providers
    /// currently require a restart because the live credential chain holds
    /// in-memory cached tokens and mtime-gated file caches that pylon cannot
    /// invalidate from outside `symbolon` (#4872).
    #[must_use]
    pub fn mutation_effect(&self, provider: &str) -> CredentialMutationEffect {
        let normalized = provider.trim().to_ascii_lowercase();
        if Self::is_managed_provider_name(&normalized) {
            // WHY: RefreshingCredentialProvider keeps an in-memory current_token
            // and FileCredentialProvider caches until the mtime interval elapses.
            // Pylon cannot hot-clear those caches without changes outside the
            // blast zone, so we report the honest restart requirement.
            CredentialMutationEffect::RestartRequired
        } else {
            // Registered provider that does not consume the pylon-managed file.
            CredentialMutationEffect::NotSupportedByRuntime
        }
    }

    /// Whether a credential mutation for `provider` is consumed by the live
    /// harness without a restart.
    ///
    /// WHY(#4878): health/capability output needs a static per-provider
    /// capability flag, not just the effect of whatever mutation happened to
    /// run most recently (which may never have happened for a given
    /// provider). Derived from [`Self::mutation_effect`] — the single
    /// source of truth for whether a provider's credential chain can be
    /// hot-reloaded stays that method; this just names the boolean.
    #[must_use]
    pub fn hot_apply_supported(&self, provider: &str) -> bool {
        matches!(
            self.mutation_effect(provider),
            CredentialMutationEffect::Applied | CredentialMutationEffect::PendingReload
        )
    }

    /// Current availability of `provider`, per the live provider health
    /// tracker, when a provider instance with that name is registered.
    ///
    /// WHY(#4878): a canonical managed provider name (e.g. "anthropic") can
    /// have credential files on disk with no corresponding registered
    /// provider instance yet (a no-credential start, #4872) — `None` is the
    /// honest answer for "not yet registered", distinct from any
    /// [`hermeneus::health::ProviderHealth`] variant, all of which mean a
    /// provider instance exists and is being tracked.
    #[must_use]
    pub fn provider_availability(&self, provider: &str) -> Option<hermeneus::health::ProviderHealth> {
        self.provider_registry.provider_health(provider)
    }

    /// Record the effect of a mutation for health/capability output.
    pub async fn record_effect(&self, provider: &str, effect: CredentialMutationEffect) {
        let mut guard = self.last_effect.lock().await;
        *guard = Some(RecordedEffect {
            provider: provider.to_owned(),
            effect,
            at: Instant::now(),
        });
    }

    /// Return the last recorded effect, if any.
    #[must_use]
    pub async fn last_effect(&self) -> Option<LastCredentialEffect> {
        let guard = self.last_effect.lock().await;
        guard.as_ref().map(|r| LastCredentialEffect {
            provider: r.provider.clone(),
            effect: r.effect,
            elapsed_secs: r.at.elapsed().as_secs(),
        })
    }

    fn is_managed_provider_name(normalized: &str) -> bool {
        Self::MANAGED_PROVIDER_NAMES
            .iter()
            .any(|name| name.to_ascii_lowercase() == normalized)
    }
}

impl CredentialMutationEffect {
    /// Stable snake-case wire name for this effect.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::RestartRequired => "restart_required",
            Self::PendingReload => "pending_reload",
            Self::NotSupportedByRuntime => "not_supported_by_runtime",
        }
    }
}

impl std::fmt::Display for CredentialMutationEffect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Errors arising from runtime credential validation.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum CredentialRuntimeError {
    /// Provider name is not known to the runtime.
    #[snafu(display(
        "provider '{provider}' is not supported by runtime credential management; supported: {supported:?}"
    ))]
    UnsupportedProvider {
        /// Provider name supplied by the caller.
        provider: String,
        /// Supported provider names at the time of the request.
        supported: Vec<String>,
    },
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use hermeneus::health::ProviderHealth;
    use hermeneus::test_utils::MockProvider;

    use super::*;

    fn manager_with(provider: Option<MockProvider>) -> CredentialRuntimeManager {
        let mut registry = ProviderRegistry::new();
        if let Some(p) = provider {
            registry.register(Box::new(p));
        }
        CredentialRuntimeManager::new(Arc::new(registry))
    }

    // ── hot_apply_supported (#4878) ──

    #[test]
    fn hot_apply_supported_false_for_managed_provider() {
        // WHY: mirrors mutation_effect's honest RestartRequired answer for
        // canonical managed providers -- pylon cannot hot-clear the
        // RefreshingCredentialProvider/FileCredentialProvider caches yet
        // (#4872). If that ever changes, mutation_effect changes and this
        // derived flag follows without a second edit.
        let manager = manager_with(None);
        assert!(!manager.hot_apply_supported("anthropic"));
        assert!(!manager.hot_apply_supported("claude"));
    }

    #[test]
    fn hot_apply_supported_false_for_unmanaged_registered_provider() {
        let manager = manager_with(Some(MockProvider::new("hi").named("custom-llm")));
        assert!(!manager.hot_apply_supported("custom-llm"));
    }

    // ── provider_availability (#4878) ──

    #[test]
    fn provider_availability_none_for_unregistered_name() {
        let manager = manager_with(None);
        assert_eq!(manager.provider_availability("anthropic"), None);
    }

    #[test]
    fn provider_availability_some_for_registered_provider() {
        let manager = manager_with(Some(MockProvider::new("hi").named("custom-llm")));
        assert_eq!(
            manager.provider_availability("custom-llm"),
            Some(ProviderHealth::Up),
            "a freshly-registered provider starts healthy"
        );
    }
}
