#![deny(missing_docs)]
//! aletheia-theatron: thin facade re-exporting theatron-core types.
//!
//! Theatron (Θέατρον): "theatre." Consumers write `use theatron::ApiClient`
//! instead of `use theatron_core::api::ApiClient`. TUI and desktop are
//! separate binaries and are not re-exported here.

// ── API client ────────────────────────────────────────────────────────────────

/// HTTP client and error types for the pylon REST API.
///
/// Provides [`ApiClient`] for all REST endpoints (agents, sessions, history,
/// auth, costs, streaming) and [`ApiError`] for structured error handling.
///
/// [`ApiClient`]: theatron_core::api::ApiClient
/// [`ApiError`]: theatron_core::api::ApiError
pub use theatron_core::api;

/// HTTP client for the pylon REST API (re-exported from `theatron-core`).
pub use theatron_core::api::ApiClient;

/// Structured API error type (re-exported from `theatron-core`).
pub use theatron_core::api::ApiError;

// ── Domain identifiers ────────────────────────────────────────────────────────

/// Newtype wrappers for domain identifiers shared across all frontends
/// (re-exported from `theatron-core`).
///
/// Includes [`NousId`], [`SessionId`], [`TurnId`], [`ToolId`], and [`PlanId`].
///
/// [`NousId`]: theatron_core::id::NousId
/// [`SessionId`]: theatron_core::id::SessionId
/// [`TurnId`]: theatron_core::id::TurnId
/// [`ToolId`]: theatron_core::id::ToolId
/// [`PlanId`]: theatron_core::id::PlanId
pub use theatron_core::id;

/// Agent (nous) identifier newtype (re-exported from `theatron-core`).
pub use theatron_core::id::NousId;

/// Session identifier newtype (re-exported from `theatron-core`).
pub use theatron_core::id::SessionId;

/// Turn identifier newtype (re-exported from `theatron-core`).
pub use theatron_core::id::TurnId;

/// Tool call identifier newtype (re-exported from `theatron-core`).
pub use theatron_core::id::ToolId;

/// Plan identifier newtype (re-exported from `theatron-core`).
pub use theatron_core::id::PlanId;

// ── Streaming events ──────────────────────────────────────────────────────────

/// Parsed streaming events from the per-session SSE endpoint
/// (re-exported from `theatron-core`).
///
/// [`StreamEvent`] covers the full per-turn event lifecycle: start, text
/// deltas, thinking deltas, tool calls, plan steps, completion, and errors.
///
/// [`StreamEvent`]: theatron_core::events::StreamEvent
pub use theatron_core::events;

/// Per-turn streaming event enum (re-exported from `theatron-core`).
pub use theatron_core::events::StreamEvent;

// ── SSE wire protocol ─────────────────────────────────────────────────────────

/// SSE wire-protocol parser for reqwest byte streams
/// (re-exported from `theatron-core`).
///
/// [`SseStream`] transforms a byte stream into parsed [`SseEvent`]s.
///
/// [`SseStream`]: theatron_core::sse::SseStream
/// [`SseEvent`]: theatron_core::sse::SseEvent
pub use theatron_core::sse;

/// Parsed SSE event from the wire protocol (re-exported from `theatron-core`).
pub use theatron_core::sse::SseEvent;

/// SSE byte-stream-to-event parser (re-exported from `theatron-core`).
pub use theatron_core::sse::SseStream;

#[cfg(test)]
mod tests {
    #[test]
    fn public_types_accessible() {
        // WHY: smoke test verifying re-exports compile and link correctly
        let _ = std::any::type_name::<super::ApiClient>();
        let _ = std::any::type_name::<super::NousId>();
        let _ = std::any::type_name::<super::SessionId>();
        let _ = std::any::type_name::<super::StreamEvent>();
        let _ = std::any::type_name::<super::SseEvent>();
    }
}
