//! Shared application state accessible in all Axum handlers.

use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

use aletheia_hermeneus::provider::ProviderRegistry;
#[cfg(feature = "knowledge-store")]
use aletheia_mneme::knowledge_store::KnowledgeStore;
use aletheia_mneme::store::SessionStore;
use aletheia_nous::manager::NousManager;
use aletheia_organon::registry::ToolRegistry;
use aletheia_symbolon::jwt::JwtManager;
use aletheia_taxis::config::AletheiaConfig;
use aletheia_taxis::oikos::Oikos;
use tokio_util::sync::CancellationToken;

/// Shared state for all Axum handlers, held behind `Arc` in the router.
pub struct AppState {
    /// Persistent session and message storage.
    pub session_store: Arc<Mutex<SessionStore>>,
    /// Manages nous actor lifecycles and provides handles.
    pub nous_manager: Arc<NousManager>,
    /// Registry of available LLM providers.
    pub provider_registry: Arc<ProviderRegistry>,
    /// Registry of tools available to nous agents.
    pub tool_registry: Arc<ToolRegistry>,
    /// Instance directory layout for file resolution.
    pub oikos: Arc<Oikos>,
    /// JWT token creation and validation.
    pub jwt_manager: Arc<JwtManager>,
    /// Server start instant for uptime calculation.
    pub start_time: Instant,
    /// Runtime configuration, updatable via config API.
    pub config: Arc<tokio::sync::RwLock<AletheiaConfig>>,
    /// Auth mode from gateway config (`"token"`, `"none"`, etc.).
    pub auth_mode: String,
    /// Root shutdown token. Cancel to initiate graceful shutdown of all subsystems.
    pub shutdown: CancellationToken,
    /// Shared knowledge store for fact/entity/relationship queries.
    #[cfg(feature = "knowledge-store")]
    pub knowledge_store: Option<Arc<KnowledgeStore>>,
}

#[cfg(test)]
static_assertions::assert_impl_all!(AppState: Send, Sync);
