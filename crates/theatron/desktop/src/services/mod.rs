//! Background services that bridge async I/O to reactive state.
//!
//! Each service runs as a background task (Dioxus coroutine or tokio task)
//! and writes into signal-backed state as events arrive.

pub mod sse;
