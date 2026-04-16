//! Computer use tool: screen capture, action dispatch, and sandboxed execution.
//!
//! Integrates with Anthropic's computer use API to provide:
//! - Screen capture via `scrot` (X11) or `grim` (Wayland)
//! - Coordinate-based actions: `click`, `type_text`, `key`, `scroll`
//! - Landlock LSM sandbox restricting filesystem access during sessions
//! - Result extraction with frame diff and structured change descriptions
//!
//! # Requirements
//!
//! - Linux kernel 5.13+ for Landlock sandbox support
//! - `scrot` or `grim` for screen capture
//! - `xdotool` for input simulation (X11)
//!
//! Feature-gated behind `computer-use` -- not compiled by default.

/// Action dispatch via xdotool.
mod actions;
/// Screen capture and frame diff logic.
mod capture;
/// Tool executor, definition, and registration.
mod executor;
/// Landlock sandbox session configuration.
mod sandbox;
/// Core types (actions, results, diff regions).
mod types;

pub use executor::register;
