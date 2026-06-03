//! Native menu bar definition and event mapping.
//!
//! Defines the application menu structure with keyboard accelerators.
//! The actual menu rendering is handled by the Dioxus desktop runtime
//! (backed by the `muda` crate); this module provides the menu model
//! and maps menu item IDs to application actions.

#[cfg(test)]
mod tests {}
