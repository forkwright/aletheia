//! Shared state for the diaporeia MCP server.
//!
//! Parallel to pylon's `AppState` but without HTTP-specific fields.
//! Constructed from the same shared `Arc`s as `AppState`: zero duplication.

use std::sync::Arc;
use std::time::Instant;

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[cfg(feature = "knowledge-store")]
use mneme::knowledge_store::KnowledgeStore;
use mneme::store::SessionStore;
use nous::manager::NousManager;
use organon::registry::ToolRegistry;
use organon::types::{BlackboardStore, NoteStore};
use symbolon::auth::AuthFacade;
use taxis::config::AletheiaConfig;
use taxis::oikos::Oikos;

/// Shared state for the diaporeia MCP server.
///
/// Holds the same shared `Arc` pointers as pylon's `AppState`.
/// Both live in the same process and access identical instances.
pub struct DiaporeiaState {
    /// Persistent session and message storage.
    pub session_store: Arc<Mutex<SessionStore>>,
    /// Manages nous actor lifecycles and provides handles.
    pub nous_manager: Arc<NousManager>,
    /// Registry of tools available to nous agents.
    pub tool_registry: Arc<ToolRegistry>,
    /// Instance directory layout for file resolution.
    pub oikos: Arc<Oikos>,
    /// Revocation-aware bearer token validation (shared with pylon).
    ///
    /// `None` when `auth_mode == "none"` (no validator required).
    pub auth_facade: Option<Arc<AuthFacade>>,
    /// Server start instant for uptime calculation.
    pub start_time: Instant,
    /// Runtime configuration, updatable via config API.
    pub config: Arc<tokio::sync::RwLock<AletheiaConfig>>,
    /// Auth mode from gateway config (`"token"`, `"none"`, etc.).
    pub auth_mode: String,
    /// Role assigned to anonymous requests when auth mode is `"none"`.
    pub none_role: String,
    /// Root shutdown token.
    pub shutdown: CancellationToken,
    /// Shared knowledge store for the knowledge graph MCP surface.
    ///
    /// `None` when the knowledge store is not configured or when
    /// `mcp.knowledge_graph.enabled` is `false`.
    #[cfg(feature = "knowledge-store")]
    pub knowledge_store: Option<Arc<KnowledgeStore>>,
    /// Session notes store for the `memory_note` tool.
    pub note_store: Option<Arc<dyn NoteStore>>,
    /// Shared blackboard store for the `memory_blackboard` tool.
    pub blackboard_store: Option<Arc<dyn BlackboardStore>>,
}

#[cfg(test)]
const _: fn() = || {
    fn assert<T: Send + Sync>() {}
    assert::<DiaporeiaState>();
};
