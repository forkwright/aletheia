//! Unicode-to-ASCII character folding table.
//!
//! Maps Unicode characters to their ASCII equivalents where one exists.
//!
//! The table is generated from UCD/CLDR by `fold_table_sovereign/generate.py`.
//! It replaced a derived match-table whose equivalence over the entire BMP was
//! proven by a dedicated test before that table was retired.
mod fold_table_sovereign;

pub(super) fn fold_non_ascii_char(c: char) -> Option<&'static str> {
    fold_table_sovereign::fold_non_ascii_char(c)
}
