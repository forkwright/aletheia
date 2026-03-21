//! Unicode-to-ASCII character folding table.
//!
//! Maps Unicode characters to their ASCII equivalents where one exists.
//! Dispatches to sub-tables by character category.

mod fold_digits_symbols;
mod fold_letters_a_m;
mod fold_letters_n_z;

use fold_digits_symbols::fold_digit_or_symbol;
use fold_letters_a_m::fold_letter_a_m;
use fold_letters_n_z::fold_letter_n_z;

pub(super) fn fold_non_ascii_char(c: char) -> Option<&'static str> {
    fold_letter_a_m(c)
        .or_else(|| fold_letter_n_z(c))
        .or_else(|| fold_digit_or_symbol(c))
}
