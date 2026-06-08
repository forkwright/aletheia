//! Sinks: emit a [`ResolvedTheme`](crate::ResolvedTheme) into a per-target
//! brand-asset format. One source → seven sinks is the rule B-002 names; each
//! sink family is the single owner of one format's serialization.
//!
//! - [`css`] — CSS custom properties for the HTML / printed-deck path.
//! - [`ooxml`] — `theme1.xml` `clrScheme` + `fontScheme` for the PPTX path.
//! - [`docvars`] — flat key/value map for the Pandoc document path.
//! - [`latex`] — LaTeX preamble for document emission.
//! - [`pptx`] — packed base PPTX template with the theme baked in.
//! - [`reference_docx`] — packed `reference.docx` template for Pandoc DOCX.
//! - [`typst`] — Typst prelude for the PDF report path.

/// CSS custom properties sink for the HTML / printed-deck path.
pub mod css;
/// Pandoc-shaped doc-vars sink (JSON + YAML).
pub mod docvars;
/// `LaTeX` preamble sink.
pub mod latex;
/// OOXML `theme1.xml` (`clrScheme` + `fontScheme`) sink for the PPTX path.
pub mod ooxml;
/// Packed base-PPTX sink.
pub mod pptx;
/// Pandoc `reference.docx` sink.
pub mod reference_docx;
/// Typst prelude sink.
pub mod typst;

pub use css::emit_css;
pub use docvars::{emit_docvars_json, emit_docvars_yaml};
pub use latex::emit_latex_template;
pub use ooxml::emit_theme_xml;
pub use pptx::emit_base_pptx;
pub use reference_docx::emit_reference_docx;
pub use typst::emit_typst_template;
