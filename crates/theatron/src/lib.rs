#![deny(missing_docs)]
//! aletheia-theatron: thin facade re-exporting skene types.
//!
//! Theatron (Θέατρον): "theatre." Consumers write `use theatron::ApiClient`
//! instead of `use skene::api::ApiClient`. TUI and desktop are
//! separate binaries and are not re-exported here.

// ── API client ────────────────────────────────────────────────────────────────

/// HTTP client and error types for the pylon REST API.
///
/// Provides [`ApiClient`] for all REST endpoints (agents, sessions, history,
/// auth, costs, streaming) and [`ApiError`] for structured error handling.
///
/// [`ApiClient`]: skene::api::ApiClient
/// [`ApiError`]: skene::api::ApiError
pub use skene::api;

/// HTTP client for the pylon REST API (re-exported from `skene`).
pub use skene::api::ApiClient;

/// Structured API error type (re-exported from `skene`).
pub use skene::api::ApiError;

// ── Domain identifiers ────────────────────────────────────────────────────────

/// Newtype wrappers for domain identifiers shared across all frontends
/// (re-exported from `skene`).
///
/// Includes [`NousId`], [`SessionId`], [`TurnId`], [`ToolId`], and [`PlanId`].
///
/// [`NousId`]: skene::id::NousId
/// [`SessionId`]: skene::id::SessionId
/// [`TurnId`]: skene::id::TurnId
/// [`ToolId`]: skene::id::ToolId
/// [`PlanId`]: skene::id::PlanId
pub use skene::id;

/// Agent (nous) identifier newtype (re-exported from `skene`).
pub use skene::id::NousId;

/// Session identifier newtype (re-exported from `skene`).
pub use skene::id::SessionId;

/// Turn identifier newtype (re-exported from `skene`).
pub use skene::id::TurnId;

/// Tool call identifier newtype (re-exported from `skene`).
pub use skene::id::ToolId;

/// Plan identifier newtype (re-exported from `skene`).
pub use skene::id::PlanId;

// ── Streaming events ──────────────────────────────────────────────────────────

/// Parsed streaming events from the per-session SSE endpoint
/// (re-exported from `skene`).
///
/// [`StreamEvent`] covers the full per-turn event lifecycle: start, text
/// deltas, thinking deltas, tool calls, plan steps, completion, and errors.
///
/// [`StreamEvent`]: skene::events::StreamEvent
pub use skene::events;

/// Per-turn streaming event enum (re-exported from `skene`).
pub use skene::events::StreamEvent;

// ── SSE wire protocol ─────────────────────────────────────────────────────────

/// SSE wire-protocol parser for reqwest byte streams
/// (re-exported from `skene`).
///
/// [`SseStream`] transforms a byte stream into parsed [`SseEvent`]s.
///
/// [`SseStream`]: skene::sse::SseStream
/// [`SseEvent`]: skene::sse::SseEvent
pub use skene::sse;

/// Parsed SSE event from the wire protocol (re-exported from `skene`).
pub use skene::sse::SseEvent;

/// SSE byte-stream-to-event parser (re-exported from `skene`).
pub use skene::sse::SseStream;

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
