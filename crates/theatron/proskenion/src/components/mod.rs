//! Dioxus components for the streaming chat interface.

/// Agent roster panel for the left sidebar (soft shaded box, distinct from nav).
pub(crate) mod agent_sidebar;
/// SVG chart primitives: horizontal bars, stacked bars, line charts, percentile bars.
pub(crate) mod chart;
pub mod chat;
pub(crate) mod checkpoint_card;
// code_block module extracted to skeue::CodeBlock and
// gramma::highlight_code (chalkeion desktop UI integration).
pub mod command_palette;
/// Confidence bar with color-coded thresholds (green/amber/red).
pub(crate) mod confidence_bar;
pub mod connection_indicator;
pub(crate) mod coverage_bar;
// diff_hunk and diff_line modules extracted to
// skeue::{DiffHunkView, DiffLineView} (chalkeion desktop UI integration).
pub mod distillation;
/// Help overlay listing all keyboard shortcuts (F1).
pub(crate) mod help_overlay;
pub(crate) mod input_bar;
pub(crate) mod markdown;
pub(crate) mod message;
pub(crate) mod option_card;
pub(crate) mod plan_card;
pub(crate) mod planning_card;
/// Quick input overlay for the global hotkey launcher.
pub(crate) mod quick_input;
/// Reusable drag-to-resize panel divider.
pub(crate) mod resize_handle;
/// Transparent routing indicator for neurodivergent UX (#2411).
pub(crate) mod routing_indicator;
pub mod session_tabs;
// table module extracted to skeue::MdTable (chalkeion desktop UI integration).
// See `skeue::table`.
pub(crate) mod theme_toggle;
pub(crate) mod thinking;
/// Reusable horizontal timeline with zoom, pan, and dependency arrows.
pub(crate) mod timeline;
// toast module extracted to skeue::ToastItem (chalkeion desktop UI integration).
// See `skeue::toast`.
pub(crate) mod toast_container;
pub(crate) mod tool_approval;
pub(crate) mod tool_panel;
pub(crate) mod tool_status;
/// Top bar with brand, agent status pills, and connection/theme controls.
pub(crate) mod topbar;
// virtual_list module extracted to skeue (chalkeion desktop UI integration).
// See `skeue::virtual_list`.
pub(crate) mod wave_band;
