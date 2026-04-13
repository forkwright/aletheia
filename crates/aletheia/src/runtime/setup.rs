//! Setup helpers: factory functions for providers, registries, and channels.

use std::sync::Arc;

use snafu::prelude::*;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use agora::listener::ChannelListener;
use agora::registry::ChannelRegistry;
use agora::router::MessageRouter;
use agora::semeion::SignalProvider;
use agora::semeion::client::SignalClient;
use agora::types::ChannelProvider;
use hermeneus::anthropic::{AnthropicProvider, ProviderBehavior};
use hermeneus::provider::{ProviderConfig, ProviderRegistry};
use koina::credential::{CredentialProvider, CredentialSource};
use mneme::embedding::{
    DegradedEmbeddingProvider, EmbeddingConfig, EmbeddingProvider, create_provider,
};
use nous::manager::NousManager;
use organon::builtins;
use organon::registry::ToolRegistry;
use symbolon::credential::{
    CredentialChain, CredentialFile, EnvCredentialProvider, FileCredentialProvider,
    RefreshingCredentialProvider, claude_code_default_path, claude_code_provider,
};
use taxis::config::{AletheiaConfig, EmbeddingSettings};
use taxis::oikos::Oikos;

use crate::error::Result;

#[expect(clippy::too_many_lines, reason = "service wiring function — splitting would scatter related provider setup logic")]
pub(super) fn build_provider_registry(config: &AletheiaConfig, oikos: &Oikos) -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();

    let pricing: std::collections::HashMap<String, hermeneus::provider::ModelPricing> =
        config
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
                info!(path = %cred_file.display(), "credential file found (OAuth auto-refresh)");
                chain.push(Box::new(refreshing));
            } else {
                info!(path = %cred_file.display(), "credential file found (static)");
                chain.push(Box::new(FileCredentialProvider::new(cred_file.clone())));
            }
        } else {
            info!(path = %cred_file.display(), "credential file found (static API key)");
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

    let resolved_source = credential_chain.get_credential().map(|c| c.source);
    if let Some(ref source) = resolved_source {
        info!(source = %source, "credential resolved");
    } else {
        warn!(
            "no credential found -- server will start in degraded mode (no LLM)\n  \
             Fix: SET ANTHROPIC_API_KEY env var, or run `aletheia credential status`"
        );
        return registry;
    }

    let provider_config = ProviderConfig {
        pricing,
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
                info!("CC subprocess provider registered (OAuth credential detected)");
            }
            Err(e) => {
                tracing::debug!(error = %e, "CC provider unavailable, falling back to direct API");
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

    registry
}

pub(super) fn build_tool_registry(
    sandbox_settings: &taxis::config::SandboxSettings,
) -> Result<ToolRegistry> {
    let mut registry = ToolRegistry::new();
    let sandbox = organon::sandbox::SandboxConfig {
        enabled: sandbox_settings.enabled,
        enforcement: match sandbox_settings.enforcement {
            taxis::config::SandboxEnforcementMode::Enforcing => {
                organon::sandbox::SandboxEnforcement::Enforcing
            }
            _ => organon::sandbox::SandboxEnforcement::Permissive,
        },
        allowed_root: sandbox_settings.allowed_root.clone(),
        extra_read_paths: sandbox_settings.extra_read_paths.clone(),
        extra_write_paths: sandbox_settings.extra_write_paths.clone(),
        extra_exec_paths: sandbox_settings.extra_exec_paths.clone(),
        egress: match sandbox_settings.egress {
            taxis::config::EgressPolicy::Deny => {
                organon::sandbox::EgressPolicy::Deny
            }
            taxis::config::EgressPolicy::Allowlist => {
                organon::sandbox::EgressPolicy::Allowlist
            }
            _ => organon::sandbox::EgressPolicy::Allow,
        },
        egress_allowlist: sandbox_settings.egress_allowlist.clone(),
        nproc_limit: sandbox_settings.nproc_limit,
    };
    builtins::register_all_with_sandbox(&mut registry, sandbox)
        .whatever_context("failed to register builtin tools")?;
    info!(count = registry.definitions().len(), "tools registered");
    Ok(registry)
}

pub(super) fn create_embedding_provider(
    settings: &EmbeddingSettings,
) -> Arc<dyn EmbeddingProvider> {
    let embedding_config = EmbeddingConfig {
        provider: settings.provider.clone(),
        model: settings.model.clone(),
        dimension: Some(settings.dimension),
        api_key: None,
    };
    match create_provider(&embedding_config) {
        Ok(p) => {
            info!(
                provider = %settings.provider,
                dim = settings.dimension,
                "embedding provider created"
            );
            Arc::from(p)
        }
        Err(e) => {
            warn!(
                error = %e,
                provider = %settings.provider,
                "embedding provider failed to load: starting in degraded mode \
                 (recall and vector search unavailable)"
            );
            Arc::new(DegradedEmbeddingProvider::new(settings.dimension))
        }
    }
}

#[cfg(feature = "recall")]
pub(super) fn open_knowledge_store(
    oikos: &Oikos,
) -> Result<Option<Arc<mneme::knowledge_store::KnowledgeStore>>> {
    let kb_path = oikos.knowledge_db();
    if let Some(parent) = kb_path.parent() {
        std::fs::create_dir_all(parent)
            .whatever_context("failed to CREATE knowledge store directory")?;
    }
    let store = match mneme::knowledge_store::KnowledgeStore::open_fjall(
        &kb_path,
        mneme::knowledge_store::KnowledgeConfig::default(),
    ) {
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
    info!(path = %kb_path.display(), dim = 384, "knowledge store opened (fjall)");
    Ok(Some(store))
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
        let base_url = format!("http://{}:{}", account_cfg.http_host, account_cfg.http_port);
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

pub(super) fn start_inbound_dispatch(
    config: &AletheiaConfig,
    nous_manager: &Arc<NousManager>,
    ready_rx: tokio::sync::watch::Receiver<bool>,
    signal_provider: Option<&Arc<SignalProvider>>,
    shutdown_token: &CancellationToken,
) -> (Arc<ChannelRegistry>, Option<tokio::task::JoinHandle<()>>) {
    let mut channel_registry = ChannelRegistry::new();

    if let Some(provider) = signal_provider {
        #[expect(
            clippy::as_conversions,
            reason = "coercion to dyn ChannelProvider trait object: required by registry API"
        )]
        channel_registry
            .register(Arc::clone(provider) as Arc<dyn ChannelProvider>)
            .unwrap_or_default();
    }
    let channel_registry = Arc::new(channel_registry);

    let handle = if let Some(provider) = signal_provider {
        let poll_interval = Some(std::time::Duration::from_millis(
            config.messaging.poll_interval_ms,
        ));
        let listener = ChannelListener::start_with_config(
            provider,
            poll_interval,
            shutdown_token.child_token(),
            config.messaging.max_concurrent_handlers,
        );
        info!("signal channel listener started");
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
    } else {
        None
    };

    (channel_registry, handle)
}
