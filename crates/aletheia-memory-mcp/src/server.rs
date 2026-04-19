//! `MemoryServer`: stdio MCP server exposing read-only memory tools.
//!
//! The server holds an `Arc<KnowledgeStore>` opened either from an explicit
//! path (fjall) or in-memory for tests. Each tool call dispatches through
//! the `#[tool_router]` macro; long-running store queries run on a blocking
//! task pool via `spawn_blocking` to avoid stalling the stdio reactor.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rmcp::ServiceExt;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{Implementation, InitializeResult, ServerCapabilities};
use rmcp::tool_handler;

use mneme::knowledge_store::{KnowledgeConfig, KnowledgeStore};
use snafu::ResultExt as _;

use crate::error::{self, JoinSnafu, OpenStoreSnafu, TransportSnafu};

/// MCP server surface for Aletheia's memory layer.
///
/// Clone is cheap (Arc-based). A single instance is registered on the
/// stdio transport and handles every request for the process lifetime.
#[derive(Clone)]
pub struct MemoryServer {
    pub(crate) store: Arc<KnowledgeStore>,
    pub(crate) store_path: Option<PathBuf>,
    #[expect(
        dead_code,
        reason = "read by #[tool_handler] macro-generated code in ServerHandler impl"
    )]
    tool_router: ToolRouter<Self>,
}

impl MemoryServer {
    /// Build a memory server from a pre-opened knowledge store.
    ///
    /// `store_path` is surfaced by `memory_stats` so callers can confirm which
    /// on-disk database is being served. Pass `None` for in-memory stores.
    #[must_use]
    pub fn new(store: Arc<KnowledgeStore>, store_path: Option<PathBuf>) -> Self {
        Self {
            store,
            store_path,
            tool_router: Self::tool_router(),
        }
    }

    /// Open a persistent knowledge store at `path` (fjall LSM-tree).
    ///
    /// The parent directory must exist and be writable. Returns an error if
    /// the on-disk format is incompatible with the current schema version.
    pub fn open_fjall(path: impl AsRef<Path>) -> error::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let store = KnowledgeStore::open_fjall(&path, KnowledgeConfig::default()).map_err(|e| {
            OpenStoreSnafu {
                message: e.to_string(),
            }
            .build()
        })?;
        Ok(Self::new(store, Some(path)))
    }

    /// Open an in-memory knowledge store (for tests and ephemeral use).
    pub fn open_in_memory() -> error::Result<Self> {
        let store = KnowledgeStore::open_mem().map_err(|e| {
            OpenStoreSnafu {
                message: e.to_string(),
            }
            .build()
        })?;
        Ok(Self::new(store, None))
    }

    /// Serve MCP over stdio until the peer closes the connection.
    ///
    /// Blocks the current tokio task; call from `main` after configuring
    /// tracing. Reads JSON-RPC requests from stdin and writes responses to
    /// stdout. stderr is free for log output.
    #[tracing::instrument(skip_all)]
    pub async fn serve_stdio(self) -> error::Result<()> {
        let service = self
            .serve(rmcp::transport::io::stdio())
            .await
            .map_err(|e| {
                TransportSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;

        service.waiting().await.map_err(|e| {
            TransportSnafu {
                message: e.to_string(),
            }
            .build()
        })?;

        Ok(())
    }

    /// Run a blocking knowledge-store closure on the tokio blocking pool.
    ///
    /// Every sync `KnowledgeStore` call must go through this helper because
    /// Datalog queries can block on disk I/O and serializing them onto the
    /// single-threaded stdio reactor would starve concurrent tool calls.
    pub(crate) async fn run_blocking<F, T>(&self, f: F) -> error::Result<T>
    where
        F: FnOnce(Arc<KnowledgeStore>) -> error::Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let store = Arc::clone(&self.store);
        tokio::task::spawn_blocking(move || f(store))
            .await
            .context(JoinSnafu)?
    }
}

// NOTE: required type alias per rmcp: `get_info` must return this exact name.
type ServerInfo = InitializeResult;

/// rmcp `ServerHandler` binding. The macro expands to a routing table over
/// the `#[tool]` methods on `MemoryServer` (defined in `tools.rs`).
#[tool_handler]
impl rmcp::handler::server::ServerHandler for MemoryServer {
    fn get_info(&self) -> ServerInfo {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "aletheia-memory-mcp",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "Read-only MCP surface for Aletheia's knowledge graph. \
                 Use memory_search for recall, memory_neighbors for graph \
                 traversal, memory_list_topics for fact-type enumeration, \
                 and memory_stats for health and counts. Writes are not \
                 exposed in this surface.",
            )
    }
}
