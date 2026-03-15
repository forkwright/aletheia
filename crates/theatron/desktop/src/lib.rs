//! Dioxus desktop streaming architecture for Aletheia.
//!
//! Provides signal-based SSE and per-message stream consumption
//! designed for reactive UI frameworks like Dioxus. The dual-stream
//! architecture mirrors the TUI's proven pattern while adapting it
//! to Dioxus's signal-driven reactivity model.

pub mod api;
pub mod components;
pub mod theme;
