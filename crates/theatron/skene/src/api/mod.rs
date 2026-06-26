//! HTTP client, SSE connection, and per-message streaming for the Aletheia API.

pub mod client;
pub mod error;
pub mod request_policy;
pub mod routes;
pub mod sse;
pub mod streaming;
pub mod types;

pub use client::ApiClient;
pub use error::ApiError;
pub use request_policy::RequestPolicy;
