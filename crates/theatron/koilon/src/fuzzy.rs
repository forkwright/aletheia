//! Re-export of parodos's fuzzy module for koilon consumers.
//!
//! The implementation now lives in `theatron/parodos`. parodos's
//! `fuzzy_match` was lifted verbatim from this file in W2 of the
//! chalkeion plan; the shim preserves the `crate::fuzzy::*` import
//! surface that koilon's command-palette and slash-completion
//! consumers (`crate::command::mod`, `crate::update::slash`) rely
//! on.

pub use parodos::fuzzy::*;
