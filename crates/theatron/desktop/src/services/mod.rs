//! Background services that bridge async I/O to reactive state.
//!
//! Each service runs as a background task (Dioxus coroutine or tokio task)
//! and writes into signal-backed state as events arrive.

pub mod config;
pub mod connection;
pub(crate) mod file_watcher;
pub mod sse;
pub(crate) mod sse_coroutine;
pub(crate) mod streaming;
pub(crate) mod toast;
