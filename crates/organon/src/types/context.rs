//! Runtime context and service locator passed to tool executors.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

use aletheia_koina::id::{NousId, SessionId, ToolName};

use super::services::{
    BlackboardStore, CrossNousService, KnowledgeSearchService, MessageService, NoteStore,
    PlanningService, SpawnService,
};

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

impl ServerToolConfig {
    /// Generate catalog entries for server tools available via `enable_tool`.
    #[must_use]
    pub(crate) fn catalog_entries(&self) -> Vec<(ToolName, String)> {
        let mut entries = Vec::new();
        if self.web_search {
            entries.push((
                ToolName::from_static("web_search"), // kanon:ignore RUST/expect
                "Search the web using Anthropic's server-side web search".to_owned(),
            ));
        }
        if self.code_execution {
            entries.push((
                ToolName::from_static("code_execution"), // kanon:ignore RUST/expect
                "Execute Python code in a sandboxed server-side environment".to_owned(),
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
    ) -> Vec<aletheia_hermeneus::types::ServerToolDefinition> {
        let mut defs = Vec::new();
        let web_search_name = ToolName::from_static("web_search"); // kanon:ignore RUST/expect
        let code_exec_name = ToolName::from_static("code_execution"); // kanon:ignore RUST/expect

        if self.web_search && active.contains(&web_search_name) {
            defs.push(aletheia_hermeneus::types::ServerToolDefinition {
                tool_type: "web_search_20250305".to_owned(),
                name: "web_search".to_owned(),
                max_uses: self.web_search_max_uses,
                allowed_domains: None,
                blocked_domains: None,
                user_location: None,
            });
        }
        if self.code_execution && active.contains(&code_exec_name) {
            defs.push(aletheia_hermeneus::types::ServerToolDefinition {
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
    pub http_client: reqwest::Client,
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
    /// Agent workspace root.
    pub workspace: PathBuf,
    /// Allowed filesystem roots for sandboxing.
    pub allowed_roots: Vec<PathBuf>,
    /// Optional runtime services for tools that need cross-cutting capabilities.
    pub services: Option<Arc<ToolServices>>,
    /// Per-session set of dynamically activated tools (via `enable_tool`).
    pub active_tools: Arc<RwLock<HashSet<ToolName>>>,
}
