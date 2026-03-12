//! Actor spawning and workspace validation.

use std::sync::Arc;

use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, info, warn};

use aletheia_mneme::embedding::EmbeddingProvider;
#[cfg(feature = "knowledge-store")]
use aletheia_mneme::knowledge_store::KnowledgeStore;
use aletheia_mneme::store::SessionStore;

use aletheia_hermeneus::provider::ProviderRegistry;
use aletheia_organon::registry::ToolRegistry;
use aletheia_taxis::cascade;
use aletheia_taxis::oikos::Oikos;

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
/// ensuring redb WAL and other state are flushed before the task completes.
#[expect(
    clippy::too_many_arguments,
    reason = "actor spawn requires all runtime dependencies"
)]
pub fn spawn(
    config: NousConfig,
    pipeline_config: PipelineConfig,
    providers: Arc<ProviderRegistry>,
    tools: Arc<ToolRegistry>,
    oikos: Arc<Oikos>,
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    vector_search: Option<Arc<dyn crate::recall::VectorSearch>>,
    session_store: Option<Arc<Mutex<SessionStore>>>,
    #[cfg(feature = "knowledge-store")] knowledge_store: Option<Arc<KnowledgeStore>>,
    tool_services: Option<Arc<aletheia_organon::types::ToolServices>>,
    extra_bootstrap: Vec<BootstrapSection>,
    cross_rx: Option<mpsc::Receiver<CrossNousEnvelope>>,
    cancel: CancellationToken,
) -> (NousHandle, tokio::task::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel(DEFAULT_INBOX_CAPACITY);
    let id = config.id.clone();
    let handle = NousHandle::new(id.clone(), tx);

    let actor = NousActor::new(
        id.clone(),
        config,
        pipeline_config,
        rx,
        cross_rx,
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
    );

    let span = tracing::info_span!("nous_actor", nous.id = %id);
    let join_handle = tokio::spawn(async move { actor.run().await }.instrument(span));

    (handle, join_handle)
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

    // Log warnings for missing optional workspace files
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
