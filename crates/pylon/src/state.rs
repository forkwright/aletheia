//! Shared application state accessible in all Axum handlers.

use std::sync::Arc;
use std::time::Instant;

use axum::extract::FromRef;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use hermeneus::provider::ProviderRegistry;
use koina::metrics::MetricsRegistry;
use mneme::embedding::EmbeddingProvider;
#[cfg(feature = "knowledge-store")]
use mneme::knowledge_store::KnowledgeStore;
use mneme::store::SessionStore;
use nous::manager::NousManager;
use organon::registry::ToolRegistry;
use symbolon::auth::AuthFacade;
use symbolon::jwt::JwtManager;
use taxis::config::AletheiaConfig;
use taxis::oikos::Oikos;

use crate::event_bus::EventBus;
use crate::idempotency::IdempotencyCache;
use crate::turn_buffer::TurnBufferRegistry;

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
    /// Revocation-aware authentication facade.
    pub auth_facade: Arc<AuthFacade>,
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
    /// Role assigned to anonymous requests when auth mode is `"none"`.
    pub none_role: String,
    /// Root shutdown token. Cancel to initiate graceful shutdown of all subsystems.
    pub shutdown: CancellationToken,
    /// Idempotency-key cache for deduplicating `POST /sessions/{id}/messages`.
    pub idempotency_cache: Arc<IdempotencyCache>,
    /// Shared knowledge store for fact/entity/relationship queries.
    #[cfg(feature = "knowledge-store")]
    pub knowledge_store: Option<Arc<KnowledgeStore>>,
    /// Active embedding provider. Used by health checks to report degraded
    /// mode when the real provider failed to load at startup (#3380).
    ///
    /// `None` only when the pylon-only standalone server builds state without
    /// a configured embedding pipeline.
    pub embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    /// Per-turn event buffer registry for SSE client recovery (#3276).
    ///
    /// Shared between streaming handlers (which record events) and the
    /// reconnect handler (which replays them).
    pub turn_buffer_registry: Arc<TurnBufferRegistry>,
    /// Shared Prometheus metrics registry.
    ///
    /// Crates register their metric families here at startup; the `/metrics`
    /// handler holds this to encode scrapes.
    pub metrics_registry: MetricsRegistry,
    /// In-process broadcast bus for domain events.
    pub event_bus: Arc<EventBus>,
}

impl AppState {
    /// Subscribe to config change notifications.
    ///
    /// Returns a `watch::Receiver` that yields the latest config after each
    /// successful reload. Actors should call `changed().await` to be notified.
    #[must_use]
    pub fn config_rx(&self) -> tokio::sync::watch::Receiver<AletheiaConfig> {
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
    /// Instance directory layout for path reporting.
    pub oikos: Arc<Oikos>,
    /// Runtime configuration for config readability checks.
    pub config: Arc<tokio::sync::RwLock<AletheiaConfig>>,
    /// Active embedding provider (for degraded-mode reporting, #3380).
    pub embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
}

impl FromRef<Arc<AppState>> for HealthState {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self {
            session_store: Arc::clone(&state.session_store),
            provider_registry: Arc::clone(&state.provider_registry),
            nous_manager: Arc::clone(&state.nous_manager),
            start_time: state.start_time,
            oikos: Arc::clone(&state.oikos),
            config: Arc::clone(&state.config),
            embedding_provider: state.embedding_provider.clone(),
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
    /// Shared Prometheus metrics registry for encoding scrapes.
    pub metrics_registry: MetricsRegistry,
}

impl FromRef<Arc<AppState>> for MetricsState {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self {
            session_store: Arc::clone(&state.session_store),
            start_time: state.start_time,
            metrics_registry: state.metrics_registry.clone(),
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
    /// Instance directory layout for file resolution.
    pub oikos: Arc<Oikos>,
}

impl FromRef<Arc<AppState>> for NousState {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self {
            nous_manager: Arc::clone(&state.nous_manager),
            tool_registry: Arc::clone(&state.tool_registry),
            oikos: Arc::clone(&state.oikos),
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
    /// Runtime configuration for API limit reads.
    pub config: Arc<tokio::sync::RwLock<AletheiaConfig>>,
    /// Per-turn event buffer registry for SSE client recovery (#3276).
    pub turn_buffer_registry: Arc<TurnBufferRegistry>,
    /// In-process broadcast bus for domain events.
    pub event_bus: Arc<EventBus>,
}

impl FromRef<Arc<AppState>> for SessionsState {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self {
            session_store: Arc::clone(&state.session_store),
            nous_manager: Arc::clone(&state.nous_manager),
            provider_registry: Arc::clone(&state.provider_registry),
            shutdown: state.shutdown.clone(),
            idempotency_cache: Arc::clone(&state.idempotency_cache),
            config: Arc::clone(&state.config),
            turn_buffer_registry: Arc::clone(&state.turn_buffer_registry),
            event_bus: Arc::clone(&state.event_bus),
        }
    }
}

/// State slice for knowledge store handlers.
#[derive(Clone)]
pub struct KnowledgeState {
    /// Shared knowledge store for fact/entity/relationship queries.
    #[cfg(feature = "knowledge-store")]
    pub knowledge_store: Option<Arc<KnowledgeStore>>,
    /// Runtime configuration for API limit reads.
    pub config: Arc<tokio::sync::RwLock<AletheiaConfig>>,
    /// In-process broadcast bus for domain events.
    pub event_bus: Arc<EventBus>,
}

impl FromRef<Arc<AppState>> for KnowledgeState {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self {
            #[cfg(feature = "knowledge-store")]
            knowledge_store: state.knowledge_store.clone(),
            config: Arc::clone(&state.config),
            event_bus: Arc::clone(&state.event_bus),
        }
    }
}

#[cfg(test)]
const _: fn() = || {
    fn assert<T: Send + Sync>() {}
    assert::<AppState>();
};

/// State slice for meta-insights handlers.
#[derive(Clone)]
pub struct InsightsState {
    /// Persistent session and message storage.
    pub session_store: Arc<Mutex<SessionStore>>,
    /// Manages nous actor lifecycles and provides handles.
    pub nous_manager: Arc<NousManager>,
}

impl FromRef<Arc<AppState>> for InsightsState {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self {
            session_store: Arc::clone(&state.session_store),
            nous_manager: Arc::clone(&state.nous_manager),
        }
    }
}

/// State slice for the domain-event subscription handler.
#[derive(Clone)]
pub struct EventBusState {
    /// In-process broadcast bus for domain events.
    pub event_bus: Arc<EventBus>,
    /// Runtime configuration for heartbeat interval reads.
    pub config: Arc<tokio::sync::RwLock<AletheiaConfig>>,
}

impl FromRef<Arc<AppState>> for EventBusState {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self {
            event_bus: Arc::clone(&state.event_bus),
            config: Arc::clone(&state.config),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const _: fn() = || {
        fn assert<T: Send + Sync + Clone>() {}
        assert::<HealthState>();
        assert::<MetricsState>();
        assert::<NousState>();
        assert::<ConfigState>();
        assert::<SessionsState>();
        assert::<KnowledgeState>();
        assert::<InsightsState>();
        assert::<EventBusState>();
    };
}
