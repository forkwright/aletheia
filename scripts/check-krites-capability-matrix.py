#!/usr/bin/env python3
"""Verify crates/krites/CAPABILITY_MATRIX.toml maps every krites capability.

Wave 0.4 of the krites retirement plan (canon: forkwright/kanon
`projects/aletheia/phases/05g-krites-overhaul/RETIREMENT-PLAN.md` — a
sibling repo; see the "Appendix A" section below for why this script
cannot read it in CI). `unmapped` -- present in
source but absent from the matrix -- is a build failure. A matrix row with
no matching source item (stale) fails the same way, so drift is caught in
both directions: the matrix cannot silently fall behind source, and it
cannot silently keep a row for something that no longer exists.

Five mechanically re-derived categories, checked against live source in
THIS repo:

  sysop          every `SysOp` variant, crates/krites/src/parse/sys/mod.rs
  datavalue      every `DataValue` variant, crates/krites/src/data/value.rs
  public_api     every public item at the `Db` / crate-root boundary,
                 crates/krites/src/lib.rs -- scoped to what wave 0.5's
                 recorder covers (the Db facade + top-level pub use
                 re-exports), not every pub item in every krites submodule.
                 `pub mod` declarations and the bare `Db`/`MultiTransaction`
                 struct names are intentionally not separate line items --
                 their methods are what the matrix rows track.
  fixed_rule     every key of `DEFAULT_FIXED_RULES`,
                 crates/krites/src/fixed_rule/mod.rs. The registry key IS
                 the capability: a script names a rule by that string, so an
                 alias (`DFS` beside `DepthFirstSearch`) is its own row.
  storage_method every method of the `Storage` and `StoreTx` traits,
                 crates/krites/src/storage/mod.rs.

A sixth category, appendix_a, mirrors RETIREMENT-PLAN.md's Appendix A table
(33 rows at authoring time). The plan lives outside this repo, so CI cannot
re-parse it; --check only verifies the mirror's internal completeness (row
count floor, required fields, unique ids). Pass --plan-md <path> for an
optional, non-gating local live-diff when both repos are checked out side
by side (e.g. the kanon clone next to the aletheia clone).

`[[capability_set]]` covers the populations too large for one row each --
the ~139 scalar functions and the ~25 aggregations. Each set records its
members as a sorted list and is re-derived from source on every run, with
EXACT set equality: a member that vanishes from source is a dropped
capability, and a member source gained that the set never recorded is an
unrecorded one. The prose these replace ("~102 functions consuming Num",
"All 22 fixed-rule algorithms") named counts that matched nothing
measurable -- the registry holds 26 entries, `define_aggr!` 25, and
`define_op!` 139 against 138 DSL-reachable names.

`gate_test` records a maintainer-selected candidate test as a
`<binary-id>::<test path>` id. Without `--nextest-list`, this script validates
only that the field has that shape. The required hosted gate supplies
machine-readable output from `cargo nextest list` for the exact test selection
it then executes; only that compiler-derived list is authoritative for whether
the candidate exists, matches the selected filters, and is not ignored.
`"none"` or an absent field is legal and counts as UNPOINTED -- an honestly
unpointed row is worth more than a pointer at a test that does not exercise the
capability.

WARNING(what a pointer proves): a pointer resolved against nextest proves only
that the named test exists and is runnable in the listed build. A green
required hosted job also proves that the listed test world passed. Neither
fact mechanically couples that test to the row's capability or proves the
row's `gate` sentence is asserted -- several gates name conformance behaviour
(recorded-vector replay, post-crash visibility, multiset-vs-sequence) that no
current test checks. Read the pointed count as "rows with a runnable candidate
recorded", never as disappearance detection or semantic conformance.

`call_sites` on a sysop/datavalue/public_api row is a measured integer,
never a guess: `check_call_sites_measured` re-executes each row's
`call_sites_method` and fails the build if the measurement has fallen BELOW
the declared count. The figure is a floor, not an equality: it exists so a
capability cannot lose its last consumer unnoticed, and a consumer being
ADDED is not a defect. Requiring equality made every unrelated PR that
gained a caller fail a krites check -- `api-db-open-mem` moved 129 -> 130 ->
131 in one session, each a hand edit invalidated by the next merge -- and it
made a real disappearance indistinguishable from ordinary growth.
`call_sites = -1` means "not measured" and is legal only for the exact row ids
and covered-row relationships owned by this checker; `call_sites_method`
retains the human reason but cannot authorize a new exception. Zero call sites
is never grounds to drop a row -- see the plan's B7 finding and kill criterion
10 -- and -1 is never a stand-in for zero.

Usage:
    python3 scripts/check-krites-capability-matrix.py
    python3 scripts/check-krites-capability-matrix.py --plan-md /path/to/RETIREMENT-PLAN.md
    python3 scripts/check-krites-capability-matrix.py --nextest-list list.json
"""

from __future__ import annotations

import argparse
import re
import shlex
import subprocess
import sys
import tomllib
from collections.abc import Mapping
from functools import lru_cache
from pathlib import Path, PurePosixPath

sys.path.insert(0, str(Path(__file__).resolve().parent))

import krites_capability_evidence as EVIDENCE

REPO_ROOT = Path(__file__).resolve().parents[1]
KRITES_DIR = REPO_ROOT / "crates" / "krites"
KRITES_SRC = KRITES_DIR / "src"
SYSOP_FILE = KRITES_SRC / "parse" / "sys" / "mod.rs"
DATAVALUE_FILE = KRITES_SRC / "data" / "value.rs"
LIB_FILE = KRITES_SRC / "lib.rs"
FIXED_RULE_FILE = KRITES_SRC / "fixed_rule" / "mod.rs"
STORAGE_FILE = KRITES_SRC / "storage" / "mod.rs"
FUNCTIONS_DIR = KRITES_SRC / "data" / "functions"
AGGR_DIR = KRITES_SRC / "data" / "aggr"
OP_LOOKUP_FILE = KRITES_SRC / "data" / "expr" / "op.rs"
MATRIX_FILE = KRITES_DIR / "CAPABILITY_MATRIX.toml"

# WHY: matches the plan's own known-count baseline (RETIREMENT-PLAN.md Appendix A, 33
# data rows as of this checker's authoring). A floor, not a ceiling -- the
# plan may grow rows; it must never silently shrink under this file.
EXPECTED_APPENDIX_A_ROWS = 33
SOURCE_DERIVED_CATEGORIES = frozenset(
    {"sysop", "datavalue", "public_api", "fixed_rule", "storage_method"}
)

IDENT_RE = re.compile(r"[A-Za-z_][A-Za-z0-9_:]*")

# The matrix normally requires one row per source capability.  These five rows
# are deliberate atomic bundles: their gate/destination applies to every named
# member together.  Pinning the exact exception here makes the checker an
# independent oracle; a matrix edit cannot authorize its own absorption of a
# sibling row merely by changing prose or adding metadata beside the change.
PUBLIC_API_SOURCE_BUNDLES: dict[str, frozenset[str]] = {
    "api-error-result": frozenset({"Error", "Result"}),
    "api-query-cache": frozenset({"QueryCache", "QueryCacheStats"}),
    "api-fixed-rule-trait": frozenset(
        {"FixedRule", "FixedRuleInputRelation", "FixedRulePayload"}
    ),
    "api-multi-transaction-error": frozenset(
        {
            "MultiTransactionError::WorkerPanicked",
            "MultiTransactionError::SendFailed",
            "MultiTransactionError::Query",
        }
    ),
    "api-multi-transaction-struct": frozenset(
        {
            "MultiTransaction::transact",
            "MultiTransaction::commit",
            "MultiTransaction::abort",
        }
    ),
}


def _is_top_level(clean: str, offset: int) -> bool:
    """Whether `offset` is outside every (), [] and {} token tree."""
    stack: list[str] = []
    pairs = {")": "(", "]": "[", "}": "{"}
    for ch in clean[:offset]:
        if ch in "([{":
            stack.append(ch)
        elif ch in pairs:
            if not stack or stack[-1] != pairs[ch]:
                raise ValueError("unbalanced delimiters while locating a source owner")
            stack.pop()
    return not stack


def _top_level_matches(clean: str, pattern: re.Pattern[str]) -> list[re.Match[str]]:
    return [
        match
        for match in pattern.finditer(clean)
        if _is_top_level(clean, match.start())
    ]


def _item_attrs(
    raw: str,
    clean: str,
    item_offset: int,
    lower_bound: int = 0,
) -> list[str]:
    return [
        *EVIDENCE.leading_inner_attributes(raw, clean),
        *EVIDENCE.preceding_outer_attributes(raw, clean, item_offset, lower_bound),
    ]


def _attrs_possible(
    inherited_branches: list[tuple[str, ...]],
    local_attrs: list[str] | tuple[str, ...],
) -> bool:
    return any(
        EVIDENCE.cfg_attrs_satisfiable([*branch, *local_attrs])
        for branch in inherited_branches
    )


def _strip_leading_attributes(text: str) -> int:
    """Consume zero or more leading `#[...]` attributes (bracket-matched,
    not a naive regex, so `#[serde(rename = "]")]`-style nested brackets
    don't truncate early) plus trailing whitespace. Returns chars consumed.

    WHY: a lone `stripped.startswith("#[")` line-skip (the prior approach)
    discards the REST of the line too -- `#[allow(dead_code)] Sneaky,` loses
    `Sneaky` along with the attribute. Consuming just the attribute lets the
    caller keep scanning the remainder of the line for a variant.
    """
    pos = 0
    while (parsed := EVIDENCE._read_attribute(text, pos)) is not None and not parsed[3]:
        pos = parsed[2]
        while pos < len(text) and text[pos].isspace():
            pos += 1
    return pos


def extract_enum_variants(
    text: str,
    enum_name: str,
    inherited_branches: list[tuple[str, ...]] | None = None,
) -> dict[str, int]:
    """Return {variant_name: line_number} for a `pub enum <enum_name> { ... }` block.

    Scoped to exactly one satisfiable top-level enum by that name. Strips comments
    and leading attributes, then scans each line for every top-level
    variant declaration it contains -- not just one anchored at column 0.
    Stops at the closing brace that returns to column 0.

    WHY(line-scan, not line-anchor): `^([A-Za-z_]...)`, matched once per
    line, is evadable by putting a second variant after the first on the
    same line (`Compact, Sneaky,`) -- the anchor never sees past the first
    comma. Both that shape and `#[attr] Sneaky,` produce a `cargo fmt
    --check` diff (rustfmt puts one variant per line), so today this is
    unreachable through a fmt-clean PR -- but `gate` includes this check
    and fmt is a separate step; nothing ties them together structurally, so
    the parser is hardened to not depend on that coincidence.
    """
    # Work from the length-preserving lexical view.  Reading raw lines here
    # makes a block-commented variant indistinguishable from live source, and
    # braces inside comments/literals corrupt the payload-depth accounting.
    clean = EVIDENCE.strip_noise(text)
    branches = inherited_branches or [()]
    headers = [
        header
        for header in _top_level_matches(
            clean,
            re.compile(
                rf"{EVIDENCE.RUST_TOKEN_START}pub\s+enum\s+{re.escape(enum_name)}"
                rf"{EVIDENCE.RUST_TOKEN_END}"
            ),
        )
        if _attrs_possible(branches, _item_attrs(text, clean, header.start()))
    ]
    if len(headers) != 1:
        raise ValueError(
            f"expected one top-level enum {enum_name}, found {len(headers)}"
        )
    header = headers[0]
    owner_attrs = _item_attrs(text, clean, header.start())
    open_at = clean.find("{", header.end())
    if open_at == -1:
        raise ValueError(f"enum {enum_name} has no body")
    close_at = _matching_delimiter(clean, open_at, "{", "}")
    body = clean[open_at + 1 : close_at]
    variants: dict[str, int] = {}
    for seg_start, seg_end in _top_level_segments(body):
        segment = body[seg_start:seg_end]
        pos = 0
        while True:
            pos += len(segment[pos:]) - len(segment[pos:].lstrip())
            consumed = _strip_leading_attributes(segment[pos:])
            if not consumed:
                break
            pos += consumed
        remainder = segment[pos:].strip()
        if not remainder:
            continue
        name_match = re.match(
            r"(?P<name>[A-Za-z_][A-Za-z0-9_]*)" + EVIDENCE.RUST_TOKEN_END,
            remainder,
        )
        if name_match is None:
            raise ValueError(
                f"enum {enum_name} has an unsupported variant segment at "
                f"line {_line_of(text, open_at + 1 + seg_start + pos)}"
            )
        name = name_match.group("name")
        shape = remainder[name_match.end() :].strip()
        if shape and shape[0] not in "({=":
            raise ValueError(
                f"enum {enum_name} variant {name} has unsupported shape {shape!r}"
            )
        if shape.startswith("=") and not shape[1:].strip():
            raise ValueError(
                f"enum {enum_name} variant {name} has an empty discriminant"
            )
        name_in_segment = segment.find(name, pos)
        absolute_name = open_at + 1 + seg_start + name_in_segment
        attrs = _item_attrs(
            text,
            clean,
            absolute_name,
            open_at + 1 + seg_start,
        )
        if not _attrs_possible(branches, [*owner_attrs, *attrs]):
            continue
        variants.setdefault(
            name,
            _line_of(text, absolute_name),
        )
    return variants


def extract_sysop_variants() -> dict[str, int]:
    text = SYSOP_FILE.read_text(encoding="utf-8")
    return {
        f"SysOp::{name}": line
        for name, line in extract_enum_variants(
            text,
            "SysOp",
            _source_branches(SYSOP_FILE),
        ).items()
    }


def extract_datavalue_variants() -> dict[str, int]:
    text = DATAVALUE_FILE.read_text(encoding="utf-8")
    return {
        f"DataValue::{name}": line
        for name, line in extract_enum_variants(
            text,
            "DataValue",
            _source_branches(DATAVALUE_FILE),
        ).items()
    }


def extract_lib_public_api() -> dict[str, int]:
    """Return {identifier: line_number} for lib.rs's Db-boundary public surface.

    Covers: `pub use` re-exports (crate-root, multi-line aware), `pub fn`
    names (all Db/MultiTransaction methods live in this one file), and the
    `MultiTransactionError` enum's variants. Does not descend into other
    `pub mod` submodules -- see module docstring for the scope rationale.
    """
    text = LIB_FILE.read_text(encoding="utf-8")
    clean = EVIDENCE.strip_noise(text)
    branches = _source_branches(LIB_FILE)
    items: dict[str, int] = {}

    # `pub use` statements: join across lines up to the terminating `;`.
    for m in _top_level_matches(clean, re.compile(r"pub use ([^;]+);", re.DOTALL)):
        if not _attrs_possible(branches, _item_attrs(text, clean, m.start())):
            continue
        line_no = text[: m.start()].count("\n") + 1
        body = m.group(1).strip()
        # Drop the leading path (crate::a::b::{...} or crate::a::b::Name or
        # module::Name) -- keep only the final `{...}` group or final segment.
        brace = re.fullmatch(r"(?P<prefix>[^{}]+)::\{(?P<names>[^{}]+)\}", body)
        if brace:
            names = [n.strip() for n in brace.group("names").split(",") if n.strip()]
        else:
            if any(token in body for token in ("{", "}", "*")):
                raise ValueError(
                    f"unsupported nested/glob pub use tree at line {line_no}: {body!r}"
                )
            names = [body.rsplit("::", 1)[-1]]
        for name in names:
            alias = re.fullmatch(
                r"(?P<name>[A-Za-z_][A-Za-z0-9_]*)(?:\s+as\s+(?P<alias>[A-Za-z_][A-Za-z0-9_]*))?",
                name,
            )
            if alias is None or alias.group("name") in {"self", "super", "crate"}:
                raise ValueError(
                    f"unsupported pub use member at line {line_no}: {name!r}"
                )
            items.setdefault(alias.group("alias") or alias.group("name"), line_no)

    # Public methods are owner-qualified.  A same-named method on another type
    # must never satisfy a Db/MultiTransaction row after the real method moves.
    method_re = re.compile(
        EVIDENCE.RUST_TOKEN_START + r"pub\s+"
        r"(?:(?:const|async|unsafe)\s+)*"
        r'(?:extern\s+(?:"[^"]*"\s+)?)?'
        r"fn\s+((?:r#)?[A-Za-z_][A-Za-z0-9_]*)" + EVIDENCE.RUST_TOKEN_END
    )
    for owner in ("Db", "MultiTransaction"):
        impl_re = re.compile(
            rf"(?m)^impl(?:<[^{{}}]*>)?\s+{re.escape(owner)}(?:<[^{{}}]*>)?"
            rf"{EVIDENCE.RUST_TOKEN_END}(?=\s*(?:where{EVIDENCE.RUST_TOKEN_END}|\{{))"
        )
        for impl_match in _top_level_matches(clean, impl_re):
            impl_attrs = _item_attrs(text, clean, impl_match.start())
            if not _attrs_possible(branches, impl_attrs):
                continue
            impl_open = EVIDENCE._function_body_open(clean, impl_match.end())
            if impl_open is None:
                raise ValueError(f"inherent impl {owner} has no body")
            impl_end = _matching_delimiter(clean, impl_open, "{", "}")
            impl_attrs = [
                *impl_attrs,
                *EVIDENCE.inner_attributes_after(text, clean, impl_open),
            ]
            if not _attrs_possible(branches, impl_attrs):
                continue
            body = clean[impl_open + 1 : impl_end]
            for method in method_re.finditer(body):
                if not _is_top_level(body, method.start()):
                    continue
                if method.group(1).startswith("r#"):
                    raise ValueError(
                        f"public {owner} method uses unsupported raw identifier "
                        f"{method.group(1)!r}"
                    )
                method_signature_end = impl_open + 1 + method.end()
                method_body_open = EVIDENCE._function_body_open(
                    clean, method_signature_end
                )
                if method_body_open is None or method_body_open >= impl_end:
                    raise ValueError(
                        f"public {owner} method {method.group(1)} has no owned body"
                    )
                method_attrs = _item_attrs(
                    text,
                    clean,
                    impl_open + 1 + method.start(),
                    impl_open + 1,
                )
                method_attrs.extend(
                    EVIDENCE.inner_attributes_after(text, clean, method_body_open)
                )
                if not _attrs_possible(branches, [*impl_attrs, *method_attrs]):
                    continue
                absolute = impl_open + 1 + method.start(1)
                items.setdefault(
                    f"{owner}::{method.group(1)}", _line_of(text, absolute)
                )

    # MultiTransactionError variants, same block-extraction as the enums above.
    for name, line_no in extract_enum_variants(
        text,
        "MultiTransactionError",
        branches,
    ).items():
        items.setdefault(f"MultiTransactionError::{name}", line_no)

    return items


def _block_span(
    text: str,
    header_re: re.Pattern[str],
    inherited_branches: list[tuple[str, ...]] | None = None,
    *,
    function_body: bool = False,
) -> tuple[int, int, list[str]]:
    """Offsets of the `{ ... }` body that follows `header_re`'s match.

    Offsets index `text` unchanged, so a caller slices the ORIGINAL source and
    still sees literal contents.

    WHY brace-matched rather than "until the next blank line" or a line count:
    `DEFAULT_FIXED_RULES` is a 130-line `BTreeMap::from([...])`, and the
    `Storage`/`StoreTx` trait bodies contain default method bodies. A span that
    stops early drops real capabilities, and the checker then passes without
    having looked at them.

    WHY the noise strip: braces inside a doc comment or a string literal are not
    structure. Matching them shortens the span, which is the same silent
    under-read.
    """
    clean = EVIDENCE.strip_noise(text)
    branches = inherited_branches or [()]
    candidates = [
        candidate
        for candidate in _top_level_matches(clean, header_re)
        if _attrs_possible(branches, _item_attrs(text, clean, candidate.start()))
    ]
    if len(candidates) != 1:
        raise ValueError(
            f"expected one top-level {header_re.pattern}, found {len(candidates)}"
        )
    m = candidates[0]
    open_at = (
        EVIDENCE._function_body_open(clean, m.end())
        if function_body
        else clean.find("{", m.end())
    )
    if open_at == -1:
        raise ValueError(f"no block body after {header_re.pattern}")
    if open_at is None:
        raise ValueError(f"no function body after {header_re.pattern}")
    depth = 0
    for i in range(open_at, len(clean)):
        if clean[i] == "{":
            depth += 1
        elif clean[i] == "}":
            depth -= 1
            if depth == 0:
                return (
                    open_at,
                    i,
                    [
                        *_item_attrs(text, clean, m.start()),
                        *EVIDENCE.inner_attributes_after(text, clean, open_at),
                    ],
                )
    raise ValueError(f"unterminated block after {header_re.pattern}")


def _line_of(text: str, offset: int) -> int:
    return text.count("\n", 0, offset) + 1


def _matching_delimiter(text: str, open_at: int, opener: str, closer: str) -> int:
    """Return the matching delimiter offset in length-preserving clean source."""
    if text[open_at] != opener:
        raise ValueError(f"expected {opener!r} at offset {open_at}")
    depth = 0
    for i in range(open_at, len(text)):
        if text[i] == opener:
            depth += 1
        elif text[i] == closer:
            depth -= 1
            if depth == 0:
                return i
    raise ValueError(f"unterminated {opener!r} at offset {open_at}")


def _first_top_level_comma(text: str) -> int | None:
    """Find a comma outside nested (), [] and {} in clean Rust source."""
    depths = {"(": 0, "[": 0, "{": 0}
    closing = {")": "(", "]": "[", "}": "{"}
    for i, ch in enumerate(text):
        if ch in depths:
            depths[ch] += 1
        elif ch in closing:
            depths[closing[ch]] -= 1
        elif ch == "," and not any(depths.values()):
            return i
    return None


def _top_level_segments(text: str) -> list[tuple[int, int]]:
    """Split clean Rust source on commas outside (), [] and {}."""
    stack: list[str] = []
    pairs = {")": "(", "]": "[", "}": "{"}
    start = 0
    segments: list[tuple[int, int]] = []
    for i, ch in enumerate(text):
        if ch in "([{":
            stack.append(ch)
        elif ch in pairs:
            if not stack or stack[-1] != pairs[ch]:
                raise ValueError("unbalanced delimiters while splitting Rust inventory")
            stack.pop()
        elif ch == "," and not stack:
            segments.append((start, i))
            start = i + 1
    if stack:
        raise ValueError("unterminated delimiter while splitting Rust inventory")
    segments.append((start, len(text)))
    return segments


def _blank_string_spans(clean: str) -> list[tuple[int, int]]:
    """Offsets of quote-delimited literals after `strip_noise` blanked content."""
    return [(m.start(), m.end()) for m in re.finditer(r'"[ \t\r\n]*"', clean)]


def _ascii_registry_key(raw_literal: str, context: str) -> str:
    """Decode the identifier-shaped registry keys this matrix can represent.

    The surrounding expression must be one of the explicitly identity-preserving
    String materializations accepted by `_identity_string_literal_span`.
    Unsupported dynamic, transformed, or escaped keys fail instead of collapsing
    to an empty source inventory.
    """
    match = re.fullmatch(r'"([A-Za-z][A-Za-z0-9_]*)"', raw_literal)
    if match is None:
        raise ValueError(
            f"{context} has unsupported non-identifier key {raw_literal!r}"
        )
    return match.group(1)


def _identity_string_literal_span(clean_expr: str, context: str) -> tuple[int, int]:
    """Locate the literal in a statically identity-preserving String expression."""
    literal = r'"[ \t\r\n]*"'
    patterns = (
        re.compile(
            rf"^[ \t\r\n]*(?P<literal>{literal})[ \t\r\n]*\.[ \t\r\n]*"
            r"(?:to_string|to_owned|into)[ \t\r\n]*\([ \t\r\n]*\)[ \t\r\n]*$"
        ),
        re.compile(
            rf"^[ \t\r\n]*String[ \t\r\n]*::[ \t\r\n]*from[ \t\r\n]*"
            rf"\([ \t\r\n]*(?P<literal>{literal})[ \t\r\n]*\)[ \t\r\n]*$"
        ),
    )
    for pattern in patterns:
        if match := pattern.fullmatch(clean_expr):
            return match.start("literal"), match.end("literal")
    raise ValueError(
        f"{context} does not use a supported identity String materialization; "
        "refusing to infer a runtime key from a transformed expression"
    )


def extract_fixed_rule_names() -> dict[str, int]:
    """Return {registry key: line} for every `DEFAULT_FIXED_RULES` entry.

    The key -- the string a script writes after `<~` -- is the capability. Two
    keys bound to one type (`DFS` and `DepthFirstSearch`) are two capabilities:
    dropping either breaks scripts that name it, and nothing else in the crate
    would notice.
    """
    text = FIXED_RULE_FILE.read_text(encoding="utf-8")
    clean = EVIDENCE.strip_noise(text)
    branches = _source_branches(FIXED_RULE_FILE)
    start, end, owner_attrs = _block_span(
        text,
        re.compile(
            EVIDENCE.RUST_TOKEN_START
            + r"(?:pub\s*(?:\([^)]*\)\s*)?)?static\s+DEFAULT_FIXED_RULES"
            + EVIDENCE.RUST_TOKEN_END
        ),
        branches,
    )
    constructors = list(
        re.finditer(
            EVIDENCE.RUST_TOKEN_START + r"BTreeMap\s*::\s*from\s*\(",
            clean[start:end],
        )
    )
    if len(constructors) != 1:
        raise ValueError(
            f"DEFAULT_FIXED_RULES has {len(constructors)} BTreeMap::from constructors; "
            "the returned inventory owner is not unique"
        )
    constructor = constructors[0]
    call_open = start + constructor.end() - 1
    call_end = _matching_delimiter(clean, call_open, "(", ")")
    if clean[call_end + 1 : end].strip():
        raise ValueError(
            "DEFAULT_FIXED_RULES BTreeMap::from is not the closure's tail expression; "
            "refusing to derive a discarded inventory"
        )
    array_at = clean.find("[", start + constructor.end())
    if array_at == -1 or array_at >= end:
        raise ValueError("DEFAULT_FIXED_RULES BTreeMap::from has no array inventory")
    array_end = _matching_delimiter(clean, array_at, "[", "]")
    if (
        clean[call_open + 1 : array_at].strip()
        or clean[array_end + 1 : call_end].strip()
    ):
        raise ValueError(
            "DEFAULT_FIXED_RULES BTreeMap::from argument is not one direct array inventory; "
            "refusing to derive a nested or transformed argument"
        )

    # Parse every top-level array element as a `(key, value)` tuple.  This
    # makes the key's structural position authoritative and avoids coupling
    # capability discovery to whichever valid String conversion happens to
    # spell the first tuple element today.
    out: dict[str, int] = {}
    pos = array_at + 1
    while pos < array_end:
        while pos < array_end and (clean[pos].isspace() or clean[pos] == ","):
            pos += 1
        consumed = _strip_leading_attributes(clean[pos:array_end])
        if consumed:
            pos += consumed
            continue
        if pos >= array_end:
            break
        if clean[pos] != "(":
            raise ValueError(
                "DEFAULT_FIXED_RULES contains a non-tuple inventory entry at "
                f"line {_line_of(text, pos)}; source extraction would be incomplete"
            )
        tuple_end = _matching_delimiter(clean, pos, "(", ")")
        entry_attrs = _item_attrs(text, clean, pos, array_at + 1)
        if not _attrs_possible(branches, [*owner_attrs, *entry_attrs]):
            pos = tuple_end + 1
            continue
        tuple_body = clean[pos + 1 : tuple_end]
        comma = _first_top_level_comma(tuple_body)
        if comma is None:
            raise ValueError(
                f"DEFAULT_FIXED_RULES tuple at line {_line_of(text, pos)} has no value"
            )
        key_clean = tuple_body[:comma]
        context = f"DEFAULT_FIXED_RULES entry at line {_line_of(text, pos)}"
        literal_start, literal_end = _identity_string_literal_span(key_clean, context)
        absolute_start = pos + 1 + literal_start
        absolute_end = pos + 1 + literal_end
        key = _ascii_registry_key(
            text[absolute_start:absolute_end],
            context,
        )
        out.setdefault(key, _line_of(text, absolute_start))
        pos = tuple_end + 1
    return out


def extract_storage_methods() -> dict[str, int]:
    """Return {`Trait::method`: line} for the `Storage` and `StoreTx` traits."""
    text = STORAGE_FILE.read_text(encoding="utf-8")
    clean = EVIDENCE.strip_noise(text)
    branches = _source_branches(STORAGE_FILE)
    out: dict[str, int] = {}
    for trait in ("Storage", "StoreTx"):
        start, end, owner_attrs = _block_span(
            text,
            re.compile(
                rf"{EVIDENCE.RUST_TOKEN_START}pub trait {trait}{EVIDENCE.RUST_TOKEN_END}"
            ),
            branches,
        )
        body = clean[start + 1 : end]
        method_re = re.compile(
            r"(?<![A-Za-z0-9_])"
            r"(?:(?:const|async|unsafe)\s+)*"
            r'(?:extern\s+(?:"[^"]*"\s+)?)?'
            r"fn\s+((?:r#)?[a-z_][A-Za-z0-9_]*)" + EVIDENCE.RUST_TOKEN_END
        )
        for m in method_re.finditer(body):
            if _is_top_level(body, m.start()):
                if m.group(1).startswith("r#"):
                    raise ValueError(
                        f"{trait} method uses unsupported raw identifier {m.group(1)!r}"
                    )
                signature_end = start + 1 + m.end()
                method_body_open = EVIDENCE._function_body_open(clean, signature_end)
                method_attrs = _item_attrs(
                    text,
                    clean,
                    start + 1 + m.start(),
                    start + 1,
                )
                if method_body_open is not None and method_body_open < end:
                    method_attrs.extend(
                        EVIDENCE.inner_attributes_after(text, clean, method_body_open)
                    )
                if not _attrs_possible(branches, [*owner_attrs, *method_attrs]):
                    continue
                out.setdefault(
                    f"{trait}::{m.group(1)}",
                    _line_of(text, start + 1 + m.start(1)),
                )
    return out


def _reachable_module_branches(root: Path) -> dict[Path, list[tuple[str, ...]]]:
    """Map reachable Rust module files to their possible inherited cfg attrs."""
    if not root.is_file():
        raise ValueError(f"module inventory root does not exist: {root}")
    branches: dict[Path, list[tuple[str, ...]]] = {}
    active: set[Path] = set()

    def visit(
        path: Path,
        inherited: tuple[str, ...],
        is_root: bool,
        depth: int,
    ) -> None:
        path = path.resolve()
        if depth > EVIDENCE.MAX_MODULE_DEPTH:
            raise ValueError(f"module inventory exceeded depth cap at {path}")
        if path in active:
            raise ValueError(f"module inventory cycle reaches {path}")
        raw = path.read_text(encoding="utf-8")
        clean = EVIDENCE.strip_noise(raw)
        effective = (*inherited, *EVIDENCE.leading_inner_attributes(raw, clean))
        if not EVIDENCE.cfg_attrs_satisfiable(effective):
            return
        if effective in branches.setdefault(path, []):
            return
        branches[path].append(effective)
        active.add(path)
        module_re = re.compile(
            EVIDENCE.RUST_TOKEN_START
            + r"(?:pub\s*(?:\([^)]*\)\s*)?)?"
            + r"mod\s+(?P<name>[A-Za-z_][A-Za-z0-9_]*)"
            + EVIDENCE.RUST_TOKEN_END
            + r"\s*;"
        )
        for declaration in _top_level_matches(clean, module_re):
            attrs = EVIDENCE.preceding_outer_attributes(
                raw,
                clean,
                declaration.start(),
            )
            child_effective = (*effective, *attrs)
            if not EVIDENCE.cfg_attrs_satisfiable(child_effective):
                continue
            try:
                path_attr = EVIDENCE._path_attr(attrs)
            except ValueError as error:
                raise ValueError(
                    f"module path at {path}:{_line_of(raw, declaration.start())} "
                    f"is unresolved: {error}"
                ) from error
            base = EVIDENCE._module_dir(path, is_root)
            child = EVIDENCE._resolve_mod_file(
                base, declaration.group("name"), path_attr
            )
            if child is None:
                raise ValueError(
                    f"module {declaration.group('name')} at "
                    f"{path}:{_line_of(raw, declaration.start())} resolves to no file"
                )
            visit(child, child_effective, False, depth + 1)

        include_re = re.compile(
            EVIDENCE.RUST_TOKEN_START + r"include\s*!\s*(?P<open>[([{])"
        )
        for declaration in _top_level_matches(clean, include_re):
            attrs = EVIDENCE.preceding_outer_attributes(
                raw,
                clean,
                declaration.start(),
            )
            child_effective = (*effective, *attrs)
            if not EVIDENCE.cfg_attrs_satisfiable(child_effective):
                continue
            rel = path.relative_to(REPO_ROOT)
            line = _line_of(raw, declaration.start())
            raise ValueError(
                f"module-level include! at {rel}:{line} requires compiler-resolved "
                "macro ownership; refusing to inventory its token argument as Rust source"
            )
        active.remove(path)

    visit(root, (), True, 0)
    return branches


@lru_cache(maxsize=1)
def _crate_module_branches() -> dict[Path, list[tuple[str, ...]]]:
    return _reachable_module_branches(LIB_FILE)


def _source_branches(path: Path) -> list[tuple[str, ...]]:
    resolved = path.resolve()
    if not resolved.is_relative_to(KRITES_SRC.resolve()):
        return [()]
    branches = _crate_module_branches().get(resolved)
    if not branches:
        raise ValueError(
            f"source inventory owner {path.relative_to(REPO_ROOT)} is unreachable from "
            f"{LIB_FILE.relative_to(REPO_ROOT)}"
        )
    return branches


def _scan_macro_items(directory: Path, macro_name: str) -> dict[str, str]:
    """Return module-item macro capabilities, rejecting nested invocations.

    The population contract is the set of declaration macros at a source
    file's module-item level.  Counting the token inside a function or another
    macro (for example `stringify!(define_op!(...))`) invents capabilities;
    ignoring that shape could hide a moved declaration.  Both therefore fail
    closed.
    """
    out: dict[str, str] = {}
    invocation = re.compile(
        rf"{EVIDENCE.RUST_TOKEN_START}(?:r#)?{re.escape(macro_name)}\s*!\s*"
        r"(?P<open>[([{])"
    )
    resolved_dir = directory.resolve()
    if resolved_dir.is_relative_to(KRITES_SRC.resolve()):
        module_root = (resolved_dir / "mod.rs").resolve()
        inherited = _source_branches(module_root)
        local = _reachable_module_branches(module_root)
        branches = {
            path: [
                (*ancestor, *descendant)
                for ancestor in inherited
                for descendant in descendant_branches
                if EVIDENCE.cfg_attrs_satisfiable([*ancestor, *descendant])
            ]
            for path, descendant_branches in local.items()
        }
    else:
        branches = _reachable_module_branches(directory / "mod.rs")
    reachable = set(branches)
    for orphan in sorted(set(directory.rglob("*.rs")) - reachable):
        orphan_clean = EVIDENCE.strip_noise(orphan.read_text(encoding="utf-8"))
        if invocation.search(orphan_clean):
            raise ValueError(
                f"{macro_name}! appears in unreachable module file "
                f"{orphan.relative_to(REPO_ROOT)}"
            )
    for path in sorted(reachable):
        text = path.read_text(encoding="utf-8")
        clean = EVIDENCE.strip_noise(text)
        rel = path.relative_to(REPO_ROOT)
        for match in invocation.finditer(clean):
            if re.search(r"::\s*$", clean[: match.start()]):
                raise ValueError(
                    f"path-qualified {macro_name}! at "
                    f"{rel}:{_line_of(text, match.start())} is unsupported; "
                    "refusing to detach declaration attributes from the macro owner"
                )
            attrs = EVIDENCE.preceding_outer_attributes(text, clean, match.start())
            if not any(
                EVIDENCE.cfg_attrs_satisfiable([*branch, *attrs])
                for branch in branches[path]
            ):
                continue
            if not _is_top_level(clean, match.start()):
                raise ValueError(
                    f"{macro_name}! at {rel}:{_line_of(text, match.start())} is nested; "
                    "the capability-set scanner only accepts module-item declarations"
                )
            first_arg = re.match(
                r"\s*(?P<name>[A-Z][A-Z0-9_]*)" + EVIDENCE.RUST_TOKEN_END,
                clean[match.end() :],
            )
            if first_arg is None:
                raise ValueError(
                    f"{macro_name}! at {rel}:{_line_of(text, match.start())} does not "
                    "start with an uppercase capability identifier"
                )
            name_start = match.end() + first_arg.start("name")
            out.setdefault(
                first_arg.group("name"),
                f"{rel}:{_line_of(text, name_start)}",
            )
    return out


def _match_arm_keys(file_path: Path, fn_name: str) -> dict[str, str]:
    """{literal match-arm key: 'relpath:line'} inside `fn_name`'s body."""
    text = file_path.read_text(encoding="utf-8")
    clean = EVIDENCE.strip_noise(text)
    branches = _source_branches(file_path)
    start, end, owner_attrs = _block_span(
        text,
        re.compile(
            EVIDENCE.RUST_TOKEN_START
            + r"(?:pub\s*(?:\([^)]*\)\s*)?)?"
            + r"(?:(?:const|async|unsafe)\s+)*"
            + r'(?:extern\s+(?:"[^"]*"\s+)?)?'
            + rf"fn\s+{re.escape(fn_name)}"
            + EVIDENCE.RUST_TOKEN_END
        ),
        branches,
        function_body=True,
    )
    rel = file_path.relative_to(REPO_ROOT)
    match_headers = list(
        re.finditer(
            EVIDENCE.RUST_TOKEN_START + r"Some\s*\(\s*match\s+name\s*\{",
            clean[start:end],
        )
    )
    if len(match_headers) != 1:
        raise ValueError(
            f"{fn_name} has {len(match_headers)} `Some(match name {{...}})` registries; "
            "the capability owner is not unique"
        )
    match_header = match_headers[0]
    match_header_start = start + match_header.start()
    if clean[start + 1 : match_header_start].strip():
        raise ValueError(
            f"{fn_name}'s capability registry is not the function's returned tail expression"
        )
    match_open = start + match_header.end() - 1
    match_end = _matching_delimiter(clean, match_open, "{", "}")
    some_open = clean.find("(", match_header_start, match_open)
    some_end = _matching_delimiter(clean, some_open, "(", ")")
    if some_end < match_end or clean[some_end + 1 : end].strip():
        raise ValueError(
            f"{fn_name}'s capability registry is not the function's returned tail expression"
        )
    arm_clean = clean[match_open + 1 : match_end]

    out: dict[str, str] = {}
    # Split on top-level arm commas/arrows with a full delimiter stack.  Regex
    # over the whole match body sees nested match arms in RHS expressions and
    # can promote a decoy to a real DSL key.
    stack: list[str] = []
    pairs = {")": "(", "]": "[", "}": "{"}
    arm_start = 0
    arrow: int | None = None
    lhs_spans: list[tuple[int, int]] = []
    i = 0
    while i < len(arm_clean):
        ch = arm_clean[i]
        if ch in "([{":
            stack.append(ch)
        elif ch in pairs:
            if not stack or stack[-1] != pairs[ch]:
                raise ValueError(f"unbalanced delimiters in {fn_name}'s match registry")
            stack.pop()
        elif not stack and arm_clean[i : i + 2] == "=>":
            if arrow is not None:
                raise ValueError(
                    f"{fn_name} has a block arm without a separating comma; "
                    "the capability extractor does not guess its next pattern"
                )
            arrow = i
            i += 1
        elif not stack and ch == "," and arrow is not None:
            lhs_spans.append((arm_start, arrow))
            arm_start = i + 1
            arrow = None
        i += 1
    if stack:
        raise ValueError(f"unterminated delimiter in {fn_name}'s match registry")
    if arrow is not None:
        lhs_spans.append((arm_start, arrow))
    elif arm_clean[arm_start:].strip():
        raise ValueError(
            f"{fn_name} has a trailing match fragment with no top-level arm"
        )

    literal_arm = re.compile(
        r"^[ \t\r\n]*(?:\|[ \t\r\n]*)?"
        r'(?P<alts>"[ \t\r\n]*"(?:[ \t\r\n]*\|[ \t\r\n]*"[ \t\r\n]*")*)'
        r"[ \t\r\n]*$"
    )
    for lhs_start, lhs_end in lhs_spans:
        lhs = arm_clean[lhs_start:lhs_end]
        stripped_at = 0
        while True:
            stripped_at += len(lhs[stripped_at:]) - len(lhs[stripped_at:].lstrip())
            consumed = _strip_leading_attributes(lhs[stripped_at:])
            if not consumed:
                break
            stripped_at += consumed
        arm_attrs = _item_attrs(
            text,
            clean,
            match_open + 1 + lhs_start + stripped_at,
            match_open + 1 + lhs_start,
        )
        if not _attrs_possible(branches, [*owner_attrs, *arm_attrs]):
            continue
        pattern_text = lhs[stripped_at:]
        if pattern_text.strip() == "_":
            continue
        arm = literal_arm.fullmatch(pattern_text)
        if arm is None:
            line = _line_of(text, match_open + 1 + lhs_start)
            raise ValueError(
                f"{fn_name} arm at line {line} is not a literal/OR pattern; "
                "capability extraction would be incomplete"
            )
        for literal_start, literal_end in _blank_string_spans(arm.group("alts")):
            absolute_start = (
                match_open
                + 1
                + lhs_start
                + stripped_at
                + arm.start("alts")
                + literal_start
            )
            absolute_end = (
                match_open
                + 1
                + lhs_start
                + stripped_at
                + arm.start("alts")
                + literal_end
            )
            key = _ascii_registry_key(
                text[absolute_start:absolute_end],
                f"{fn_name} arm at line {_line_of(text, absolute_start)}",
            )
            out.setdefault(key, f"{rel}:{_line_of(text, absolute_start)}")
    return out


# WHY these four sets and not four hundred rows: `define_op!` alone declares 139
# scalar functions. Hand-writing a row each would bury the matrix's readable
# per-capability rows under a generated wall, and each row would carry the same
# gate sentence. A recorded member list re-derived from source holds the same
# line -- nothing leaves without the file changing -- at one entry per name.
# Both bindings are recorded separately because they fail differently: an op can
# stay defined while its DSL name is deleted from the lookup, which removes the
# capability from every script while leaving the implementation in place.
CAPABILITY_SET_SOURCES: dict[str, tuple[str, object]] = {
    "scalar-functions": (
        "crates/krites/src/data/functions/**  `define_op!(NAME, ...)`",
        lambda: _scan_macro_items(FUNCTIONS_DIR, "define_op"),
    ),
    "scalar-function-dsl-names": (
        "crates/krites/src/data/expr/op.rs  `get_op` match arms",
        lambda: _match_arm_keys(OP_LOOKUP_FILE, "get_op"),
    ),
    "aggregations": (
        "crates/krites/src/data/aggr/**  `define_aggr!(NAME, ...)`",
        lambda: _scan_macro_items(AGGR_DIR, "define_aggr"),
    ),
    "aggregation-dsl-names": (
        "crates/krites/src/data/aggr/mod.rs  `parse_aggr` match arms",
        lambda: _match_arm_keys(AGGR_DIR / "mod.rs", "parse_aggr"),
    ),
}


def item_tokens(item_text: str) -> set[str]:
    """Tokenize a matrix row's `item` prose into matchable identifiers.

    "SysOp::Compact" -> {"SysOp::Compact", "Compact"}
    "Error, Result" -> {"Error", "Result"}
    "MultiTransaction (transact, commit, abort)" ->
        {"MultiTransaction", "transact", "commit", "abort"}
    """
    tokens: set[str] = set()
    for raw in IDENT_RE.findall(item_text):
        tokens.add(raw)
        if "::" in raw:
            tokens.add(raw.rsplit("::", 1)[-1])
    return tokens


def matrix_item_members(item_text: str) -> set[str]:
    """Return the source capabilities a row's structured `item` names.

    A scoped item (`Db::run`) owns its final segment.  Rows that name a
    container followed by a parenthesized member list own the members, not the
    container (`MultiTransaction (transact, commit, abort)`).  This keeps the
    human-facing item as the visible expression of the mapping while the
    independent `PUBLIC_API_SOURCE_BUNDLES` contract pins deliberate many-to-one
    rows exactly.
    """
    parenthesized = re.fullmatch(
        r"(?P<owner>[A-Za-z_][A-Za-z0-9_:]*)\s*\((?P<members>.*)\)", item_text.strip()
    )
    if parenthesized:
        owner = parenthesized.group("owner")
        return {
            f"{owner}::{token.rsplit('::', 1)[-1]}"
            for token in IDENT_RE.findall(parenthesized.group("members"))
        }
    return set(IDENT_RE.findall(item_text))


def load_matrix() -> list[dict]:
    with MATRIX_FILE.open("rb") as fh:
        data = tomllib.load(fh)
    return data.get("capability", [])


def load_capability_sets() -> list[dict]:
    with MATRIX_FILE.open("rb") as fh:
        data = tomllib.load(fh)
    return data.get("capability_set", [])


def check_category(
    category: str,
    source_items: dict[str, int],
    rows: list[dict],
    source_label: str,
    *,
    allowed_bundles: dict[str, frozenset[str]] | None = None,
) -> list[str]:
    errors: list[str] = []
    cat_rows = [r for r in rows if r.get("category") == category]
    source_names = set(source_items)
    bundle_contract = allowed_bundles or {}
    source_to_rows: dict[str, list[str]] = {name: [] for name in source_names}
    row_matches: list[tuple[dict, set[str]]] = []
    for row in cat_rows:
        row_id = row.get("id", "<no id>")
        declared = matrix_item_members(row.get("item", ""))
        expected_bundle = bundle_contract.get(row_id)
        if expected_bundle is not None:
            if declared != set(expected_bundle):
                errors.append(
                    f"BUNDLE DRIFT [{category}] matrix row '{row_id}' names "
                    f"{sorted(declared)}, but its independent bundle contract is "
                    f"{sorted(expected_bundle)}"
                )
        elif len(declared) > 1:
            errors.append(
                f"OVERBROAD [{category}] matrix row '{row_id}' names multiple source "
                f"capabilities {sorted(declared)} without an approved bundle contract"
            )

        missing_recorded = sorted(declared - source_names)
        for name in missing_recorded:
            errors.append(
                f"MISSING [{category}] matrix row '{row_id}' records source member {name!r}, "
                f"but it does not exist in the current {source_label} inventory"
            )

        matches = declared & source_names
        row_matches.append((row, matches))
        for name in matches:
            source_to_rows[name].append(row_id)

    unmapped = sorted(name for name, owners in source_to_rows.items() if not owners)
    for name in unmapped:
        errors.append(
            f"UNMAPPED [{category}] {name} ({source_label}:{source_items[name]}) "
            f"has no row in {MATRIX_FILE.relative_to(REPO_ROOT)}"
        )

    for row, matches in row_matches:
        row_id = row.get("id", "<no id>")
        if not matches:
            errors.append(
                f"STALE [{category}] matrix row '{row_id}' matches no current "
                f"{source_label} item -- source drifted or the row is fabricated"
            )
    for name, owners in sorted(source_to_rows.items()):
        if len(owners) > 1:
            errors.append(
                f"MULTIMAPPED [{category}] {name} is claimed by rows {sorted(owners)} -- "
                "each source capability must have exactly one matrix owner"
            )

    row_ids = {row.get("id") for row in cat_rows}
    for row_id in sorted(set(bundle_contract) - row_ids):
        errors.append(
            f"MISSING BUNDLE [{category}] approved bundled row '{row_id}' is absent from the matrix"
        )

    return errors


def check_appendix_a(rows: list[dict]) -> list[str]:
    errors: list[str] = []
    cat_rows = [r for r in rows if r.get("category") == "appendix_a"]

    if len(cat_rows) < EXPECTED_APPENDIX_A_ROWS:
        errors.append(
            f"appendix_a row count {len(cat_rows)} is below the floor "
            f"{EXPECTED_APPENDIX_A_ROWS} -- a plan capability may have been "
            "dropped from the matrix (RETIREMENT-PLAN.md Appendix A itself lives outside "
            "this repo and cannot be re-checked from CI; see --plan-md)"
        )

    seen_ids: set[str] = set()
    for row in cat_rows:
        row_id = row.get("id")
        if not row_id:
            errors.append("appendix_a row missing required field 'id'")
            continue
        if row_id in seen_ids:
            errors.append(f"appendix_a row id '{row_id}' is duplicated")
        seen_ids.add(row_id)
        for field in ("item", "source", "dest_wave", "gate"):
            if not row.get(field):
                errors.append(
                    f"appendix_a row '{row_id}' missing required field '{field}'"
                )

    return errors


def check_capability_sets(sets: list[dict]) -> list[str]:
    """Re-derive every `[[capability_set]]` from source and require exact equality.

    Both directions fail. A recorded member absent from source is a dropped
    capability -- the whole point of the file. A source member absent from the
    record is an unrecorded one, and letting that pass would make the set a
    floor that grows silently until it no longer describes anything.
    """
    errors: list[str] = []
    seen: set[str] = set()
    for entry in sets:
        set_id = entry.get("id")
        if not set_id:
            errors.append(f"capability_set missing required field 'id': {entry}")
            continue
        if set_id in seen:
            errors.append(f"duplicate capability_set id '{set_id}'")
        seen.add(set_id)
        if set_id not in CAPABILITY_SET_SOURCES:
            errors.append(
                f"capability_set '{set_id}' has no source derivation in "
                "CAPABILITY_SET_SOURCES -- a set nothing re-derives is prose"
            )
            continue
        for field in ("source", "dest_wave", "gate"):
            if not entry.get(field):
                errors.append(
                    f"capability_set '{set_id}' missing required field '{field}'"
                )
        members = entry.get("members")
        if not isinstance(members, list) or not members:
            errors.append(f"capability_set '{set_id}' has no 'members' list")
            continue
        if sorted(members) != list(members):
            errors.append(f"capability_set '{set_id}' members are not sorted")
        derived = CAPABILITY_SET_SOURCES[set_id][1]()
        dropped = sorted(set(members) - set(derived))
        unrecorded = sorted(set(derived) - set(members))
        for name in dropped:
            errors.append(
                f"DROPPED [capability_set {set_id}] '{name}' is recorded in "
                f"{MATRIX_FILE.relative_to(REPO_ROOT)} but no longer exists in source -- "
                "a capability was removed; the matrix is the record that says it was not "
                "supposed to be"
            )
        for name in unrecorded:
            errors.append(
                f"UNRECORDED [capability_set {set_id}] '{name}' ({derived[name]}) exists in "
                "source but is not in the set's members -- add it, so the set stays the "
                "complete enumeration it claims to be"
            )
    missing_sets = sorted(set(CAPABILITY_SET_SOURCES) - seen)
    for set_id in missing_sets:
        errors.append(
            f"capability_set '{set_id}' has a source derivation but no row in "
            f"{MATRIX_FILE.relative_to(REPO_ROOT)} -- deleting the set would silence the "
            "only check over that population"
        )
    return errors


GATE_TEST_UNPOINTED = {"", "none"}
GATE_TEST_ID_RE = re.compile(r"^[^:\s]+(?:::[^:\s]+)+$")


def check_gate_tests(
    rows: list[dict],
    *,
    nextest_tests: Mapping[str, bool] | None = None,
) -> tuple[list[str], list[str], int, int]:
    """Validate `gate_test` fields, using nextest as the optional authority.

    Returns (errors, notes, pointed, unpointed). Without ``nextest_tests`` this
    check can establish only that a row records a well-shaped pointer. When a
    machine-readable ``cargo nextest list`` is supplied, the mapping is the
    authority for existence and ignored state. An explicitly empty mapping is
    therefore different from no mapping: it rejects every recorded pointer.
    """
    errors: list[str] = []
    notes: list[str] = []

    pointed = 0
    unpointed = 0
    for row in rows:
        row_id = row.get("id", "<no id>")
        value = row.get("gate_test")
        if value is None or (
            isinstance(value, str) and value.strip().lower() in GATE_TEST_UNPOINTED
        ):
            unpointed += 1
            continue
        if not isinstance(value, str):
            errors.append(f"row '{row_id}': gate_test must be a string, got {value!r}")
            unpointed += 1
            continue
        if GATE_TEST_ID_RE.fullmatch(value) is None:
            errors.append(
                f"row '{row_id}': gate_test '{value}' is not a `<binary-id>::<test path>` id"
            )
            unpointed += 1
            continue
        if nextest_tests is None:
            pointed += 1
            continue
        ignored = nextest_tests.get(value)
        if ignored is None:
            errors.append(
                f"row '{row_id}': gate_test '{value}' does not resolve to a runnable, "
                "filter-matching test in the supplied `cargo nextest list` result. "
                'Use "none" rather than a pointer that resolves to nothing'
            )
            unpointed += 1
            continue
        if ignored:
            errors.append(
                f"row '{row_id}': gate_test '{value}' is #[ignore]d in the supplied "
                "nextest result, so it never runs and cannot gate anything"
            )
            unpointed += 1
            continue
        pointed += 1
    return errors, notes, pointed, unpointed


def check_all_rows_well_formed(rows: list[dict]) -> list[str]:
    errors: list[str] = []
    seen_ids: set[str] = set()
    valid_categories = {*SOURCE_DERIVED_CATEGORIES, "appendix_a"}
    for row in rows:
        row_id = row.get("id")
        if not row_id:
            errors.append(f"row missing required field 'id': {row}")
            continue
        if row_id in seen_ids:
            errors.append(f"duplicate row id '{row_id}'")
        seen_ids.add(row_id)
        category = row.get("category")
        if category not in valid_categories:
            errors.append(f"row '{row_id}' has invalid category '{category}'")
        if not row.get("item"):
            errors.append(f"row '{row_id}' missing required field 'item'")
        if not row.get("gate"):
            errors.append(f"row '{row_id}' missing required field 'gate'")
        if not row.get("dest_wave"):
            errors.append(f"row '{row_id}' missing required field 'dest_wave'")
        if category in SOURCE_DERIVED_CATEGORIES and not row.get("source"):
            errors.append(
                f"row '{row_id}' missing required field 'source' for "
                f"source-derived category '{category}'"
            )
    return errors


# WHY(call_sites = -1): the documented not-measured sentinel. A krites
# capability whose call-site count is either too generic a token to grep
# meaningfully, or already counted under a sibling row, records -1 rather
# than a fabricated 0 -- and MUST say which, via `call_sites_method`
# starting with one of these prefixes (or the `covered under <id>` form
# below). check_call_sites_measured enforces this; a bare -1 with no
# recognized reason is a checker failure, not a silent pass.
CALL_SITES_NOT_MEASURED = -1
NOT_MEASURED_PREFIXES = ("not measured:", "not separately measured:")
# WHY these exceptions live outside CAPABILITY_MATRIX.toml: a row must not be
# able to turn its own measured count into `-1` by adding persuasive prose to
# the same mutable record. These are the reviewed judgment boundary. The four
# covered rows additionally bind to one exact measured owner below; the other
# rows are explicitly admitted as not mechanically measurable.
CALL_SITES_COVERED_BY = {
    "api-validity-ts": "datavalue-validity",
    "api-vector": "datavalue-vec",
    "api-persist-mode": "api-db-config",
    "api-db-with-config": "api-db-config",
}
CALL_SITES_UNMEASURED_ROWS = frozenset(
    {
        "api-error-result",
        "api-datavalue-type",
        "api-named-rows",
        "api-array1",
        "api-db-run",
        "api-db-run-read-only",
        "storage-batch-put",
        "storage-range-compact",
        "storage-storage-kind",
        "storage-transact",
        "store-tx-commit",
        "store-tx-del",
        "store-tx-del-range-from-persisted",
        "store-tx-exists",
        "store-tx-get",
        "store-tx-par-put",
        "store-tx-put",
        "store-tx-range-count",
        "store-tx-range-scan",
        "store-tx-range-scan-tuple",
        "store-tx-range-skip-scan-tuple",
        "store-tx-supports-par-put",
    }
)

# WHY: the one row (`sysop-remove-index`) whose call_sites_method aggregates
# several sub-greps in prose rather than being one literal command:
# "sum of quote-anchored grep for 'A', 'B', ... (n1+n2+...)". Parsed and
# re-run per pattern below so it stays measured, not just narrated.
_AGGREGATE_PROSE_RE = re.compile(
    r"^sum of quote-anchored grep for (?P<patterns>.+) across crates/ "
    r"excluding crates/krites/ \((?P<breakdown>[0-9+]+)\)$"
)
_QUOTED_RE = re.compile(r"'([^']+)'")

# WHY: a runnable call_sites_method may carry a trailing prose annotation
# after the literal shell pipeline (e.g. "... | grep -v ^crates/krites/
# (proxy: not disambiguated from ...)"). The two-space run is the
# deliberate separator -- no recorded grep pipeline in this file contains
# one, so splitting on it can't truncate real command text.
_TRAILING_ANNOTATION_SEP = "  "

_FILE_LINE_RE = re.compile(r"^(crates/[\w./-]+\.(?:rs|pest)):([0-9]+(?:,[0-9]+)*)$")


def _quote_anchored_grep(pattern: str) -> str:
    """Reproduce the sysop category's quote-anchored grep template (see the
    category header WHY comment) for one literal DSL-keyword pattern."""
    return (
        "grep -rn -- '[\"'\\''`]" + pattern + "' crates/ "
        "--include='*.rs' | grep -v ^crates/krites/"
    )


def _below_floor(measured: int, recorded: int) -> bool:
    """Whether a measured call-site count has fallen below what the row records.

    WHY a floor and not equality: the recorded figure is a DISAPPEARANCE guard —
    the matrix exists so a capability cannot lose its last consumer unnoticed on
    the way to a sovereign rewrite. Exact equality also fails when a consumer is
    ADDED, which no unrelated PR can avoid causing: `api-db-open-mem` moved
    129 -> 130 -> 131 in a single session, every time because a different crate
    gained a caller, and each correction was a hand edit invalidated by the next
    merge. That made a krites check fail for PRs that never touched krites, and
    made a real disappearance indistinguishable from ordinary growth.

    WHY a row already recording zero is correctly unfailable: the guard fires on
    the TRANSITION to zero — a row recording 5 that measures 0 is caught, because
    0 < 5. A row that already records 0 has no consumer left to lose, and that
    state is recorded rather than hidden. 30 rows sit at zero today; failing them
    would report a disappearance that already happened and was accepted, every
    run, forever.
    """
    return measured < recorded


def _run_grep_pipeline(cmd: str) -> int:
    """Execute the one checker-owned grep shape and return its line count.

    `call_sites_method` is evidence, not executable authority.  Parse its
    historical shell spelling into a closed argv grammar, execute only the
    first grep without a shell, and apply the two admitted exclusions here.
    Exit 1 is grep's honest zero-match result; every other non-zero status is
    a measurement failure, never a fabricated zero.
    """
    try:
        tokens = shlex.split(cmd)
    except ValueError as error:
        raise ValueError(f"invalid call-site grep quoting: {error}") from error

    pipes = [index for index, token in enumerate(tokens) if token == "|"]
    if len(pipes) not in (1, 2):
        raise ValueError("call-site measurement must contain one or two fixed filters")
    first_pipe = pipes[0]
    search = tokens[:first_pipe]
    exclude_krites_end = pipes[1] if len(pipes) == 2 else len(tokens)
    exclude_krites = tokens[first_pipe + 1 : exclude_krites_end]
    exclude_comments = tokens[pipes[1] + 1 :] if len(pipes) == 2 else []

    if (
        len(search) not in (5, 6)
        or search[0] != "grep"
        or search[1] not in {"-rn", "-rEn"}
        or (len(search) == 6 and search[2] != "--")
        or search[-2:] != ["crates/", "--include=*.rs"]
    ):
        raise ValueError("call-site measurement is not an admitted repository grep")
    if exclude_krites != ["grep", "-v", "^crates/krites/"]:
        raise ValueError("call-site measurement must exclude crates/krites exactly")
    if exclude_comments and exclude_comments != [
        "grep",
        "-vP",
        r"^\S+:[0-9]+:\s*//",
    ]:
        raise ValueError("call-site measurement has an unsupported trailing filter")

    pattern_index = 3 if len(search) == 6 else 2
    search_argv = [
        "grep",
        search[1],
        "--include=*.rs",
        "--",
        search[pattern_index],
        "crates/",
    ]
    result = subprocess.run(
        search_argv,
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode not in (0, 1):
        detail = result.stderr.strip() or "grep reported no diagnostic"
        raise ValueError(f"call-site grep exited {result.returncode}: {detail}")
    lines = [
        line
        for line in result.stdout.splitlines()
        if line and not line.startswith("crates/krites/")
    ]
    if exclude_comments:
        comment_line = re.compile(r"^\S+:[0-9]+:\s*//")
        lines = [line for line in lines if not comment_line.search(line)]
    return len(lines)


def check_call_sites_measured(rows: list[dict]) -> list[str]:
    """Enforce the file header's claim that `call_sites` is "a measured
    integer ... never a guess": for every sysop/datavalue/public_api row,
    either call_sites == -1 with a recognized not-measured reason, or
    call_sites_method is actually executed and its output count must be at
    least the declared floor. This is what makes downward drift between the
    prose and reality a build failure instead of something only an
    adversarial review catches by hand.
    """
    errors: list[str] = []
    rows_by_id = {r.get("id"): r for r in rows}
    for row in rows:
        if "call_sites" not in row:
            continue
        row_id = row.get("id", "<no id>")
        call_sites = row["call_sites"]
        method = row.get("call_sites_method", "")

        if not isinstance(call_sites, int) or isinstance(call_sites, bool):
            errors.append(
                f"row '{row_id}': call_sites must be an integer, got {call_sites!r}"
            )
            continue

        if call_sites == CALL_SITES_NOT_MEASURED:
            expected_owner = CALL_SITES_COVERED_BY.get(row_id)
            if expected_owner is not None:
                expected_method = (
                    f"covered under {expected_owner}; not separately re-measured"
                )
                if method != expected_method:
                    errors.append(
                        f"row '{row_id}': call_sites = -1 must bind to the reviewed "
                        f"measurement owner {expected_owner!r}, not {method!r}"
                    )
                    continue
                owner = rows_by_id.get(expected_owner)
                owner_count = owner.get("call_sites") if owner is not None else None
                if (
                    not isinstance(owner_count, int)
                    or isinstance(owner_count, bool)
                    or owner_count < 0
                ):
                    errors.append(
                        f"row '{row_id}': reviewed measurement owner {expected_owner!r} "
                        "is absent or is not itself mechanically measured"
                    )
                continue
            if row_id in CALL_SITES_UNMEASURED_ROWS:
                if not method.startswith(NOT_MEASURED_PREFIXES):
                    errors.append(
                        f"row '{row_id}': reviewed not-measured exception must retain "
                        f"an explicit reason, got {method!r}"
                    )
                continue
            errors.append(
                f"row '{row_id}': call_sites = -1 is not in the checker-owned "
                "reviewed exception contract"
            )
            continue

        if call_sites < 0:
            errors.append(
                f"row '{row_id}': call_sites = {call_sites} is negative and not "
                f"the documented {CALL_SITES_NOT_MEASURED} sentinel"
            )
            continue

        aggregate = _AGGREGATE_PROSE_RE.match(method)
        if aggregate:
            patterns = _QUOTED_RE.findall(aggregate.group("patterns"))
            breakdown = [int(x) for x in aggregate.group("breakdown").split("+")]
            if len(patterns) != len(breakdown):
                errors.append(
                    f"row '{row_id}': aggregate call_sites_method names "
                    f"{len(patterns)} patterns but the breakdown has "
                    f"{len(breakdown)} terms"
                )
                continue
            try:
                measured = [
                    _run_grep_pipeline(_quote_anchored_grep(p)) for p in patterns
                ]
            except ValueError as error:
                errors.append(f"row '{row_id}': call_sites measurement failed: {error}")
                continue
            if measured != breakdown:
                errors.append(
                    f"row '{row_id}': call_sites_method breakdown {breakdown} "
                    f"does not reproduce -- measured {measured} for {patterns}"
                )
                continue
            if _below_floor(sum(measured), call_sites):
                errors.append(
                    f"row '{row_id}': call_sites = {call_sites} is a FLOOR but the "
                    f"aggregate call_sites_method measures only {sum(measured)} -- a "
                    "consumer disappeared; re-verify the capability is still reachable "
                    "before lowering the figure"
                )
            continue

        if not method.strip().startswith("grep"):
            errors.append(
                f"row '{row_id}': call_sites = {call_sites} but "
                f"call_sites_method {method!r} is neither a runnable grep "
                "pipeline nor the recognized aggregate-prose form -- cannot "
                "verify the figure is measured, not guessed"
            )
            continue

        runnable = method.split(_TRAILING_ANNOTATION_SEP, 1)[0].rstrip()
        try:
            measured_count = _run_grep_pipeline(runnable)
        except ValueError as error:
            errors.append(f"row '{row_id}': call_sites measurement failed: {error}")
            continue
        if _below_floor(measured_count, call_sites):
            errors.append(
                f"row '{row_id}': call_sites = {call_sites} is a FLOOR but "
                f"call_sites_method measures only {measured_count} -- a consumer "
                "disappeared; re-verify the capability is still reachable before "
                f"lowering the figure -- `{runnable}`"
            )

    return errors


def check_file_line_refs(rows: list[dict]) -> list[str]:
    """Validate every source-derived row's `source`/`exec_site` citation names
    a real file, an in-range line, AND a line that actually mentions the row's
    item.

    Scoped to the source-derived categories -- appendix_a's `source` cites
    RETIREMENT-PLAN.md, a sibling repo CI cannot read (see module docstring), not a
    local file:line, so it's exempt by construction rather than by a
    fragile regex-non-match skip.

    WHY the anchor check and not just the range check: an in-range line proves
    the file is long enough, nothing more. Every `pub fn` citation into lib.rs
    stayed "valid" through a 61-line drift while pointing at unrelated code,
    and a reader following one landed somewhere plausible. A citation that
    survives the thing it cites moving is a citation to the file, not to the
    capability.
    """
    errors: list[str] = []
    file_cache: dict[str, tuple[list[str], list[str]] | None] = {}
    repo_root = REPO_ROOT.resolve()
    fixed_rule_sources: dict[str, int] | None = None
    for row in rows:
        if row.get("category") not in SOURCE_DERIVED_CATEGORIES:
            continue
        row_id = row.get("id", "<no id>")
        tokens = item_tokens(row.get("item", ""))
        for field in ("source", "exec_site"):
            value = row.get(field)
            if value is None:
                continue
            m = _FILE_LINE_RE.match(value)
            if not m:
                errors.append(
                    f"row '{row_id}': {field} {value!r} is not in the "
                    "required 'crates/.../file.{rs,pest}:N[,N...]' form"
                )
                continue
            rel_path, line_list = m.group(1), m.group(2)
            if rel_path not in file_cache:
                rel = PurePosixPath(rel_path)
                if any(part in {".", ".."} for part in rel.parts):
                    errors.append(
                        f"row '{row_id}': {field} path {rel_path!r} is not lexically contained "
                        "under the repository"
                    )
                    file_cache[rel_path] = None
                else:
                    path = (repo_root / Path(*rel.parts)).resolve()
                    try:
                        path.relative_to(repo_root)
                    except ValueError:
                        errors.append(
                            f"row '{row_id}': {field} path {rel_path!r} resolves outside "
                            "the repository"
                        )
                        file_cache[rel_path] = None
                    else:
                        if path.is_file():
                            raw_text = path.read_text(encoding="utf-8")
                            raw_lines = raw_text.splitlines()
                            code_lines = (
                                EVIDENCE.strip_noise(raw_text).splitlines()
                                if path.suffix == ".rs"
                                else raw_lines
                            )
                            file_cache[rel_path] = (raw_lines, code_lines)
                        else:
                            file_cache[rel_path] = None
            cached = file_cache[rel_path]
            if cached is None:
                errors.append(
                    f"row '{row_id}': {field} references missing file {rel_path}"
                )
                continue
            raw_lines, code_lines = cached
            for line_str in line_list.split(","):
                line_no = int(line_str)
                if not 1 <= line_no <= len(raw_lines):
                    errors.append(
                        f"row '{row_id}': {field} line {line_no} out of range "
                        f"for {rel_path} ({len(raw_lines)} lines)"
                    )
                    continue
                raw_cited = raw_lines[line_no - 1]
                if row.get("category") == "fixed_rule" and field == "source":
                    if fixed_rule_sources is None:
                        fixed_rule_sources = extract_fixed_rule_names()
                    fixed_rel = str(FIXED_RULE_FILE.relative_to(REPO_ROOT))
                    anchored = rel_path == fixed_rel and any(
                        fixed_rule_sources.get(token) == line_no for token in tokens
                    )
                else:
                    cited = code_lines[line_no - 1]
                    anchored = any(
                        re.search(
                            rf"{EVIDENCE.RUST_TOKEN_START}{re.escape(tok)}{EVIDENCE.RUST_TOKEN_END}",
                            cited,
                        )
                        for tok in tokens
                    )
                if not anchored:
                    errors.append(
                        f"row '{row_id}': {field} {rel_path}:{line_no} names none of the "
                        f"row's item tokens {sorted(tokens)} -- the cited line reads "
                        f"{raw_cited.strip()!r}; source moved and the citation did not"
                    )
    return errors


def live_plan_diff(plan_md: Path, rows: list[dict]) -> list[str]:
    """Best-effort, non-gating: diff RETIREMENT-PLAN.md's Appendix A table against the
    matrix's appendix_a rows when the plan repo is locally reachable."""
    if not plan_md.exists():
        return [f"--plan-md {plan_md} does not exist -- skipping live diff"]

    text = plan_md.read_text(encoding="utf-8")
    m = re.search(
        r"^## Appendix A.*?\n(.*?)^## Appendix B", text, re.DOTALL | re.MULTILINE
    )
    if not m:
        return ["could not locate '## Appendix A' ... '## Appendix B' span in RETIREMENT-PLAN.md"]

    plan_rows: list[str] = []
    for line in m.group(1).splitlines():
        if not line.startswith("| "):
            continue
        cells = line.split("|")
        if len(cells) < 2:
            continue
        first_cell = cells[1].strip()
        if first_cell in ("Capability", "---") or set(first_cell) <= {"-"}:
            continue
        plan_rows.append(first_cell)

    matrix_tokens: set[str] = set()
    for row in rows:
        if row.get("category") == "appendix_a":
            matrix_tokens |= item_tokens(row.get("item", ""))

    # WHY: tokenize the raw cell, not a backtick/paren-stripped version.
    # `item_tokens`'s IDENT_RE already extracts only identifier-shaped text
    # -- backticks and parens are non-identifier characters it ignores on
    # its own. Pre-stripping them was actively destructive: a plan cell
    # that is ENTIRELY backticked (`` `MultiTransaction` ``) or entirely
    # parenthesized reduces to the empty string before tokenization ever
    # runs, so `toks` is empty, `toks & matrix_tokens` is always empty, and
    # every such row is reported as drift regardless of whether it's
    # mirrored correctly. Verified against the real RETIREMENT-PLAN.md: this produced
    # 11 false-positive warnings out of 33 rows; removing the strip drops
    # that to 0 while still flagging a genuinely-unmirrored row (tested by
    # injecting one).
    warnings = []
    for plan_row in plan_rows:
        toks = item_tokens(plan_row)
        if not toks & matrix_tokens:
            warnings.append(f"RETIREMENT-PLAN.md Appendix A row not found in matrix: {plan_row!r}")

    if len(plan_rows) != EXPECTED_APPENDIX_A_ROWS:
        warnings.append(
            f"RETIREMENT-PLAN.md Appendix A now has {len(plan_rows)} data rows "
            f"(matrix was authored against {EXPECTED_APPENDIX_A_ROWS}) -- "
            "re-mirror and bump EXPECTED_APPENDIX_A_ROWS"
        )

    return warnings


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--plan-md",
        type=Path,
        default=None,
        help="optional local path to RETIREMENT-PLAN.md for a non-gating live diff",
    )
    parser.add_argument(
        "--nextest-list",
        type=argparse.FileType("r", encoding="utf-8"),
        default=None,
        metavar="LIST_JSON",
        help=(
            "optional `cargo nextest list --message-format json` dump; when supplied, "
            "it is the authority for gate_test existence and ignored state"
        ),
    )
    args = parser.parse_args()

    rows = load_matrix()
    sets = load_capability_sets()

    errors: list[str] = []
    errors += check_all_rows_well_formed(rows)
    errors += check_category(
        "sysop", extract_sysop_variants(), rows, "parse/sys/mod.rs"
    )
    errors += check_category(
        "datavalue", extract_datavalue_variants(), rows, "data/value.rs"
    )
    errors += check_category(
        "public_api",
        extract_lib_public_api(),
        rows,
        "lib.rs",
        allowed_bundles=PUBLIC_API_SOURCE_BUNDLES,
    )
    errors += check_category(
        "fixed_rule", extract_fixed_rule_names(), rows, "fixed_rule/mod.rs"
    )
    errors += check_category(
        "storage_method", extract_storage_methods(), rows, "storage/mod.rs"
    )
    errors += check_appendix_a(rows)
    errors += check_capability_sets(sets)
    errors += check_call_sites_measured(rows)
    errors += check_file_line_refs(rows)
    nextest_tests = (
        EVIDENCE.load_nextest_list(args.nextest_list)
        if args.nextest_list is not None
        else None
    )
    gate_errors, gate_notes, pointed, unpointed = check_gate_tests(
        rows,
        nextest_tests=nextest_tests,
    )
    errors += gate_errors

    if args.plan_md is not None:
        for warning in live_plan_diff(args.plan_md, rows):
            print(f"warning: {warning}", file=sys.stderr)

    for note in gate_notes:
        print(f"note: {note}", file=sys.stderr)

    if errors:
        print("krites capability-coverage matrix check failed:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        print(
            "\nFix by adding/removing a row in "
            f"{MATRIX_FILE.relative_to(REPO_ROOT)} with a named destination "
            "wave and gate -- an unmapped capability is never dropped "
            "silently (RETIREMENT-PLAN.md kill criterion 10).",
            file=sys.stderr,
        )
        return 1

    n_appendix_a = len([r for r in rows if r.get("category") == "appendix_a"])
    set_members = sum(len(s.get("members", [])) for s in sets)
    print(
        "krites capability-coverage matrix check passed: "
        f"{len(extract_sysop_variants())} SysOp variants, "
        f"{len(extract_datavalue_variants())} DataValue variants, "
        f"{len(extract_lib_public_api())} public API items, "
        f"{len(extract_fixed_rule_names())} fixed-rule registry keys, "
        f"{len(extract_storage_methods())} Storage/StoreTx methods -- "
        "the five source-derived categories are mapped exactly; "
        f"{n_appendix_a} internally checked Appendix A mirror rows "
        "(external plan drift remains non-gating; #6867); "
        f"{len(sets)} capability sets covering {set_members} members re-derived exactly."
    )
    if nextest_tests is None:
        print(
            f"gate_test declarations: {pointed} of {len(rows)} rows carry a syntactically "
            f"valid pointer; {unpointed} are unpointed. Existence and ignored state are "
            "deliberately not claimed without --nextest-list."
        )
    else:
        print(
            f"gate_test pointers: {pointed} of {len(rows)} rows resolve to an existing, "
            f"non-ignored test in the supplied nextest result; {unpointed} are unpointed."
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
