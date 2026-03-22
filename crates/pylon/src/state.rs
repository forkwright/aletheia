//! Shared application state accessible in all Axum handlers.

use std::sync::Arc;
use std::time::Instant;

use axum::extract::FromRef;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use aletheia_hermeneus::provider::ProviderRegistry;
#[cfg(feature = "knowledge-store")]
use aletheia_mneme::knowledge_store::KnowledgeStore;
use aletheia_mneme::store::SessionStore;
use aletheia_nous::manager::NousManager;
use aletheia_organon::registry::ToolRegistry;
use aletheia_symbolon::jwt::JwtManager;
use aletheia_taxis::config::AletheiaConfig;
use aletheia_taxis::oikos::Oikos;

use crate::idempotency::IdempotencyCache;

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
    /// Broadcast channel for config change notifications.
    ///
    /// Actors and subsystems subscribe via `config_rx` to receive the latest
    /// config after each hot-reload.
    pub config_tx: tokio::sync::watch::Sender<AletheiaConfig>,
    /// Auth mode from gateway config (`"token"`, `"none"`, etc.).
    pub auth_mode: String,
    /// Root shutdown token. Cancel to initiate graceful shutdown of all subsystems.
    pub shutdown: CancellationToken,
    /// Idempotency-key cache for deduplicating `POST /sessions/{id}/messages`.
    pub idempotency_cache: Arc<IdempotencyCache>,
    /// Shared knowledge store for fact/entity/relationship queries.
    #[cfg(feature = "knowledge-store")]
    pub knowledge_store: Option<Arc<KnowledgeStore>>,
}

impl AppState {
    // WHY: config_rx is a subscriber API for hot-reload propagation; not yet
    // wired into actor lifecycle, but required for future config-change support.
    #[allow(dead_code)]
    /// Subscribe to config change notifications.
    ///
    /// Returns a `watch::Receiver` that yields the latest config after each
    /// successful reload. Actors should call `changed().await` to be notified.
    #[must_use]
    pub(crate) fn config_rx(&self) -> tokio::sync::watch::Receiver<AletheiaConfig> {
        self.config_tx.subscribe()
    }
}

/// State slice for health-check handlers.
#[derive(Clone)]
pub struct HealthState {
    /// Persistent session and message storage.
    pub session_store: Arc<Mutex<SessionStore>>,
    /// Registry of available LLM providers.
    pub provider_registry: Arc<ProviderRegistry>,
    /// Manages nous actor lifecycles and provides handles.
    pub nous_manager: Arc<NousManager>,
    /// Server start instant for uptime calculation.
    pub start_time: std::time::Instant,
}

impl FromRef<Arc<AppState>> for HealthState {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self {
            session_store: Arc::clone(&state.session_store),
            provider_registry: Arc::clone(&state.provider_registry),
            nous_manager: Arc::clone(&state.nous_manager),
            start_time: state.start_time,
        }
    }
}

/// State slice for Prometheus metrics handlers.
#[derive(Clone)]
pub struct MetricsState {
    /// Persistent session and message storage.
    pub session_store: Arc<Mutex<SessionStore>>,
    /// Server start instant for uptime calculation.
    pub start_time: std::time::Instant,
}

impl FromRef<Arc<AppState>> for MetricsState {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self {
            session_store: Arc::clone(&state.session_store),
            start_time: state.start_time,
        }
    }
}

/// State slice for nous agent handlers.
#[derive(Clone)]
pub struct NousState {
    /// Manages nous actor lifecycles and provides handles.
    pub nous_manager: Arc<NousManager>,
    /// Registry of tools available to nous agents.
    pub tool_registry: Arc<ToolRegistry>,
}

impl FromRef<Arc<AppState>> for NousState {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self {
            nous_manager: Arc::clone(&state.nous_manager),
            tool_registry: Arc::clone(&state.tool_registry),
        }
    }
}

/// State slice for config read/write handlers.
#[derive(Clone)]
pub struct ConfigState {
    /// Runtime configuration, updatable via config API.
    pub config: Arc<tokio::sync::RwLock<AletheiaConfig>>,
    /// Broadcast channel for config change notifications.
    pub config_tx: tokio::sync::watch::Sender<AletheiaConfig>,
    /// Instance directory layout for file resolution.
    pub oikos: Arc<Oikos>,
}

impl FromRef<Arc<AppState>> for ConfigState {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self {
            config: Arc::clone(&state.config),
            config_tx: state.config_tx.clone(),
            oikos: Arc::clone(&state.oikos),
        }
    }
}

/// State slice for session management and streaming handlers.
#[derive(Clone)]
pub struct SessionsState {
    /// Persistent session and message storage.
    pub session_store: Arc<Mutex<SessionStore>>,
    /// Manages nous actor lifecycles and provides handles.
    pub nous_manager: Arc<NousManager>,
    /// Registry of available LLM providers.
    pub provider_registry: Arc<ProviderRegistry>,
    /// Root shutdown token. Cancel to initiate graceful shutdown of all subsystems.
    pub shutdown: CancellationToken,
    /// Idempotency-key cache for deduplicating `POST /sessions/{id}/messages`.
    pub idempotency_cache: Arc<IdempotencyCache>,
}

impl FromRef<Arc<AppState>> for SessionsState {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self {
            session_store: Arc::clone(&state.session_store),
            nous_manager: Arc::clone(&state.nous_manager),
            provider_registry: Arc::clone(&state.provider_registry),
            shutdown: state.shutdown.clone(),
            idempotency_cache: Arc::clone(&state.idempotency_cache),
        }
    }
}

/// State slice for knowledge store handlers.
#[derive(Clone)]
pub struct KnowledgeState {
    /// Shared knowledge store for fact/entity/relationship queries.
    #[cfg(feature = "knowledge-store")]
    pub knowledge_store: Option<Arc<KnowledgeStore>>,
}

impl FromRef<Arc<AppState>> for KnowledgeState {
    #[cfg_attr(
        not(feature = "knowledge-store"),
        expect(
            unused_variables,
            reason = "state only used when knowledge-store feature is enabled"
        )
    )]
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self {
            #[cfg(feature = "knowledge-store")]
            knowledge_store: state.knowledge_store.clone(),
        }
    }
}

#[cfg(test)]
static_assertions::assert_impl_all!(AppState: Send, Sync);

#[cfg(test)]
mod tests {
    use static_assertions::assert_impl_all;

    use super::*;

    assert_impl_all!(HealthState: Send, Sync, Clone);
    assert_impl_all!(MetricsState: Send, Sync, Clone);
    assert_impl_all!(NousState: Send, Sync, Clone);
    assert_impl_all!(ConfigState: Send, Sync, Clone);
    assert_impl_all!(SessionsState: Send, Sync, Clone);
    assert_impl_all!(KnowledgeState: Send, Sync, Clone);
}
