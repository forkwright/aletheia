#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Pure aggregator of remaining krites -> sovereign CozoDB tethers.

Prints a per-line breakdown of five tether classes and one total. Every
number this script prints is READ from an existing artifact (PROVENANCE.toml,
CAPABILITY_MATRIX.toml, the crate's license/provenance files under
crates/krites/) or a direct filesystem/API presence check -- this script
computes nothing that check-krites-provenance.py, check-krites-capability-
matrix.py, or check-krites-verbatim-drift.py do not already own, and it must
never become a second source of truth for any of those numbers.

NECESSARY, NOT SUFFICIENT: reaching TOTAL=0 makes a false "krites is fully
de-tethered from CozoDB" completion claim structurally impossible to assert
-- all five conditions below have to be independently true first. It does
NOT prove the rewrite is clean. The verbatim-similarity metric this repo
already relies on cannot separate the two classes a completion claim would
need it to separate: a confirmed statement-for-statement transliteration
(fixed_rule/algos/dfs_native.rs) measured 26.6% against its real source,
while a confirmed independent rewrite (degree_centrality_native.rs) measured
HIGHER at 32.1% (aletheia#6656, open). A metric that ranks a transliteration
below a rewrite cannot be the thing that certifies either one. A TOTAL of 0
says the ledger, the matrix, and the tracked issue set no longer assert
anything false; it says nothing about whether any individual sovereign file
is actually a clean rewrite.

Lines:
  1. PROVENANCE.toml rows whose status != 'sovereign' (derived/dual rows: an
     explicit, ledger-recorded remaining tether).
  2. sovereign rows whose authorship method is unknown. PROVENANCE.toml's
     schema has no field recording HOW a file was written -- this counts
     EVERY sovereign row, because 'unknown' is the honest current value.
     Defaulting them to 'clean' would repeat the ledger's own history:
     krites-provenance-transition.py once hardcoded verbatim_pct=0.0 on every
     dual -> sovereign transition; 17 files entered 'sovereign' that way and
     later re-measured at 18-41%. A fiat value read identically to a
     measurement.
  3. license/provenance artifacts still present under crates/krites/:
     LICENSE-MPL-2.0, the CozoDB/MPL attribution section of NOTICE.md, and
     upstream-snapshot/ -- each one present is one tether, checked by direct
     filesystem read.
  4. CAPABILITY_MATRIX.toml capabilities with no gate_test pointer. The field
     does not exist in the schema yet -- this counts ALL rows, for the same
     reason as line 2: the honest current value, not a default to clean.
  5. OPEN GitHub issues, from a hardcoded tracked set, labelled as
     compromising the provenance mechanism itself rather than a single
     file's measurement: #6656, #6797, #6865, #6866, #6867. LOAD-BEARING:
     this line can only be closed by fixing what each issue names. It cannot
     be closed by narrowing what this script counts, because the set is
     hardcoded here rather than derived from a query this script controls.

Usage:
    python3 scripts/krites-tethers-remaining.py [--json]

Requires `gh` on PATH plus network/API access for line 5. If that query
fails, line 5 reports UNKNOWN (never silently 0) and the script exits 2 --
distinct from exit 1 (a measured nonzero total) and exit 0 (a measured total
of zero).
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from krites_provenance_lib import (  # noqa: E402
    KRITES_DIR,
    LEDGER_PATH,
    NOTICE_PATH,
    REPO_ROOT,
)

CAPABILITY_MATRIX_PATH = KRITES_DIR / "CAPABILITY_MATRIX.toml"
LICENSE_MPL_PATH = KRITES_DIR / "LICENSE-MPL-2.0"
# NOTE: the whole vendored tree, not krites_provenance_lib.UPSTREAM_SNAPSHOT_DIR
# (that constant points one level deeper, at cozo-core-src/, because
# check-krites-provenance.py's recompute check only ever reads Rust source
# under it). The tether this line reports on is the artifact named in the
# task -- "upstream-snapshot/" itself, NOTICE.md included -- not that other
# script's narrower read scope.
UPSTREAM_SNAPSHOT_ROOT = KRITES_DIR / "upstream-snapshot"

GH_REPO = "forkwright/aletheia"

# INVARIANT: this set only ever GROWS by someone naming a new mechanism-level
# issue and widening it, or SHRINKS because GitHub reports an issue CLOSED --
# never by editing this tuple down to make line 5 read lower. The tuple is
# the tracked set; GitHub state is the only thing that moves a member out of
# the open count.
TRACKED_MECHANISM_ISSUES = (6656, 6797, 6865, 6866, 6867)


@dataclass(frozen=True)
class Line:
    label: str
    source: str
    count: int | None  # None == UNKNOWN (measurement failed, never defaulted to 0)
    detail: str


def _load_ledger_rows() -> list[dict]:
    if not LEDGER_PATH.exists():
        raise SystemExit(f"{LEDGER_PATH} is absent; nothing to aggregate against.")
    try:
        data = tomllib.loads(LEDGER_PATH.read_text())
    except tomllib.TOMLDecodeError as exc:
        raise SystemExit(f"{LEDGER_PATH} could not be parsed: {exc}") from exc
    return data.get("file", [])


def _load_capability_rows() -> list[dict]:
    if not CAPABILITY_MATRIX_PATH.exists():
        raise SystemExit(f"{CAPABILITY_MATRIX_PATH} is absent; nothing to aggregate against.")
    try:
        data = tomllib.loads(CAPABILITY_MATRIX_PATH.read_text())
    except tomllib.TOMLDecodeError as exc:
        raise SystemExit(f"{CAPABILITY_MATRIX_PATH} could not be parsed: {exc}") from exc
    return data.get("capability", [])


def line_1_non_sovereign(rows: list[dict]) -> Line:
    non_sovereign = [r for r in rows if r.get("status") != "sovereign"]
    by_status: dict[str, int] = {}
    for row in rows:
        status = row.get("status", "<missing>")
        by_status[status] = by_status.get(status, 0) + 1
    return Line(
        label="1. PROVENANCE.toml rows not yet sovereign",
        source=f"{LEDGER_PATH.relative_to(REPO_ROOT)} ({len(rows)} [[file]] rows total)",
        count=len(non_sovereign),
        detail=f"status breakdown: {by_status}",
    )


def line_2_unknown_authorship(rows: list[dict]) -> Line:
    sovereign = [r for r in rows if r.get("status") == "sovereign"]
    return Line(
        label="2. sovereign rows with unknown authorship method",
        source=(
            f"{LEDGER_PATH.relative_to(REPO_ROOT)} -- counts ALL status=sovereign rows: "
            "the schema {path, upstream_path, replaced_upstream_path, verbatim_pct, "
            "status, soak_expires_at_commit_count} has no field recording HOW a file "
            "was written, so 'unknown' is the honest value for every one, not 0"
        ),
        count=len(sovereign),
        detail=f"{len(sovereign)} of {len(rows)} ledger rows are status=sovereign",
    )


def line_3_license_artifacts() -> Line:
    present: list[str] = []
    absent: list[str] = []

    if LICENSE_MPL_PATH.is_file():
        present.append(str(LICENSE_MPL_PATH.relative_to(REPO_ROOT)))
    else:
        absent.append(str(LICENSE_MPL_PATH.relative_to(REPO_ROOT)))

    # WHY these two substrings: krites_provenance_lib.render_notice emits
    # both unconditionally whenever NOTICE.md carries the CozoDB attribution
    # section check-krites-provenance.py's check_notice_sync keeps in sync
    # with PROVENANCE.toml -- reading the real on-disk file for them is a
    # presence check on that section, not a re-derivation of its content.
    notice_text = NOTICE_PATH.read_text() if NOTICE_PATH.is_file() else ""
    notice_label = f"{NOTICE_PATH.relative_to(REPO_ROOT)} (CozoDB/MPL attribution section)"
    if "CozoDB" in notice_text and "Mozilla Public License 2.0" in notice_text:
        present.append(notice_label)
    else:
        absent.append(notice_label)

    snapshot_files = (
        [p for p in UPSTREAM_SNAPSHOT_ROOT.rglob("*") if p.is_file()]
        if UPSTREAM_SNAPSHOT_ROOT.is_dir()
        else []
    )
    if snapshot_files:
        present.append(f"{UPSTREAM_SNAPSHOT_ROOT.relative_to(REPO_ROOT)}/ ({len(snapshot_files)} files)")
    else:
        absent.append(f"{UPSTREAM_SNAPSHOT_ROOT.relative_to(REPO_ROOT)}/")

    detail = f"present: {present}"
    if absent:
        detail += f"; absent: {absent}"
    return Line(
        label="3. license/provenance artifacts still present",
        source="direct filesystem read of crates/krites/{LICENSE-MPL-2.0,NOTICE.md,upstream-snapshot/}",
        count=len(present),
        detail=detail,
    )


def line_4_no_gate_test(rows: list[dict]) -> Line:
    return Line(
        label="4. CAPABILITY_MATRIX.toml capabilities with no gate_test pointer",
        source=(
            f"{CAPABILITY_MATRIX_PATH.relative_to(REPO_ROOT)} -- counts ALL rows: the "
            "'gate_test' field does not exist in the schema yet, so every row is "
            "unpointed, not 0"
        ),
        count=len(rows),
        detail=f"{len(rows)} [[capability]] rows total",
    )


def _gh_issue_state(number: int) -> str | None:
    """Return 'OPEN'/'CLOSED' for issue `number`, or None if the query could
    not be completed. WARNING: None must never be read as 0 by the caller --
    it means the state is genuinely unknown, not that no such issue exists."""
    try:
        result = subprocess.run(
            ["gh", "issue", "view", str(number), "--repo", GH_REPO, "--json", "state"],
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        print(f"gh issue view {number} --repo {GH_REPO} failed to run: {exc}", file=sys.stderr)
        return None
    if result.returncode != 0:
        print(
            f"gh issue view {number} --repo {GH_REPO} exited {result.returncode}: "
            f"{result.stderr.strip()}",
            file=sys.stderr,
        )
        return None
    try:
        payload = json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        print(f"gh issue view {number} --repo {GH_REPO} returned unparsable JSON: {exc}", file=sys.stderr)
        return None
    return payload.get("state")


def line_5_open_mechanism_issues() -> Line:
    states: dict[int, str | None] = {n: _gh_issue_state(n) for n in TRACKED_MECHANISM_ISSUES}
    failed = sorted(n for n, s in states.items() if s is None)
    source = f"gh issue view <N> --repo {GH_REPO} --json state, for N in {TRACKED_MECHANISM_ISSUES}"
    if failed:
        return Line(
            label="5. OPEN issues compromising the provenance mechanism itself",
            source=source,
            count=None,
            detail=(
                f"gh query failed for issue(s) {failed} -- state UNKNOWN, never counted as "
                "0. This line is load-bearing: a hole in the checker cannot be closed by "
                "narrowing what the checker counts, only by fixing the hole -- an unmeasured "
                "line must not read as clean either."
            ),
        )
    open_issues = sorted(n for n, s in states.items() if s == "OPEN")
    return Line(
        label="5. OPEN issues compromising the provenance mechanism itself",
        source=source,
        count=len(open_issues),
        detail=f"states: {dict(sorted(states.items()))}",
    )


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--json", action="store_true", help="emit the breakdown as JSON")
    args = parser.parse_args()

    ledger_rows = _load_ledger_rows()
    capability_rows = _load_capability_rows()

    lines = [
        line_1_non_sovereign(ledger_rows),
        line_2_unknown_authorship(ledger_rows),
        line_3_license_artifacts(),
        line_4_no_gate_test(capability_rows),
        line_5_open_mechanism_issues(),
    ]

    unknown = [ln for ln in lines if ln.count is None]
    measured_total = sum(ln.count for ln in lines if ln.count is not None)

    if args.json:
        payload = {
            "necessary_not_sufficient": __doc__.strip(),
            "lines": [
                {"label": ln.label, "source": ln.source, "count": ln.count, "detail": ln.detail}
                for ln in lines
            ],
            "total": None if unknown else measured_total,
            "unknown_lines": [ln.label for ln in unknown],
        }
        print(json.dumps(payload, indent=2))
    else:
        print(__doc__.strip())
        print()
        print("=" * 78)
        for ln in lines:
            count_str = "UNKNOWN" if ln.count is None else str(ln.count)
            print(f"{ln.label}: {count_str}")
            print(f"    source: {ln.source}")
            print(f"    {ln.detail}")
            print()
        if unknown:
            print(f"TOTAL: UNKNOWN ({len(unknown)} of {len(lines)} line(s) could not be measured)")
        else:
            print(f"TOTAL: {measured_total}")

    if unknown:
        return 2
    return 0 if measured_total == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
