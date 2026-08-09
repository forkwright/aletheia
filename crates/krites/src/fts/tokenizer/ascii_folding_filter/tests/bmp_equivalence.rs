//! Wave 2a conformance gate: the sovereign ASCII-folding table must produce byte-identical
//! output to the derived table over the full Basic Multilingual Plane (retirement PLAN.md
//! Sec.2's "both compile, conformance corpus run against both"). This is the "make it a
//! test, not a one-off script" sweep — every codepoint, not just the derived table's
//! known-covered arms, so an over-eager sovereign fold is caught as surely as a missing one.

use super::super::fold_table::{fold_non_ascii_char_derived, fold_non_ascii_char_sovereign};

#[test]
fn bmp_equivalence() {
    let mut mismatches = Vec::new();
    for codepoint in 0u32..=0xFFFF {
        let Some(c) = char::from_u32(codepoint) else {
            continue; // surrogate range: not a valid `char`
        };
        let derived = fold_non_ascii_char_derived(c);
        let sovereign = fold_non_ascii_char_sovereign(c);
        if derived != sovereign {
            mismatches.push((codepoint, derived, sovereign));
        }
    }
    assert!(
        mismatches.is_empty(),
        "sovereign ascii-folding table diverges from the derived table at {} codepoint(s); \
         first 20 (codepoint, derived, sovereign): {:?}",
        mismatches.len(),
        mismatches.iter().take(20).collect::<Vec<_>>(),
    );
}
