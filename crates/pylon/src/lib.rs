//! Axum HTTP gateway for Aletheia.

pub mod error;
pub mod extract;
pub mod handlers;
pub mod router;
pub mod server;
pub mod state;
pub mod stream;

#[cfg(test)]
mod tests;
