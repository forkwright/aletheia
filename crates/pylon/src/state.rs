//! Shared application state accessible in all Axum handlers.

use std::sync::Mutex;
use std::time::Instant;

use aletheia_hermeneus::provider::ProviderRegistry;
use aletheia_mneme::store::SessionStore;
use aletheia_nous::session::SessionManager;
use aletheia_organon::registry::ToolRegistry;
use aletheia_taxis::oikos::Oikos;

/// Shared state for all Axum handlers, held behind `Arc` in the router.
pub struct AppState {
    pub session_store: Mutex<SessionStore>,
    pub session_manager: SessionManager,
    pub provider_registry: ProviderRegistry,
    pub tool_registry: ToolRegistry,
    pub oikos: Oikos,
    pub start_time: Instant,
}
