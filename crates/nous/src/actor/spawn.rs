//! Actor spawning and workspace validation.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};

use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, info, warn};

use mneme::embedding::EmbeddingProvider;
#[cfg(feature = "knowledge-store")]
use mneme::knowledge_store::KnowledgeStore;
use mneme::store::SessionStore;

use aletheia_routing::Router;
use hermeneus::provider::ProviderRegistry;
use organon::registry::ToolRegistry;
use taxis::cascade;
use taxis::oikos::Oikos;

use crate::bootstrap::BootstrapSection;
use crate::config::{NousConfig, PipelineConfig};
use crate::cross::CrossNousEnvelope;
use crate::handle::NousHandle;

use super::{DEFAULT_INBOX_CAPACITY, NousActor};

/// Spawn a nous actor, returning its handle and join handle.
///
/// Creates a bounded channel with [`DEFAULT_INBOX_CAPACITY`], builds the actor,
/// and starts it on the Tokio runtime.
///
/// `cancel` is a child token derived from the manager's root token.
/// When cancelled, the actor exits its message loop and releases all resources,
/// ensuring fjall WAL and other state are flushed before the task completes.
#[expect(
    clippy::too_many_arguments,
    reason = "actor spawn requires all runtime dependencies"
)]
pub(crate) fn spawn(
    config: NousConfig,
    pipeline_config: PipelineConfig,
    providers: Arc<ProviderRegistry>,
    tools: Arc<ToolRegistry>,
    oikos: Arc<Oikos>,
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    vector_search: Option<Arc<dyn crate::recall::VectorSearch>>,
    // kanon:ignore RUST/no-arc-mutex-anti-pattern — std::sync::Mutex for SessionStore in block_in_place bridge
    session_store: Option<Arc<Mutex<SessionStore>>>,
    #[cfg(feature = "knowledge-store")] knowledge_store: Option<Arc<KnowledgeStore>>,
    tool_services: Option<Arc<organon::types::ToolServices>>,
    extra_bootstrap: Vec<BootstrapSection>,
    cross_rx: Option<mpsc::Receiver<CrossNousEnvelope>>,
    cross_tx: Option<mpsc::Sender<CrossNousEnvelope>>,
    cancel: CancellationToken,
    nous_behavior: taxis::config::NousBehaviorConfig,
    tool_config: Arc<taxis::config::ToolLimitsConfig>,
    audit_log: Option<Arc<crate::audit::PromptAuditLog>>,
    router: Option<Arc<dyn Router>>,
    cross_router: Option<Arc<crate::cross::CrossNousRouter>>,
) -> (
    NousHandle,
    tokio::task::JoinHandle<()>,
    Arc<AtomicBool>,
    Arc<AtomicU64>,
) {
    let (tx, rx) = mpsc::channel(DEFAULT_INBOX_CAPACITY);
    let id = config.id.to_string();
    let handle = NousHandle::new(id.clone(), tx);

    let active_turn = Arc::new(AtomicBool::new(false));
    let turn_started_at_ms = Arc::new(AtomicU64::new(0));

    let actor = NousActor::new(
        id.clone(),
        config,
        pipeline_config,
        rx,
        cross_rx,
        cross_tx,
        cancel,
        providers,
        tools,
        oikos,
        embedding_provider,
        vector_search,
        session_store,
        #[cfg(feature = "knowledge-store")]
        knowledge_store,
        tool_services,
        extra_bootstrap,
        Arc::clone(&active_turn),
        Arc::clone(&turn_started_at_ms),
        nous_behavior,
        tool_config,
        audit_log,
        router,
        cross_router,
    );

    let span = tracing::info_span!("nous_actor", nous.id = %id);
    let join_handle = tokio::spawn(async move { actor.run().await }.instrument(span));

    (handle, join_handle, active_turn, turn_started_at_ms)
}

/// Validate the workspace directory exists and required files are resolvable.
///
/// Called at actor startup before entering the message loop. Creates the
/// workspace directory if missing and fails fast if SOUL.md cannot be found
/// through the cascade.
pub(crate) async fn validate_workspace(oikos: &Oikos, nous_id: &str) -> crate::error::Result<()> {
    let workspace = oikos.nous_dir(nous_id);
    let exists = tokio::fs::try_exists(&workspace).await.unwrap_or(false);
    if !exists {
        warn!(
            agent = nous_id,
            path = %workspace.display(),
            "workspace directory missing, creating"
        );
        tokio::fs::create_dir_all(&workspace).await.map_err(|e| {
            crate::error::WorkspaceValidationSnafu {
                nous_id: nous_id.to_owned(),
                message: format!("failed to create workspace directory: {e}"),
            }
            .build()
        })?;
    }

    if cascade::resolve(oikos, nous_id, "SOUL.md", None).is_none() {
        return Err(crate::error::WorkspaceValidationSnafu {
            nous_id: nous_id.to_owned(),
            message: "SOUL.md not found in cascade (nous/, shared/, theke/)".to_owned(),
        }
        .build());
    }

    for filename in &[
        "USER.md",
        "AGENTS.md",
        "GOALS.md",
        "TOOLS.md",
        "MEMORY.md",
        "IDENTITY.md",
        "PROSOCHE.md",
        "CONTEXT.md",
    ] {
        if cascade::resolve(oikos, nous_id, filename, None).is_none() {
            debug!(
                agent = nous_id,
                file = *filename,
                "optional workspace file not found"
            );
        }
    }

    info!(agent = nous_id, "workspace validated");
    Ok(())
}
