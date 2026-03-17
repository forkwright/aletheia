//! Integration tests for the pylon HTTP gateway.

#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::expect_used, reason = "test assertions")]

mod auth;
mod error;
mod error_envelope;
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
