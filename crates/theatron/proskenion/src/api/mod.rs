//! HTTP client, SSE connection, and per-message streaming for the desktop UI.

pub(crate) mod client;
pub(crate) mod error;
pub(crate) mod health;
pub mod sse;
pub mod streaming;
/// Backend subsystem status contract: `GET /api/v1/system/status` (#5315).
pub(crate) mod system_status;
