//! Integration tests for the pylon HTTP gateway.

// TODO(#1915): Replace all .unwrap()/.expect() with proper assertions.
// These suppressions are temporary until the dispatch prompt lands.
#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "TODO(#1915): replace with proper assertions"
)]
#![expect(
    clippy::indexing_slicing,
    reason = "TODO(#1915): replace with bounds-checked access"
)]

mod approval_turns;
mod auth;
mod config;
mod credentials;
mod error;
mod error_envelope;
mod handler_doc;
mod health;
mod helpers;
mod idempotency;
mod insights;
mod knowledge;
mod message;
mod middleware;
mod nous;
mod ops;
mod per_user_rate_limit;
mod route_contract;
mod session;
mod signal;
mod sse_events;
mod streaming;
mod subscribe;
mod workspace;
