//! Sovereign ASCII-folding table: regenerated from the Unicode Character Database and the
//! CLDR Latin-ASCII transliteration table rather than transcribed from `CozoDB`'s derived
//! match-table. See `generate.py` for the full source/methodology citation and `table.rs`
//! for the generated data (both committed; `table.rs` is not hand-edited).
//!
//! Its fold set is frozen to that of the derived table it replaced, which was proved
//! equivalent over the full Basic Multilingual Plane before being retired.

mod table;

use table::FOLD_TABLE;

pub(super) fn fold_non_ascii_char(c: char) -> Option<&'static str> {
    let codepoint = u32::from(c);
    FOLD_TABLE
        .binary_search_by_key(&codepoint, |&(cp, _)| cp)
        .ok()
        .and_then(|idx| FOLD_TABLE.get(idx))
        .map(|&(_, value)| value)
}
