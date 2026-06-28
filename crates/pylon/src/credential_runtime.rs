//! Runtime credential-management state for pylon-managed provider credentials.
//!
//! The `/api/v1/system/credentials` endpoints mutate encrypted credential files
//! under the Oikos credential root. This module tracks whether those mutations
//! can be consumed by the live provider registry without a process restart, and
//! exposes that effect state to health/capability output (#4872).

use std::sync::Arc;
use std::time::Instant;

use tokio::sync::Mutex;

use hermeneus::provider::{ProviderCredentialValidation, ProviderRegistry};
use koina::secret::SecretString;
use serde::Serialize;
use snafu::Snafu;
use symbolon::types::ManagedCredentialStatus;
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

    /// Validate a supplied credential secret against the matching runtime
    /// provider, when the provider can test arbitrary secrets.
    ///
    /// Canonical managed providers remain supported even when no matching
    /// provider is registered, but their validation status is `Unknown` because
    /// pylon cannot prove provider acceptance without a provider hook.
    pub async fn validate_credential(
        &self,
        provider: &str,
        credential: &SecretString,
    ) -> ManagedCredentialStatus {
        let normalized = provider.trim().to_ascii_lowercase();
        let providers = self.provider_registry.providers();
        let selected = providers
            .iter()
            .copied()
            .find(|candidate| candidate.name().to_ascii_lowercase() == normalized)
            .or_else(|| {
                if Self::is_managed_provider_name(&normalized) {
                    providers.iter().copied().find(|candidate| {
                        Self::is_managed_provider_name(&candidate.name().to_ascii_lowercase())
                    })
                } else {
                    None
                }
            });

        let Some(provider) = selected else {
            return ManagedCredentialStatus::Unknown;
        };

        match provider.validate_credential(credential).await {
            ProviderCredentialValidation::Accepted => ManagedCredentialStatus::ProviderAccepted,
            ProviderCredentialValidation::Rejected => ManagedCredentialStatus::ProviderRejected,
            ProviderCredentialValidation::Expired => ManagedCredentialStatus::Expired,
            ProviderCredentialValidation::Malformed => ManagedCredentialStatus::Malformed,
            ProviderCredentialValidation::Unreachable => {
                ManagedCredentialStatus::ProviderUnreachable
            }
            _ => ManagedCredentialStatus::Unknown,
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
