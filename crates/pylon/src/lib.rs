//! Axum HTTP gateway for Aletheia.
//!
//! Pylon (πυλών) — "gateway." Routes HTTP and SSE requests to the agent pipeline.

pub mod error;
pub mod extract;
pub mod handlers;
pub mod router;
pub mod server;
pub mod state;
pub mod stream;

#[cfg(test)]
mod tests;
