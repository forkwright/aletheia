//! Axum HTTP gateway for Aletheia.
//!
//! Exposes the REST API and SSE streaming endpoints consumed by the UI and
//! external clients. Delegates business logic to [`aletheia_nous`] actors and
//! [`aletheia_mneme`] storage through shared [`state::AppState`].

pub(crate) mod error;
pub(crate) mod extract;
pub(crate) mod handlers;
pub mod router;
pub mod server;
pub mod state;
pub(crate) mod stream;

#[cfg(test)]
mod tests;
