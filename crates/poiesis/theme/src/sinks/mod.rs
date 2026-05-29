//! Sinks: emit a [`ResolvedTheme`](crate::ResolvedTheme) into a per-target
//! brand-asset format. One source → three sinks is the rule B-002 names; each
//! sink is the single owner of one format's serialization.
//!
//! - [`css`] — CSS custom properties for the HTML / printed-deck path.
//! - [`ooxml`] — `theme1.xml` `clrScheme` + `fontScheme` for the PPTX path.
//! - [`docvars`] — flat key/value map for the Pandoc document path.

/// CSS custom properties sink for the HTML / printed-deck path.
pub mod css;
/// Pandoc-shaped doc-vars sink (JSON + YAML).
pub mod docvars;
/// OOXML `theme1.xml` (`clrScheme` + `fontScheme`) sink for the PPTX path.
pub mod ooxml;

pub use css::emit_css;
pub use docvars::{emit_docvars_json, emit_docvars_yaml};
pub use ooxml::emit_theme_xml;
