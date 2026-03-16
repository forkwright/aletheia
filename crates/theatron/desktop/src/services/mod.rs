//! Background services for the desktop application.
//!
//! Services run as async tasks (spawned or coroutines) and communicate
//! with the UI layer via Dioxus signals. Each service owns its lifecycle
//! and respects `CancellationToken` for clean shutdown.

pub mod config;
pub mod connection;
