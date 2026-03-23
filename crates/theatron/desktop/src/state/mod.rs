//! Reactive state model for Dioxus-based Theatron frontends.
//!
//! Each module defines the data shape for a domain. The types are plain structs
//! and enums, suitable for wrapping in `Signal<T>` or `Store<T>` at the
//! component layer.

pub mod app;
pub mod collections;
pub mod connection;
pub mod events;
pub(crate) mod fetch;
pub mod toasts;
