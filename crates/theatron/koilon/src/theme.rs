//! Re-export of parodos's theme module for koilon consumers.
//!
//! The implementation now lives in `theatron/parodos`. This shim
//! preserves the `crate::theme::*` import surface that koilon's
//! views and update handlers rely on.

pub use parodos::theme::*;

/// Auto-detected theme, available to tests for deterministic output.
///
/// Mirrors the `THEME` static that koilon's tests previously relied on
/// when the implementation lived here. Wrapped here (not in parodos)
/// because the trigger to build it is koilon-test-tier specific.
#[cfg(test)]
pub static THEME: std::sync::LazyLock<Theme> = std::sync::LazyLock::new(Theme::default);
