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

Usage:
    python3 scripts/check-krites-capability-matrix.py
    python3 scripts/check-krites-capability-matrix.py --plan-md /path/to/PLAN.md
"""

from __future__ import annotations

import argparse
import re
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


def extract_enum_variants(text: str, enum_name: str) -> dict[str, int]:
    """Return {variant_name: line_number} for a `pub enum <enum_name> { ... }` block.

    Scoped to the first top-level enum by that name; skips doc comments
    (`///`), inner comments, and attributes (`#[...]`). Stops at the
    closing brace that returns to column 0.
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
    depth = 0
    started_body = False
    for i in range(start, len(lines)):
        line = lines[i]
        depth += line.count("{") - line.count("}")
        if "{" in line and not started_body:
            started_body = True
            continue
        if not started_body:
            continue
        if depth <= 0:
            break
        stripped = line.strip()
        if not stripped or stripped.startswith("///") or stripped.startswith("//"):
            continue
        if stripped.startswith("#["):
            continue
        # WHY: `[,({]` covers all three variant shapes -- unit (`Name,`),
        # tuple (`Name(...)`), and struct (`Name { ... }`, e.g.
        # MultiTransactionError::Query { source: Box<Error> }). Missing the
        # `{` case here silently drops struct variants from extraction,
        # which would make the checker pass without ever having looked at
        # them -- an "unverifiable collapsing to clean", exactly what the
        # tri-state Verdict pattern (Appendix A row: Tri-state Verdict) exists
        # to forbid.
        m = re.match(r"^([A-Za-z_][A-Za-z0-9_]*)\s*[,({]", stripped)
        if m:
            variants[m.group(1)] = i + 1
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

    warnings = []
    for plan_row in plan_rows:
        toks = item_tokens(re.sub(r"`[^`]*`|\([^)]*\)", "", plan_row))
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
