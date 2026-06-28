//! `MemoryServer`: stdio MCP server exposing read tools and token-gated writes.
//!
//! The server holds an `Arc<KnowledgeStore>` opened either from an explicit
//! path (fjall) or in-memory for tests. Each tool call dispatches through
//! the `#[tool_router]` macro; long-running store queries run on a blocking
//! task pool via `spawn_blocking` to avoid stalling the stdio reactor. Write
//! tools are registered only when `ALETHEIA_MEMORY_MCP_WRITE_TOKEN` is present.

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
    /// Whether `nous_stats` may expose full local store paths on explicit request.
    pub(crate) admin_diagnostics: bool,
    /// Bound caller identity (nous) for read-tool recall scope.
    /// If `None`, read tools reject every request.
    pub(crate) nous_id: Option<String>,
    /// Capability token for write tools, if configured.
    /// If `None`, write tools are not registered.
    pub(crate) write_token: Option<String>,
    tool_router: ToolRouter<Self>,
}

impl MemoryServer {
    const WRITE_TOOLS: [&'static str; 3] = ["nous_annotate", "nous_supersede", "nous_forget"];

    /// Minimum token length enforcing the documented 32-byte random token expectation.
    const MIN_WRITE_TOKEN_LEN: usize = 32;

    /// Environment flag that enables admin-only path diagnostics.
    ///
    /// Full paths remain hidden unless this flag is truthy and write/admin
    /// capability is configured, then callers still have to ask for the path
    /// in `nous_stats`.
    const ADMIN_DIAGNOSTICS_ENV: &str = "ALETHEIA_MEMORY_MCP_ADMIN_DIAGNOSTICS";

    /// Sanitize a raw caller identity read from env or provided by the caller.
    ///
    /// Returns `None` when the value is absent or blank. A bound identity is
    /// required for all read tools; returning `None` makes them fail closed.
    fn sanitize_nous_id(raw: Option<String>) -> Option<String> {
        match raw {
            None => None,
            Some(s) if s.trim().is_empty() => {
                tracing::warn!(
                    "ALETHEIA_MEMORY_MCP_NOUS_ID is set but blank; read tools will be rejected"
                );
                None
            }
            Some(s) => Some(s.trim().to_owned()),
        }
    }

    /// Sanitize a raw write token value read from env or provided by the caller.
    ///
    /// Returns `None` (write-disabled) when the value is absent, blank, or shorter
    /// than [`MIN_WRITE_TOKEN_LEN`]. A token that is present but too short emits a
    /// tracing warning so operators can detect misconfiguration without the token
    /// value appearing in logs.
    ///
    /// [`MIN_WRITE_TOKEN_LEN`]: Self::MIN_WRITE_TOKEN_LEN
    fn sanitize_write_token(raw: Option<String>) -> Option<String> {
        match raw {
            None => None,
            Some(s) if s.trim().is_empty() => {
                tracing::warn!(
                    "ALETHEIA_MEMORY_MCP_WRITE_TOKEN is set but blank; write tools will be disabled"
                );
                None
            }
            Some(s) if s.len() < Self::MIN_WRITE_TOKEN_LEN => {
                tracing::warn!(
                    min_len = Self::MIN_WRITE_TOKEN_LEN,
                    actual_len = s.len(),
                    "write token is shorter than the minimum required length; write tools will be disabled"
                );
                None
            }
            Some(s) => Some(s),
        }
    }

    fn sanitize_admin_diagnostics(raw: Option<String>, write_token: Option<&String>) -> bool {
        let Some(raw) = raw else {
            return false;
        };
        let normalized = raw.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return false;
        }
        let requested = matches!(normalized.as_str(), "1" | "true" | "yes" | "admin");
        if !requested {
            tracing::warn!(
                env = Self::ADMIN_DIAGNOSTICS_ENV,
                "unsupported admin diagnostics flag value; store paths will remain redacted"
            );
            return false;
        }
        if write_token.is_none() {
            tracing::warn!(
                env = Self::ADMIN_DIAGNOSTICS_ENV,
                "admin diagnostics require ALETHEIA_MEMORY_MCP_WRITE_TOKEN; store paths will remain redacted"
            );
            return false;
        }
        true
    }

    fn router_for(write_token: Option<&String>) -> ToolRouter<Self> {
        let mut tool_router = Self::tool_router();
        if write_token.is_none() {
            for name in Self::WRITE_TOOLS {
                tool_router.remove_route(name);
            }
        }
        tool_router
    }

    /// Build a memory server from a pre-opened knowledge store.
    ///
    /// `store_path` is fingerprinted by `nous_stats` so callers can identify
    /// the served on-disk database without learning local filesystem layout.
    /// Pass `None` for in-memory stores.
    ///
    /// The caller identity is read from `ALETHEIA_MEMORY_MCP_NOUS_ID`.
    /// Write tools are registered if `ALETHEIA_MEMORY_MCP_WRITE_TOKEN` is set.
    #[must_use]
    pub fn new(store: Arc<KnowledgeStore>, store_path: Option<PathBuf>) -> Self {
        let nous_id = Self::sanitize_nous_id(std::env::var("ALETHEIA_MEMORY_MCP_NOUS_ID").ok());
        let write_token =
            Self::sanitize_write_token(std::env::var("ALETHEIA_MEMORY_MCP_WRITE_TOKEN").ok());
        let admin_diagnostics = Self::sanitize_admin_diagnostics(
            std::env::var(Self::ADMIN_DIAGNOSTICS_ENV).ok(),
            write_token.as_ref(),
        );
        let tool_router = Self::router_for(write_token.as_ref());
        Self {
            store,
            store_path,
            admin_diagnostics,
            nous_id,
            write_token,
            tool_router,
        }
    }

    /// Build a memory server with an explicit write token (for testing).
    ///
    /// This bypasses environment variable lookup. For production, use [`Self::new()`]
    /// which reads from `ALETHEIA_MEMORY_MCP_WRITE_TOKEN` and `ALETHEIA_MEMORY_MCP_NOUS_ID`.
    #[must_use]
    pub fn with_write_token(
        store: Arc<KnowledgeStore>,
        store_path: Option<PathBuf>,
        write_token: Option<String>,
    ) -> Self {
        let nous_id = Self::sanitize_nous_id(std::env::var("ALETHEIA_MEMORY_MCP_NOUS_ID").ok());
        let write_token = Self::sanitize_write_token(write_token);
        let tool_router = Self::router_for(write_token.as_ref());
        Self {
            store,
            store_path,
            admin_diagnostics: false,
            nous_id,
            write_token,
            tool_router,
        }
    }

    /// Bind an explicit caller identity (for testing).
    ///
    /// WHY: production identity comes from `ALETHEIA_MEMORY_MCP_NOUS_ID`; tests
    /// must be able to set it without racing on process-global environment.
    #[must_use]
    pub fn with_nous_id(mut self, nous_id: Option<String>) -> Self {
        self.nous_id = Self::sanitize_nous_id(nous_id);
        self
    }

    /// Enable or disable full-path admin diagnostics for tests and controlled callers.
    ///
    /// The flag only takes effect when a write/admin capability token is also
    /// configured; otherwise path diagnostics stay redacted.
    #[must_use]
    pub fn with_admin_diagnostics(mut self, enabled: bool) -> Self {
        self.admin_diagnostics = enabled && self.write_token.is_some();
        self
    }

    /// Return the server-bound caller identity, failing closed when unbound.
    ///
    /// All read tools use this instead of any model-supplied argument.
    pub(crate) fn requester_nous_id(&self) -> error::Result<&str> {
        self.nous_id
            .as_deref()
            .ok_or_else(|| error::CallerNotConfiguredSnafu.build())
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

    /// Enforce server-side write authorization.
    ///
    /// Write tools are registered only when a capability token is configured at
    /// server startup. This method ensures that, even if a route is reachable,
    /// the server fails closed when no token is configured. There is no
    /// model-visible credential to compare; the capability is the server's
    /// configured secret.
    // kanon:ignore RUST/validate-returns-unit — returns Result<()> where Err carries the specific failure reason; Ok(()) signals validation passed
    pub(crate) fn require_write_token(&self) -> error::Result<()> {
        if self.write_token.is_some() {
            Ok(())
        } else {
            Err(error::WriteNotAvailableSnafu.build())
        }
    }
}

// NOTE: required type alias per rmcp: `get_info` must return this exact name.
type ServerInfo = InitializeResult;

/// rmcp `ServerHandler` binding. The macro expands to a routing table over
/// the `#[tool]` methods on `MemoryServer` (defined in `tools.rs`).
#[tool_handler(router = self.tool_router)]
impl rmcp::handler::server::ServerHandler for MemoryServer {
    fn get_info(&self) -> ServerInfo {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "aletheia-memory-mcp",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "MCP surface for Aletheia's nous local knowledge store \
                 (session-scoped, distinct from kanon mnemosyne's durable corpus). \
                 Use nous_search for recall, nous_neighbors for graph \
                 traversal, nous_list_topics for fact-type enumeration, \
                 and nous_stats for health and counts. Write tools are exposed \
                 only when ALETHEIA_MEMORY_MCP_WRITE_TOKEN is configured.",
            )
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    #[test]
    fn require_write_token_accepts_when_configured() {
        let store = KnowledgeStore::open_mem().unwrap();
        let token = "a".repeat(32);
        let server = MemoryServer::with_write_token(store, None, Some(token));
        assert!(server.require_write_token().is_ok());
    }

    #[test]
    fn write_not_available_when_no_token_configured() {
        let store = KnowledgeStore::open_mem().unwrap();
        let server = MemoryServer::with_write_token(store, None, None);
        let result = server.require_write_token();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, crate::error::Error::WriteNotAvailable { .. }));
    }

    #[test]
    fn open_in_memory_creates_server() {
        let server = MemoryServer::open_in_memory().unwrap();
        assert!(server.store_path.is_none());
    }

    #[test]
    fn empty_write_token_is_rejected() {
        let store = KnowledgeStore::open_mem().unwrap();
        let server = MemoryServer::with_write_token(store, None, Some(String::new()));
        // Empty token must be treated as write-disabled
        let result = server.require_write_token();
        assert!(matches!(
            result,
            Err(crate::error::Error::WriteNotAvailable { .. })
        ));
    }

    #[test]
    fn blank_write_token_is_rejected() {
        let store = KnowledgeStore::open_mem().unwrap();
        let server = MemoryServer::with_write_token(store, None, Some("   ".to_owned()));
        let result = server.require_write_token();
        assert!(matches!(
            result,
            Err(crate::error::Error::WriteNotAvailable { .. })
        ));
    }

    #[test]
    fn short_write_token_is_rejected() {
        let store = KnowledgeStore::open_mem().unwrap();
        let short_token = "short".to_owned();
        let server = MemoryServer::with_write_token(store, None, Some(short_token));
        // Token shorter than MIN_WRITE_TOKEN_LEN must be treated as write-disabled
        let result = server.require_write_token();
        assert!(matches!(
            result,
            Err(crate::error::Error::WriteNotAvailable { .. })
        ));
    }

    #[test]
    fn valid_length_write_token_is_accepted() {
        let store = KnowledgeStore::open_mem().unwrap();
        let token = "x".repeat(MemoryServer::MIN_WRITE_TOKEN_LEN);
        let server = MemoryServer::with_write_token(store, None, Some(token));
        assert!(server.require_write_token().is_ok());
    }

    #[test]
    fn requester_nous_id_fails_when_unbound() {
        let store = KnowledgeStore::open_mem().unwrap();
        let server = MemoryServer::with_write_token(store, None, None);
        let result = server.requester_nous_id();
        assert!(matches!(
            result,
            Err(crate::error::Error::CallerNotConfigured { .. })
        ));
    }

    #[test]
    fn requester_nous_id_returns_bound_identity() {
        let store = KnowledgeStore::open_mem().unwrap();
        let server = MemoryServer::with_write_token(store, None, None)
            .with_nous_id(Some("alice".to_owned()));
        assert_eq!(server.requester_nous_id().unwrap(), "alice");
    }

    #[test]
    fn blank_nous_id_is_rejected() {
        let store = KnowledgeStore::open_mem().unwrap();
        let server =
            MemoryServer::with_write_token(store, None, None).with_nous_id(Some("   ".to_owned()));
        assert!(matches!(
            server.requester_nous_id(),
            Err(crate::error::Error::CallerNotConfigured { .. })
        ));
    }
}
