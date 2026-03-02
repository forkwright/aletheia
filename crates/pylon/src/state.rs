//! Shared application state accessible in all Axum handlers.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use aletheia_hermeneus::provider::ProviderRegistry;
use aletheia_mneme::store::SessionStore;
use aletheia_nous::manager::NousManager;
use aletheia_organon::registry::ToolRegistry;
use aletheia_symbolon::jwt::JwtManager;
use aletheia_taxis::oikos::Oikos;

/// Shared state for all Axum handlers, held behind `Arc` in the router.
pub struct AppState {
    pub session_store: Mutex<SessionStore>,
    pub nous_manager: NousManager,
    pub provider_registry: Arc<ProviderRegistry>,
    pub tool_registry: Arc<ToolRegistry>,
    pub oikos: Arc<Oikos>,
    pub jwt_manager: Arc<JwtManager>,
    pub start_time: Instant,
}
