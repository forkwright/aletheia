//! HTTP client, SSE connection, and per-message streaming for the Aletheia API.

pub mod client;
pub mod error;
pub mod sse;
pub mod streaming;
pub mod types;

pub use client::ApiClient;
pub use error::ApiError;
