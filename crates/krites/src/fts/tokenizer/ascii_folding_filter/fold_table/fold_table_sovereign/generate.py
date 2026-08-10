#!/usr/bin/env python3
"""Regenerate table.rs: the sovereign ASCII-folding table.

Sources, strongest first:
  (a) Unicode Character Database canonical + compatibility decomposition,
      via Python's `unicodedata` (CPython's own UCD binding). Accounts for
      the large majority of entries: accented Latin letters, fullwidth
      forms, superscript/subscript digits and punctuation, circled and
      parenthesized digit forms, and ligatures with a formal decomposition.
  (b) CLDR's "Latin-ASCII" transliteration table (Unicode Consortium,
      https://github.com/unicode-org/cldr, common/transforms/Latin-ASCII.xml,
      Unicode-3.0 licensed) for the small residual of Latin letters, IPA
      extensions, and punctuation that carry no UCD decomposition. Consulted
      by hand during development (not fetched at generation time); the
      constants below cite each entry's own codepoint and Unicode name.
  (c) The Unicode Name field itself, mechanically parsed, for two closes:
      (i) ligature/small-capital letters whose name spells their own fold
      ("LATIN CAPITAL LETTER AE" -> "AE"), and (ii) enclosed/dingbat digit
      forms whose name spells an English number word with no decomposition
      of its own ("NEGATIVE CIRCLED NUMBER ELEVEN" -> "11").
  (d) A short, individually-cited manual table for codepoints with no UCD
      decomposition, no CLDR rule, and no parseable Name pattern (mirrored
      dashes/quotes with no formal equivalence, a handful of "EPIGRAPHIC
      LETTER" glyphs whose case is not encoded in the Name field, and two
      confirmed upstream table quirks -- see QUIRKS below).

The conformance oracle was krites' derived ascii_folding_filter table, frozen
at an older Unicode edition predating several Unicode 14-16 Latin Extended-D
additions. Equivalence over the full Basic Multilingual Plane was proved
against it before it was retired.

EXCLUDE_SET below lists the codepoints where this generator's UCD/CLDR/Name
derivation produces a fold the oracle did not: real coverage this
primary-source method offers beyond what the frozen table carried, held back
to keep folding behaviour unchanged across the swap. Each group is commented
with why.

NOTE: the oracle no longer exists, so nothing now forces these exclusions.
Lifting them widens fold coverage and CHANGES tokenizer output, so it is a
deliberate behavioural change with its own tests -- not a cleanup to fold
into an unrelated edit.

Usage: python3 generate.py   (writes table.rs beside this script)

INVARIANT: pinned to the interpreter's UCD version below. All ranges in
ALLOWED_BLOCKS and all entries in EXCLUDE_SET were derived against this
exact version; a version bump can shift which codepoints are assigned in
Latin Extended-D and change what needs excluding. Re-derive EXCLUDE_SET and
diff the emitted table after bumping, and update this constant deliberately.
"""

from __future__ import annotations

import pathlib
import re
import unicodedata

EXPECTED_UNICODE_VERSION = "15.0.0"

OUT_PATH = pathlib.Path(__file__).parent / "table.rs"

# Unicode block allowlist (block boundaries per the UCD's own Blocks.txt),
# matching this table's own scope as split across fold_letters_a_m.rs,
# fold_letters_n_z.rs, and fold_digits_symbols.rs: Latin letters (base,
# extended, IPA/phonetic), general punctuation used as ASCII substitutes,
# enclosed/dingbat alphanumerics, presentation-form ligatures, and
# half/fullwidth forms. Excludes letterlike symbols, number forms (Roman
# numerals, vulgar fractions), arrows, math operators, and CJK-compatibility
# blocks -- confirmed absent from the conformance oracle.
ALLOWED_BLOCKS = [
    (0x00A0, 0x00FF),  # Latin-1 Supplement
    (0x0100, 0x017F),  # Latin Extended-A
    (0x0180, 0x024F),  # Latin Extended-B
    (0x0250, 0x02AF),  # IPA Extensions
    (0x1D00, 0x1D7F),  # Phonetic Extensions
    (0x1D80, 0x1DBF),  # Phonetic Extensions Supplement
    (0x1E00, 0x1EFF),  # Latin Extended Additional
    (0x2000, 0x206F),  # General Punctuation
    (0x2070, 0x209F),  # Superscripts and Subscripts
    (0x2184, 0x2184),  # LATIN SMALL LETTER REVERSED C (Number Forms block; this one entry only)
    (0x2460, 0x24FF),  # Enclosed Alphanumerics
    (0x2700, 0x27BF),  # Dingbats
    (0x2E00, 0x2E7F),  # Supplemental Punctuation
    (0x2C60, 0x2C7F),  # Latin Extended-C
    (0xA720, 0xA7FF),  # Latin Extended-D
    (0xFB00, 0xFB4F),  # Alphabetic Presentation Forms
    (0xFF00, 0xFFEF),  # Halfwidth and Fullwidth Forms
]


def in_scope(cp: int) -> bool:
    return any(lo <= cp <= hi for lo, hi in ALLOWED_BLOCKS)


def uname(c: str) -> str | None:
    try:
        return unicodedata.name(c)
    except ValueError:
        return None


# --- Phase A: UCD decomposition -------------------------------------------
def phase_a(c: str) -> str | None:
    """NFKD-decompose and strip nonspacing marks (category Mn). Excludes
    three UCD categories that decompose to ASCII but are out of this
    table's scope (confirmed against the oracle): phonetic MODIFIER LETTERs
    (aspiration/palatalization marks, distinct from the SUBSCRIPT/
    SUPERSCRIPT LETTER forms this table does fold), GREEK-named characters
    that happen to canonically equal an ASCII punctuation mark, and the two
    ORDINAL INDICATOR characters (superscript a/o with no "SUPERSCRIPT" in
    their own name)."""
    name = uname(c)
    if name and (
        name.startswith("MODIFIER LETTER ")
        or name.startswith("GREEK ")
        or "ORDINAL INDICATOR" in name
    ):
        return None
    decomposed = unicodedata.normalize("NFKD", c)
    if decomposed == c:
        return None
    stripped = "".join(ch for ch in decomposed if unicodedata.category(ch) != "Mn")
    if not stripped or not stripped.isascii() or stripped == c or stripped.strip() == "":
        return None
    return stripped


# --- Phase B: residual data, each entry traceable to CLDR Latin-ASCII.xml
# (cited by the character's own codepoint + Unicode name, per that file's
# own citation convention) or to the character's own name (dashes, bracket
# ornaments). None of these carry a UCD decomposition; NFKD is a no-op on
# every key below.
QUOTES = {  # krites groups these by quote-shape; CLDR's Latin-ASCII takes
    # the angle-bracket-shape reading for guillemets/single-angle-quotes
    # instead ("<<"/"<") -- overridden here to match the oracle's grouping.
    0x00AB: '"', 0x00BB: '"',  # LEFT/RIGHT-POINTING DOUBLE ANGLE QUOTATION MARK
    0x2018: "'", 0x2019: "'",  # LEFT/RIGHT SINGLE QUOTATION MARK
    0x201A: "'", 0x201B: "'",  # SINGLE LOW-9 / SINGLE HIGH-REVERSED-9 QUOTATION MARK
    0x201C: '"', 0x201D: '"', 0x201E: '"',  # LEFT/RIGHT/DOUBLE LOW-9 QUOTATION MARK
    0x2032: "'", 0x2033: '"',  # PRIME / DOUBLE PRIME
    0x2039: "'", 0x203A: "'",  # SINGLE LEFT/RIGHT-POINTING ANGLE QUOTATION MARK
}
DASHES = {cp: "-" for cp in (0x2010, 0x2011, 0x2012, 0x2013, 0x2014)}  # HYPHEN .. EM DASH
BRACKETS_MISC = {
    0x2045: "[", 0x2046: "]",  # LEFT/RIGHT SQUARE BRACKET WITH QUILL
    0x2768: "(", 0x2769: ")", 0x276A: "(", 0x276B: ")",  # MEDIUM (FLATTENED) PARENTHESIS ORNAMENT
    0x276C: "<", 0x276D: ">", 0x2770: "<", 0x2771: ">",  # MEDIUM/HEAVY ANGLE BRACKET ORNAMENT
    0x2772: "[", 0x2773: "]",  # LIGHT TORTOISE SHELL BRACKET ORNAMENT
    0x2774: "{", 0x2775: "}",  # MEDIUM CURLY BRACKET ORNAMENT
    0x2E28: "((", 0x2E29: "))",  # LEFT/RIGHT DOUBLE PARENTHESIS -- name states "double"
}
EXCLAIM_QUESTION = {0x203C: "!!", 0x2047: "??", 0x2048: "?!", 0x2049: "!?"}
OTHER_PUNCTUATION = {
    0x2044: "/",  # FRACTION SLASH
    0x204E: "*",  # LOW ASTERISK
    0x2053: "~",  # SWUNG DASH -- wavy dash, ASCII tilde is its standard substitute
    0x2038: "^",  # CARET -- the character's own name is the ASCII symbol's name
    0x204F: ";",  # REVERSED SEMICOLON -- mirrored form of SEMICOLON
    0x2052: "%",  # COMMERCIAL MINUS SIGN -- historically typeset as a percent-sign variant
    0x2035: "'",  # REVERSED PRIME -- mirrored form of PRIME
    0x2036: '"',  # REVERSED DOUBLE PRIME -- mirrored form of DOUBLE PRIME
    0x207B: "-", 0x208B: "-",  # SUPERSCRIPT/SUBSCRIPT MINUS
}
# Non-decomposable Latin letters cited from CLDR Latin-ASCII.xml directly
# (each is a single rule line there keyed by this exact codepoint).
LETTERS_MANUAL = {
    0x00C6: "AE", 0x00E6: "ae",  # LATIN (CAPITAL|SMALL) LETTER AE
    0x0152: "OE", 0x0153: "oe",  # LATIN (CAPITAL|SMALL) LIGATURE OE
    0x00D8: "O", 0x00F8: "o",  # LATIN (CAPITAL|SMALL) LETTER O WITH STROKE
    0x00D0: "D", 0x00F0: "d",  # LATIN (CAPITAL|SMALL) LETTER ETH
    0x00DE: "TH", 0x00FE: "th",  # LATIN (CAPITAL|SMALL) LETTER THORN
    0x00DF: "ss",  # LATIN SMALL LETTER SHARP S
    0x1E9E: "SS",  # LATIN CAPITAL LETTER SHARP S
    0x0138: "q",  # LATIN SMALL LETTER KRA (collates with q in DUCET, per CLDR's own comment)
    0x014A: "N", 0x014B: "n",  # LATIN (CAPITAL|SMALL) LETTER ENG
    0x01F6: "HV", 0x01BF: "w", 0x01F7: "W",  # LATIN CAPITAL LETTER HWAIR / LETTER WYNN / CAPITAL LETTER WYNN
    0x021C: "Z", 0x021D: "z",  # LATIN (CAPITAL|SMALL) LETTER YOGH
    0x2184: "c",  # LATIN SMALL LETTER REVERSED C
    0x0297: "C",  # LATIN LETTER STRETCHED C
    0xA768: "V",  # LATIN CAPITAL LETTER VEND
}
# LATIN EPIGRAPHIC LETTER {REVERSED F, REVERSED P, INVERTED M, I LONGA,
# ARCHAIC M}: the Name field carries no case marker for this small family
# (unlike every other "LATIN CAPITAL/SMALL LETTER" name); folded to the
# case the oracle's own table uses.
EPIGRAPHIC = {0xA7FB: "F", 0xA7FC: "p", 0xA7FD: "M", 0xA7FE: "I", 0xA7FF: "M"}
# Verified byte-for-byte against the derived table's own match-arm grouping
# (crates/krites/src/fts/tokenizer/ascii_folding_filter/fold_table/): these
# five codepoints are grouped with a different base letter than their own
# Name would suggest. Preserved as-is to match behavior, not "corrected" --
# matching the conformance oracle is this generator's job, not auditing it.
QUIRKS = {
    0x01E4: "G", 0x01E5: "G", 0x01E7: "G",  # G WITH STROKE / SMALL G WITH STROKE / SMALL G WITH CARON
    0x1E9B: "f",  # LATIN SMALL LETTER LONG S WITH DOT ABOVE -- grouped with the "f" arm
    0x2C6F: "a",  # LATIN CAPITAL LETTER TURNED A -- grouped with the "a" arm
    0xA73E: "c",  # LATIN CAPITAL LETTER REVERSED C WITH DOT -- grouped with the "c" arm
    0xA784: "s",  # LATIN CAPITAL LETTER INSULAR S -- grouped with the "s" arm
    0xA785: "S",  # LATIN SMALL LETTER INSULAR S -- grouped with the "S" arm
}

RESIDUAL: dict[int, str] = {}
for _table in (QUOTES, DASHES, BRACKETS_MISC, EXCLAIM_QUESTION, OTHER_PUNCTUATION, LETTERS_MANUAL, EPIGRAPHIC):
    RESIDUAL.update(_table)


# --- Phase C: UCD Name-field base-letter extraction ------------------------
# The Unicode Name for a Latin-script letterform without a formal
# decomposition is still compositional English text: "LATIN {CAPITAL,SMALL}
# LETTER [descriptors...] <BASE> [WITH modifiers...]", built from a fixed
# descriptive vocabulary (the Unicode Names List conventions). STOPWORDS is
# that descriptor vocabulary; SYMBOL_NAMES covers the handful of historical-
# letter proper names (schwa, eth, thorn, iota) whose ASCII fold is a known
# convention not spelled in the name itself.
STOPWORDS = {
    "LATIN", "LETTER", "CAPITAL", "SMALL", "WITH", "AND", "TURNED", "REVERSED",
    "OPEN", "CLOSED", "BARRED", "STROKE", "HOOK", "HOOKED", "TAIL", "RETROFLEX",
    "PALATAL", "MIDDLE", "TILDE", "CURL", "BAR", "TOPBAR", "LEG", "LONG",
    "RIGHT", "LEFT", "DESCENDER", "HORIZONTAL", "DIAGONAL", "DOT", "DOTLESS",
    "RING", "LOOP", "FLOURISH", "SQUIRREL", "INSULAR", "OBLIQUE", "DIGRAPH",
    "HIGH", "LOW", "PRECEDED", "BY", "APOSTROPHE", "STRIKETHROUGH", "SWASH",
    "BELT", "FISHHOOK", "CROSSED", "NOTCH", "HALF", "TOP", "BOTTOM",
    "INVERTED", "STRETCHED", "ROTUNDA", "VISIGOTHIC", "SUBSCRIPT", "SUPERSCRIPT",
    "SIDEWAYS", "DIAERESIZED", "THROUGH",
}
SYMBOL_NAMES = {"SCHWA": "A", "IOTA": "I", "ETH": "D", "THORN": "TH"}


def phase_c(c: str) -> str | None:
    name = uname(c)
    if not name or not name.startswith("LATIN "):
        return None
    tokens = name.split(" ")
    if "LETTER" not in tokens:
        return None
    if "SMALL CAPITAL" in name:
        case = "up"  # small-caps typography of a capital letter folds to actual capital
    elif "CAPITAL" in tokens:
        case = "up"
    elif "SMALL" in tokens:
        case = "low"
    else:
        return None  # no case marker (e.g. "LATIN LETTER WYNN") -> handled in LETTERS_MANUAL

    candidates = []
    for tok in tokens:
        if tok in ("LATIN", "LETTER", "CAPITAL", "SMALL"):
            continue
        if tok in SYMBOL_NAMES:
            candidates.append(SYMBOL_NAMES[tok])
            continue
        if tok in STOPWORDS:
            continue
        if tok.isalpha() and tok.isupper() and len(tok) <= 2:
            candidates.append(tok)
    if len(candidates) != 1:
        return None  # ambiguous or no base token -- not derivable from the name alone
    base = candidates[0]
    return base.upper() if case == "up" else base.lower()


# --- Phase D: UCD Name-field spelled-out-number extraction -----------------
# Enclosed/dingbat digit forms beyond 10 (negative-circled, double-circled,
# dingbat-sans-serif) carry no UCD decomposition of their own, but their
# Name spells the value as an English number word.
NUMBER_WORDS = {
    "ZERO": "0", "ONE": "1", "TWO": "2", "THREE": "3", "FOUR": "4", "FIVE": "5",
    "SIX": "6", "SEVEN": "7", "EIGHT": "8", "NINE": "9", "TEN": "10",
    "ELEVEN": "11", "TWELVE": "12", "THIRTEEN": "13", "FOURTEEN": "14",
    "FIFTEEN": "15", "SIXTEEN": "16", "SEVENTEEN": "17", "EIGHTEEN": "18",
    "NINETEEN": "19", "TWENTY": "20",
}
NUM_NAME_RE = re.compile(r"^(DINGBAT )?(NEGATIVE )?CIRCLED (SANS-SERIF )?(DIGIT|NUMBER) ([A-Z]+)$")
DOUBLE_NUM_RE = re.compile(r"^DOUBLE CIRCLED (DIGIT|NUMBER) ([A-Z]+)$")


def phase_d(c: str) -> str | None:
    name = uname(c)
    if not name:
        return None
    m = NUM_NAME_RE.match(name) or DOUBLE_NUM_RE.match(name)
    if not m:
        return None
    return NUMBER_WORDS.get(m.groups()[-1])


# --- Phase E: UCD Name-field punctuation-ornament classifier ---------------
# Dingbat "MEDIUM/HEAVY/LIGHT ... ORNAMENT" presentation variants of
# quotation marks and bracket punctuation directly name their own ASCII
# target's category (PARENTHESIS, BRACKET, ANGLE BRACKET, QUOTATION MARK,
# COMMA); LEFT/RIGHT selects the specific character.
BRACKET_TARGETS = [
    (re.compile(r"CURLY BRACKET"), "{", "}"),
    (re.compile(r"TORTOISE SHELL BRACKET"), "[", "]"),
    (re.compile(r"SQUARE BRACKET"), "[", "]"),
    (re.compile(r"DOUBLE ANGLE BRACKET"), "<<", ">>"),
    (re.compile(r"ANGLE BRACKET"), "<", ">"),
    (re.compile(r"PARENTHESIS"), "(", ")"),
]


def phase_e(c: str) -> str | None:
    name = uname(c)
    if not name or not name.endswith("ORNAMENT"):
        return None
    if "QUOTATION MARK" in name or "COMMA" in name:
        return "'" if "SINGLE" in name else '"'
    for pat, left, right in BRACKET_TARGETS:
        if pat.search(name):
            if "LEFT" in name:
                return left
            if "RIGHT" in name:
                return right
    return None


# --- Scope exclusions --------------------------------------------------
# Codepoints where phases A-E above produce a fold the conformance oracle
# (krites' now-retired derived table) did not carry, confirmed by a full BMP
# sweep while both tables existed. Not excluded because the derivation is
# wrong: this generator's primary sources are more complete than a table
# frozen at an older Unicode edition. Excluded to keep folding behaviour
# unchanged across the swap. Lifting them is a behavioural change with its
# own tests, not a cleanup.
EXCLUDE_SET = {
    # CLDR "Latin letters and IPA" entries the derived table's curation
    # never picked up (OI digraph, HENG WITH HOOK)
    0x01A2, 0x01A3, 0x0267,
    # Name-parser reaches bare IPA/phonetic letters the derived table omits
    0x0269, 0x0279, 0x027A, 0x027B, 0x1D11, 0x1D12, 0x1D13, 0x1D1D, 0x1D1E, 0x1D1F, 0x2C79,
    # CLDR carries the small-cap Middle-Welsh v; derived table has only the capital
    0x1EFD,
    # General Punctuation: leader dots/ellipsis, bars, a low-quote variant out of scope
    0x2015, 0x2016, 0x201F, 0x2024, 0x2025, 0x2026,
    # Subscript Latin letters beyond the derived table's chosen subset
    0x2095, 0x2096, 0x2097, 0x2098, 0x2099, 0x209A, 0x209B, 0x209C,
    # Dingbat low comma-quotation ornaments (derived table folds only the plain/turned family)
    0x275F, 0x2760,
    # Latin Extended-C swash-tail S/Z
    0x2C7E, 0x2C7F,
    # Latin Extended-D: additions from Unicode 14-16, postdating the derived table's curation
    0xA764, 0xA765, 0xA76A, 0xA76B, 0xA76C, 0xA76D, 0xA771, 0xA772, 0xA773, 0xA774,
    0xA775, 0xA776, 0xA777, 0xA778, 0xA787, 0xA78D, 0xA78E, 0xA790, 0xA791, 0xA792,
    0xA793, 0xA794, 0xA795, 0xA796, 0xA797, 0xA798, 0xA799, 0xA79A, 0xA79B, 0xA79C,
    0xA79D, 0xA79E, 0xA79F, 0xA7A0, 0xA7A1, 0xA7A2, 0xA7A3, 0xA7A4, 0xA7A5, 0xA7A6,
    0xA7A7, 0xA7A8, 0xA7A9, 0xA7AA, 0xA7AB, 0xA7AC, 0xA7AD, 0xA7AE, 0xA7AF, 0xA7B0,
    0xA7B1, 0xA7B2, 0xA7B8, 0xA7B9, 0xA7BA, 0xA7BB, 0xA7BC, 0xA7BD, 0xA7BE, 0xA7BF,
    0xA7C0, 0xA7C1, 0xA7C2, 0xA7C3, 0xA7C4, 0xA7C5, 0xA7C6, 0xA7C7, 0xA7C8, 0xA7C9,
    0xA7CA, 0xA7CC, 0xA7CD, 0xA7D0, 0xA7D1, 0xA7D3, 0xA7D5, 0xA7D6, 0xA7D7, 0xA7D8,
    0xA7D9, 0xA7F5, 0xA7F6, 0xA7FA,
    # Presentation-forms/Fullwidth/Halfwidth extras the derived table omits
    0xFB05, 0xFB29, 0xFF40, 0xFF5C, 0xFF5F, 0xFF60, 0xFF61, 0xFF64, 0xFFE9, 0xFFEB,
}


def fold(cp: int) -> str | None:
    if not in_scope(cp) or cp in EXCLUDE_SET:
        return None
    if cp in QUIRKS:
        return QUIRKS[cp]
    c = chr(cp)
    for fn in (
        lambda: phase_a(c),
        lambda: RESIDUAL.get(cp),
        lambda: phase_c(c),
        lambda: phase_d(c),
        lambda: phase_e(c),
    ):
        v = fn()
        if v is not None:
            return v
    return None


def rust_str_literal(s: str) -> str:
    out = ['"']
    for ch in s:
        if ch == "\\":
            out.append("\\\\")
        elif ch == '"':
            out.append('\\"')
        else:
            out.append(ch)
    out.append('"')
    return "".join(out)


def main() -> None:
    actual_version = unicodedata.unidata_version
    if actual_version != EXPECTED_UNICODE_VERSION:
        raise SystemExit(
            f"interpreter UCD version {actual_version} != pinned {EXPECTED_UNICODE_VERSION} -- "
            "re-validate ALLOWED_BLOCKS/EXCLUDE_SET and diff the emitted table before "
            "bumping EXPECTED_UNICODE_VERSION"
        )

    entries: list[tuple[int, str]] = []
    for cp in range(0x80, 0x10000):
        if 0xD800 <= cp <= 0xDFFF:
            continue
        v = fold(cp)
        if v is not None:
            entries.append((cp, v))
    entries.sort()

    lines = [
        "//! Sovereign ASCII-folding table -- generated by `generate.py`, do not hand-edit.",
        "//!",
        "//! Regenerate: `python3 fold_table_sovereign/generate.py` from the module's",
        "//! own directory (see that script for the full source/methodology citation).",
        "//! Frozen to the fold set of the retired derived table: proved equivalent to it",
        "//! over the full BMP before that table was deleted. See `generate.py`'s",
        "//! `EXCLUDE_SET` for the codepoints this holds back.",
        "",
        "/// `(codepoint, ascii fold)` pairs, sorted ascending by codepoint for binary search.",
        f"pub(super) static FOLD_TABLE: [(u32, &str); {len(entries)}] = [",
    ]
    for cp, val in entries:
        lines.append(f"    (0x{cp:04X}, {rust_str_literal(val)}),")
    lines.append("];")

    OUT_PATH.write_text("\n".join(lines) + "\n")
    print(f"wrote {OUT_PATH} ({len(entries)} entries)")


if __name__ == "__main__":
    main()
