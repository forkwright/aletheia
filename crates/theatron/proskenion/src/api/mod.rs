//! HTTP client and global SSE connection for the desktop UI.
//!
//! Per-turn streaming and health checks are owned by `skene`
//! (`skene::api::streaming`, `skene::api::health`) end-to-end (#4925); this
//! module no longer carries local copies of either.

pub(crate) mod client;
pub mod sse;
/// Backend subsystem status contract: `GET /api/v1/system/status` (#5315).
pub(crate) mod system_status;
