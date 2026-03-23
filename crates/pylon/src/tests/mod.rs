//! Integration tests for the pylon HTTP gateway.

#![expect(clippy::unwrap_used, clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: vec/JSON indices valid after asserting len or known structure"
)]

mod auth;
mod error;
mod error_envelope;
mod handler_doc;
mod health;
mod helpers;
mod idempotency;
mod message;
mod middleware;
mod nous;
mod per_user_rate_limit;
mod session;
mod sse_events;
mod streaming;
