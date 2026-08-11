#!/usr/bin/env python3
"""Verify crates/krites/CAPABILITY_MATRIX.toml maps every krites capability.

Wave 0.4 of the krites retirement plan (canon: metis-ops/deliverables/
krites-replacement/PLAN.md, a sibling repo -- see the "Appendix A" section
below for why this script cannot read it in CI). `unmapped` -- present in
source but absent from the matrix -- is a build failure. A matrix row with
no matching source item (stale) fails the same way, so drift is caught in
both directions: the matrix cannot silently fall behind source, and it
cannot silently keep a row for something that no longer exists.

Three mechanically re-derived categories, checked against live source in
THIS repo:

  sysop        every `SysOp` variant, crates/krites/src/parse/sys/mod.rs
  datavalue    every `DataValue` variant, crates/krites/src/data/value.rs
  public_api   every public item at the `Db` / crate-root boundary,
               crates/krites/src/lib.rs -- scoped to what wave 0.5's
               recorder covers (the Db facade + top-level pub use
               re-exports), not every pub item in every krites submodule.
               `pub mod` declarations and the bare `Db`/`MultiTransaction`
               struct names are intentionally not separate line items --
               their methods are what the matrix rows track.

A fourth category, appendix_a, mirrors PLAN.md's Appendix A table (33 rows
at authoring time). PLAN.md lives outside this repo, so CI cannot re-parse
it; --check only verifies the mirror's internal completeness (row count
floor, required fields, unique ids). Pass --plan-md <path> for an optional,
non-gating local live-diff when both repos are checked out side by side
(e.g. on metis, ~/metis-ops next to ~/dev/aletheia or a dispatch worktree).

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
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tomllib
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
KRITES_SRC = REPO_ROOT / "crates" / "krites" / "src"
SYSOP_FILE = KRITES_SRC / "parse" / "sys" / "mod.rs"
DATAVALUE_FILE = KRITES_SRC / "data" / "value.rs"
LIB_FILE = KRITES_SRC / "lib.rs"
MATRIX_FILE = REPO_ROOT / "crates" / "krites" / "CAPABILITY_MATRIX.toml"

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


def check_all_rows_well_formed(rows: list[dict]) -> list[str]:
    errors: list[str] = []
    seen_ids: set[str] = set()
    valid_categories = {"sysop", "datavalue", "public_api", "appendix_a"}
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


def check_file_line_refs(rows: list[dict]) -> list[str]:
    """Validate every sysop/datavalue/public_api `source`/`exec_site`
    citation resolves to a real file with the cited line(s) in range.

    Scoped to these three categories -- appendix_a's `source` cites
    PLAN.md, a sibling repo CI cannot read (see module docstring), not a
    local file:line, so it's exempt by construction rather than by a
    fragile regex-non-match skip.
    """
    errors: list[str] = []
    file_cache: dict[str, int] = {}
    for row in rows:
        if row.get("category") not in ("sysop", "datavalue", "public_api"):
            continue
        row_id = row.get("id", "<no id>")
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
            path = REPO_ROOT / rel_path
            if rel_path not in file_cache:
                if not path.is_file():
                    file_cache[rel_path] = -1
                else:
                    file_cache[rel_path] = len(path.read_text(encoding="utf-8").splitlines())
            n_lines = file_cache[rel_path]
            if n_lines == -1:
                errors.append(f"row '{row_id}': {field} references missing file {rel_path}")
                continue
            for line_str in line_list.split(","):
                line_no = int(line_str)
                if not 1 <= line_no <= n_lines:
                    errors.append(
                        f"row '{row_id}': {field} line {line_no} out of range "
                        f"for {rel_path} ({n_lines} lines)"
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
    args = parser.parse_args()

    rows = load_matrix()

    errors: list[str] = []
    errors += check_all_rows_well_formed(rows)
    errors += check_category("sysop", extract_sysop_variants(), rows, "parse/sys/mod.rs")
    errors += check_category("datavalue", extract_datavalue_variants(), rows, "data/value.rs")
    errors += check_category("public_api", extract_lib_public_api(), rows, "lib.rs")
    errors += check_appendix_a(rows)
    errors += check_call_sites_measured(rows)
    errors += check_file_line_refs(rows)

    if args.plan_md is not None:
        for warning in live_plan_diff(args.plan_md, rows):
            print(f"warning: {warning}", file=sys.stderr)

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

    n_sysop = len(extract_sysop_variants())
    n_datavalue = len(extract_datavalue_variants())
    n_public_api = len(extract_lib_public_api())
    n_appendix_a = len([r for r in rows if r.get("category") == "appendix_a"])
    print(
        "krites capability-coverage matrix check passed: "
        f"{n_sysop} SysOp variants, {n_datavalue} DataValue variants, "
        f"{n_public_api} public API items, {n_appendix_a} Appendix A rows -- "
        "all mapped, no stale rows."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
