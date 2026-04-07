//! Shared state for the diaporeia MCP server.
//!
//! Parallel to pylon's `AppState` but without HTTP-specific fields.
//! Constructed from the same shared `Arc`s as `AppState`: zero duplication.

use std::sync::Arc;
use std::time::Instant;

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use aletheia_mneme::store::SessionStore;
use aletheia_nous::manager::NousManager;
use aletheia_organon::registry::ToolRegistry;
use aletheia_symbolon::jwt::JwtManager;
use aletheia_taxis::config::AletheiaConfig;
use aletheia_taxis::oikos::Oikos;

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
    /// JWT token validation (shared with pylon).
    ///
    /// `None` when `auth_mode == "none"` (no signing key available).
    pub jwt_manager: Option<Arc<JwtManager>>,
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
}

#[cfg(test)]
static_assertions::assert_impl_all!(DiaporeiaState: Send, Sync);
