//! SSE (Server-Sent Events) parser — re-exported from `keryx`.
//!
//! The owned SSE parser lives in `keryx::sse` (theatron's networking
//! crate). This module is a thin re-export so existing `skene::sse`
//! paths keep working; consumers may switch to `keryx::sse` directly
//! at any time.

pub use keryx::sse::{SseEvent, SseStream};
