//! Dioxus components for the streaming chat interface.

/// Agent presence roster for the sidebar (the sole agent home).
pub(crate) mod agent_sidebar;
/// Shared badge style helpers (wave / checkpoint / tool approval).
pub(crate) mod badge;
/// SVG chart primitives: horizontal bars, stacked bars, line charts, percentile bars.
pub(crate) mod chart;
pub mod chat;
pub(crate) mod checkpoint_card;
pub mod command_palette;
/// Confidence bar with color-coded thresholds (green/amber/red).
pub(crate) mod confidence_bar;
pub mod connection_indicator;
pub(crate) mod coverage_bar;
pub mod distillation;
/// Help overlay listing all keyboard shortcuts (F1).
pub(crate) mod help_overlay;
pub(crate) mod input_bar;
pub(crate) mod markdown;
pub(crate) mod message;
pub(crate) mod option_card;
pub(crate) mod plan_card;
pub(crate) mod planning_card;
/// Quick input overlay for in-window message submission.
pub(crate) mod quick_input;
/// Reusable drag-to-resize panel divider.
pub(crate) mod resize_handle;
/// Transparent routing indicator for neurodivergent UX (#2411).
pub(crate) mod routing_indicator;
pub mod session_tabs;
pub(crate) mod theme_toggle;
pub(crate) mod thinking;
/// Reusable horizontal timeline with zoom, pan, and dependency arrows.
pub(crate) mod timeline;
pub(crate) mod toast_container;
pub(crate) mod tool_approval;
pub(crate) mod tool_panel;
pub(crate) mod tool_status;
/// Top bar with brand, theme toggle, and connection indicator.
pub(crate) mod topbar;
pub(crate) mod wave_band;
