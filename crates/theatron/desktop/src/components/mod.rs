//! Dioxus components for the streaming chat interface.

pub mod agent_sidebar;
pub mod chat;
pub(crate) mod checkpoint_card;
pub(crate) mod code_block;
pub mod command_palette;
pub mod connection_indicator;
pub(crate) mod coverage_bar;
pub(crate) mod diff_hunk;
pub(crate) mod diff_line;
pub mod distillation;
pub(crate) mod input_bar;
pub(crate) mod markdown;
pub(crate) mod message;
pub(crate) mod option_card;
pub(crate) mod plan_card;
pub(crate) mod planning_card;
/// Quick input overlay for the global hotkey launcher.
pub(crate) mod quick_input;
pub mod session_tabs;
pub(crate) mod table;
pub(crate) mod theme_toggle;
pub(crate) mod thinking;
/// Reusable horizontal timeline with zoom, pan, and dependency arrows.
pub(crate) mod timeline;
pub(crate) mod toast;
pub(crate) mod toast_container;
pub(crate) mod tool_approval;
pub(crate) mod tool_panel;
pub(crate) mod tool_status;
pub(crate) mod wave_band;
