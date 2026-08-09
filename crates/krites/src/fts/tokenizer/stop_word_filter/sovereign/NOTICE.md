# Third-party notice — stop word lists

The word lists in [`stopwords.rs`](stopwords.rs) are copied from the
[stopwords-iso](https://github.com/stopwords-iso/stopwords-iso/) project,
copyright Gene Diaz, licensed under the **MIT License**. A copy of that
license sits beside this file at
[LICENSE-MIT-stopwords-iso](../../../../../LICENSE-MIT-stopwords-iso)
(`crates/krites/LICENSE-MIT-stopwords-iso`).

## Why this file exists, and why it is not just a paragraph in `crates/krites/NOTICE.md`

`crates/krites/PROVENANCE.toml` tracks lineage from one upstream, **CozoDB**
(`cozo-core`, MPL-2.0) — the ledger's `[meta]` table names that repository
specifically, and the crate-level `NOTICE.md` is rendered from it. Krites'
stop word lists are not, in the copyright sense, CozoDB's expression: CozoDB
did not author these words. Its own `stop_word_filter/gen_stopwords.py`
fetches the same stopwords-iso JSON this crate's `sovereign/gen_stopwords.py`
was seeded from, writes a header naming stopwords-iso and MIT, and does
nothing else creative to the word content. CozoDB is a second vendor of the
same third-party corpus, not its author — attributing the word data to
"derived from CozoDB" names the wrong copyright holder.

That misattribution is exactly what `derived/` (the CozoDB-lineage,
`PROVENANCE.toml` `dual`-status sibling of this module, soaking before
deletion) inherited: its file-header comments always said "stopwords-iso
project ... MIT license" correctly, but the crate-level `NOTICE.md` — the
actual legal notice a distributor reads — never mentioned stopwords-iso or
MIT anywhere, only CozoDB and MPL-2.0. A recipient reading only
`crates/krites/NOTICE.md` had no way to know a second, MIT-licensed
third-party corpus was embedded in the crate, and MIT's one substantive
condition — "The above copyright notice and this permission notice shall be
included" — was not met by a per-file source comment that never leaves the
Rust doc comments.

This file, plus the vendored license text beside it, is that notice: the
attribution and the required license text, both actually present at the crate
distribution boundary, independent of `crates/krites/NOTICE.md`'s CozoDB/MPL
notice (which continues to cover the `derived/` sibling and every other
CozoDB-lineage file, correctly, until deletion — PLAN.md §2's soak-then-delete
schedule, tracked via that module's own `dual`-status ledger rows).

## What did not change

The word data itself: `sovereign/gen_stopwords.py`'s docstring records that it
reads from the (still-soaking) `derived/stopwords/` copy rather than
re-fetching, and the wave-2b PR description records the verification —
token-multiset identity against both the pre-existing vendored copy and a
fresh stopwords-iso fetch, 21,707 literals across 58 languages, zero
additions, zero removals. PLAN.md conflict C4 decided against re-sourcing:
re-fetching today could pick up upstream corpus changes since CozoDB's
original vendoring, trading a measurable retrieval-behavior change across 58
languages for a licensing benefit this crate does not need (`krites`'
combined MPL/AGPL license tag applies to the CozoDB-derived files regardless
of this notice, and disappears with the crate at the program's end; see
`crates/krites/NOTICE.md`'s "What that requires").
