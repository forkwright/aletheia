// kanon:ignore RUST/file-too-long — setup factories are cohesive initialization helpers; no natural split point
//! Setup helpers: factory functions for providers, registries, and channels.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use snafu::prelude::*;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use agora::listener::ChannelListener;
use agora::matrix::MatrixProvider;
use agora::matrix::client::MatrixClient;
use agora::registry::ChannelRegistry;
use agora::router::MessageRouter;
use agora::semeion::SignalProvider;
use agora::semeion::client::SignalClient;
use agora::types::ChannelProvider;
use hermeneus::RetryPolicy;
use hermeneus::anthropic::{AnthropicProvider, ProviderBehavior};
use hermeneus::concurrency::ConcurrencyConfig;
use hermeneus::openai::{
    OpenAiApiFamily as HermeneusOpenAiApiFamily, OpenAiProvider, OpenAiProviderConfig,
};
use hermeneus::provider::{
    DeploymentTarget as HermeneusDeploymentTarget, ProviderConfig, ProviderRegistry,
};
use koina::credential::{CredentialProvider, CredentialSource};
use koina::secret::SecretString;
use mneme::embedding::{DegradedEmbeddingProvider, EmbeddingProvider, create_provider};
use mneme::store::SessionStore;
use nous::manager::NousManager;
use symbolon::credential::{
    ClaudeCodeCredentialSource, CredentialChain, CredentialFile, EnvCredentialProvider,
    FileCredentialProvider, RefreshingCredentialProvider, claude_code_credential_path_resolution,
    claude_code_provider,
};
use taxis::config::{AletheiaConfig, EmbeddingSettings};
use taxis::oikos::Oikos;
use tokio::sync::Mutex;

use crate::error::Result;

mod tool_registry;

pub(super) use tool_registry::{build_tool_registry, sandbox_config};

#[derive(Clone, Copy)]
enum ProviderPlanEntry<'a> {
    Declared(&'a taxis::config::LlmProviderConfig),
    LegacyAnthropic,
    #[cfg(feature = "cc-provider")]
    AutoClaudeCode,
    #[cfg(feature = "codex-provider")]
    AutoCodex,
    #[cfg(feature = "kimi-provider")]
    AutoKimi,
}

fn retry_policy_from_config(config: &AletheiaConfig) -> RetryPolicy {
    RetryPolicy {
        max_retries: config.retry.max_attempts,
        backoff_base_ms: config.retry.backoff_base_ms,
        backoff_max_ms: config.retry.backoff_max_ms,
    }
}

fn concurrency_config_from_provider_behavior(
    behavior: &taxis::config::ProviderBehaviorConfig,
) -> ConcurrencyConfig {
    ConcurrencyConfig {
        ewma_alpha: behavior.concurrency_ewma_alpha,
        latency_threshold_secs: behavior.concurrency_latency_threshold_secs,
        ..ConcurrencyConfig::default()
    }
}

pub(super) fn build_provider_registry(config: &AletheiaConfig, oikos: &Oikos) -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();

    // WHY(#5583): `taxis::config::ModelPricing` and `hermeneus::provider::ModelPricing`
    // are both aliases of `koina::models::ModelPrice`; no field-copy conversion needed.
    let pricing = config.pricing.clone();

    let cred_source = config.credential.source.as_str();
    let credential_chain = if provider_plan_needs_credential_chain(config) {
        build_anthropic_credential_chain(config, oikos, cred_source)
    } else {
        let empty_chain: Arc<dyn CredentialProvider> = Arc::new(CredentialChain::new(Vec::new()));
        empty_chain
    };
    let resolved_source = credential_chain.get_credential().map(|c| c.source);
    if let Some(ref source) = resolved_source {
        // SAFETY: logging credential source name (e.g. "oauth", "api-key"), not credential value
        info!(source = %source, "credential resolved"); // kanon:ignore SECURITY/credential-logging -- logs source type string, not the secret
    } else if config.providers.is_empty() {
        warn!(
            "no credential found -- server will start in degraded mode (no LLM)\n  \
             Fix: SET ANTHROPIC_API_KEY env var, or run `aletheia credential status`"
        );
    }

    // WHY(#3410): the taxis and hermeneus PromptCacheMode enums are
    // intentionally decoupled so taxis does not depend on hermeneus; both
    // default to `Disabled` (sovereignty-first).
    let prompt_cache_mode = match config.anthropic.prompt_cache_mode {
        taxis::config::PromptCacheMode::Ephemeral => {
            hermeneus::provider::PromptCacheMode::Ephemeral
        }
        taxis::config::PromptCacheMode::Extended => hermeneus::provider::PromptCacheMode::Extended,
        // WHY: taxis::config::PromptCacheMode is #[non_exhaustive] to keep
        // future additions non-breaking. Unknown/Disabled variants default to
        // the sovereignty-first policy.
        _ => hermeneus::provider::PromptCacheMode::Disabled,
    };
    // WHY(#5044): `[retry]` is the single operator-facing owner for provider
    // retry attempts/backoff. `providerBehavior` owns provider timing,
    // concurrency, and routing controls.
    let retry_policy = retry_policy_from_config(config);
    let concurrency = concurrency_config_from_provider_behavior(&config.provider_behavior);
    let provider_config = ProviderConfig {
        pricing,
        prompt_cache_mode,
        retry_policy,
        concurrency,
        ..ProviderConfig::default()
    };

    let behavior = ProviderBehavior {
        non_streaming_timeout: std::time::Duration::from_secs(
            config.provider_behavior.non_streaming_timeout_secs,
        ),
        sse_retry_ms: config.provider_behavior.sse_default_retry_ms,
    };
    let provider_plan = build_provider_plan(config, cred_source, resolved_source.as_ref());

    register_provider_plan(
        &mut registry,
        &provider_plan,
        oikos,
        &credential_chain,
        resolved_source.is_some(),
        &provider_config,
        &behavior,
    );

    registry
}

fn provider_plan_needs_credential_chain(config: &AletheiaConfig) -> bool {
    use taxis::config::ProviderKind;

    config.providers.is_empty()
        || config
            .providers
            .iter()
            .any(|entry| entry.kind == ProviderKind::Anthropic && entry.api_key_env.is_none())
}

/// Whether the `auto` credential source may consume a Claude Code path resolution.
///
/// INVARIANT(#5252): `PlatformConfigDir` means nothing was configured — implicit
/// discovery of a co-installed agent's private credential store is opt-in only
/// (env var, configured path, or `cred_source = "claude-code"`).
fn auto_may_use_claude_code(source: ClaudeCodeCredentialSource) -> bool {
    source != ClaudeCodeCredentialSource::PlatformConfigDir
}

#[cfg(test)]
mod claude_code_opt_in_tests {
    use symbolon::credential::ClaudeCodeCredentialSource;

    use super::auto_may_use_claude_code;

    #[test]
    fn auto_never_uses_platform_config_dir_discovery() {
        assert!(!auto_may_use_claude_code(
            ClaudeCodeCredentialSource::PlatformConfigDir
        ));
    }

    #[test]
    fn auto_uses_explicit_env_and_config_sources() {
        assert!(auto_may_use_claude_code(ClaudeCodeCredentialSource::Env));
        assert!(auto_may_use_claude_code(ClaudeCodeCredentialSource::Config));
    }
}

fn build_anthropic_credential_chain(
    config: &AletheiaConfig,
    oikos: &Oikos,
    cred_source: &str,
) -> Arc<dyn CredentialProvider> {
    let cred_file = oikos.credentials().join("anthropic.json");
    let mut chain: Vec<Box<dyn CredentialProvider>> = Vec::new();

    let claude_code_resolution = claude_code_credential_path_resolution(
        config.credential.claude_code_credentials.as_deref(),
    );

    if cred_source == "claude-code"
        && let Some(ref resolution) = claude_code_resolution
    {
        if let Some(provider) = claude_code_provider(&resolution.path) {
            chain.push(provider);
        }
        // SAFETY: logs the resolved file path and source rule, never the credential value.
        info!(path = %resolution.path.display(), source = %resolution.source, "Claude Code credential path resolved");
    }

    if cred_file.exists()
        && let Some(cred) = CredentialFile::load(&cred_file)
    {
        if cred.has_refresh_token() {
            if let Some(refreshing) = RefreshingCredentialProvider::new(cred_file.clone()) {
                // SAFETY: logging file path, not credential value
                info!(path = %cred_file.display(), "credential file found (OAuth auto-refresh)"); // kanon:ignore SECURITY/credential-logging -- logs file path, not credential value
                chain.push(Box::new(refreshing));
            } else {
                // SAFETY: logging file path, not credential value
                info!(path = %cred_file.display(), "credential file found (static)"); // kanon:ignore SECURITY/credential-logging -- logs file path, not credential value
                chain.push(Box::new(FileCredentialProvider::new(cred_file.clone())));
            }
        } else {
            // SAFETY: logging file path, not credential value
            info!(path = %cred_file.display(), "credential file found (static API key)"); // kanon:ignore SECURITY/credential-logging -- logs file path, not credential value
            chain.push(Box::new(FileCredentialProvider::new(cred_file.clone())));
        }
    }

    // INVARIANT(#5250): explicit environment credentials must be checked
    // BEFORE the OS keyring. A keyring entry can be a stale leftover from
    // another install, account, or role sharing this machine; an operator
    // who exports ANTHROPIC_AUTH_TOKEN/ANTHROPIC_API_KEY for this run has
    // stated intent that a background keyring entry cannot override.
    chain.push(Box::new(EnvCredentialProvider::with_source(
        "ANTHROPIC_AUTH_TOKEN",
        CredentialSource::OAuth,
    )));
    chain.push(Box::new(EnvCredentialProvider::new("ANTHROPIC_API_KEY")));

    #[cfg(feature = "keyring")]
    {
        use symbolon::credential::KeyringCredentialProvider;
        warn_if_keyring_entry_is_shadowed_by_env(oikos);
        chain.push(Box::new(KeyringCredentialProvider::for_instance(
            oikos.root(),
            "anthropic",
        )));
    }

    // WHY(#5252): platform-dir discovery of Claude Code's private credential store is
    // OPT-IN ONLY (cred_source = "claude-code", env var, or an explicit configured path).
    // Under "auto", a PlatformConfigDir resolution means nothing was configured — an
    // unconfigured install must never implicitly read a co-installed agent's secrets.
    if cred_source == "auto"
        && let Some(ref resolution) = claude_code_resolution
        && auto_may_use_claude_code(resolution.source)
    {
        if let Some(provider) = claude_code_provider(&resolution.path) {
            chain.push(provider);
        }
        // SAFETY: logs the resolved file path and source rule, never the credential value.
        info!(path = %resolution.path.display(), source = %resolution.source, "Claude Code credential path resolved");
    }

    Arc::new(CredentialChain::new(chain))
}

/// Whether an explicit `ANTHROPIC_AUTH_TOKEN`/`ANTHROPIC_API_KEY` credential
/// is present in `env`. Pure and independent of the `keyring` feature so it
/// can be unit tested without touching a real OS keyring backend.
///
/// WHY the `test` arm: its only caller is `keyring`-gated, and `cargo check`
/// does not compile `#[cfg(test)]` code -- so without the feature this reads
/// as dead in the bin target while the tests that exercise it are invisible.
/// Gating on the feature alone would take the tests with it, which is the
/// independence the doc above is claiming.
#[cfg(any(feature = "keyring", test))]
fn any_anthropic_env_credential_present(env: &impl koina::system::Environment) -> bool {
    let is_set = |var: &str| env.var(var).is_some_and(|v| !v.is_empty());
    is_set("ANTHROPIC_AUTH_TOKEN") || is_set("ANTHROPIC_API_KEY")
}

/// Warn when an explicit environment credential is about to silently take
/// precedence over a credential already stored in this instance's keyring.
///
/// WHY(#5250): the chain now checks env before keyring, so once an env var
/// is set the keyring entry is never even probed by resolution — an
/// operator who unsets the env var later would see behavior change back to
/// the keyring value with no record of why. This check runs ONLY when an
/// env var is actually present, so it adds no keyring access (and no
/// unlock-prompt / D-Bus round trip) in the common case where none is set.
#[cfg(feature = "keyring")]
fn warn_if_keyring_entry_is_shadowed_by_env(oikos: &Oikos) {
    use koina::system::RealSystem;
    use symbolon::credential::KeyringCredentialProvider;

    if !any_anthropic_env_credential_present(&RealSystem) {
        return;
    }
    let keyring = KeyringCredentialProvider::for_instance(oikos.root(), "anthropic");
    if keyring.get_credential().is_some() {
        warn!(
            "an ANTHROPIC_AUTH_TOKEN/ANTHROPIC_API_KEY environment credential is set and \
             takes precedence; a credential is also stored in this instance's OS keyring \
             and is being ignored while the environment variable is set"
        );
    }
}

/// Build the complete provider registration plan in the exact order the
/// registry should see providers.
///
/// When `providers` is empty, this preserves the legacy single-Anthropic
/// startup path, including auto-discovered subprocess adapters. Once the
/// operator supplies any `[[providers]]` entries, that declarative list is the
/// complete routing contract; legacy Anthropic is only registered when an
/// `anthropic` entry appears and is placed at that entry's position.
fn build_provider_plan<'a>(
    config: &'a AletheiaConfig,
    #[cfg_attr(
        not(feature = "cc-provider"),
        expect(
            unused_variables,
            reason = "WHY(#4595): only the cc-provider auto-registration branch needs the credential source"
        )
    )]
    cred_source: &str,
    #[cfg_attr(
        not(feature = "cc-provider"),
        expect(
            unused_variables,
            reason = "WHY(#4595): only the cc-provider auto-registration branch needs the resolved credential source"
        )
    )]
    resolved_source: Option<&CredentialSource>,
) -> Vec<ProviderPlanEntry<'a>> {
    if !config.providers.is_empty() {
        return config
            .providers
            .iter()
            .map(ProviderPlanEntry::Declared)
            .collect();
    }

    let mut plan = Vec::new();

    #[cfg(feature = "kimi-provider")]
    plan.push(ProviderPlanEntry::AutoKimi);

    // WHY: Only auto-register CC on the legacy empty-provider path when the
    // credential source is not "api-key" AND the resolved credential is OAuth.
    // With declarative providers, `claude-code` must be listed explicitly so
    // it cannot bypass provider ordering.
    #[cfg(feature = "cc-provider")]
    if cred_source != "api-key" && resolved_source == Some(&CredentialSource::OAuth) {
        plan.push(ProviderPlanEntry::AutoClaudeCode);
    }

    #[cfg(feature = "codex-provider")]
    plan.push(ProviderPlanEntry::AutoCodex);

    plan.push(ProviderPlanEntry::LegacyAnthropic);
    plan
}

fn register_provider_plan(
    registry: &mut ProviderRegistry,
    plan: &[ProviderPlanEntry<'_>],
    oikos: &Oikos,
    credential_chain: &Arc<dyn CredentialProvider>,
    has_credential: bool,
    provider_config: &ProviderConfig,
    behavior: &ProviderBehavior,
) {
    for entry in plan {
        match entry {
            ProviderPlanEntry::Declared(entry) => register_declared_provider(
                registry,
                entry,
                oikos,
                credential_chain,
                has_credential,
                provider_config,
                behavior,
            ),
            ProviderPlanEntry::LegacyAnthropic => register_credential_chain_anthropic(
                registry,
                "anthropic",
                credential_chain,
                has_credential,
                provider_config,
                behavior,
            ),
            #[cfg(feature = "cc-provider")]
            ProviderPlanEntry::AutoClaudeCode => register_auto_claude_code(registry),
            #[cfg(feature = "codex-provider")]
            ProviderPlanEntry::AutoCodex => register_auto_codex(registry),
            #[cfg(feature = "kimi-provider")]
            ProviderPlanEntry::AutoKimi => register_auto_kimi(registry),
        }
    }
}

/// Translate the taxis-side [`taxis::config::DeploymentTarget`] to the
/// hermeneus-side [`HermeneusDeploymentTarget`] (#3736).
///
/// Both enums encode the same three boundaries — `Cloud`, `LocalHosted`,
/// `Embedded` — but live in separate crates so neither depends on the
/// other. This site is the first place both types are in scope, so the
/// mapping lives here alongside every other config→provider conversion
/// done by the provider registration plan. Any unknown variant
/// (`#[non_exhaustive]` guard) falls back to `Cloud`, the sovereignty-safe
/// default that strips `Internal` / `Confidential` facts rather than
/// leaking them to an unclassified boundary.
fn map_deployment_target(src: taxis::config::DeploymentTarget) -> HermeneusDeploymentTarget {
    use taxis::config::DeploymentTarget as TaxisDeploymentTarget;
    match src {
        TaxisDeploymentTarget::LocalHosted => HermeneusDeploymentTarget::LocalHosted,
        TaxisDeploymentTarget::Embedded => HermeneusDeploymentTarget::Embedded,
        // WHY: explicit Cloud + wildcard for `#[non_exhaustive]` — any
        // future variant this code hasn't been taught about is treated as
        // Cloud so operators cannot accidentally leak classified facts.
        TaxisDeploymentTarget::Cloud | _ => HermeneusDeploymentTarget::Cloud,
    }
}

fn map_openai_api_family(src: taxis::config::OpenAiApiFamily) -> HermeneusOpenAiApiFamily {
    use taxis::config::OpenAiApiFamily as TaxisOpenAiApiFamily;
    match src {
        TaxisOpenAiApiFamily::Responses => HermeneusOpenAiApiFamily::Responses,
        // WHY: future taxis variants should not silently move local
        // OpenAI-compatible endpoints onto a cloud-only wire contract.
        TaxisOpenAiApiFamily::ChatCompletions | _ => HermeneusOpenAiApiFamily::ChatCompletions,
    }
}

fn configured_openai_api_family(
    entry: &taxis::config::LlmProviderConfig,
) -> HermeneusOpenAiApiFamily {
    use taxis::config::ProviderKind;

    entry.api_family.map_or_else(
        || {
            if entry.kind == ProviderKind::OpenAi {
                HermeneusOpenAiApiFamily::Responses
            } else {
                HermeneusOpenAiApiFamily::ChatCompletions
            }
        },
        map_openai_api_family,
    )
}

/// Register one declarative `[[providers]]` entry at its exact list position.
fn register_declared_provider(
    registry: &mut ProviderRegistry,
    entry: &taxis::config::LlmProviderConfig,
    oikos: &Oikos,
    credential_chain: &Arc<dyn CredentialProvider>,
    has_credential: bool,
    provider_config: &ProviderConfig,
    behavior: &ProviderBehavior,
) {
    use taxis::config::ProviderKind;

    match entry.kind {
        ProviderKind::OpenAi | ProviderKind::OpenAiCompatible => {
            register_declared_openai(registry, entry, oikos, provider_config, behavior);
        }
        ProviderKind::Anthropic => register_declared_anthropic(
            registry,
            entry,
            credential_chain,
            has_credential,
            provider_config,
            behavior,
        ),
        ProviderKind::ClaudeCode => register_declared_claude_code(registry, entry, oikos),
        ProviderKind::CodexOauth => register_declared_codex(registry, entry, oikos),
        ProviderKind::Kimi => register_declared_kimi(registry, entry, oikos),
        // WHY: ProviderKind is #[non_exhaustive] so future additions
        // never accidentally break the build. Unknown variants fall
        // through to a clear operator warning.
        _ => {
            warn!(
                provider = %entry.name,
                "unknown provider kind in config — skipping"
            );
        }
    }
}

fn register_declared_openai(
    registry: &mut ProviderRegistry,
    entry: &taxis::config::LlmProviderConfig,
    oikos: &Oikos,
    provider_config: &ProviderConfig,
    behavior: &ProviderBehavior,
) {
    let Some(cfg) = openai_config_for_entry(entry, oikos, provider_config, behavior) else {
        return;
    };
    let api_family = cfg.api_family;

    match OpenAiProvider::new(cfg) {
        Ok(provider) => {
            info!(
                provider = %entry.name,
                target = ?entry.deployment_target,
                api_family = ?api_family,
                models = ?entry.models,
                "OpenAI provider registered"
            );
            registry.register(Box::new(provider));
        }
        Err(e) => warn!(
            provider = %entry.name,
            error = %e,
            "failed to init OpenAI-compatible provider"
        ),
    }
}

fn openai_config_for_entry(
    entry: &taxis::config::LlmProviderConfig,
    oikos: &Oikos,
    provider_config: &ProviderConfig,
    behavior: &ProviderBehavior,
) -> Option<OpenAiProviderConfig> {
    use taxis::config::ProviderKind;

    let api_family = configured_openai_api_family(entry);
    let base_url = if entry.kind == ProviderKind::OpenAi {
        entry
            .base_url
            .clone()
            .unwrap_or_else(|| OpenAiProviderConfig::default().base_url)
    } else if let Some(base_url) = entry.base_url.clone() {
        base_url
    } else {
        warn!(
            provider = %entry.name,
            "OpenAI-compatible provider missing base_url — skipping"
        );
        return None;
    };
    let api_key = openai_api_key(entry, oikos);
    Some(OpenAiProviderConfig {
        name: entry.name.clone(),
        base_url,
        api_key,
        models: entry.models.clone(),
        api_family,
        request_timeout: behavior.non_streaming_timeout,
        retry_policy: provider_config.retry_policy,
        concurrency: provider_config.concurrency.clone(),
        // WHY (#3736): the operator-declared deployment target
        // was previously logged below but never threaded to the
        // provider, so every OpenAI-compat provider silently
        // inherited the `Cloud` trait default. That broke the
        // air-gap claim in `docs/AIR-GAPPED.md` — the recall
        // filter stripped `Internal` / `Confidential` facts
        // from traffic bound for loopback llama.cpp / logismos.
        deployment_target: map_deployment_target(entry.deployment_target),
        ..OpenAiProviderConfig::default()
    })
}

fn openai_api_key(entry: &taxis::config::LlmProviderConfig, oikos: &Oikos) -> Option<SecretString> {
    use taxis::config::ProviderKind;

    if let Some(env_name) = entry.api_key_env.as_deref() {
        match std::env::var(env_name) {
            Ok(v) if !v.is_empty() => return Some(SecretString::from(v)),
            Ok(_) => {}
            Err(_) => {
                // WHY: missing env var is expected for loopback
                // llama.cpp / ollama (no auth required). Log at
                // debug, not warn.
                tracing::debug!(
                    provider = %entry.name,
                    env = env_name,
                    "api_key_env unset for OpenAI-compatible provider"
                );
            }
        }
    }

    if entry.kind != ProviderKind::OpenAi {
        return None;
    }

    let cred_path = oikos.credentials().join("openai.json");
    FileCredentialProvider::new(cred_path)
        .get_credential()
        .map(|credential| credential.secret)
}

/// Register a declarative Anthropic-protocol provider entry at list position.
///
/// An entry with `apiKeyEnv` is an independent static-key endpoint. An entry
/// without `apiKeyEnv` normalizes the legacy Anthropic credential chain into
/// the declarative provider list at this exact position.
fn register_declared_anthropic(
    registry: &mut ProviderRegistry,
    entry: &taxis::config::LlmProviderConfig,
    credential_chain: &Arc<dyn CredentialProvider>,
    has_credential: bool,
    provider_config: &ProviderConfig,
    behavior: &ProviderBehavior,
) {
    let Some(env_name) = entry.api_key_env.as_deref() else {
        let cfg = anthropic_config_for_entry(provider_config, entry, None);
        register_credential_chain_anthropic(
            registry,
            &entry.name,
            credential_chain,
            has_credential,
            &cfg,
            behavior,
        );
        return;
    };
    let key = match std::env::var(env_name) {
        Ok(v) if !v.is_empty() => SecretString::from(v),
        _ => {
            warn!(
                provider = %entry.name,
                env = env_name,
                "apiKeyEnv unset or empty for declarative Anthropic provider — skipping"
            );
            return;
        }
    };
    let cfg = anthropic_config_for_entry(provider_config, entry, Some(key));
    match AnthropicProvider::from_config(&cfg) {
        Ok(provider) => {
            info!(
                provider = %entry.name,
                target = ?entry.deployment_target,
                base_url = ?entry.base_url,
                models = ?entry.models,
                "Anthropic-protocol provider registered"
            );
            registry.register(Box::new(provider));
        }
        Err(e) => warn!(
            provider = %entry.name,
            error = %e,
            "failed to init declarative Anthropic provider"
        ),
    }
}

fn anthropic_config_for_entry(
    base: &ProviderConfig,
    entry: &taxis::config::LlmProviderConfig,
    api_key: Option<SecretString>,
) -> ProviderConfig {
    ProviderConfig {
        api_key,
        base_url: entry.base_url.clone(),
        name: Some(entry.name.clone()),
        models: entry.models.clone(),
        deployment_target: map_deployment_target(entry.deployment_target),
        ..base.clone()
    }
}

fn register_credential_chain_anthropic(
    registry: &mut ProviderRegistry,
    provider_name: &str,
    credential_chain: &Arc<dyn CredentialProvider>,
    has_credential: bool,
    provider_config: &ProviderConfig,
    behavior: &ProviderBehavior,
) {
    if !has_credential {
        warn!(
            provider = provider_name,
            "Anthropic provider skipped because no credential-chain credential is available"
        );
        return;
    }

    match AnthropicProvider::with_credential_provider_and_behavior(
        Arc::clone(credential_chain),
        provider_config,
        behavior,
    ) {
        Ok(provider) => {
            registry.register(Box::new(provider));
            info!(provider = provider_name, "Anthropic provider registered");
        }
        Err(e) => warn!(
            provider = provider_name,
            error = %e,
            "failed to init Anthropic provider"
        ),
    }
}

#[cfg(feature = "cc-provider")]
fn register_auto_claude_code(registry: &mut ProviderRegistry) {
    use hermeneus::cc::{CcProvider, CcProviderConfig};

    let cc_config = CcProviderConfig::default();
    match CcProvider::new(&cc_config) {
        Ok(provider) => {
            registry.register(Box::new(provider));
            // SAFETY: logging provider registration status, not credential value
            info!("CC subprocess provider registered (OAuth credential detected)"); // kanon:ignore SECURITY/credential-logging -- logs provider registration, no secret
        }
        Err(e) => {
            tracing::debug!(error = %e, "CC provider unavailable, falling back to direct API");
        }
    }
}

#[cfg(any(feature = "cc-provider", feature = "codex-provider"))]
fn configured_subprocess_path(oikos: &Oikos, path: Option<&Path>) -> Option<PathBuf> {
    path.map(|path| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            oikos.root().join(path)
        }
    })
}

#[cfg(any(feature = "cc-provider", feature = "codex-provider"))]
fn configured_subprocess_timeout(
    entry: &taxis::config::LlmProviderConfig,
    default: Duration,
) -> Duration {
    entry.timeout_secs.map_or(default, Duration::from_secs)
}

#[cfg(any(feature = "cc-provider", feature = "codex-provider"))]
fn default_model_for_entry(
    entry: &taxis::config::LlmProviderConfig,
    default_model: String,
) -> String {
    entry.models.first().cloned().unwrap_or(default_model)
}

#[cfg(feature = "cc-provider")]
fn register_declared_claude_code(
    registry: &mut ProviderRegistry,
    entry: &taxis::config::LlmProviderConfig,
    oikos: &Oikos,
) {
    use hermeneus::cc::{CcProvider, CcProviderConfig};

    let defaults = CcProviderConfig::default();
    let cc_config = CcProviderConfig {
        name: entry.name.clone(),
        cc_binary: configured_subprocess_path(oikos, entry.binary.as_deref()),
        working_directory: configured_subprocess_path(oikos, entry.workdir.as_deref()),
        models: entry.models.clone(),
        default_model: default_model_for_entry(entry, defaults.default_model),
        timeout: configured_subprocess_timeout(entry, defaults.timeout),
        deployment_target: map_deployment_target(entry.deployment_target),
    };
    match CcProvider::new(&cc_config) {
        Ok(provider) => {
            registry.register(Box::new(provider));
            // SAFETY: logging provider registration status, not credential value
            info!(provider = %entry.name, "CC subprocess provider registered"); // kanon:ignore SECURITY/credential-logging -- logs provider registration, no secret
        }
        Err(e) => {
            warn!(
                provider = %entry.name,
                error = %e,
                "CC provider unavailable"
            );
        }
    }
}

#[cfg(not(feature = "cc-provider"))]
fn register_declared_claude_code(
    _registry: &mut ProviderRegistry,
    entry: &taxis::config::LlmProviderConfig,
    _oikos: &Oikos,
) {
    warn!(
        provider = %entry.name,
        "ClaudeCode provider declared but cc-provider feature is disabled — skipping"
    );
}

#[cfg(feature = "codex-provider")]
fn register_auto_codex(registry: &mut ProviderRegistry) {
    use hermeneus::codex::{CodexProvider, CodexProviderConfig};

    let codex_config = CodexProviderConfig::default();
    match CodexProvider::new(&codex_config) {
        Ok(provider) => {
            registry.register(Box::new(provider));
            info!("Codex subprocess provider registered");
        }
        Err(e) => {
            tracing::debug!(error = %e, "Codex provider unavailable");
        }
    }
}

#[cfg(feature = "codex-provider")]
fn register_declared_codex(
    registry: &mut ProviderRegistry,
    entry: &taxis::config::LlmProviderConfig,
    oikos: &Oikos,
) {
    use hermeneus::codex::{CodexProvider, CodexProviderConfig};

    let defaults = CodexProviderConfig::default();
    let codex_config = CodexProviderConfig {
        name: entry.name.clone(),
        codex_binary: configured_subprocess_path(oikos, entry.binary.as_deref()),
        working_directory: configured_subprocess_path(oikos, entry.workdir.as_deref()),
        models: entry.models.clone(),
        default_model: default_model_for_entry(entry, defaults.default_model),
        timeout: configured_subprocess_timeout(entry, defaults.timeout),
        deployment_target: map_deployment_target(entry.deployment_target),
    };
    match CodexProvider::new(&codex_config) {
        Ok(provider) => {
            registry.register(Box::new(provider));
            info!(provider = %entry.name, "Codex subprocess provider registered");
        }
        Err(e) => {
            warn!(
                provider = %entry.name,
                error = %e,
                "Codex provider unavailable"
            );
        }
    }
}

#[cfg(not(feature = "codex-provider"))]
fn register_declared_codex(
    _registry: &mut ProviderRegistry,
    entry: &taxis::config::LlmProviderConfig,
    _oikos: &Oikos,
) {
    warn!(
        provider = %entry.name,
        "Codex OAuth provider declared but codex-provider feature is disabled — skipping"
    );
}

/// Register a `[[providers]]` entry of kind `kimi` from its declared settings.
///
/// WHY(#5258) this exists where `register_auto_kimi` already did: the auto path builds
/// `KimiProviderConfig::default()`, so `kimi_binary`, `working_directory`,
/// `default_model` and `timeout` were reachable in the struct and unreachable from
/// config. Claude Code and Codex have had a declared path for a while; kimi did not,
/// and `ProviderKind` had no variant to select one, so an operator could not say which
/// binary or working directory a kimi provider should use.
#[cfg(feature = "kimi-provider")]
fn register_declared_kimi(
    registry: &mut ProviderRegistry,
    entry: &taxis::config::LlmProviderConfig,
    oikos: &Oikos,
) {
    use hermeneus::kimi::{KimiProvider, KimiProviderConfig};

    let defaults = KimiProviderConfig::default();
    let kimi_config = KimiProviderConfig {
        name: entry.name.clone(),
        kimi_binary: configured_subprocess_path(oikos, entry.binary.as_deref()),
        working_directory: configured_subprocess_path(oikos, entry.workdir.as_deref()),
        default_model: default_model_for_entry(entry, defaults.default_model),
        timeout: configured_subprocess_timeout(entry, defaults.timeout),
    };
    match KimiProvider::new(&kimi_config) {
        Ok(provider) => {
            registry.register(Box::new(provider));
            // SAFETY: logging provider registration status, not credential value
            info!(provider = %entry.name, "Kimi subprocess provider registered"); // kanon:ignore SECURITY/credential-logging -- logs provider registration, no secret
        }
        Err(e) => {
            warn!(
                provider = %entry.name,
                error = %e,
                "Kimi provider unavailable"
            );
        }
    }
}

#[cfg(not(feature = "kimi-provider"))]
fn register_declared_kimi(
    _registry: &mut ProviderRegistry,
    entry: &taxis::config::LlmProviderConfig,
    _oikos: &Oikos,
) {
    warn!(
        provider = %entry.name,
        "Kimi provider declared but kimi-provider feature is disabled — skipping"
    );
}

#[cfg(feature = "kimi-provider")]
fn register_auto_kimi(registry: &mut ProviderRegistry) {
    use hermeneus::kimi::{KimiProvider, KimiProviderConfig};

    let kimi_config = KimiProviderConfig::default();
    match KimiProvider::new(&kimi_config) {
        Ok(provider) => {
            registry.register(Box::new(provider));
            // SAFETY: logging provider registration status, not credential value
            info!("Kimi subprocess provider registered"); // kanon:ignore SECURITY/credential-logging -- logs provider registration, no secret
        }
        Err(e) => {
            tracing::debug!(error = %e, "Kimi provider unavailable");
        }
    }
}

/// Lazily-initialized embedding provider.
///
/// WHY (#3474): the embedding model download and initialization can be slow
/// (network fetch, model load) or fail transiently. Loading synchronously
/// during server startup blocks the HTTP gateway from accepting health
/// checks, making the server appear dead. This wrapper defers the real
/// initialization to first use so the gateway can bind immediately.
pub(crate) struct LazyEmbeddingProvider {
    inner: tokio::sync::OnceCell<Arc<dyn EmbeddingProvider>>,
    /// Fallback provider returned before initialization completes.
    degraded: DegradedEmbeddingProvider,
    settings: EmbeddingSettings,
    dimension: usize,
}

fn degraded_embedding_provider(dimension: usize) -> Arc<dyn EmbeddingProvider> {
    Arc::new(DegradedEmbeddingProvider::new(dimension))
}

impl LazyEmbeddingProvider {
    pub(crate) fn new(settings: EmbeddingSettings) -> Self {
        let dimension = settings.dimension;
        Self {
            inner: tokio::sync::OnceCell::new(),
            degraded: DegradedEmbeddingProvider::new(dimension),
            settings,
            dimension,
        }
    }

    /// Returns the underlying provider, initializing on first call.
    ///
    /// If initialization fails, stores a `DegradedEmbeddingProvider` so
    /// subsequent calls do not retry a broken init path.
    pub(crate) async fn get(&self) -> &Arc<dyn EmbeddingProvider> {
        self.inner
            .get_or_init(|| async {
                let embedding_config =
                    match crate::embedding_config::runtime_embedding_config(&self.settings) {
                        Ok(config) => config,
                        Err(error) => {
                            warn!(
                                error = %error,
                                provider = %self.settings.provider,
                                "embedding provider config invalid: degraded mode \
                                 (recall and vector search unavailable)"
                            );
                            return degraded_embedding_provider(self.settings.dimension);
                        }
                    };
                #[expect(
                    clippy::as_conversions,
                    reason = "coercion to dyn EmbeddingProvider trait object: required for OnceCell<Arc<dyn Trait>>"
                )]
                match create_provider(&embedding_config) {
                    Ok(p) => {
                        info!(
                            provider = %self.settings.provider,
                            dim = self.settings.dimension,
                            "embedding provider initialized (lazy)"
                        );
                        Arc::from(p) as Arc<dyn EmbeddingProvider>
                    }
                    Err(e) => {
                        warn!(
                            error = %e,
                            provider = %self.settings.provider,
                            "embedding provider failed to load: degraded mode \
                             (recall and vector search unavailable)"
                        );
                        Arc::new(DegradedEmbeddingProvider::new(self.settings.dimension))
                            as Arc<dyn EmbeddingProvider>
                    }
                }
            })
            .await
    }

    /// Returns `true` if the provider has been initialized and is NOT degraded.
    #[expect(
        dead_code,
        reason = "diagnostic helper for callers that need to check provider readiness"
    )]
    fn is_ready(&self) -> bool {
        self.inner
            .get()
            .is_some_and(|p| !mneme::embedding::is_degraded_provider(p.as_ref()))
    }

    /// Returns `true` if initialization has started (provider present, degraded or not).
    #[expect(
        dead_code,
        reason = "diagnostic helper for callers that need to check init status"
    )]
    fn is_initialized(&self) -> bool {
        self.inner.get().is_some()
    }
}

impl EmbeddingProvider for LazyEmbeddingProvider {
    fn embed(&self, text: &str) -> std::result::Result<Vec<f32>, mneme::embedding::EmbeddingError> {
        match self.inner.get() {
            Some(provider) => provider.embed(text),
            // WHY: before init completes, delegate to the degraded provider
            // which returns a descriptive error for callers that need embeddings.
            None => self.degraded.embed(text),
        }
    }

    fn embed_batch(
        &self,
        texts: &[&str],
    ) -> std::result::Result<Vec<Vec<f32>>, mneme::embedding::EmbeddingError> {
        match self.inner.get() {
            Some(provider) => provider.embed_batch(texts),
            None => self.degraded.embed_batch(texts),
        }
    }

    fn dimension(&self) -> usize {
        self.inner.get().map_or(self.dimension, |p| p.dimension())
    }

    fn model_name(&self) -> &str {
        match self.inner.get() {
            Some(provider) => provider.model_name(),
            None => mneme::embedding::LOADING_MODEL_NAME,
        }
    }
}

#[cfg(feature = "recall")]
pub(super) fn open_knowledge_stores(
    oikos: &Oikos,
    cohorts: impl IntoIterator<Item = String>,
    embedding: &EmbeddingSettings,
    knowledge: &taxis::config::KnowledgeConfig,
) -> Result<std::collections::HashMap<String, Arc<mneme::knowledge_store::KnowledgeStore>>> {
    let mut stores = std::collections::HashMap::new();
    for cohort in cohorts {
        let kb_path = oikos.knowledge_cohort_db(&cohort);
        if let Some(parent) = kb_path.parent() {
            std::fs::create_dir_all(parent)
                .whatever_context("failed to CREATE knowledge store directory")?;
        }

        // WHY (aletheia#5779 F4): the verified pre-migration snapshot now
        // runs as the first statement of `KnowledgeStore::open_fjall` itself
        // (gated on whether a schema migration might actually run) — every
        // production caller is protected without needing its own explicit
        // call. See `episteme::knowledge_store::snapshot` module docs.
        let rules_dir = oikos.knowledge_cohort_rules_dir(&cohort);
        std::fs::create_dir_all(&rules_dir)
            .whatever_context("failed to CREATE knowledge rule directory")?;
        let mut knowledge_config = build_knowledge_config(embedding, knowledge, false);
        knowledge_config.rule_dir = Some(rules_dir);
        let store =
            match mneme::knowledge_store::KnowledgeStore::open_fjall(&kb_path, knowledge_config) {
                Ok(s) => s,
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("InvalidTag") || msg.contains("CompressionType") {
                        tracing::error!(
                            path = %kb_path.display(),
                            "Knowledge store format incompatible (written by older fjall version). \
                             Back up data/knowledge.fjall and delete it to start fresh. \
                             Session data in sessions.db is NOT affected."
                        );
                    }
                    return Err(e).whatever_context("failed to open knowledge store");
                }
            };
        mneme::trace_ingest::ensure_ops_schema(&store);
        info!(cohort = %cohort, path = %kb_path.display(), dim = embedding.dimension, "knowledge store opened (fjall)");
        stores.insert(cohort, store);
    }
    Ok(stores)
}

/// Build a `mneme::KnowledgeConfig` from the taxis knowledge config section.
///
/// WHY: separates the taxis config structs (serializable, TOML-friendly) from
/// the episteme runtime trait objects so neither crate depends on the other
/// directly. Threshold fields from the TOML cascade are forwarded into
/// [`mneme::admission::StructuredAdmissionConfig`] so operators can tune the
/// admission gate without recompiling.
#[cfg(feature = "recall")]
pub(super) fn build_knowledge_config(
    embedding: &EmbeddingSettings,
    knowledge: &taxis::config::KnowledgeConfig,
    allow_assumed_embedding_meta: bool,
) -> mneme::knowledge_store::KnowledgeConfig {
    let policy: Box<dyn mneme::admission::AdmissionPolicy> = match knowledge.admission_policy {
        taxis::config::AdmissionPolicyKind::Structured => {
            Box::new(mneme::admission::StructuredAdmissionPolicy::new(
                mneme::admission::StructuredAdmissionConfig {
                    threshold: knowledge.admission_threshold,
                    min_confidence: knowledge.admission_min_confidence,
                    content_hash_dedup: knowledge.admission_content_hash_dedup,
                    ..Default::default()
                },
            ))
        }
        // NOTE: Default and any future variants fall through to admit-all.
        _ => Box::new(mneme::admission::DefaultAdmissionPolicy),
    };
    let embedding_config = embedding.to_embedding_config();
    mneme::knowledge_store::KnowledgeConfig {
        dim: embedding.dimension,
        embedding_model: embedding_config.effective_model_name(),
        allow_assumed_embedding_meta,
        admission_policy: policy,
        // WHY: rule_dir is per-cohort (each cohort watches its own rule
        // directory), so the caller sets it after this shared config is built.
        rule_dir: None,
    }
}

pub(super) fn build_signal_provider(
    signal_config: &taxis::config::SignalConfig,
    messaging_config: &taxis::config::MessagingConfig,
) -> Option<Arc<SignalProvider>> {
    if !signal_config.enabled {
        info!("signal channel disabled");
        return None;
    }

    if signal_config.accounts.is_empty() {
        tracing::debug!("signal enabled but no accounts configured");
        return None;
    }

    let mut provider = SignalProvider::from_config(messaging_config);
    let rpc_timeout = std::time::Duration::from_secs(messaging_config.rpc_timeout_secs);
    let health_timeout = std::time::Duration::from_secs(messaging_config.health_timeout_secs);
    let receive_timeout = std::time::Duration::from_secs(messaging_config.receive_timeout_secs);
    for (account_id, account_cfg) in &signal_config.accounts {
        if !account_cfg.enabled {
            continue;
        }
        let Some(provider_account_id) = signal_provider_account_id(account_id, account_cfg) else {
            warn!(
                account = %account_id,
                "Signal account config has an empty account field; skipping account"
            );
            continue;
        };
        let cli_path = resolve_signal_cli_path(account_cfg.cli_path.as_deref());
        if account_cfg.cli_path.is_some() && cli_path.is_none() {
            warn!(
                account = %account_id,
                display_name = %signal_account_display_name(account_id, account_cfg),
                "configured signal-cli path is unavailable; skipping Signal account"
            );
            continue;
        }
        if account_cfg.cli_path.is_none() && cli_path.is_none() {
            tracing::debug!(
                account = %account_id,
                "signal-cli not found on PATH; assuming the JSON-RPC daemon is managed externally"
            );
        }
        let cli_path_label = cli_path
            .as_ref()
            .map_or_else(|| "external".to_owned(), |path| path.display().to_string());
        // WHY(#5199) no longer "SAFE: defaults to localhost": the default was never the
        // question -- an operator who overrode `httpHost` got plaintext to wherever they
        // pointed it. `SignalClient::with_timeouts` now refuses a non-loopback plaintext
        // host, and the Err arm below skips that account with a warning, exactly as a
        // bad `cli_path` is skipped.
        let base_url = format!("http://{}:{}", account_cfg.http_host, account_cfg.http_port);
        match SignalClient::with_timeouts(&base_url, rpc_timeout, health_timeout, receive_timeout) {
            Ok(client) => {
                provider.add_account(provider_account_id, client, account_cfg.auto_start);
                info!(
                    account = %account_id,
                    display_name = %signal_account_display_name(account_id, account_cfg),
                    cli_path = %cli_path_label,
                    auto_start = account_cfg.auto_start,
                    "signal account added"
                );
            }
            Err(e) => {
                warn!(account = %account_id, error = %e, "failed to CREATE signal client");
            }
        }
    }

    Some(Arc::new(provider))
}

fn signal_provider_account_id(
    account_id: &str,
    account_cfg: &taxis::config::SignalAccountConfig,
) -> Option<String> {
    match account_cfg.account.as_deref() {
        Some(account) if account.trim().is_empty() => None,
        Some(account) => Some(account.to_owned()),
        None => Some(account_id.to_owned()),
    }
}

fn signal_account_display_name<'a>(
    account_id: &'a str,
    account_cfg: &'a taxis::config::SignalAccountConfig,
) -> &'a str {
    account_cfg
        .name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(account_id)
}

fn resolve_signal_cli_path(configured: Option<&Path>) -> Option<PathBuf> {
    match configured {
        Some(path) if path.as_os_str().is_empty() => None,
        Some(path) if path.is_file() => Some(path.to_path_buf()),
        Some(path) if path.components().count() == 1 => find_on_path(path),
        Some(_) => None,
        None => find_on_path(Path::new("signal-cli")),
    }
}

fn find_on_path(command: &Path) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(command))
        .find(|candidate| candidate.is_file())
}

pub(super) fn build_matrix_provider(
    matrix_config: &taxis::config::MatrixConfig,
    messaging_config: &taxis::config::MessagingConfig,
) -> Option<Arc<MatrixProvider>> {
    if !matrix_config.enabled {
        info!("Matrix channel disabled");
        return None;
    }

    if matrix_config.accounts.is_empty() {
        tracing::debug!("Matrix enabled but no accounts configured");
        return None;
    }

    let mut provider = MatrixProvider::from_config(messaging_config);
    let rpc_timeout = std::time::Duration::from_secs(messaging_config.rpc_timeout_secs);
    let receive_timeout = std::time::Duration::from_secs(messaging_config.receive_timeout_secs);

    for (account_id, account_cfg) in &matrix_config.accounts {
        if !account_cfg.enabled {
            continue;
        }
        let access_token = match std::env::var(&account_cfg.access_token_env) {
            Ok(token) if !token.is_empty() => token,
            Ok(_) => {
                warn!(
                    account = %account_id,
                    env = %account_cfg.access_token_env,
                    "Matrix access token environment variable is empty"
                );
                continue;
            }
            Err(e) => {
                warn!(
                    account = %account_id,
                    env = %account_cfg.access_token_env,
                    error = %e,
                    "Matrix access token environment variable is unavailable"
                );
                continue;
            }
        };

        match MatrixClient::with_timeouts(
            &account_cfg.homeserver,
            &access_token,
            rpc_timeout,
            receive_timeout,
        ) {
            Ok(client) => {
                provider.add_account(
                    account_id.clone(),
                    client,
                    account_cfg.user_id.clone(),
                    account_cfg.auto_start,
                    account_cfg.initial_since.clone(),
                );
                info!(account = %account_id, auto_start = account_cfg.auto_start, "Matrix account added");
            }
            Err(e) => {
                warn!(account = %account_id, error = %e, "failed to CREATE Matrix client");
            }
        }
    }

    Some(Arc::new(provider))
}

pub(super) fn start_inbound_dispatch(
    config: &AletheiaConfig,
    nous_manager: &Arc<NousManager>,
    session_store: Arc<Mutex<SessionStore>>,
    ready_rx: tokio::sync::watch::Receiver<bool>,
    signal_provider: Option<&Arc<SignalProvider>>,
    matrix_provider: Option<&Arc<MatrixProvider>>,
    shutdown_token: &CancellationToken,
) -> Result<(Arc<ChannelRegistry>, Option<tokio::task::JoinHandle<()>>)> {
    let mut channel_registry =
        ChannelRegistry::new().with_outbound_policy(config.messaging.outbound.clone());
    let mut listen_providers: Vec<&dyn ChannelProvider> = Vec::new();

    if let Some(provider) = signal_provider {
        #[expect(
            clippy::as_conversions,
            reason = "coercion to dyn ChannelProvider trait object: required by registry API"
        )]
        register_channel_provider(
            &mut channel_registry,
            Arc::clone(provider) as Arc<dyn ChannelProvider>,
        )?;
        listen_providers.push(provider.as_ref());
    }
    if let Some(provider) = matrix_provider {
        #[expect(
            clippy::as_conversions,
            reason = "coercion to dyn ChannelProvider trait object: required by registry API"
        )]
        register_channel_provider(
            &mut channel_registry,
            Arc::clone(provider) as Arc<dyn ChannelProvider>,
        )?;
        listen_providers.push(provider.as_ref());
    }
    let channel_registry = Arc::new(channel_registry);

    let handle = if listen_providers.is_empty() {
        None
    } else {
        let poll_interval = Some(std::time::Duration::from_millis(
            config.messaging.poll_interval_ms,
        ));
        let listener = ChannelListener::start_many_with_config(
            listen_providers,
            poll_interval,
            &shutdown_token.child_token(),
            config.messaging.max_concurrent_handlers,
        );
        info!("channel listeners started");
        let (rx, _poll_handles) = listener.into_receiver();

        let default_nous_id = resolve_default_nous_id(&config.agents.list);
        let router = Arc::new(MessageRouter::new(config.bindings.clone(), default_nous_id));

        Some(crate::dispatch::spawn_dispatcher(
            rx,
            router,
            Arc::clone(nous_manager),
            Arc::clone(&channel_registry),
            session_store,
            ready_rx,
        ))
    };

    Ok((channel_registry, handle))
}

/// Resolve the global-default nous for channel senders that miss every
/// group, source, and channel-wildcard binding.
///
/// INVARIANT: only an agent the operator has explicitly marked
/// `default = true` may catch unmatched senders. There is deliberately no
/// "first configured agent" fallback — that fallback made an explicit
/// source-binding allowlist unenforceable, because any sender the operator
/// never listed still reached whichever agent happened to be declared
/// first. Absent an explicit default, `MessageRouter` falls through to its
/// documented "no match" state and the caller drops the message.
fn resolve_default_nous_id(agents: &[taxis::config::NousDefinition]) -> Option<String> {
    agents.iter().find(|a| a.default).map(|a| a.id.to_string())
}

fn register_channel_provider(
    channel_registry: &mut ChannelRegistry,
    provider: Arc<dyn ChannelProvider>,
) -> Result<()> {
    let channel_id = provider.id().to_owned();
    channel_registry
        .register(provider)
        .with_whatever_context(|_| format!("failed to register channel provider '{channel_id}'"))
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::error::Error as _;
    use std::ffi::OsString;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Mutex, MutexGuard};

    use agora::types::{ChannelCapabilities, InboundMessage, ProbeResult, SendParams, SendResult};
    use taxis::config::{MessagingConfig, SignalAccountConfig, SignalConfig};
    use tokio::sync::mpsc;
    use tokio::task::JoinSet;

    use super::*;

    static TEST_CAPABILITIES: ChannelCapabilities = ChannelCapabilities {
        threads: false,
        reactions: false,
        typing: false,
        media: false,
        streaming: false,
        rich_formatting: false,
        max_text_length: 1000,
    };

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvVarGuard {
        originals: Vec<(&'static str, Option<OsString>)>,
        _lock: MutexGuard<'static, ()>,
    }

    impl EnvVarGuard {
        #[expect(
            unsafe_code,
            reason = "std::env::set_var is unsafe in edition 2024; tests serialize env access with ENV_LOCK"
        )]
        fn set(key: &'static str, value: &str) -> Self {
            let lock = ENV_LOCK.lock().expect("lock env var mutex");
            let original = std::env::var_os(key);
            // SAFETY: ENV_LOCK serializes all test env mutations in this module.
            unsafe { std::env::set_var(key, value) };
            Self {
                originals: vec![(key, original)],
                _lock: lock,
            }
        }

        #[expect(
            unsafe_code,
            reason = "std::env::remove_var is unsafe in edition 2024; tests serialize env access with ENV_LOCK"
        )]
        fn remove(key: &'static str) -> Self {
            let lock = ENV_LOCK.lock().expect("lock env var mutex");
            let original = std::env::var_os(key);
            // SAFETY: ENV_LOCK serializes all test env mutations in this module.
            unsafe { std::env::remove_var(key) };
            Self {
                originals: vec![(key, original)],
                _lock: lock,
            }
        }

        #[expect(
            unsafe_code,
            reason = "std::env::{set_var,remove_var} are unsafe in edition 2024; tests serialize env access with ENV_LOCK"
        )]
        fn set_and_remove_many(
            set_key: &'static str,
            value: &str,
            remove_keys: &[&'static str],
        ) -> Self {
            let lock = ENV_LOCK.lock().expect("lock env var mutex");
            let mut originals = Vec::with_capacity(remove_keys.len() + 1);
            originals.push((set_key, std::env::var_os(set_key)));
            // SAFETY: ENV_LOCK serializes all test env mutations in this module.
            unsafe { std::env::set_var(set_key, value) };
            for &key in remove_keys {
                originals.push((key, std::env::var_os(key)));
                // SAFETY: ENV_LOCK serializes all test env mutations in this module.
                unsafe { std::env::remove_var(key) };
            }
            Self {
                originals,
                _lock: lock,
            }
        }
    }

    #[expect(
        unsafe_code,
        reason = "std::env::{set_var,remove_var} are unsafe in edition 2024; tests serialize env access with ENV_LOCK"
    )]
    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            for (key, original) in self.originals.drain(..).rev() {
                match original {
                    Some(value) => {
                        // SAFETY: ENV_LOCK is held until this guard is dropped.
                        unsafe { std::env::set_var(key, value) };
                    }
                    None => {
                        // SAFETY: ENV_LOCK is held until this guard is dropped.
                        unsafe { std::env::remove_var(key) };
                    }
                }
            }
        }
    }

    struct TestProvider {
        id: &'static str,
    }

    impl ChannelProvider for TestProvider {
        fn id(&self) -> &str {
            self.id
        }

        fn name(&self) -> &str {
            self.id
        }

        fn capabilities(&self) -> &ChannelCapabilities {
            &TEST_CAPABILITIES
        }

        fn send<'a>(
            &'a self,
            _params: &'a SendParams,
        ) -> Pin<Box<dyn Future<Output = SendResult> + Send + 'a>> {
            Box::pin(async { SendResult::ok() })
        }

        fn listen(
            &self,
            _poll_interval: Option<std::time::Duration>,
            _cancel: CancellationToken,
        ) -> (mpsc::Receiver<InboundMessage>, JoinSet<()>) {
            let (_tx, rx) = mpsc::channel(1);
            (rx, JoinSet::new())
        }

        fn probe<'a>(&'a self) -> Pin<Box<dyn Future<Output = ProbeResult> + Send + 'a>> {
            Box::pin(async {
                ProbeResult {
                    ok: true,
                    latency_ms: None,
                    error: None,
                    details: None,
                }
            })
        }
    }

    #[test]
    fn register_channel_provider_surfaces_duplicate_errors() {
        let mut registry = ChannelRegistry::new();
        register_channel_provider(&mut registry, Arc::new(TestProvider { id: "signal" }))
            .expect("first registration succeeds");

        let error =
            register_channel_provider(&mut registry, Arc::new(TestProvider { id: "signal" }))
                .expect_err("duplicate registration should fail");

        let message = error.to_string();
        assert!(message.contains("failed to register channel provider 'signal'"));
        let source = error.source().expect("duplicate channel source");
        assert!(source.to_string().contains("duplicate channel: signal"));
    }

    #[test]
    fn build_signal_provider_uses_configured_signal_account() {
        organon::testing::install_crypto_provider();
        let mut signal = SignalConfig::default();
        signal.accounts.insert(
            "default".to_owned(),
            SignalAccountConfig {
                name: Some("Primary Signal".to_owned()),
                account: Some("+15551234567".to_owned()), // pii-allow: synthetic Signal test number
                cli_path: Some(std::env::current_exe().expect("current test binary path")),
                ..SignalAccountConfig::default()
            },
        );

        let provider = build_signal_provider(&signal, &MessagingConfig::default())
            .expect("Signal provider should build");
        let debug = format!("{provider:?}");

        assert!(
            debug.contains("+15551234567"), // pii-allow: synthetic Signal test number
            "configured Signal account should become the provider account: {debug}"
        );
        assert!(
            !debug.contains("default_account: Some(\"default\")"),
            "provider should not send the account label as the signal-cli account: {debug}"
        );
    }

    #[test]
    fn build_signal_provider_skips_bad_configured_cli_path() {
        let mut signal = SignalConfig::default();
        signal.accounts.insert(
            "default".to_owned(),
            SignalAccountConfig {
                cli_path: Some(PathBuf::from(
                    "/definitely/missing/aletheia-test-signal-cli",
                )),
                ..SignalAccountConfig::default()
            },
        );

        let provider = build_signal_provider(&signal, &MessagingConfig::default())
            .expect("Signal provider should still build");
        let debug = format!("{provider:?}");

        assert!(
            debug.contains("accounts: []"),
            "bad configured cli_path should keep the account out of the provider: {debug}"
        );
    }

    #[test]
    fn openai_api_family_mapping_and_defaults_are_explicit() {
        use taxis::config::{DeploymentTarget, LlmProviderConfig, ProviderKind};

        let mut entry = LlmProviderConfig {
            name: "openai-cloud".to_owned(),
            kind: ProviderKind::OpenAi,
            base_url: None,
            api_key_env: None,
            api_family: None,
            binary: None,
            workdir: None,
            timeout_secs: None,
            deployment_target: DeploymentTarget::Cloud,
            models: vec!["gpt-5".to_owned()],
        };
        assert_eq!(
            configured_openai_api_family(&entry),
            HermeneusOpenAiApiFamily::Responses
        );

        entry.kind = ProviderKind::OpenAiCompatible;
        entry.base_url = Some("http://127.0.0.1:8088/v1".to_owned());
        assert_eq!(
            configured_openai_api_family(&entry),
            HermeneusOpenAiApiFamily::ChatCompletions
        );
    }

    fn local_openai_provider(name: &str, model: &str) -> taxis::config::LlmProviderConfig {
        use taxis::config::{DeploymentTarget, LlmProviderConfig, ProviderKind};

        LlmProviderConfig {
            name: name.to_owned(),
            kind: ProviderKind::OpenAiCompatible,
            base_url: Some("http://127.0.0.1:8088/v1".to_owned()),
            api_key_env: None,
            api_family: None,
            binary: None,
            workdir: None,
            timeout_secs: None,
            deployment_target: DeploymentTarget::Embedded,
            models: vec![model.to_owned()],
        }
    }

    fn credential_chain_anthropic_provider(
        name: &str,
        model: &str,
    ) -> taxis::config::LlmProviderConfig {
        use taxis::config::{DeploymentTarget, LlmProviderConfig, ProviderKind};

        LlmProviderConfig {
            name: name.to_owned(),
            kind: ProviderKind::Anthropic,
            base_url: None,
            api_key_env: None,
            api_family: None,
            binary: None,
            workdir: None,
            timeout_secs: None,
            deployment_target: DeploymentTarget::Cloud,
            models: vec![model.to_owned()],
        }
    }

    fn cloud_openai_provider(name: &str, model: &str) -> taxis::config::LlmProviderConfig {
        use taxis::config::{DeploymentTarget, LlmProviderConfig, OpenAiApiFamily, ProviderKind};

        LlmProviderConfig {
            name: name.to_owned(),
            kind: ProviderKind::OpenAi,
            base_url: None,
            api_key_env: Some("ALETHEIA_TEST_OPENAI_API_KEY".to_owned()),
            api_family: Some(OpenAiApiFamily::Responses),
            binary: None,
            workdir: None,
            timeout_secs: None,
            deployment_target: DeploymentTarget::Cloud,
            models: vec![model.to_owned()],
        }
    }

    #[cfg(feature = "cc-provider")]
    fn claude_code_provider(
        name: &str,
        model: &str,
        binary: PathBuf,
        workdir: PathBuf,
    ) -> taxis::config::LlmProviderConfig {
        use taxis::config::{DeploymentTarget, LlmProviderConfig, ProviderKind};

        LlmProviderConfig {
            name: name.to_owned(),
            kind: ProviderKind::ClaudeCode,
            base_url: None,
            api_key_env: None,
            api_family: None,
            binary: Some(binary),
            workdir: Some(workdir),
            timeout_secs: Some(30),
            deployment_target: DeploymentTarget::Cloud,
            models: vec![model.to_owned()],
        }
    }

    fn build_test_provider_registry(config: &AletheiaConfig) -> ProviderRegistry {
        let oikos_dir = tempfile::tempdir().expect("create temp oikos");
        let oikos = Oikos::from_root(oikos_dir.path());
        build_provider_registry(config, &oikos)
    }

    #[test]
    fn taxis_retry_and_provider_behavior_map_to_openai_runtime_config() {
        let mut config = AletheiaConfig::default();
        config.retry.max_attempts = 2;
        config.retry.backoff_base_ms = 250;
        config.retry.backoff_max_ms = 4_000;
        config.provider_behavior.non_streaming_timeout_secs = 17;
        config.provider_behavior.concurrency_ewma_alpha = 0.25;
        config.provider_behavior.concurrency_latency_threshold_secs = 3.5;

        let provider_config = ProviderConfig {
            retry_policy: retry_policy_from_config(&config),
            concurrency: concurrency_config_from_provider_behavior(&config.provider_behavior),
            ..ProviderConfig::default()
        };
        let behavior = ProviderBehavior {
            non_streaming_timeout: std::time::Duration::from_secs(
                config.provider_behavior.non_streaming_timeout_secs,
            ),
            sse_retry_ms: config.provider_behavior.sse_default_retry_ms,
        };
        let oikos_dir = tempfile::tempdir().expect("create temp oikos");
        let oikos = Oikos::from_root(oikos_dir.path());
        let entry = local_openai_provider("local-runtime", "qwen-runtime");

        let openai_config = openai_config_for_entry(&entry, &oikos, &provider_config, &behavior)
            .expect("local OpenAI-compatible config should build");

        assert_eq!(
            openai_config.request_timeout,
            std::time::Duration::from_secs(17),
            "providerBehavior.nonStreamingTimeoutSecs must reach OpenAI providers"
        );
        assert_eq!(openai_config.retry_policy.max_retries, 2);
        assert_eq!(openai_config.retry_policy.backoff_base_ms, 250);
        assert_eq!(openai_config.retry_policy.backoff_max_ms, 4_000);
        assert!(
            (openai_config.concurrency.ewma_alpha - 0.25).abs() < f64::EPSILON,
            "providerBehavior.concurrencyEwmaAlpha must reach the limiter config"
        );
        assert!(
            (openai_config.concurrency.latency_threshold_secs - 3.5).abs() < f64::EPSILON,
            "providerBehavior.concurrencyLatencyThresholdSecs must reach the limiter config"
        );
    }

    #[test]
    fn declared_provider_order_wins_before_credential_chain_anthropic() {
        let _env = EnvVarGuard::set("ANTHROPIC_API_KEY", "sk-test-123");
        let model = koina::models::names::sonnet();
        let mut config = AletheiaConfig::default();
        config.credential.source = "api-key".to_owned();
        config.providers = vec![
            local_openai_provider("local-claude", model),
            credential_chain_anthropic_provider("anthropic-cloud", model),
        ];

        let registry = build_test_provider_registry(&config);
        let provider = registry
            .find_provider(model)
            .expect("equal-specificity model should resolve");

        assert_eq!(
            provider.name(),
            "local-claude",
            "the provider declared first must win equal-specificity routing"
        );
    }

    #[test]
    fn credential_chain_anthropic_keeps_its_declared_order_position() {
        let _env = EnvVarGuard::set("ANTHROPIC_API_KEY", "sk-test-123");
        let model = koina::models::names::sonnet();
        let mut config = AletheiaConfig::default();
        config.credential.source = "api-key".to_owned();
        config.providers = vec![
            credential_chain_anthropic_provider("anthropic-cloud", model),
            local_openai_provider("local-claude", model),
        ];

        let registry = build_test_provider_registry(&config);
        let provider = registry
            .find_provider(model)
            .expect("equal-specificity model should resolve");

        assert_eq!(
            provider.name(),
            "anthropic-cloud",
            "credential-chain Anthropic must win only when declared first"
        );
    }

    #[test]
    fn declared_local_provider_registers_without_anthropic_credential() {
        let _env = EnvVarGuard::remove("ANTHROPIC_API_KEY");
        let model = "local-test-model";
        let mut config = AletheiaConfig::default();
        config.credential.source = "api-key".to_owned();
        config.providers = vec![local_openai_provider("local-only", model)];

        let registry = build_test_provider_registry(&config);
        let provider = registry
            .find_provider(model)
            .expect("declared local provider should register without Anthropic credentials");

        assert_eq!(provider.name(), "local-only");
    }

    #[cfg(feature = "cc-provider")]
    #[expect(
        clippy::disallowed_methods,
        reason = "WHY(#4889): test fixture writes a fake Claude Code CLI binary under a temp dir"
    )]
    #[test]
    fn declared_claude_code_routes_configured_model_with_declared_identity() {
        let fixture = tempfile::tempdir().expect("create fake subprocess fixture");
        let binary = fixture.path().join("claude");
        std::fs::write(&binary, "#!/bin/sh\n").expect("write fake Claude Code binary");
        let model = "team-claude";
        let config = AletheiaConfig {
            providers: vec![claude_code_provider(
                "cc-seat",
                model,
                binary,
                fixture.path().to_path_buf(),
            )],
            ..AletheiaConfig::default()
        };

        let registry = build_test_provider_registry(&config);
        let provider = registry
            .find_provider(model)
            .expect("declared Claude Code provider should claim its configured model");

        assert_eq!(provider.name(), "cc-seat");
        assert!(
            registry
                .find_provider("claude-future-family-model")
                .is_none(),
            "declared subprocess models should not silently become broad claude-* catchalls"
        );
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "test fixture writes a synthetic OpenAI credential file under a temp instance"
    )]
    #[test]
    fn declared_openai_uses_provider_credential_file() {
        let _env = EnvVarGuard::remove("ALETHEIA_TEST_OPENAI_API_KEY");
        let oikos_dir = tempfile::tempdir().expect("create temp oikos");
        let credential_dir = oikos_dir.path().join("config/credentials");
        std::fs::create_dir_all(&credential_dir).expect("create credential dir");
        std::fs::write(
            credential_dir.join("openai.json"),
            r#"{"token":"sk-openai-test"}"#,
        )
        .expect("write OpenAI credential");

        let model = "gpt-5";
        let config = AletheiaConfig {
            providers: vec![cloud_openai_provider("openai-cloud", model)],
            ..AletheiaConfig::default()
        };
        let oikos = Oikos::from_root(oikos_dir.path());

        let registry = build_provider_registry(&config, &oikos);
        let provider = registry
            .find_provider(model)
            .expect("OpenAI provider should register from config/credentials/openai.json");

        assert_eq!(provider.name(), "openai-cloud");
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "test fixture writes a fake Claude Code credential file under a temp HOME"
    )]
    #[test]
    fn declared_local_provider_does_not_touch_claude_code_refresh_credentials() {
        let fake_config = tempfile::tempdir().expect("create fake config dir");
        let claude_dir = fake_config.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).expect("create fake claude dir");
        std::fs::write(
            claude_dir.join(".credentials.json"),
            r#"{"accessToken":"sk-ant-oat-local","refreshToken":"rt-local"}"#,
        )
        .expect("write fake Claude Code credentials");
        let fake_config = fake_config
            .path()
            .to_str()
            .expect("temp config path should be utf-8");
        let _env = EnvVarGuard::set_and_remove_many(
            "XDG_CONFIG_HOME",
            fake_config,
            &["ANTHROPIC_API_KEY", "CLAUDE_CODE_CREDS"],
        );
        let model = "local-test-model";
        let config = AletheiaConfig {
            providers: vec![local_openai_provider("local-only", model)],
            ..AletheiaConfig::default()
        };

        let registry = build_test_provider_registry(&config);
        let provider = registry
            .find_provider(model)
            .expect("declared local provider should register without legacy credential discovery");

        assert_eq!(provider.name(), "local-only");
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "test fixture writes a fake Claude Code credential file under a temp config dir"
    )]
    #[test]
    fn auto_source_ignores_platform_config_dir_credentials() {
        let config_dir = tempfile::tempdir().expect("create fake config dir");
        let claude_dir = config_dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).expect("create fake claude dir");
        std::fs::write(
            claude_dir.join(".credentials.json"),
            r#"{"token":"sk-ant-api-from-cc"}"#,
        )
        .expect("write fake Claude Code credentials");
        let config_dir = config_dir
            .path()
            .to_str()
            .expect("temp config path should be utf-8");
        let _env = EnvVarGuard::set_and_remove_many(
            "XDG_CONFIG_HOME",
            config_dir,
            &[
                "ANTHROPIC_API_KEY",
                "ANTHROPIC_AUTH_TOKEN",
                "CLAUDE_CODE_CREDS",
                "HOME",
            ],
        );
        let config = AletheiaConfig::default();
        let oikos_dir = tempfile::tempdir().expect("create temp oikos");
        let oikos = Oikos::from_root(oikos_dir.path());

        // WHY(#5252): under "auto" with nothing configured, a co-installed
        // Claude Code agent's private store must NOT be discovered. The chain
        // may still resolve other sources; it must never yield this token.
        let chain = build_anthropic_credential_chain(&config, &oikos, "auto");
        if let Some(credential) = chain.get_credential() {
            assert_ne!(
                credential.secret.expose_secret(),
                "sk-ant-api-from-cc",
                "auto must not read the platform config-dir Claude Code store"
            );
        }

        let explicit = build_anthropic_credential_chain(&config, &oikos, "claude-code");
        let credential = explicit
            .get_credential()
            .expect("explicit claude-code source resolves the platform default");
        assert_eq!(credential.secret.expose_secret(), "sk-ant-api-from-cc");
        assert_eq!(credential.source, CredentialSource::File);
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "test fixture writes a fake Claude Code credential file under a temp config dir"
    )]
    #[test]
    fn claude_code_source_resolves_when_home_is_missing() {
        let config_dir = tempfile::tempdir().expect("create fake config dir");
        let claude_dir = config_dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).expect("create fake claude dir");
        std::fs::write(
            claude_dir.join(".credentials.json"),
            r#"{"token":"sk-ant-api-no-home"}"#,
        )
        .expect("write fake Claude Code credentials");
        let config_dir = config_dir
            .path()
            .to_str()
            .expect("temp config path should be utf-8");
        let _env = EnvVarGuard::set_and_remove_many(
            "XDG_CONFIG_HOME",
            config_dir,
            &[
                "ANTHROPIC_API_KEY",
                "ANTHROPIC_AUTH_TOKEN",
                "CLAUDE_CODE_CREDS",
                "HOME",
            ],
        );
        let config = AletheiaConfig::default();
        let oikos_dir = tempfile::tempdir().expect("create temp oikos");
        let oikos = Oikos::from_root(oikos_dir.path());

        // WHY(#5252): missing-HOME portability is exercised through the
        // explicit opt-in source; "auto" must not discover this store at all.
        let auto_chain = build_anthropic_credential_chain(&config, &oikos, "auto");
        if let Some(credential) = auto_chain.get_credential() {
            assert_ne!(
                credential.secret.expose_secret(),
                "sk-ant-api-no-home",
                "auto must not read the platform config-dir Claude Code store"
            );
        }

        let chain = build_anthropic_credential_chain(&config, &oikos, "claude-code");
        let credential = chain
            .get_credential()
            .expect("credential discovery must work when HOME is missing");

        assert_eq!(credential.secret.expose_secret(), "sk-ant-api-no-home");
        assert_eq!(credential.source, CredentialSource::File);
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "test fixture writes a fake Claude Code credential file under a temp dir"
    )]
    #[test]
    fn auto_source_uses_explicit_claude_code_creds_env_path() {
        let fixture = tempfile::tempdir().expect("create fake credential fixture");
        let cc_path = fixture.path().join("claude-code.json");
        std::fs::write(&cc_path, r#"{"token":"sk-ant-api-from-cc"}"#)
            .expect("write fake Claude Code credentials");
        let cc_path = cc_path.to_str().expect("temp path should be utf-8");
        let _env = EnvVarGuard::set_and_remove_many(
            "CLAUDE_CODE_CREDS",
            cc_path,
            &["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"],
        );
        let config = AletheiaConfig::default();
        let oikos_dir = tempfile::tempdir().expect("create temp oikos");
        let oikos = Oikos::from_root(oikos_dir.path());

        let chain = build_anthropic_credential_chain(&config, &oikos, "auto");
        let credential = chain
            .get_credential()
            .expect("explicit Claude Code credential path should resolve");

        assert_eq!(credential.secret.expose_secret(), "sk-ant-api-from-cc");
        assert_eq!(credential.source, CredentialSource::File);
    }

    #[test]
    fn declarative_anthropic_with_own_key_registers() {
        use taxis::config::{DeploymentTarget, LlmProviderConfig, ProviderKind};

        let _env = EnvVarGuard::set("TEST_DECL_ANTHROPIC_KEY", "sk-test-123");
        let mut config = AletheiaConfig::default();
        config.credential.source = "api-key".to_owned();
        config.providers.push(LlmProviderConfig {
            name: "kimi-coding".to_owned(),
            kind: ProviderKind::Anthropic,
            base_url: Some("https://compat.api.example.com".to_owned()),
            api_key_env: Some("TEST_DECL_ANTHROPIC_KEY".to_owned()),
            api_family: None,
            binary: None,
            workdir: None,
            timeout_secs: None,
            deployment_target: DeploymentTarget::Cloud,
            models: vec!["kimi-for-coding".to_owned()],
        });
        let registry = build_test_provider_registry(&config);
        assert!(
            registry.find_provider("kimi-for-coding").is_some(),
            "declarative Anthropic-protocol entry with its own key must register and claim its models"
        );
        assert!(
            registry
                .find_provider(koina::models::names::opus())
                .is_none(),
            "#5874: an operator-declared endpoint (explicit models list) must NOT catch undeclared claude-* traffic (endpoint isolation)"
        );
    }

    #[test]
    fn declarative_anthropic_without_key_env_needs_credential_chain() {
        use taxis::config::{DeploymentTarget, LlmProviderConfig, ProviderKind};

        let _env = EnvVarGuard::remove("ANTHROPIC_API_KEY");
        let mut config = AletheiaConfig::default();
        config.credential.source = "api-key".to_owned();
        config.providers.push(LlmProviderConfig {
            name: "anthropic-cloud".to_owned(),
            kind: ProviderKind::Anthropic,
            base_url: None,
            api_key_env: None,
            api_family: None,
            binary: None,
            workdir: None,
            timeout_secs: None,
            deployment_target: DeploymentTarget::Cloud,
            models: Vec::new(),
        });
        let registry = build_test_provider_registry(&config);
        assert!(
            registry.find_provider("claude-opus-4-6").is_none(),
            "entry without apiKeyEnv uses the credential chain and must skip when no credential is available"
        );
    }

    #[test]
    fn lazy_provider_reports_the_shared_loading_sentinel() {
        // WHY(#5580): pylon's /api/health embedding check compares
        // `model_name()` against `mneme::embedding::LOADING_MODEL_NAME` and
        // cannot depend on this binary to learn the value. Pinning the
        // pre-initialization name to that same constant keeps producer and
        // consumer on one definition — reintroducing a local literal here
        // fails this test instead of silently sending the health check down
        // the "pass" branch while the model is still loading.
        let provider = LazyEmbeddingProvider::new(EmbeddingSettings::default());
        assert_eq!(
            provider.model_name(),
            mneme::embedding::LOADING_MODEL_NAME,
            "an uninitialized lazy provider must report the shared loading sentinel"
        );
    }

    fn nous(id: &'static str, default: bool) -> taxis::config::NousDefinition {
        taxis::config::NousDefinition {
            id: koina::id::NousId::from_static(id),
            workspace: format!("/home/user/nous/{id}"),
            default,
            ..Default::default()
        }
    }

    #[test]
    fn unmatched_sender_fallback_requires_an_explicit_default_agent() {
        // WHY(#6439): with no agent marked `default = true`, the first
        // configured agent must NOT catch unmatched channel senders. Before
        // this test existed, `.or_else(|| config.agents.list.first())`
        // routed every sender the operator never listed to "syn" here,
        // silently defeating an explicit source-binding allowlist.
        let agents = vec![nous("syn", false), nous("alt", false)];
        assert_eq!(
            resolve_default_nous_id(&agents),
            None,
            "no explicit default agent must mean unmatched senders are dropped, not routed to the first-configured agent"
        );
    }

    #[test]
    fn empty_agent_list_has_no_default() {
        assert_eq!(resolve_default_nous_id(&[]), None);
    }

    #[test]
    fn explicit_default_agent_is_used_regardless_of_order() {
        let agents = vec![nous("syn", false), nous("alt", true), nous("third", false)];
        assert_eq!(resolve_default_nous_id(&agents), Some("alt".to_owned()));
    }

    #[derive(Default)]
    struct TestEnv {
        vars: std::collections::HashMap<String, String>,
    }

    impl TestEnv {
        fn with_env(mut self, key: &str, value: &str) -> Self {
            self.vars.insert(key.to_owned(), value.to_owned());
            self
        }
    }

    impl koina::system::Environment for TestEnv {
        fn var(&self, name: &str) -> Option<String> {
            self.vars.get(name).cloned()
        }

        fn var_os(&self, name: &str) -> Option<std::ffi::OsString> {
            self.vars.get(name).map(Into::into)
        }

        fn vars(&self) -> Vec<(String, String)> {
            self.vars
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        }

        fn current_dir(&self) -> std::io::Result<PathBuf> {
            Ok(PathBuf::from("/test"))
        }

        fn temp_dir(&self) -> PathBuf {
            PathBuf::from("/tmp")
        }

        fn current_exe(&self) -> std::io::Result<PathBuf> {
            Ok(PathBuf::from("/test/bin/aletheia"))
        }

        fn args(&self) -> Vec<String> {
            vec!["aletheia".to_owned()]
        }
    }

    #[test]
    fn no_anthropic_env_credential_present_when_neither_var_set() {
        assert!(!any_anthropic_env_credential_present(&TestEnv::default()));
    }

    #[test]
    fn anthropic_env_credential_present_via_auth_token() {
        // WHY(#5250): ANTHROPIC_AUTH_TOKEN (OAuth) must be detected, not
        // only ANTHROPIC_API_KEY.
        let env = TestEnv::default().with_env("ANTHROPIC_AUTH_TOKEN", "sk-oauth");
        assert!(any_anthropic_env_credential_present(&env));
    }

    #[test]
    fn anthropic_env_credential_present_via_api_key() {
        let env = TestEnv::default().with_env("ANTHROPIC_API_KEY", "sk-static");
        assert!(any_anthropic_env_credential_present(&env));
    }

    #[test]
    fn empty_string_env_var_does_not_count_as_present() {
        // WHY: an exported-but-empty var (e.g. `ANTHROPIC_API_KEY=` in a
        // sourced shell profile) must not be treated as an explicit
        // credential -- it would otherwise silently shadow the keyring.
        let env = TestEnv::default().with_env("ANTHROPIC_API_KEY", "");
        assert!(!any_anthropic_env_credential_present(&env));
    }
}

#[cfg(all(test, feature = "energeia"))]
#[expect(clippy::expect_used, reason = "test assertions")]
mod setup_energeia_tests;
