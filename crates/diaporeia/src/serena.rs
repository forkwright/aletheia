//! Serena LSP MCP client.
//!
//! Connects to a Serena MCP server (github.com/oraios/serena) over a Unix
//! domain socket and proxies tool calls. Serena wraps rust-analyzer as MCP
//! tools, giving agents IDE-level navigation capabilities.
//!
//! # Connection lifecycle
//!
//! 1. Open a Unix socket connection to the Serena server.
//! 2. Run the MCP initialization handshake (`initialize` → `initialized`).
//! 3. Cache the [`Peer<RoleClient>`] for subsequent tool calls.
//! 4. The background task holding the [`RunningService`] keeps the connection
//!    alive until the shutdown token fires.

use std::path::Path;

use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParams;
use rmcp::service::Peer;
use tokio_util::sync::CancellationToken;

use crate::error::{Error, SerenaSnafu, SerenaUnavailableSnafu};

/// Client handle for a running Serena MCP server connection.
///
/// Cheaply cloneable — all clones share the same underlying socket.
#[derive(Clone)]
pub struct SerenaClient {
    peer: Peer<rmcp::RoleClient>,
    shutdown: CancellationToken,
}

impl SerenaClient {
    /// Connect to a Serena MCP server listening on a Unix domain socket.
    ///
    /// Performs the MCP handshake and spawns a background task to keep the
    /// connection alive. Returns a client handle that can be used to call
    /// tools on the Serena server.
    ///
    /// # Errors
    ///
    /// Returns [`Error::SerenaUnavailable`] when the socket does not exist or
    /// the Serena server is not accepting connections.
    pub async fn connect(socket_path: &Path) -> Result<Self, Error> {
        let stream = tokio::net::UnixStream::connect(socket_path)
            .await
            .map_err(|e| {
                SerenaUnavailableSnafu {
                    message: format!(
                        "cannot connect to Serena socket at {path}: {e}",
                        path = socket_path.display()
                    ),
                }
                .build()
            })?;

        let shutdown = CancellationToken::new();
        let running =
            ().serve_with_ct(stream, shutdown.clone())
                .await
                .map_err(|e| {
                    SerenaUnavailableSnafu {
                        message: format!("Serena MCP handshake failed: {e}"),
                    }
                    .build()
                })?;

        let peer = running.peer().clone();

        // Keep the running service alive in a background task so the
        // connection stays open for subsequent tool calls.
        tokio::spawn(async move {
            let _ = running.waiting().await;
        });

        Ok(Self { peer, shutdown })
    }

    /// Call a tool on the Serena server.
    ///
    /// # Errors
    ///
    /// Returns [`Error::SerenaError`] when the Serena server returns an MCP
    /// error or the connection is lost mid-request.
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Map<String, serde_json::Value>,
    ) -> Result<rmcp::model::CallToolResult, Error> {
        let mut params = CallToolRequestParams::new(name.to_owned());
        params.arguments = Some(arguments);
        self.peer.call_tool(params).await.map_err(|e| {
            SerenaSnafu {
                message: e.to_string(),
            }
            .build()
        })
    }

    /// Shut down the client connection.
    ///
    /// Cancels the background task and closes the socket. This is called
    /// automatically when the last clone is dropped if the shutdown token
    /// is not held elsewhere.
    pub fn close(&self) {
        self.shutdown.cancel();
    }
}

// ---------------------------------------------------------------------------
// Mock Serena server for testing
// ---------------------------------------------------------------------------

#[cfg(any(test, feature = "test-core"))]
pub mod mock {
    //! A minimal mock Serena MCP server for end-to-end socket tests.

    use rmcp::handler::server::ServerHandler;
    use rmcp::model::{
        CallToolResult, Content, Implementation, InitializeResult, ServerCapabilities,
    };
    use rmcp::service::RequestContext;
    use rmcp::{tool, tool_handler, tool_router};
    use tokio_util::sync::CancellationToken;

    /// A mock Serena server that returns canned responses for LSP tools.
    #[derive(Clone)]
    pub struct MockSerenaServer;

    #[tool_handler]
    impl ServerHandler for MockSerenaServer {
        fn get_info(&self) -> InitializeResult {
            InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
                .with_server_info(Implementation::new("serena-mock", "0.1.0"))
                .with_instructions("Mock Serena server for testing.")
        }
    }

    #[tool_router]
    impl MockSerenaServer {
        #[tool(description = "Go to definition")]
        #[expect(clippy::unwrap_used, reason = "serde_json::to_string_pretty on json! is infallible")]
        async fn go_to_definition(
            &self,
            _params: rmcp::handler::server::wrapper::Parameters<GoToDefinitionParams>,
            _context: RequestContext<rmcp::RoleServer>,
        ) -> Result<CallToolResult, rmcp::ErrorData> {
            Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&serde_json::json!({
                    "uri": "file:///workspace/crates/koina/src/lib.rs",
                    "range": { "start": { "line": 10, "character": 4 }, "end": { "line": 10, "character": 12 } }
                }))
                .unwrap(),
            )]))
        }

        #[tool(description = "Find references")]
        #[expect(clippy::unwrap_used, reason = "serde_json::to_string_pretty on json! is infallible")]
        async fn find_references(
            &self,
            _params: rmcp::handler::server::wrapper::Parameters<FindReferencesParams>,
            _context: RequestContext<rmcp::RoleServer>,
        ) -> Result<CallToolResult, rmcp::ErrorData> {
            Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&serde_json::json!([
                    { "uri": "file:///workspace/crates/nous/src/lib.rs", "range": { "start": { "line": 5, "character": 0 }, "end": { "line": 5, "character": 10 } } },
                    { "uri": "file:///workspace/crates/pylon/src/lib.rs", "range": { "start": { "line": 8, "character": 0 }, "end": { "line": 8, "character": 10 } } }
                ]))
                .unwrap(),
            )]))
        }

        #[tool(description = "Type hierarchy")]
        #[expect(clippy::unwrap_used, reason = "serde_json::to_string_pretty on json! is infallible")]
        async fn type_hierarchy(
            &self,
            _params: rmcp::handler::server::wrapper::Parameters<TypeHierarchyParams>,
            _context: RequestContext<rmcp::RoleServer>,
        ) -> Result<CallToolResult, rmcp::ErrorData> {
            Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&serde_json::json!({
                    "type": "struct",
                    "name": "AletheiaConfig",
                    "crate": "taxis"
                }))
                .unwrap(),
            )]))
        }

        #[tool(description = "Rename symbol")]
        #[expect(clippy::unwrap_used, reason = "serde_json::to_string_pretty on json! is infallible")]
        async fn rename(
            &self,
            params: rmcp::handler::server::wrapper::Parameters<RenameParams>,
            _context: RequestContext<rmcp::RoleServer>,
        ) -> Result<CallToolResult, rmcp::ErrorData> {
            Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&serde_json::json!({
                    "changes": 3,
                    "new_name": params.0.new_name,
                }))
                .unwrap(),
            )]))
        }

        #[tool(description = "Get diagnostics")]
        #[expect(clippy::unwrap_used, reason = "serde_json::to_string_pretty on json! is infallible")]
        async fn diagnostics(
            &self,
            _params: rmcp::handler::server::wrapper::Parameters<DiagnosticsParams>,
            _context: RequestContext<rmcp::RoleServer>,
        ) -> Result<CallToolResult, rmcp::ErrorData> {
            Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&serde_json::json!({
                    "diagnostics": [],
                    "file": "src/lib.rs"
                }))
                .unwrap(),
            )]))
        }
    }

    use schemars::JsonSchema;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, JsonSchema)]
    #[expect(dead_code, reason = "fields are deserialized by rmcp tool router")]
    struct GoToDefinitionParams {
        file_path: String,
        line: u32,
        column: u32,
    }

    #[derive(Debug, Deserialize, JsonSchema)]
    #[expect(dead_code, reason = "fields are deserialized by rmcp tool router")]
    struct FindReferencesParams {
        file_path: String,
        line: u32,
        column: u32,
    }

    #[derive(Debug, Deserialize, JsonSchema)]
    #[expect(dead_code, reason = "fields are deserialized by rmcp tool router")]
    struct TypeHierarchyParams {
        file_path: String,
        line: u32,
        column: u32,
    }

    #[derive(Debug, Deserialize, JsonSchema)]
    #[expect(dead_code, reason = "fields are deserialized by rmcp tool router")]
    struct RenameParams {
        file_path: String,
        line: u32,
        column: u32,
        new_name: String,
    }

    #[derive(Debug, Deserialize, JsonSchema)]
    #[expect(dead_code, reason = "field is deserialized by rmcp tool router")]
    struct DiagnosticsParams {
        file_path: String,
    }

    /// Start a mock Serena server on a Unix domain socket and return the
    /// socket path and a shutdown token.
    #[expect(clippy::expect_used, reason = "test helper — panicking on setup failure is acceptable")]
    pub async fn start_mock_server(
        dir: &std::path::Path,
    ) -> (std::path::PathBuf, CancellationToken) {
        let socket_path = dir.join("serena.sock");
        let listener = tokio::net::UnixListener::bind(&socket_path).expect("bind unix socket");
        let shutdown = CancellationToken::new();
        let task_shutdown = shutdown.clone();

        tokio::spawn(async move {
            tokio::select! {
                biased;
                () = task_shutdown.cancelled() => {}
                result = listener.accept() => {
                    if let Ok((stream, _)) = result {
                        let server = MockSerenaServer;
                        if let Ok(running) = rmcp::ServiceExt::serve_with_ct(
                            server,
                            stream,
                            task_shutdown.child_token(),
                        ).await {
                            let _ = running.waiting().await;
                        }
                    }
                }
            }
        });

        // Give the listener a moment to start.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        (socket_path, shutdown)
    }
}

#[cfg(test)]
mod tests {
    use super::mock::start_mock_server;
    use super::*;

    #[tokio::test]
    #[expect(clippy::expect_used, reason = "unit test — panicking on failure is the point")]
    async fn client_connects_and_calls_tool() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (socket_path, _shutdown) = start_mock_server(tmp.path()).await;

        let client = SerenaClient::connect(&socket_path).await.expect("connect");
        let mut args = serde_json::Map::new();
        args.insert(
            "file_path".to_owned(),
            serde_json::Value::String("src/lib.rs".to_owned()),
        );
        args.insert("line".to_owned(), serde_json::Value::Number(0.into()));
        args.insert("column".to_owned(), serde_json::Value::Number(0.into()));
        let result = client
            .call_tool("go_to_definition", args)
            .await
            .expect("call tool");

        assert!(
            result.is_error.is_none() || result.is_error == Some(false),
            "tool call should succeed"
        );
        let text = result
            .content
            .into_iter()
            .map(|c| c.as_text().map(|t| t.text.clone()).unwrap_or_default())
            .collect::<String>();
        assert!(
            text.contains("koina"),
            "mock response should reference koina crate"
        );
    }
}
