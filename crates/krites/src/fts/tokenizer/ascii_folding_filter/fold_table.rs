//! Unicode-to-ASCII character folding table.
//!
//! Maps Unicode characters to their ASCII equivalents where one exists.
//! Dispatches to sub-tables by character category.
//!
//! Two implementations exist side by side behind the `krites_sovereign_ascii_folding_table`
//! feature (retirement PLAN.md Sec.2 land-dark/soak/delete): the derived match-table
//! (`fold_table/`, default) and the sovereign UCD/CLDR-generated table
//! (`fold_table_sovereign/`). `tests/bmp_equivalence.rs` proves them equivalent over the
//! full BMP; each side compiles only when selected or when running tests, so the
//! non-selected side never sits as dead code in a plain build.

#[expect(
    clippy::too_many_lines,
    reason = "generated Unicode folding table — one match arm per codepoint"
)]
#[cfg(any(not(feature = "krites_sovereign_ascii_folding_table"), test))]
mod fold_digits_symbols;
#[expect(
    clippy::too_many_lines,
    reason = "generated Unicode folding table — one match arm per codepoint"
)]
#[cfg(any(not(feature = "krites_sovereign_ascii_folding_table"), test))]
mod fold_letters_a_m;
#[expect(
    clippy::too_many_lines,
    reason = "generated Unicode folding table — one match arm per codepoint"
)]
#[cfg(any(not(feature = "krites_sovereign_ascii_folding_table"), test))]
mod fold_letters_n_z;

#[cfg(any(feature = "krites_sovereign_ascii_folding_table", test))]
mod fold_table_sovereign;

#[cfg(any(not(feature = "krites_sovereign_ascii_folding_table"), test))]
use fold_digits_symbols::fold_digit_or_symbol;
#[cfg(any(not(feature = "krites_sovereign_ascii_folding_table"), test))]
use fold_letters_a_m::fold_letter_a_m;
#[cfg(any(not(feature = "krites_sovereign_ascii_folding_table"), test))]
use fold_letters_n_z::fold_letter_n_z;

/// The derived match-table lookup (CozoDB-derived, `PROVENANCE.toml` status `dual`).
/// Compiled whenever the sovereign feature is off, and always under `cfg(test)` so
/// `tests/bmp_equivalence.rs` can compare it directly against [`fold_non_ascii_char_sovereign`].
#[cfg(any(not(feature = "krites_sovereign_ascii_folding_table"), test))]
pub(super) fn fold_non_ascii_char_derived(c: char) -> Option<&'static str> {
    fold_letter_a_m(c)
        .or_else(|| fold_letter_n_z(c))
        .or_else(|| fold_digit_or_symbol(c))
}

/// The sovereign UCD/CLDR-generated table lookup (`PROVENANCE.toml` status `sovereign`).
/// Compiled whenever the sovereign feature is on, and always under `cfg(test)` for the
/// same equivalence-test reason as [`fold_non_ascii_char_derived`].
#[cfg(any(feature = "krites_sovereign_ascii_folding_table", test))]
pub(super) fn fold_non_ascii_char_sovereign(c: char) -> Option<&'static str> {
    fold_table_sovereign::fold_non_ascii_char(c)
}

#[cfg(not(feature = "krites_sovereign_ascii_folding_table"))]
pub(super) use fold_non_ascii_char_derived as fold_non_ascii_char;
#[cfg(feature = "krites_sovereign_ascii_folding_table")]
pub(super) use fold_non_ascii_char_sovereign as fold_non_ascii_char;
