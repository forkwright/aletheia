//! Reactive state model for Dioxus-based Theatron frontends.
//!
//! Each module defines the data shape for a domain. The types are plain structs
//! and enums, suitable for wrapping in `Signal<T>` or `Store<T>` at the
//! component layer.

pub mod agents;
pub mod app;
pub(crate) mod chat;
/// Checkpoint approval gate state for the planning project detail view.
pub(crate) mod checkpoints;
pub mod collections;
pub mod commands;
pub mod connection;
/// Credential management state for the ops view.
pub(crate) mod credentials;
pub(crate) mod diff;
/// Discussion state for planning gray-area questions.
pub(crate) mod discussion;
pub mod events;
/// Execution state for wave-based plan progress.
pub(crate) mod execution;
pub(crate) mod fetch;
/// Workspace file tree explorer state.
pub(crate) mod files;
pub(crate) mod input;
pub(crate) mod navigation;
/// Planning project, requirements, and roadmap state.
pub(crate) mod planning;
/// Session list, detail, and selection state.
pub(crate) mod sessions;
pub(crate) mod streaming;
pub mod toasts;
/// Enhanced tool call, approval, and planning state for desktop UI.
pub mod tools;
/// Goal-backward verification state for the planning project detail view.
pub(crate) mod verification;

/// Ops dashboard state: agent status cards, service health, toggle controls.
pub(crate) mod ops;
/// Settings state: server configs, appearance, keybindings, wizard flow.
pub(crate) mod settings;
