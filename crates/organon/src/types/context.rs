//! Runtime context and service locator passed to tool executors.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};

use hermeneus::secret::SecretVault;

use serde::{Deserialize, Serialize};

use koina::id::{NousId, SessionId, ToolName};
use taxis::config::ToolLimitsConfig;
use tokio_util::sync::CancellationToken;

use crate::surface::EffectiveToolSurface;

use super::services::{
    BlackboardStore, CrossNousService, KnowledgeSearchService, MessageService, NoteStore,
    PlanningService, SpawnService,
};

tokio::task_local! {
    static TURN_CANCEL: CancellationToken;
}

/// Configuration for server-side tools that execute on the API provider's infrastructure.
///
/// Controls which server tools are available for per-session activation via `enable_tool`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServerToolConfig {
    /// Whether web search is available for activation.
    #[serde(default)]
    pub web_search: bool,
    /// Maximum web search uses per turn (None = provider default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_search_max_uses: Option<u32>,
    /// Whether code execution is available for activation.
    #[serde(default)]
    pub code_execution: bool,
}

/// Metadata describing one server tool available for activation via `enable_tool`.
#[derive(Debug, Clone)]
pub(crate) struct ServerToolCatalogEntry {
    /// Tool name as exposed to the agent.
    pub name: ToolName,
    /// Human-readable description shown in the catalog.
    pub description: String,
    /// Whether activating this tool is considered sensitive for audit events.
    pub sensitive: bool,
}

impl ServerToolCatalogEntry {
    pub(crate) fn new(name: ToolName, description: impl Into<String>, sensitive: bool) -> Self {
        Self {
            name,
            description: description.into(),
            sensitive,
        }
    }
}

impl ServerToolConfig {
    /// Generate catalog entries for server tools available via `enable_tool`.
    #[must_use]
    pub(crate) fn catalog_entries(&self) -> Vec<(ToolName, String)> {
        self.catalog_entries_with_metadata()
            .into_iter()
            .map(|entry| (entry.name, entry.description))
            .collect()
    }

    /// Catalog entries with sensitivity metadata for policy checks.
    #[must_use]
    pub(crate) fn catalog_entries_with_metadata(&self) -> Vec<ServerToolCatalogEntry> {
        let mut entries = Vec::new();
        if self.web_search {
            entries.push(ServerToolCatalogEntry::new(
                ToolName::from_static("web_search"), // kanon:ignore RUST/expect
                "Search the web using Anthropic's server-side web search",
                false,
            ));
        }
        if self.code_execution {
            entries.push(ServerToolCatalogEntry::new(
                ToolName::from_static("code_execution"), // kanon:ignore RUST/expect
                "Execute Python code in a sandboxed server-side environment",
                true,
            ));
        }
        entries
    }

    /// Produce server tool definitions for tools that are currently active.
    #[must_use]
    pub fn active_definitions(
        // kanon:ignore RUST/pub-visibility
        &self,
        active: &HashSet<ToolName>,
    ) -> Vec<hermeneus::types::ServerToolDefinition> {
        let mut defs = Vec::new();
        let web_search_name = ToolName::from_static("web_search"); // kanon:ignore RUST/expect
        let code_exec_name = ToolName::from_static("code_execution"); // kanon:ignore RUST/expect

        if self.web_search && active.contains(&web_search_name) {
            defs.push(hermeneus::types::ServerToolDefinition {
                tool_type: "web_search_20250305".to_owned(),
                name: "web_search".to_owned(),
                max_uses: self.web_search_max_uses,
                allowed_domains: None,
                blocked_domains: None,
                user_location: None,
            });
        }
        if self.code_execution && active.contains(&code_exec_name) {
            defs.push(hermeneus::types::ServerToolDefinition {
                tool_type: "code_execution_20250522".to_owned(),
                name: "code_execution".to_owned(),
                max_uses: None,
                allowed_domains: None,
                blocked_domains: None,
                user_location: None,
            });
        }
        defs
    }
}

/// Paired HTTP clients supplied to tool executors.
///
/// `general` is a standard client for outbound requests.
/// `ssrf_safe` has auto-redirect disabled so tools that perform their own
/// SSRF-safe redirect validation (web_fetch, http_request) can revalidate
/// each hop before following it.
///
/// WHY: reqwest does not expose a way to reconfigure an existing client's
/// redirect policy, so the runtime supplies a pre-built SSRF-safe client
/// alongside the general-purpose client. Both should be constructed from the
/// same operator HTTP configuration (proxy, TLS CA, connection pool) so
/// uniform policy enforcement is possible.
pub struct ToolHttpClients {
    /// General-purpose HTTP client with default redirect policy.
    pub general: reqwest::Client,
    /// HTTP client with auto-redirect disabled for SSRF-safe redirect validation.
    pub ssrf_safe: reqwest::Client,
}

impl ToolHttpClients {
    /// Build a `ToolHttpClients` suitable for unit tests.
    #[cfg(test)]
    pub(crate) fn for_tests() -> Self {
        Self {
            general: reqwest::Client::new(),
            ssrf_safe: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }
}

/// Service locator for tool executors needing access to runtime services.
#[expect(
    missing_docs,
    reason = "service locator fields are self-documenting by name"
)]
pub struct ToolServices {
    pub cross_nous: Option<Arc<dyn CrossNousService>>,
    pub messenger: Option<Arc<dyn MessageService>>,
    pub note_store: Option<Arc<dyn NoteStore>>,
    pub blackboard_store: Option<Arc<dyn BlackboardStore>>,
    pub spawn: Option<Arc<dyn SpawnService>>,
    pub planning: Option<Arc<dyn PlanningService>>,
    pub knowledge: Option<Arc<dyn KnowledgeSearchService>>,
    pub working_checkpoint_store: Option<Arc<dyn crate::types::WorkingCheckpointStore>>,
    /// Paired HTTP clients: general-purpose and SSRF-safe (redirect-disabled).
    pub http_clients: ToolHttpClients,
    /// In-memory vault for session-scoped secrets (AWS SSO keys, API tokens, etc.).
    ///
    /// Referenced via `{{secret:<name>}}` or `$SECRET(<name>)` placeholders in
    /// tool arguments and substituted at dispatch time so resolved values never
    /// reach transcripts or outbound LLM payloads.
    pub secret_vault: SecretVault,
    /// Catalog of lazy tools available for activation via `enable_tool`.
    pub lazy_tool_catalog: Vec<(ToolName, String)>,
    /// Server tool configuration for provider-side tools (web search, code execution).
    pub server_tool_config: ServerToolConfig,
}

impl std::fmt::Debug for ToolServices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolServices")
            .field("cross_nous", &self.cross_nous.is_some())
            .field("messenger", &self.messenger.is_some())
            .field("note_store", &self.note_store.is_some())
            .field("blackboard_store", &self.blackboard_store.is_some())
            .field("spawn", &self.spawn.is_some())
            .field("planning", &self.planning.is_some())
            .field("knowledge", &self.knowledge.is_some())
            .field(
                "working_checkpoint_store",
                &self.working_checkpoint_store.is_some(),
            )
            .field("http_clients", &"ToolHttpClients { .. }")
            .field("secret_vault_len", &self.secret_vault.len())
            .field("lazy_tool_catalog_len", &self.lazy_tool_catalog.len())
            .field("server_tool_config", &self.server_tool_config)
            .finish_non_exhaustive()
    }
}

/// Execution context passed to every tool invocation.
#[derive(Debug, Clone)]
pub struct ToolContext {
    /// The agent executing this tool.
    pub nous_id: NousId,
    /// Current session.
    pub session_id: SessionId,
    /// Current turn number within the session.
    pub turn_number: u64,
    /// Agent workspace root.
    pub workspace: PathBuf,
    /// Allowed filesystem roots for sandboxing.
    pub allowed_roots: Vec<PathBuf>,
    /// Optional runtime services for tools that need cross-cutting capabilities.
    pub services: Option<Arc<ToolServices>>,
    /// Per-session set of dynamically activated tools (via `enable_tool`).
    pub active_tools: Arc<RwLock<HashSet<ToolName>>>,
    /// Deployment-tunable tool size and timeout limits from taxis config.
    pub tool_config: Arc<ToolLimitsConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SurfaceBindingKey {
    nous_id: String,
    session_id: String,
    turn_number: u64,
}

/// Scoped guard for an effective-surface binding.
pub struct EffectiveSurfaceBinding {
    key: SurfaceBindingKey,
}

impl ToolContext {
    /// Return the current turn cancellation token, or a detached token outside turns.
    #[must_use]
    pub fn turn_cancel(&self) -> CancellationToken {
        TURN_CANCEL
            .try_with(Clone::clone)
            .unwrap_or_else(|_| CancellationToken::new())
    }

    /// Run a future with a turn cancellation token visible to tool executors.
    pub async fn scope_turn_cancel<F>(token: CancellationToken, future: F) -> F::Output
    where
        F: Future,
    {
        TURN_CANCEL.scope(token, future).await
    }

    /// Bind an effective surface for this context until the returned guard drops.
    #[must_use]
    pub fn bind_effective_surface(
        &self,
        surface: Arc<EffectiveToolSurface>,
    ) -> EffectiveSurfaceBinding {
        let key = self.surface_binding_key();
        let mut guard = surface_bindings().write().unwrap_or_else(|poisoned| {
            tracing::warn!("effective tool surface binding lock poisoned, recovering");
            poisoned.into_inner()
        });
        guard.insert(key.clone(), surface);
        EffectiveSurfaceBinding { key }
    }

    /// Return the effective surface currently bound for this context.
    #[must_use]
    pub fn effective_surface(&self) -> Option<Arc<EffectiveToolSurface>> {
        let key = self.surface_binding_key();
        let guard = surface_bindings().read().unwrap_or_else(|poisoned| {
            tracing::warn!("effective tool surface binding lock poisoned, recovering");
            poisoned.into_inner()
        });
        guard.get(&key).cloned()
    }

    fn surface_binding_key(&self) -> SurfaceBindingKey {
        SurfaceBindingKey {
            nous_id: self.nous_id.as_ref().to_owned(),
            session_id: self.session_id.to_string(),
            turn_number: self.turn_number,
        }
    }
}

impl Drop for EffectiveSurfaceBinding {
    fn drop(&mut self) {
        let mut guard = surface_bindings().write().unwrap_or_else(|poisoned| {
            tracing::warn!("effective tool surface binding lock poisoned, recovering");
            poisoned.into_inner()
        });
        guard.remove(&self.key);
    }
}

fn surface_bindings() -> &'static RwLock<HashMap<SurfaceBindingKey, Arc<EffectiveToolSurface>>> {
    static BINDINGS: OnceLock<RwLock<HashMap<SurfaceBindingKey, Arc<EffectiveToolSurface>>>> =
        OnceLock::new();
    BINDINGS.get_or_init(|| RwLock::new(HashMap::new()))
}
