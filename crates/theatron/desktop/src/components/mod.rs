//! Dioxus component prototypes for the streaming chat interface.
//!
//! These modules define the signal-based state management patterns and
//! component architecture for a Dioxus desktop chat UI. The actual Dioxus
//! dependency is deferred until the framework is added; these modules
//! define the state machines and update logic that components will use.

pub mod chat;
pub mod connection_indicator;
pub(crate) mod input_bar;
pub(crate) mod theme_toggle;
pub(crate) mod thinking;
