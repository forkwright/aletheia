//! SSE (Server-Sent Events) parser — re-exported from theatron-net.
//!
//! The owned SSE parser was extracted to `theatron_net::sse` (W4
//! chalkeion plan). This module is now a thin re-export so existing
//! `skene::sse` paths continue to work; consumers may switch to
//! `theatron_net::sse` directly at any time.

pub use theatron_net::sse::{SseEvent, SseStream};
