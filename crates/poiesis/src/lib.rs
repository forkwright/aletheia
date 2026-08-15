//! Poiesis: format-agnostic document model, renderers, inspection, lint, and
//! verification, behind one import surface.
//!
//! WHY: The `poiesis-*` workspace members are internal package names, not a
//! public API story -- there was no crate a consumer could depend on to get
//! "the document stack" as one coherent unit. This facade re-exports each
//! backend as a feature-gated module (`poiesis::doc`, `poiesis::sheet`, ...)
//! so a consumer opts into exactly the renderers it needs while importing one
//! crate. `poiesis-core` (the format-agnostic `Document`/`Block`/`Renderer`
//! types every backend shares) has no feature gate: it is the load-bearing
//! shared model, not an optional backend.
//!
//! Each module's own feature surface (e.g. `poiesis-doc`'s `docx`/`pandoc`/
//! `pdf-typst` split) is unaffected by this facade -- enable the matching
//! feature on the underlying `poiesis-*` crate directly via Cargo feature
//! unification when finer control than "the whole backend" is needed.

pub use poiesis_core as core;

#[cfg(feature = "charts")]
pub use poiesis_charts as charts;
#[cfg(feature = "deck")]
pub use poiesis_deck as deck;
#[cfg(feature = "diff")]
pub use poiesis_diff as diff;
#[cfg(feature = "doc")]
pub use poiesis_doc as doc;
#[cfg(feature = "inspect")]
pub use poiesis_inspect as inspect;
#[cfg(feature = "intake")]
pub use poiesis_intake as intake;
#[cfg(feature = "lint")]
pub use poiesis_lint as lint;
#[cfg(feature = "ooxml-parse")]
pub use poiesis_ooxml_parse as ooxml_parse;
#[cfg(feature = "printer-chromium")]
pub use poiesis_printer_chromium as printer_chromium;
#[cfg(feature = "scaffold")]
pub use poiesis_scaffold as scaffold;
#[cfg(feature = "sheet")]
pub use poiesis_sheet as sheet;
#[cfg(feature = "slides")]
pub use poiesis_slides as slides;
#[cfg(feature = "text")]
pub use poiesis_text as text;
#[cfg(feature = "theme")]
pub use poiesis_theme as theme;
#[cfg(feature = "typst")]
pub use poiesis_typst as typst;
#[cfg(feature = "verify")]
pub use poiesis_verify as verify;

#[cfg(test)]
mod tests {
    // WHY: Exercises the always-on re-export; feature-gated modules are
    // covered by the workspace's per-feature CI matrix (release-feature-
    // policy derived `feature-checks`), not by unit tests in this crate.
    #[test]
    fn core_reexport_is_reachable() {
        let _document = super::core::Document::new("facade smoke test");
    }
}
