//! Background services that bridge async I/O to reactive state.
//!
//! Each service runs as a background task (Dioxus coroutine or tokio task)
//! and writes into signal-backed state as events arrive.

/// In-memory API response cache with TTL and request deduplication.
pub(crate) mod cache;
pub mod config;
pub mod connection;
pub(crate) mod file_watcher;
/// Global keyboard navigation handler.
pub(crate) mod keybindings;
pub mod sse;
pub(crate) mod notification_dispatch;
pub(crate) mod sse_coroutine;
pub(crate) mod streaming;
pub(crate) mod toast;
/// Settings config persistence: server list, appearance, keybindings.
pub(crate) mod settings_config;
