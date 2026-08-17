#!/usr/bin/env python3
"""Verify crates/krites/CAPABILITY_MATRIX.toml maps every krites capability.

Wave 0.4 of the krites retirement plan (canon: metis-ops/deliverables/
krites-replacement/PLAN.md, a sibling repo -- see the "Appendix A" section
below for why this script cannot read it in CI). `unmapped` -- present in
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

A sixth category, appendix_a, mirrors PLAN.md's Appendix A table (33 rows
at authoring time). PLAN.md lives outside this repo, so CI cannot re-parse
it; --check only verifies the mirror's internal completeness (row count
floor, required fields, unique ids). Pass --plan-md <path> for an optional,
non-gating local live-diff when both repos are checked out side by side
(e.g. on metis, ~/metis-ops next to ~/dev/aletheia or a dispatch worktree).

`[[capability_set]]` covers the populations too large for one row each --
the ~139 scalar functions and the ~25 aggregations. Each set records its
members as a sorted list and is re-derived from source on every run, with
EXACT set equality: a member that vanishes from source is a dropped
capability, and a member source gained that the set never recorded is an
unrecorded one. The prose these replace ("~102 functions consuming Num",
"All 22 fixed-rule algorithms") named counts that matched nothing
measurable -- the registry holds 26 entries, `define_aggr!` 25, and
`define_op!` 139 against 138 DSL-reachable names.

`gate_test` names the test that would fail if the capability disappeared,
as a `<binary-id>::<test path>` id resolved against a source-derived index
of the crate's tests (scripts/krites_test_index.py). The named test must
exist and must not be `#[ignore]`d. `"none"` or an absent field is legal
and counts as UNPOINTED -- an honestly unpointed row is worth more than a
pointer at a test that does not exercise the capability.

WARNING(what a pointer proves): a resolving `gate_test` proves the
capability cannot be deleted without a test disappearing or failing. It
does NOT prove the row's `gate` sentence is asserted anywhere -- several
gates name conformance behaviour (recorded-vector replay, post-crash
visibility, multiset-vs-sequence) that no current test checks. It also does
not prove the test PASSED: this checker runs in a pure-python CI job with
no cargo and no compiled test binaries, so it verifies existence and
runnability, never a result. Read the pointed count as "cannot vanish
unnoticed", not as "verified".

`call_sites` on a sysop/datavalue/public_api row is a measured integer,
never a guess: `check_call_sites_measured` re-executes each row's
`call_sites_method` and fails the build if the measurement has fallen BELOW
the declared count. The figure is a floor, not an equality: it exists so a
capability cannot lose its last consumer unnoticed, and a consumer being
ADDED is not a defect. Requiring equality made every unrelated PR that
gained a caller fail a krites check -- `api-db-open-mem` moved 129 -> 130 ->
131 in one session, each a hand edit invalidated by the next merge -- and it
made a real disappearance indistinguishable from ordinary growth. `call_sites = -1` is the one documented exception -- it
means "not measured", and is only legal when `call_sites_method` states why
(a generic-token/type-level rationale, or `covered under <other-row-id>;
not separately re-measured`). Zero call sites is never grounds to drop a
row -- see the plan's B7 finding and kill criterion 10 -- and -1 is never a
stand-in for zero.

Usage:
    python3 scripts/check-krites-capability-matrix.py
    python3 scripts/check-krites-capability-matrix.py --plan-md /path/to/PLAN.md
    python3 scripts/check-krites-capability-matrix.py --nextest-list list.json
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tomllib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import krites_test_index as KTI  # noqa: E402

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

# WHY: matches the plan's own known-count baseline (PLAN.md Appendix A, 33
# data rows as of this checker's authoring). A floor, not a ceiling -- the
# plan may grow rows; it must never silently shrink under this file.
EXPECTED_APPENDIX_A_ROWS = 33

IDENT_RE = re.compile(r"[A-Za-z_][A-Za-z0-9_:]*")


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
    while text[pos : pos + 2] == "#[":
        depth = 0
        j = pos
        while j < len(text):
            if text[j] == "[":
                depth += 1
            elif text[j] == "]":
                depth -= 1
                if depth == 0:
                    j += 1
                    break
            j += 1
        pos = j
        while pos < len(text) and text[pos] in " \t":
            pos += 1
    return pos


def extract_enum_variants(text: str, enum_name: str) -> dict[str, int]:
    """Return {variant_name: line_number} for a `pub enum <enum_name> { ... }` block.

    Scoped to the first top-level enum by that name. Strips line comments
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
    lines = text.splitlines()
    start = None
    for i, line in enumerate(lines):
        if re.match(rf"^\s*pub enum {re.escape(enum_name)}\b", line):
            start = i
            break
    if start is None:
        raise ValueError(f"enum {enum_name} not found")

    variants: dict[str, int] = {}
    enum_depth = 0
    started_body = False
    # WHY: bracket depth left open by a struct/tuple variant payload that
    # spans multiple lines (e.g. `Query {\n    source: Box<Error>,\n },`).
    # While > 0, subsequent lines are payload interior (field names, etc.)
    # -- never a new variant -- so they're depth-tracked only, not scanned.
    payload_depth = 0
    for i in range(start, len(lines)):
        # Strip trailing line comments before anything else -- `// Sneaky,`
        # must never be mistaken for code.
        code = re.sub(r"//.*$", "", lines[i])
        enum_depth += code.count("{") - code.count("}")
        if "{" in code and not started_body:
            started_body = True
            code = code[code.index("{") + 1 :]
        elif not started_body:
            continue
        if enum_depth <= 0:
            break

        if payload_depth > 0:
            payload_depth += (
                code.count("(") + code.count("{") - code.count(")") - code.count("}")
            )
            continue

        pos = 0
        while pos < len(code):
            stripped = code[pos:].lstrip()
            pos += len(code[pos:]) - len(stripped)
            if not stripped:
                break
            consumed = _strip_leading_attributes(code[pos:])
            if consumed:
                pos += consumed
                continue
            # WHY: `[,({]` covers all three variant shapes -- unit
            # (`Name,`), tuple (`Name(...)`), and struct (`Name { ... }`,
            # e.g. MultiTransactionError::Query { source: Box<Error> }).
            # Missing the `{` case here silently drops struct variants from
            # extraction, which would make the checker pass without ever
            # having looked at them -- an "unverifiable collapsing to
            # clean", exactly what the tri-state Verdict pattern (Appendix
            # A row: Tri-state Verdict) exists to forbid.
            m = re.match(r"^([A-Za-z_][A-Za-z0-9_]*)\s*([,({])", code[pos:])
            if not m:
                break
            name, opener = m.group(1), m.group(2)
            variants.setdefault(name, i + 1)
            pos += m.end()
            if opener in "({":
                depth = 1
                j = pos
                while j < len(code) and depth > 0:
                    if code[j] in "({":
                        depth += 1
                    elif code[j] in ")}":
                        depth -= 1
                    j += 1
                if depth > 0:
                    payload_depth = depth
                    break
                pos = j
            while pos < len(code) and code[pos] in ", \t":
                pos += 1
    return variants


def extract_sysop_variants() -> dict[str, int]:
    text = SYSOP_FILE.read_text(encoding="utf-8")
    return extract_enum_variants(text, "SysOp")


def extract_datavalue_variants() -> dict[str, int]:
    text = DATAVALUE_FILE.read_text(encoding="utf-8")
    return extract_enum_variants(text, "DataValue")


def extract_lib_public_api() -> dict[str, int]:
    """Return {identifier: line_number} for lib.rs's Db-boundary public surface.

    Covers: `pub use` re-exports (crate-root, multi-line aware), `pub fn`
    names (all Db/MultiTransaction methods live in this one file), and the
    `MultiTransactionError` enum's variants. Does not descend into other
    `pub mod` submodules -- see module docstring for the scope rationale.
    """
    text = LIB_FILE.read_text(encoding="utf-8")
    lines = text.splitlines()
    items: dict[str, int] = {}

    # `pub use` statements: join across lines up to the terminating `;`.
    for m in re.finditer(r"pub use ([^;]+);", text, re.DOTALL):
        line_no = text[: m.start()].count("\n") + 1
        body = m.group(1)
        # Drop the leading path (crate::a::b::{...} or crate::a::b::Name or
        # module::Name) -- keep only the final `{...}` group or final segment.
        brace = re.search(r"\{([^}]*)\}", body)
        if brace:
            names = [n.strip() for n in brace.group(1).split(",") if n.strip()]
        else:
            names = [body.strip().rsplit("::", 1)[-1]]
        for name in names:
            name = name.split(" as ")[-1].strip()
            items.setdefault(name, line_no)

    # `pub fn <name>` -- all methods in this file belong to Db or
    # MultiTransaction (verified by inspection; this file defines no other
    # pub-fn-bearing type).
    for i, line in enumerate(lines):
        m = re.match(r"^\s*pub fn ([A-Za-z_][A-Za-z0-9_]*)", line)
        if m:
            items.setdefault(m.group(1), i + 1)

    # MultiTransactionError variants, same block-extraction as the enums above.
    for name, line_no in extract_enum_variants(text, "MultiTransactionError").items():
        items.setdefault(name, line_no)

    return items


def _block_span(text: str, header_re: re.Pattern[str]) -> tuple[int, int]:
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
    clean = KTI.strip_noise(text)
    m = header_re.search(clean)
    if m is None:
        raise ValueError(f"could not locate {header_re.pattern} in source")
    open_at = clean.find("{", m.end())
    if open_at == -1:
        raise ValueError(f"no block body after {header_re.pattern}")
    depth = 0
    for i in range(open_at, len(clean)):
        if clean[i] == "{":
            depth += 1
        elif clean[i] == "}":
            depth -= 1
            if depth == 0:
                return open_at, i
    raise ValueError(f"unterminated block after {header_re.pattern}")


def _line_of(text: str, offset: int) -> int:
    return text.count("\n", 0, offset) + 1


def extract_fixed_rule_names() -> dict[str, int]:
    """Return {registry key: line} for every `DEFAULT_FIXED_RULES` entry.

    The key -- the string a script writes after `<~` -- is the capability. Two
    keys bound to one type (`DFS` and `DepthFirstSearch`) are two capabilities:
    dropping either breaks scripts that name it, and nothing else in the crate
    would notice.
    """
    text = FIXED_RULE_FILE.read_text(encoding="utf-8")
    start, end = _block_span(text, re.compile(r"static DEFAULT_FIXED_RULES\b"))
    body = text[start:end]
    out: dict[str, int] = {}
    for m in re.finditer(r'"([A-Za-z][A-Za-z0-9_]*)"\.to_string\(\)', body):
        out.setdefault(m.group(1), _line_of(text, start + m.start()))
    return out


def extract_storage_methods() -> dict[str, int]:
    """Return {`Trait::method`: line} for the `Storage` and `StoreTx` traits."""
    text = STORAGE_FILE.read_text(encoding="utf-8")
    out: dict[str, int] = {}
    for trait in ("Storage", "StoreTx"):
        start, end = _block_span(text, re.compile(rf"pub trait {trait}\b"))
        for m in re.finditer(r"^\s{4}fn\s+([a-z_][A-Za-z0-9_]*)", text[start:end], re.MULTILINE):
            out.setdefault(f"{trait}::{m.group(1)}", _line_of(text, start + m.start(1)))
    return out


def _scan_dir(directory: Path, pattern: re.Pattern[str]) -> dict[str, str]:
    """{captured name: 'relpath:line'} over every .rs file under `directory`."""
    out: dict[str, str] = {}
    for path in sorted(directory.rglob("*.rs")):
        text = path.read_text(encoding="utf-8")
        rel = path.relative_to(REPO_ROOT)
        for m in pattern.finditer(text):
            out.setdefault(m.group(1), f"{rel}:{_line_of(text, m.start(1))}")
    return out


def _match_arm_keys(file_path: Path, fn_name: str) -> dict[str, str]:
    """{literal match-arm key: 'relpath:line'} inside `fn_name`'s body."""
    text = file_path.read_text(encoding="utf-8")
    start, end = _block_span(text, re.compile(rf"fn {re.escape(fn_name)}\b"))
    rel = file_path.relative_to(REPO_ROOT)
    out: dict[str, str] = {}
    for m in re.finditer(r'^\s*"([^"]+)"\s*=>', text[start:end], re.MULTILINE):
        out.setdefault(m.group(1), f"{rel}:{_line_of(text, start + m.start(1))}")
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
        lambda: _scan_dir(FUNCTIONS_DIR, re.compile(r"define_op!\(\s*([A-Z][A-Z0-9_]*)")),
    ),
    "scalar-function-dsl-names": (
        "crates/krites/src/data/expr/op.rs  `get_op` match arms",
        lambda: _match_arm_keys(OP_LOOKUP_FILE, "get_op"),
    ),
    "aggregations": (
        "crates/krites/src/data/aggr/**  `define_aggr!(NAME, ...)`",
        lambda: _scan_dir(AGGR_DIR, re.compile(r"define_aggr!\(\s*([A-Z][A-Z0-9_]*)")),
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
) -> list[str]:
    errors: list[str] = []
    cat_rows = [r for r in rows if r.get("category") == category]

    all_tokens: set[str] = set()
    row_tokens: list[tuple[dict, set[str]]] = []
    for row in cat_rows:
        toks = item_tokens(row.get("item", ""))
        row_tokens.append((row, toks))
        all_tokens |= toks

    unmapped = sorted(name for name in source_items if name not in all_tokens)
    for name in unmapped:
        errors.append(
            f"UNMAPPED [{category}] {name} ({source_label}:{source_items[name]}) "
            f"has no row in {MATRIX_FILE.relative_to(REPO_ROOT)}"
        )

    stale = []
    for row, toks in row_tokens:
        if not toks & set(source_items):
            stale.append(row.get("id", "<no id>"))
    for row_id in sorted(stale):
        errors.append(
            f"STALE [{category}] matrix row '{row_id}' matches no current "
            f"{source_label} item -- source drifted or the row is fabricated"
        )

    return errors


def check_appendix_a(rows: list[dict]) -> list[str]:
    errors: list[str] = []
    cat_rows = [r for r in rows if r.get("category") == "appendix_a"]

    if len(cat_rows) < EXPECTED_APPENDIX_A_ROWS:
        errors.append(
            f"appendix_a row count {len(cat_rows)} is below the floor "
            f"{EXPECTED_APPENDIX_A_ROWS} -- a plan capability may have been "
            "dropped from the matrix (PLAN.md Appendix A itself lives outside "
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
                errors.append(f"appendix_a row '{row_id}' missing required field '{field}'")

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
                errors.append(f"capability_set '{set_id}' missing required field '{field}'")
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
                f"source but is not in the set's members -- add it, so the set stays the "
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


def check_gate_tests(rows: list[dict]) -> tuple[list[str], list[str], int, int]:
    """Resolve every `gate_test` against the crate's source-derived test index.

    Returns (errors, notes, pointed, unpointed). A pointer must name a test the
    index contains and that is not `#[ignore]`d; anything else is a pointer to
    nothing, which is the failure mode this field exists to end.
    """
    errors: list[str] = []
    notes: list[str] = []
    index, unresolved = KTI.build_index(KRITES_DIR, REPO_ROOT)
    for problem in unresolved:
        errors.append(
            f"test index could not resolve a module -- gate_test resolution would be "
            f"incomplete and would report real tests as missing: {problem}"
        )

    pointed = 0
    unpointed = 0
    for row in rows:
        row_id = row.get("id", "<no id>")
        value = row.get("gate_test")
        if value is None or (isinstance(value, str) and value.strip().lower() in GATE_TEST_UNPOINTED):
            unpointed += 1
            continue
        if not isinstance(value, str):
            errors.append(f"row '{row_id}': gate_test must be a string, got {value!r}")
            unpointed += 1
            continue
        case = index.get(value)
        if case is None:
            errors.append(
                f"row '{row_id}': gate_test '{value}' names no test in crates/krites. "
                "Expected a `<binary-id>::<test path>` id as listed by "
                "`cargo nextest list -p krites` (or scripts/krites_test_index.py). "
                "Use \"none\" rather than a pointer that resolves to nothing"
            )
            unpointed += 1
            continue
        if case.ignored:
            errors.append(
                f"row '{row_id}': gate_test '{value}' ({case.file}:{case.line}) is "
                "#[ignore]d, so it never runs and cannot gate anything"
            )
            unpointed += 1
            continue
        # WHY only feature-shaped guards: `cfg(test)` is on every unit test by
        # construction and carries no information. A `feature = ` / `not(...)`
        # guard does: the test runs only in a build that selects that arm, so
        # the pointer gates less than it appears to.
        conditional = [g for g in case.cfg_guards if "feature" in g or "not(" in g]
        if conditional:
            notes.append(
                f"row '{row_id}': gate_test '{value}' sits under {conditional} -- "
                "it runs only in a build that selects those cfgs"
            )
        pointed += 1
    return errors, notes, pointed, unpointed


def check_all_rows_well_formed(rows: list[dict]) -> list[str]:
    errors: list[str] = []
    seen_ids: set[str] = set()
    valid_categories = {
        "sysop",
        "datavalue",
        "public_api",
        "fixed_rule",
        "storage_method",
        "appendix_a",
    }
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
_COVERED_UNDER_RE = re.compile(r"^covered under ([a-z0-9-]+); not separately re-measured$")

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
    """Execute a recorded grep pipeline from the repo root; return the
    matched line count. A pipeline with zero matches exits non-zero (the
    trailing `grep -v` convention) -- expected, not an error; only stdout
    is read."""
    result = subprocess.run(
        cmd, shell=True, cwd=REPO_ROOT, capture_output=True, text=True, check=False
    )
    return len([line for line in result.stdout.splitlines() if line])


def check_call_sites_measured(rows: list[dict]) -> list[str]:
    """Enforce the file header's claim that `call_sites` is "a measured
    integer ... never a guess": for every sysop/datavalue/public_api row,
    either call_sites == -1 with a recognized not-measured reason, or
    call_sites_method is actually executed and its output count must equal
    the declared figure. This is what makes drift between the prose and
    reality a build failure instead of something only an adversarial review
    catches by hand.
    """
    errors: list[str] = []
    row_ids = {r.get("id") for r in rows}
    for row in rows:
        if "call_sites" not in row:
            continue
        row_id = row.get("id", "<no id>")
        call_sites = row["call_sites"]
        method = row.get("call_sites_method", "")

        if not isinstance(call_sites, int) or isinstance(call_sites, bool):
            errors.append(f"row '{row_id}': call_sites must be an integer, got {call_sites!r}")
            continue

        if call_sites == CALL_SITES_NOT_MEASURED:
            covered = _COVERED_UNDER_RE.match(method)
            if covered:
                if covered.group(1) not in row_ids:
                    errors.append(
                        f"row '{row_id}': call_sites_method references "
                        f"unknown row id '{covered.group(1)}'"
                    )
                continue
            if method.startswith(NOT_MEASURED_PREFIXES):
                continue
            errors.append(
                f"row '{row_id}': call_sites = -1 (not-measured sentinel) but "
                f"call_sites_method {method!r} doesn't start with a recognized "
                "reason ('not measured:', 'not separately measured:', or "
                "'covered under <id>; not separately re-measured')"
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
            measured = [_run_grep_pipeline(_quote_anchored_grep(p)) for p in patterns]
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
        measured_count = _run_grep_pipeline(runnable)
        if _below_floor(measured_count, call_sites):
            errors.append(
                f"row '{row_id}': call_sites = {call_sites} is a FLOOR but "
                f"call_sites_method measures only {measured_count} -- a consumer "
                f"disappeared; re-verify the capability is still reachable before "
                f"lowering the figure -- `{runnable}`"
            )

    return errors


_CITED_CATEGORIES = ("sysop", "datavalue", "public_api", "fixed_rule", "storage_method")


def check_file_line_refs(rows: list[dict]) -> list[str]:
    """Validate every source-derived row's `source`/`exec_site` citation names
    a real file, an in-range line, AND a line that actually mentions the row's
    item.

    Scoped to the source-derived categories -- appendix_a's `source` cites
    PLAN.md, a sibling repo CI cannot read (see module docstring), not a
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
    file_cache: dict[str, list[str] | None] = {}
    for row in rows:
        if row.get("category") not in _CITED_CATEGORIES:
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
                path = REPO_ROOT / rel_path
                file_cache[rel_path] = (
                    path.read_text(encoding="utf-8").splitlines() if path.is_file() else None
                )
            lines = file_cache[rel_path]
            if lines is None:
                errors.append(f"row '{row_id}': {field} references missing file {rel_path}")
                continue
            for line_str in line_list.split(","):
                line_no = int(line_str)
                if not 1 <= line_no <= len(lines):
                    errors.append(
                        f"row '{row_id}': {field} line {line_no} out of range "
                        f"for {rel_path} ({len(lines)} lines)"
                    )
                    continue
                cited = lines[line_no - 1]
                if not any(re.search(rf"\b{re.escape(tok)}", cited) for tok in tokens):
                    errors.append(
                        f"row '{row_id}': {field} {rel_path}:{line_no} names none of the "
                        f"row's item tokens {sorted(tokens)} -- the cited line reads "
                        f"{cited.strip()!r}; source moved and the citation did not"
                    )
    return errors


def live_plan_diff(plan_md: Path, rows: list[dict]) -> list[str]:
    """Best-effort, non-gating: diff PLAN.md's Appendix A table against the
    matrix's appendix_a rows when the plan repo is locally reachable."""
    if not plan_md.exists():
        return [f"--plan-md {plan_md} does not exist -- skipping live diff"]

    text = plan_md.read_text(encoding="utf-8")
    m = re.search(r"^## Appendix A.*?\n(.*?)^## Appendix B", text, re.DOTALL | re.MULTILINE)
    if not m:
        return ["could not locate '## Appendix A' ... '## Appendix B' span in PLAN.md"]

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
    # mirrored correctly. Verified against the real PLAN.md: this produced
    # 11 false-positive warnings out of 33 rows; removing the strip drops
    # that to 0 while still flagging a genuinely-unmirrored row (tested by
    # injecting one).
    warnings = []
    for plan_row in plan_rows:
        toks = item_tokens(plan_row)
        if not toks & matrix_tokens:
            warnings.append(f"PLAN.md Appendix A row not found in matrix: {plan_row!r}")

    if len(plan_rows) != EXPECTED_APPENDIX_A_ROWS:
        warnings.append(
            f"PLAN.md Appendix A now has {len(plan_rows)} data rows "
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
        help="optional local path to PLAN.md for a non-gating live diff",
    )
    parser.add_argument(
        "--nextest-list",
        type=Path,
        default=None,
        help=(
            "optional `cargo nextest list --message-format json` dump; cross-checks the "
            "source-derived test index against a real build's test list"
        ),
    )
    args = parser.parse_args()

    rows = load_matrix()
    sets = load_capability_sets()

    errors: list[str] = []
    errors += check_all_rows_well_formed(rows)
    errors += check_category("sysop", extract_sysop_variants(), rows, "parse/sys/mod.rs")
    errors += check_category("datavalue", extract_datavalue_variants(), rows, "data/value.rs")
    errors += check_category("public_api", extract_lib_public_api(), rows, "lib.rs")
    errors += check_category("fixed_rule", extract_fixed_rule_names(), rows, "fixed_rule/mod.rs")
    errors += check_category("storage_method", extract_storage_methods(), rows, "storage/mod.rs")
    errors += check_appendix_a(rows)
    errors += check_capability_sets(sets)
    errors += check_call_sites_measured(rows)
    errors += check_file_line_refs(rows)
    gate_errors, gate_notes, pointed, unpointed = check_gate_tests(rows)
    errors += gate_errors

    if args.plan_md is not None:
        for warning in live_plan_diff(args.plan_md, rows):
            print(f"warning: {warning}", file=sys.stderr)

    if args.nextest_list is not None:
        index, _ = KTI.build_index(KRITES_DIR, REPO_ROOT)
        delta = KTI.cross_validate(index, KTI.load_nextest_list(args.nextest_list))
        for key, values in delta.items():
            print(f"nextest cross-check {key}: {len(values)}", file=sys.stderr)
            for value in values:
                print(f"    {value}", file=sys.stderr)
        if delta["only_in_nextest"] or delta["ignored_disagrees"]:
            errors.append(
                "the source-derived test index disagrees with the supplied nextest listing "
                "in the direction that matters: a real test the index cannot see would make "
                "a correct gate_test read as missing"
            )

    for note in gate_notes:
        print(f"note: {note}", file=sys.stderr)

    if errors:
        print("krites capability-coverage matrix check failed:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        print(
            f"\nFix by adding/removing a row in "
            f"{MATRIX_FILE.relative_to(REPO_ROOT)} with a named destination "
            "wave and gate -- an unmapped capability is never dropped "
            "silently (PLAN.md kill criterion 10).",
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
        f"{len(extract_storage_methods())} Storage/StoreTx methods, "
        f"{n_appendix_a} Appendix A rows -- all mapped, no stale rows; "
        f"{len(sets)} capability sets covering {set_members} members re-derived exactly."
    )
    print(
        f"gate_test pointers: {pointed} of {len(rows)} rows resolve to an existing, "
        f"non-ignored test; {unpointed} are unpointed. Resolution is existence + "
        "runnability against a source-derived index -- this job has no cargo, so no "
        "pointer here is evidence that its test PASSED."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
