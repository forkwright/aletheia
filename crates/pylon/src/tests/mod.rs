#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::expect_used, reason = "test assertions")]

mod api;
mod auth;
mod errors;
mod helpers;
mod messages;
mod sessions;
mod sse_types;
