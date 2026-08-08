#!/usr/bin/env python3
"""One-time authoring tool: re-emit the vendored stopwords-iso word lists in a
freshly authored, single-file, alphabetized layout (`stopwords.rs`).

WHY: PLAN.md wave 2b / conflict C4 forecloses re-sourcing the word data itself
(same reasoning as the sibling `derived/gen_stopwords.py`, which fetches from
a live URL and is likewise a one-time tool, not a reproducible build step) —
so this script's *input* is the still-present `../derived/stopwords/*.rs`
vendored copy, not a fresh network fetch, and it changes nothing about which
words are present: it re-sorts languages by ISO code and re-sorts each
language's words by codepoint for a deterministic, reviewable diff, and
nothing else. Verified separately (token-multiset identity, 21,707 literals
across 58 languages) against both the pre-existing vendored copy and a live
stopwords-iso fetch — see the wave-2b PR description.

Run once, by hand, from `crates/krites/src/fts/tokenizer/stop_word_filter/sovereign/`:
    python3 gen_stopwords.py
"""

from __future__ import annotations

import pathlib
import re

DERIVED_STOPWORDS = pathlib.Path(__file__).resolve().parent.parent / "derived" / "stopwords"
OUT = pathlib.Path(__file__).resolve().parent / "stopwords.rs"

CONST_RE = re.compile(r'pub\(crate\) const (\w+): &\[&str\] = &\[(.*?)\];', re.DOTALL)
LIT_RE = re.compile(r'r#"(.*?)"#', re.DOTALL)

HEADER = """\
//! Vendored stop word lists, 58 languages.
//!
//! Source: the [stopwords-iso](https://github.com/stopwords-iso/stopwords-iso/)
//! project, MIT licensed — see `../NOTICE.md` for the full attribution and
//! license text. Word content is unchanged from the vendored copy this file
//! replaces (verified token-multiset identical: 21,707 literals, 58
//! languages, zero additions, zero removals — see the wave-2b PR
//! description). Languages are ordered alphabetically by ISO 639-1 code and
//! each list is sorted by codepoint; a `HashSet` lookup does not care about
//! either ordering, so this is a readability choice, not a behavioral one.
//!
//! `#[rustfmt::skip]` + the raw-string-hash/unicode-nfc `#[expect]`s live as
//! outer attributes on `mod stopwords;` in `../mod.rs`, not in here — matching
//! the sibling `derived/stopwords/` convention.
"""


def main() -> None:
    langs: dict[str, list[str]] = {}
    for f in sorted(DERIVED_STOPWORDS.glob("*.rs")):
        if f.name == "mod.rs":
            continue
        text = f.read_text(encoding="utf-8")
        for m in CONST_RE.finditer(text):
            name = m.group(1)
            words = LIT_RE.findall(m.group(2))
            if name in langs:
                raise SystemExit(f"duplicate const {name}")
            langs[name] = words

    if len(langs) != 58:
        raise SystemExit(f"expected 58 languages, found {len(langs)}")
    total = sum(len(v) for v in langs.values())
    if total != 21707:
        raise SystemExit(f"expected 21707 literals, found {total}")

    lines = [HEADER]
    for name in sorted(langs):
        words = sorted(langs[name])
        lines.append(f"pub(crate) const {name}: &[&str] = &[")
        for word in words:
            lines.append(f'    r#"{word}"#,')
        lines.append("];")
        lines.append("")

    OUT.write_text("\n".join(lines).rstrip("\n") + "\n", encoding="utf-8")
    print(f"wrote {OUT} ({len(langs)} languages, {total} literals)")


if __name__ == "__main__":
    main()
