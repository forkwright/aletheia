//! Reactive state model for Dioxus-based Theatron frontends.
//!
//! Each module defines the data shape for a domain. The types are plain structs
//! and enums, suitable for wrapping in `Signal<T>` or `Store<T>` at the
//! component layer.

pub mod agents;
pub mod app;
pub(crate) mod chat;
pub mod collections;
pub mod commands;
pub mod connection;
pub mod events;
pub(crate) mod fetch;
/// Workspace file tree explorer state.
pub(crate) mod files;
pub(crate) mod input;
pub(crate) mod streaming;
pub mod toasts;
/// Enhanced tool call, approval, and planning state for desktop UI.
pub mod tools;
