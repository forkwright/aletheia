// kanon:ignore RUST/file-too-long — setup factories are cohesive initialization helpers; no natural split point
//! Setup helpers: factory functions for providers, registries, and channels.

use std::sync::Arc;

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
use hermeneus::anthropic::{AnthropicProvider, ProviderBehavior};
use hermeneus::openai::{
    OpenAiApiFamily as HermeneusOpenAiApiFamily, OpenAiProvider, OpenAiProviderConfig,
};
use hermeneus::provider::{
    DeploymentTarget as HermeneusDeploymentTarget, ProviderConfig, ProviderRegistry,
};
use koina::credential::{CredentialProvider, CredentialSource};
use koina::secret::SecretString;
use mneme::embedding::{
    DegradedEmbeddingProvider, EmbeddingConfig, EmbeddingProvider, create_provider,
};
use nous::manager::NousManager;
use symbolon::credential::{
    CredentialChain, CredentialFile, EnvCredentialProvider, FileCredentialProvider,
    RefreshingCredentialProvider, claude_code_default_path, claude_code_provider,
};
use taxis::config::{AletheiaConfig, EmbeddingSettings};
use taxis::oikos::Oikos;

use crate::error::Result;

mod tool_registry;

pub(super) use tool_registry::build_tool_registry;

#[expect(
    clippy::too_many_lines,
    reason = "service wiring function — splitting would scatter related provider setup logic"
)]
pub(super) fn build_provider_registry(config: &AletheiaConfig, oikos: &Oikos) -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();

    let pricing: std::collections::HashMap<String, hermeneus::provider::ModelPricing> = config
        .pricing
        .iter()
        .map(|(model, p)| {
            (
                model.clone(),
                hermeneus::provider::ModelPricing {
                    input_cost_per_mtok: p.input_cost_per_mtok,
                    output_cost_per_mtok: p.output_cost_per_mtok,
                },
            )
        })
        .collect();

    let cred_source = config.credential.source.as_str();
    let cred_file = oikos.credentials().join("anthropic.json");
    let mut chain: Vec<Box<dyn CredentialProvider>> = Vec::new();

    let claude_code_path = config
        .credential
        .claude_code_credentials
        .as_ref()
        .map(std::path::PathBuf::from)
        .or_else(claude_code_default_path);

    if cred_source == "claude-code"
        && let Some(ref cc_path) = claude_code_path
        && let Some(provider) = claude_code_provider(cc_path)
    {
        chain.push(provider);
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

    #[cfg(feature = "keyring")]
    {
        use symbolon::credential::KeyringCredentialProvider;
        chain.push(Box::new(KeyringCredentialProvider::new()));
    }

    chain.push(Box::new(EnvCredentialProvider::with_source(
        "ANTHROPIC_AUTH_TOKEN",
        CredentialSource::OAuth,
    )));
    chain.push(Box::new(EnvCredentialProvider::new("ANTHROPIC_API_KEY")));

    if cred_source == "auto"
        && let Some(ref cc_path) = claude_code_path
        && let Some(provider) = claude_code_provider(cc_path)
    {
        chain.push(provider);
    }

    let credential_chain: Arc<dyn CredentialProvider> = Arc::new(CredentialChain::new(chain));

    #[cfg(feature = "kimi-provider")]
    {
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

    let resolved_source = credential_chain.get_credential().map(|c| c.source);
    if let Some(ref source) = resolved_source {
        // SAFETY: logging credential source name (e.g. "oauth", "api-key"), not credential value
        info!(source = %source, "credential resolved"); // kanon:ignore SECURITY/credential-logging -- logs source type string, not the secret
    } else {
        warn!(
            "no credential found -- server will start in degraded mode (no LLM)\n  \
             Fix: SET ANTHROPIC_API_KEY env var, or run `aletheia credential status`"
        );
        return registry;
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
    let provider_config = ProviderConfig {
        pricing,
        prompt_cache_mode,
        ..ProviderConfig::default()
    };

    // WHY: Only register CC subprocess provider when the credential source
    // is not "api-key" AND the resolved credential is OAuth. The CC provider
    // accepts all claude-* models and wins first-match routing, so registering
    // it unconditionally causes API key users to be routed through the CC CLI
    // unnecessarily, and OAuth tokens to be forwarded raw to the API (which
    // rejects them with 401).
    #[cfg(feature = "cc-provider")]
    if cred_source != "api-key" && resolved_source == Some(CredentialSource::OAuth) {
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

    #[cfg(feature = "codex-provider")]
    {
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

    let behavior = ProviderBehavior {
        non_streaming_timeout: std::time::Duration::from_secs(
            config.provider_behavior.non_streaming_timeout_secs,
        ),
        sse_retry_ms: config.provider_behavior.sse_default_retry_ms,
    };

    match AnthropicProvider::with_credential_provider_and_behavior(
        credential_chain,
        &provider_config,
        &behavior,
    ) {
        Ok(provider) => {
            registry.register(Box::new(provider));
            info!("anthropic provider registered");
        }
        Err(e) => warn!(error = %e, "failed to init anthropic provider"),
    }

    register_declared_providers(&mut registry, config);

    registry
}

/// Translate the taxis-side [`taxis::config::DeploymentTarget`] to the
/// hermeneus-side [`HermeneusDeploymentTarget`] (#3736).
///
/// Both enums encode the same three boundaries — `Cloud`, `LocalHosted`,
/// `Embedded` — but live in separate crates so neither depends on the
/// other. This site is the first place both types are in scope, so the
/// mapping lives here alongside every other config→provider conversion
/// done by `register_declared_providers`. Any unknown variant
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

/// Iterate `config.providers` and register each entry with the provider
/// registry (#3424, #3414).
///
/// Dispatches on `ProviderKind` to pick between Anthropic, OpenAI-compatible
/// HTTP, and subprocess adapters. Anthropic and subprocess entries in the
/// declarative list are skipped when their legacy registration path already
/// owns the provider so we do not double-register. Empty list (the default) is
/// a no-op, preserving legacy single-provider behavior.
fn register_declared_providers(registry: &mut ProviderRegistry, config: &AletheiaConfig) {
    use taxis::config::ProviderKind;

    for entry in &config.providers {
        match entry.kind {
            ProviderKind::OpenAi | ProviderKind::OpenAiCompatible => {
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
                    continue;
                };
                let api_key =
                    entry
                        .api_key_env
                        .as_deref()
                        .and_then(|name| match std::env::var(name) {
                            Ok(v) if !v.is_empty() => Some(SecretString::from(v)),
                            Ok(_) => None,
                            Err(_) => {
                                // WHY: missing env var is expected for loopback
                                // llama.cpp / ollama (no auth required). Log at
                                // debug, not warn.
                                tracing::debug!(
                                    provider = %entry.name,
                                    env = name,
                                    "api_key_env unset for OpenAI-compatible provider"
                                );
                                None
                            }
                        });
                let cfg = OpenAiProviderConfig {
                    name: entry.name.clone(),
                    base_url,
                    api_key,
                    models: entry.models.clone(),
                    api_family,
                    // WHY (#3736): the operator-declared deployment target
                    // was previously logged below but never threaded to the
                    // provider, so every OpenAI-compat provider silently
                    // inherited the `Cloud` trait default. That broke the
                    // air-gap claim in `docs/AIR-GAPPED.md` — the recall
                    // filter stripped `Internal` / `Confidential` facts
                    // from traffic bound for loopback llama.cpp / logismos.
                    deployment_target: map_deployment_target(entry.deployment_target),
                    ..OpenAiProviderConfig::default()
                };
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
            ProviderKind::Anthropic => {
                register_declared_anthropic(registry, entry);
            }
            ProviderKind::ClaudeCode => {
                // WHY: declarative registration of the CC adapter would
                // duplicate the `cred_source` check above, and the binary
                // path is already resolved by `CcProvider::new`. The
                // credential-chain branch above handles this case.
                tracing::debug!(
                    provider = %entry.name,
                    "declarative ClaudeCode entry skipped — provider already registered via credential chain"
                );
            }
            ProviderKind::CodexOauth => {
                // WHY: codex-provider registration is feature-gated above and
                // owns binary discovery / CLI OAuth inheritance when enabled.
                // Accepting the typed config shape here makes the provider
                // declarable without changing startup behavior or introducing
                // duplicate routing.
                tracing::debug!(
                    provider = %entry.name,
                    "declarative Codex OAuth entry accepted; registration remains controlled by codex-provider feature path"
                );
            }
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
}

/// Register a declarative Anthropic-protocol provider entry.
///
/// WHY: the first-party Anthropic provider is registered by the
/// credential-chain path in [`build_provider_registry`], so a declarative
/// entry without its own credential would double-register it. An entry
/// naming its own `apiKeyEnv` is a distinct Anthropic-protocol endpoint
/// (proxy, self-hosted, or a compatible third-party host) and registers
/// independently with its configured base URL and model claims.
fn register_declared_anthropic(
    registry: &mut ProviderRegistry,
    entry: &taxis::config::LlmProviderConfig,
) {
    let Some(env_name) = entry.api_key_env.as_deref() else {
        tracing::debug!(
            provider = %entry.name,
            "declarative Anthropic entry without apiKeyEnv skipped — first-party provider is registered via the credential chain"
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
    let cfg = ProviderConfig {
        api_key: Some(key),
        base_url: entry.base_url.clone(),
        name: Some(entry.name.clone()),
        models: entry.models.clone(),
        deployment_target: map_deployment_target(entry.deployment_target),
        ..ProviderConfig::default()
    };
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
                let embedding_config = EmbeddingConfig {
                    provider: self.settings.provider.clone(),
                    model: self.settings.model.clone(),
                    dimension: Some(self.settings.dimension),
                    api_key: None,
                    base_url: None,
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
            None => LazyEmbeddingProvider::LOADING_MODEL_NAME,
        }
    }
}

impl LazyEmbeddingProvider {
    /// Sentinel model name reported while the provider is still loading.
    ///
    /// Health checks use this to report `"degraded: embedding-loading"`.
    pub(crate) const LOADING_MODEL_NAME: &'static str = "embedding-loading";
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
        let knowledge_config = build_knowledge_config(embedding, knowledge, false);
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
    let embedding_config = EmbeddingConfig {
        provider: embedding.provider.clone(),
        model: embedding.model.clone(),
        dimension: Some(embedding.dimension),
        api_key: None,
        base_url: None,
    };
    mneme::knowledge_store::KnowledgeConfig {
        dim: embedding.dimension,
        embedding_model: embedding_config.effective_model_name(),
        allow_assumed_embedding_meta,
        admission_policy: policy,
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
        let base_url = format!("http://{}:{}", account_cfg.http_host, account_cfg.http_port); // SAFE: signal-cli daemon, defaults to localhost
        match SignalClient::with_timeouts(&base_url, rpc_timeout, health_timeout, receive_timeout) {
            Ok(client) => {
                provider.add_account(account_id.clone(), client, account_cfg.auto_start);
                info!(account = %account_id, auto_start = account_cfg.auto_start, "signal account added");
            }
            Err(e) => {
                warn!(account = %account_id, error = %e, "failed to CREATE signal client");
            }
        }
    }

    Some(Arc::new(provider))
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
    ready_rx: tokio::sync::watch::Receiver<bool>,
    signal_provider: Option<&Arc<SignalProvider>>,
    matrix_provider: Option<&Arc<MatrixProvider>>,
    shutdown_token: &CancellationToken,
) -> Result<(Arc<ChannelRegistry>, Option<tokio::task::JoinHandle<()>>)> {
    let mut channel_registry = ChannelRegistry::new();
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

        let default_nous_id = config
            .agents
            .list
            .iter()
            .find(|a| a.default)
            .or_else(|| config.agents.list.first())
            .map(|a| a.id.clone());
        let router = Arc::new(MessageRouter::new(config.bindings.clone(), default_nous_id));

        Some(crate::dispatch::spawn_dispatcher(
            rx,
            router,
            Arc::clone(nous_manager),
            Arc::clone(&channel_registry),
            ready_rx,
        ))
    };

    Ok((channel_registry, handle))
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
    use std::future::Future;
    use std::pin::Pin;

    use agora::types::{ChannelCapabilities, InboundMessage, ProbeResult, SendParams, SendResult};
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
    fn openai_api_family_mapping_and_defaults_are_explicit() {
        use taxis::config::{DeploymentTarget, LlmProviderConfig, ProviderKind};

        let mut entry = LlmProviderConfig {
            name: "openai-cloud".to_owned(),
            kind: ProviderKind::OpenAi,
            base_url: None,
            api_key_env: None,
            api_family: None,
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
    #[expect(
        unsafe_code,
        reason = "std::env::set_var requires unsafe in edition 2024; nextest isolates each test in its own process"
    )]
    #[test]
    fn declarative_anthropic_with_own_key_registers() {
        use taxis::config::{DeploymentTarget, LlmProviderConfig, ProviderKind};

        // SAFETY: nextest runs this test in its own process; no other
        // thread reads or mutates the environment concurrently.
        unsafe { std::env::set_var("TEST_DECL_ANTHROPIC_KEY", "sk-test-123") };
        let mut config = AletheiaConfig::default();
        config.providers.push(LlmProviderConfig {
            name: "kimi-coding".to_owned(),
            kind: ProviderKind::Anthropic,
            base_url: Some("https://compat.api.example.com".to_owned()),
            api_key_env: Some("TEST_DECL_ANTHROPIC_KEY".to_owned()),
            api_family: None,
            deployment_target: DeploymentTarget::Cloud,
            models: vec!["kimi-for-coding".to_owned()],
        });
        let mut registry = ProviderRegistry::new();
        register_declared_providers(&mut registry, &config);
        assert!(
            registry.find_provider("kimi-for-coding").is_some(),
            "declarative Anthropic-protocol entry with its own key must register and claim its models"
        );
        assert!(
            registry
                .find_provider(koina::models::names::opus())
                .is_some(),
            "custom-model instance must catch claude-* at lower precedence"
        );
    }

    #[test]
    fn declarative_anthropic_without_key_env_is_skipped() {
        use taxis::config::{DeploymentTarget, LlmProviderConfig, ProviderKind};

        let mut config = AletheiaConfig::default();
        config.providers.push(LlmProviderConfig {
            name: "anthropic-cloud".to_owned(),
            kind: ProviderKind::Anthropic,
            base_url: None,
            api_key_env: None,
            api_family: None,
            deployment_target: DeploymentTarget::Cloud,
            models: Vec::new(),
        });
        let mut registry = ProviderRegistry::new();
        register_declared_providers(&mut registry, &config);
        assert!(
            registry.find_provider("claude-opus-4-6").is_none(),
            "entry without apiKeyEnv defers to the credential-chain registration"
        );
    }
}

#[cfg(all(test, feature = "energeia"))]
#[expect(clippy::expect_used, reason = "test assertions")]
mod setup_energeia_tests;
