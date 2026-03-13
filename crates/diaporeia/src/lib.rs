//! Diaporeia — MCP server interface for Aletheia.
//!
//! The passage through (διαπορεία) for external AI agents to access Aletheia's
//! cognitive agent capabilities via the Model Context Protocol.
//!
//! # Architecture
//!
//! Diaporeia sits alongside pylon (HTTP gateway) in the same binary, sharing
//! identical `Arc` pointers to `NousManager`, `ToolRegistry`, `SessionStore`,
//! and other core services. Zero serialization overhead for internal access.
//!
//! # Transports
//!
//! - **Streamable HTTP**: Mounted into pylon's Axum router at `/mcp`.
//! - **stdio**: For `aletheia mcp` subcommand (Claude Code / local agent).

pub mod error;
mod resources;
pub(crate) mod sanitize;
pub mod server;
pub mod state;
mod tools;
pub mod transport;
