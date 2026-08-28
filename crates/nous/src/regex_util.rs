//! Shared compile-time regex construction.
//!
//! [`crate::bootstrap::preinject_scan`] and [`crate::training::pii`] each
//! build their pattern tables from literals that are constant and known-valid
//! at compile time; this owns the one place that documents and asserts that.

use regex::Regex;

/// Compile a regex literal known at compile time to be valid.
///
/// # Panics
///
/// Panics if `re` is not a valid regex — reserved for callers passing a
/// compile-time-constant literal, where that cannot happen in practice.
pub(crate) fn compile_regex(re: &str) -> Regex {
    #[expect(
        clippy::expect_used,
        reason = "compile-time-constant regex literals cannot fail"
    )]
    {
        Regex::new(re).expect("compile-time-constant regex literals cannot fail")
    }
}
