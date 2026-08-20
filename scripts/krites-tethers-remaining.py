#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Aggregator of remaining krites -> sovereign CozoDB tethers.

Prints a per-line breakdown of five tether classes and one total. Every
number this script prints is READ from an existing artifact (PROVENANCE.toml,
CAPABILITY_MATRIX.toml, the crate's license/provenance files under
crates/krites/) or a direct filesystem/API presence check. It computes
nothing that check-krites-provenance.py, check-krites-capability-matrix.py,
or check-krites-verbatim-drift.py do not already own, and it must never
become a second source of truth for any of those numbers.

INVARIANT: an aggregator over records is only as honest as the records are
hard to edit. Every count below either derives directly from the real
source tree (crates/krites/src/), or is cross-validated against it before
being trusted, and the aggregator REFUSES to print a total at all --
distinct from printing a false one -- when a precondition that number
depends on does not hold. A row deleted from a ledger, a file rm'd, an
issue closed as wontfix, or a tracked-set tuple quietly shrunk are each
individually a bare edit to a record; none of them is real progress on the
rewrite, and none of them is allowed to read as one.

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
     explicit, ledger-recorded remaining tether). Checked FIRST against
     iter_src_files() (krites_provenance_lib's own completeness boundary,
     the same one check-krites-provenance.py's check_completeness uses): the
     ledger's row set must exactly match the real crates/krites/src/ file
     set, or this script refuses to count against it at all. A row deleted
     from the ledger is not a file that stopped existing -- it is a source
     file the ledger no longer accounts for, caught as MISSING before either
     line 1 or line 2 sums anything.
  2. sovereign rows whose authorship method is unknown. PROVENANCE.toml's
     schema has no field recording HOW a file was written -- this counts
     EVERY sovereign row, because 'unknown' is the honest current value.
     Defaulting them to 'clean' would repeat the ledger's own history:
     krites-provenance-transition.py once hardcoded verbatim_pct=0.0 on every
     dual -> sovereign transition; 17 files entered 'sovereign' that way and
     later re-measured at 18-41%. A fiat value read identically to a
     measurement. Lines 1 and 2 partition the SAME row set
     (non-sovereign/sovereign) -- a row leaving line 1 always enters line 2,
     so hand-flipping a single row's status is net-zero on the total by
     construction, not by a check that could be evaded.
  3. license/provenance artifacts still present under crates/krites/:
     LICENSE-MPL-2.0 and the CozoDB/MPL attribution section of NOTICE.md --
     each one present is one tether, checked by direct filesystem read
     against the SAME rows line 1 counts. An artifact reported absent while
     any row is still non-sovereign is not a lower count; under MPL section
     3.1 it is a violation (MPL-covered code on disk with its attribution
     notice removed), and this script refuses to report it as one.
     NOTICE.md's attribution is verified by an EXACT match against
     krites_provenance_lib.render_notice(meta, rows) -- the same
     ledger-derived render check-krites-provenance.py's check_notice_sync
     keeps in sync -- not a substring probe a reword can defeat while
     leaving NOTICE.md out of sync with the ledger. upstream-snapshot/'s
     presence is reported here for completeness, but its removal is caught
     upstream by check-krites-provenance.py's check_verbatim_recompute
     (which fails outright, unconditionally, when the snapshot is absent):
     main() runs that check before loading anything below, so this script
     never reaches a total without it having already passed.
  4. CAPABILITY_MATRIX.toml capabilities with no gate_test pointer -- a row
     with no recorded runnable-test candidate. `gate_test`
     names the candidate as a `<binary-id>::<test path>` id; this no-cargo
     inventory can validate only that shape. The required hosted gate
     separately resolves every recorded pointer against compiler-derived,
     filter-matching `cargo nextest list` output and executes that same test
     selection.
     `"none"` and an absent field both count as unpointed: the honest state
     of a capability with no recorded candidate. Checked first against
     the matrix's OWN source-derived categories -- reusing
     check-krites-capability-matrix.py's extract_sysop_variants /
     extract_datavalue_variants / extract_lib_public_api /
     extract_fixed_rule_names / extract_storage_methods / check_category /
     check_appendix_a / check_capability_sets, not a second reimplementation
     of that mapping -- so a row deleted to shrink this count is caught the
     same way a deleted ledger row is caught in line 1.

     WARNING: resolution by the required hosted nextest check proves only that
     the named test exists, is runnable, and belongs to the listed world that
     the job executes. It does NOT mechanically couple the test to this row's
     capability or prove the `gate` sentence is asserted. Reaching 0 on this
     line means every capability records a runnable-test candidate, not that
     any capability has disappearance detection or semantic verification.
  5. GitHub issues, from a tracked set, labelled as compromising the
     provenance mechanism itself rather than a single file's measurement.
     No GitHub label captures exactly this set (checked live, every run --
     see TRACKED_MECHANISM_ISSUES below for what was found and why this
     line's cardinality stays reviewer-enforced beyond what is checked). A
     CLOSED issue counts as resolved only when GitHub's stateReason is
     COMPLETED and GitHub links the closing PR. A NOT_PLANNED ("wontfix") or
     bare manual close reads identically to still OPEN, because a close bit is
     not proof of the repair this line claims. The closing reference is
     printed for every tracked issue so a reviewer can inspect what closed it.

Usage:
    python3 scripts/krites-tethers-remaining.py [--json]

Requires `gh` on PATH plus network/API access for line 5, and a local git
history that reaches TRACKED_ISSUES_ANCHOR_COMMIT (see below). Exit codes:
  0  every precondition held and the measured total is 0
  1  every precondition held and the measured total is nonzero
  2  a precondition held but a live measurement (line 5's gh query) failed
     -- reported as UNKNOWN, never silently 0
  3  a precondition this total depends on did NOT hold, so no total -- even
     UNKNOWN -- is printed at all (ledger/matrix completeness, NOTICE/
     LICENSE drift, a sibling CI gate failing, or a tracked-issue removal
     this script could not independently verify as a real GitHub close)
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import re
import subprocess
import sys
import tomllib
from dataclasses import dataclass, field
from pathlib import Path
from types import ModuleType
from typing import NoReturn

sys.path.insert(0, str(Path(__file__).resolve().parent))

from krites_provenance_lib import (
    KRITES_DIR,
    LEDGER_PATH,
    NOTICE_PATH,
    REPO_ROOT,
    LedgerError,
    iter_src_files,
    parse_ledger,
    render_notice,
)

CAPABILITY_MATRIX_PATH = KRITES_DIR / "CAPABILITY_MATRIX.toml"
LICENSE_MPL_PATH = KRITES_DIR / "LICENSE-MPL-2.0"
UPSTREAM_SNAPSHOT_ROOT = KRITES_DIR / "upstream-snapshot"

GH_REPO = "forkwright/aletheia"

SCRIPT_DIR = Path(__file__).resolve().parent
SIBLING_PROVENANCE_CHECK = SCRIPT_DIR / "check-krites-provenance.py"
SIBLING_CAPABILITY_CHECK = SCRIPT_DIR / "check-krites-capability-matrix.py"


def refuse(message: str) -> NoReturn:
    # WHY exit 3, distinct from 1 (nonzero measured total) and 2 (a live
    # measurement failed): this is neither. It means a precondition the
    # total depends on did not hold, so no total -- not even UNKNOWN -- is
    # printed. The aggregator must never substitute a partial or
    # lower-than-real number here.
    #
    # WARNING: defined before _load_sibling_module (below), which can call
    # it at MODULE-LOAD time -- refuse() must already exist in the module
    # namespace before that call runs, not merely before main().
    print(f"REFUSED: {message}", file=sys.stderr)
    raise SystemExit(3)


def _load_sibling_module(path: Path, name: str) -> ModuleType:
    # WHY: importlib rather than a plain `import`, because the sibling's
    # filename carries hyphens and is not a valid module name. This is a
    # REUSE of that script's own row/source-derived-category logic (line 4's
    # requirement), not a second implementation of the same mapping.
    #
    # WARNING: spec_from_file_location does not check the path exists --
    # spec/spec.loader come back non-None for a missing file too, so the
    # real failure (FileNotFoundError, SyntaxError, ...) only surfaces
    # inside exec_module. Both stages are guarded here so a missing or
    # broken sibling script fails as a clean refuse(), not a raw traceback.
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        refuse(f"could not load {path} for reuse -- its checks cannot be applied")
    module = importlib.util.module_from_spec(spec)
    try:
        spec.loader.exec_module(module)
    # WHY the blind catch is correct here rather than narrowed: exec_module runs
    # arbitrary module-level code from a sibling script, which can raise any
    # exception type. Narrowing would let an unanticipated one escape as a raw
    # traceback, which is the failure this guard exists to prevent -- a checker
    # that dies untidily reads as broken tooling rather than as a refusal.
    except Exception as exc:  # noqa: BLE001
        refuse(
            f"could not load {path} for reuse -- its checks cannot be applied: {exc}"
        )
    return module


_capmatrix = _load_sibling_module(
    SIBLING_CAPABILITY_CHECK, "krites_capability_matrix_check"
)

# INVARIANT: the only legal way a member leaves this tuple is a live GitHub
# query confirming it CLOSED with stateReason COMPLETED (checked below,
# every run, against TRACKED_ISSUES_ANCHOR_COMMIT's original membership) --
# never by editing the tuple down to make line 5 read lower. Growing it (a
# new mechanism-level issue named and added) is always legitimate and
# unchecked.
#
# WHY not a GitHub label query (line 5's first design choice): checked live
# against this exact set -- the repo's 'krites' label is neither necessary
# (aletheia#6797, a tracked member here, carries no 'krites' label) nor
# sufficient (the label also spans 40+ issues with no relation to the
# provenance mechanism, including several multi-wave epics) to stand in for
# this set. No other label or query in this repo distinguishes
# mechanism-compromising issues from ordinary krites bugs or cleanup, so
# there is no live derivation to fall back to -- this line's cardinality
# stays REVIEWER-ENFORCED beyond the anchor-commit removal check below. That
# check catches every entry removed from the ORIGINAL set without a real
# GitHub close; it does not catch a shrink that never happened here (a fresh
# clone of this file that starts smaller) and cannot verify that every
# CURRENT member is a genuine mechanism-level issue rather than padding --
# only a human reading the diff can.
TRACKED_MECHANISM_ISSUES = (6656, 6797, 6865, 6866, 6867)

# WHY this exact commit, not a branch ref or HEAD~N: a branch ref moves
# under rebase/force-push and HEAD~N shifts as this branch gains the very
# commits that fix the attacks below. This is the commit that introduced
# TRACKED_MECHANISM_ISSUES, already pushed to origin before this hardening
# pass -- an immutable git object, not this file's mutable working-tree
# content. Rewriting it requires a force-push to a remote ref: a bigger,
# monitored action, not a quiet content edit.
TRACKED_ISSUES_ANCHOR_COMMIT = "847ef4bf42ff613d7989781702ba879b1e4a4152"


@dataclass(frozen=True)
class Line:
    label: str
    source: str
    count: int | None  # None == UNKNOWN (measurement failed, never defaulted to 0)
    detail: str


@dataclass(frozen=True)
class IssueStatus:
    state: str | None
    state_reason: str | None
    labels: tuple[str, ...] = field(default_factory=tuple)
    closing_refs: tuple[str, ...] = field(default_factory=tuple)


def _run_sibling_validator(script: Path) -> None:
    try:
        result = subprocess.run(
            [sys.executable, str(script)],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            timeout=180,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        refuse(f"{script.name} could not be run: {exc}")
        return
    if result.returncode != 0:
        refuse(
            f"{script.name} failed (exit {result.returncode}) -- this aggregator's total is only "
            "meaningful once the artifacts it counts against have independently passed their own CI "
            f"gate. {script.name}'s output:\n{result.stdout}{result.stderr}"
        )


def _load_ledger() -> tuple[dict, list[dict]]:
    if not LEDGER_PATH.exists():
        refuse(f"{LEDGER_PATH} is absent; nothing to aggregate against.")
    try:
        return parse_ledger(LEDGER_PATH.read_text())
    except (tomllib.TOMLDecodeError, LedgerError) as exc:
        refuse(f"{LEDGER_PATH} could not be parsed/validated: {exc}")
    raise AssertionError("unreachable: refuse() always raises")


def _load_capability_rows() -> list[dict]:
    if not CAPABILITY_MATRIX_PATH.exists():
        refuse(f"{CAPABILITY_MATRIX_PATH} is absent; nothing to aggregate against.")
    try:
        return _capmatrix.load_matrix()
    except tomllib.TOMLDecodeError as exc:
        refuse(f"{CAPABILITY_MATRIX_PATH} could not be parsed: {exc}")
    raise AssertionError("unreachable: refuse() always raises")


def _validate_ledger_completeness(rows: list[dict]) -> None:
    ledger_paths = {r.get("path") for r in rows}
    src_paths = set(iter_src_files())
    missing = sorted(src_paths - ledger_paths)
    extra = sorted(p for p in ledger_paths - src_paths if p is not None)
    if missing or extra:
        detail = [
            (
                f"{LEDGER_PATH} does not exactly match crates/krites/src/ "
                "(iter_src_files(), the same completeness boundary "
                "check-krites-provenance.py's check_completeness uses) -- refusing to count "
                "lines 1/2 against a ledger that cannot be trusted to enumerate the real tree."
            )
        ]
        if missing:
            detail.append(f"  source files with no ledger row: {missing}")
        if extra:
            detail.append(f"  ledger rows with no matching source file: {extra}")
        refuse("\n".join(detail))


def _validate_capability_matrix_completeness(rows: list[dict]) -> None:
    errors: list[str] = []
    errors += _capmatrix.check_all_rows_well_formed(rows)
    errors += _capmatrix.check_category(
        "sysop", _capmatrix.extract_sysop_variants(), rows, "parse/sys/mod.rs"
    )
    errors += _capmatrix.check_category(
        "datavalue", _capmatrix.extract_datavalue_variants(), rows, "data/value.rs"
    )
    errors += _capmatrix.check_category(
        "public_api",
        _capmatrix.extract_lib_public_api(),
        rows,
        "lib.rs",
        allowed_bundles=_capmatrix.PUBLIC_API_SOURCE_BUNDLES,
    )
    errors += _capmatrix.check_category(
        "fixed_rule", _capmatrix.extract_fixed_rule_names(), rows, "fixed_rule/mod.rs"
    )
    errors += _capmatrix.check_category(
        "storage_method", _capmatrix.extract_storage_methods(), rows, "storage/mod.rs"
    )
    errors += _capmatrix.check_appendix_a(rows)
    errors += _capmatrix.check_capability_sets(_capmatrix.load_capability_sets())
    if errors:
        refuse(
            f"{CAPABILITY_MATRIX_PATH} does not validate against its own source-derived categories "
            "(reusing check-krites-capability-matrix.py's own checks) -- refusing to count line 4 "
            "against a matrix that cannot be trusted to enumerate real capabilities:\n"
            + "\n".join(f"  - {e}" for e in errors)
        )


def line_1_non_sovereign(rows: list[dict]) -> Line:
    non_sovereign = [r for r in rows if r.get("status") != "sovereign"]
    by_status: dict[str, int] = {}
    for row in rows:
        status = row.get("status", "<missing>")
        by_status[status] = by_status.get(status, 0) + 1
    return Line(
        label="1. PROVENANCE.toml rows not yet sovereign",
        source=(
            f"{LEDGER_PATH.relative_to(REPO_ROOT)} ({len(rows)} [[file]] rows total, "
            "row set verified to exactly match crates/krites/src/ before this line is computed)"
        ),
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


def line_3_license_artifacts(meta: dict, rows: list[dict]) -> Line:
    # SAFETY: must match line 1's partition exactly (status != 'sovereign')
    # -- this is the same condition MPL section 3.1 obligation is checked
    # against below, and the two must never drift apart.
    non_sovereign = [r for r in rows if r.get("status") != "sovereign"]
    tethered_exists = len(non_sovereign) > 0

    present: list[str] = []
    absent: list[str] = []

    license_label = str(LICENSE_MPL_PATH.relative_to(REPO_ROOT))
    if LICENSE_MPL_PATH.is_file():
        present.append(license_label)
    elif tethered_exists:
        refuse(
            f"{LICENSE_MPL_PATH} is absent but {len(non_sovereign)} PROVENANCE.toml row(s) are "
            "still non-sovereign -- MPL-derived code remains under crates/krites/src/. Removing the "
            "license file while MPL-covered code remains on disk is an MPL section 3.1 violation, "
            "not a reduction in tethers; this line refuses to report it as one."
        )
    else:
        absent.append(license_label)

    notice_label = (
        f"{NOTICE_PATH.relative_to(REPO_ROOT)} (CozoDB/MPL attribution section)"
    )
    expected_notice = render_notice(meta, rows)
    # WHY an absent NOTICE.md is its own case rather than folded into the drift
    # comparison below: deleting the file and hand-editing it to drop the
    # attribution are different acts with the same consequence, and a reader
    # given "out of sync with the ledger" for a file that does not exist is
    # sent to diff something that is not there. Separating them also removes a
    # nullable from the comparison, so `expected_notice` cannot be narrowed to
    # a possible None by the equality check that follows.
    if not NOTICE_PATH.is_file():
        refuse(
            f"{NOTICE_PATH} does not exist. It is generated from the ledger and is the artifact "
            "recording which files carry CozoDB lineage; its absence is not evidence the "
            "attribution obligation ended. Run scripts/measure-krites-provenance.py to regenerate."
        )
    actual_notice = NOTICE_PATH.read_text()
    if actual_notice != expected_notice:
        # WHY refuse rather than count as absent: an edit that drifts
        # NOTICE.md from what the ledger renders -- including one that
        # happens to remove the CozoDB/MPL wording -- is drift, the same
        # condition check-krites-provenance.py's check_notice_sync already
        # fails the build on. A hand-edit is not evidence the attribution
        # obligation is gone; only a ledger-derived render is.
        refuse(
            f"{NOTICE_PATH} does not exactly match krites_provenance_lib.render_notice(meta, rows) "
            "-- out of sync with the ledger. Run scripts/measure-krites-provenance.py or "
            "scripts/render-krites-notice.py and commit the result, or revert the hand-edit; this "
            "line will not read a desynced NOTICE.md as evidence of anything."
        )
    if "CozoDB" in expected_notice and "Mozilla Public License 2.0" in expected_notice:
        present.append(notice_label)
    elif tethered_exists:
        refuse(
            f"{NOTICE_PATH} (in sync with the ledger) no longer carries the CozoDB/MPL attribution "
            f"while {len(non_sovereign)} PROVENANCE.toml row(s) are still non-sovereign -- refusing "
            "to report the attribution absent while the obligation it covers still exists."
        )
    else:
        absent.append(notice_label)

    # NOTE: upstream-snapshot/'s own removal is caught upstream, unconditionally,
    # by check-krites-provenance.py's check_verbatim_recompute (main() never
    # reaches this line without that check having already passed) -- reported
    # here for completeness, not re-verified against tethered_exists.
    snapshot_files = (
        [p for p in UPSTREAM_SNAPSHOT_ROOT.rglob("*") if p.is_file()]
        if UPSTREAM_SNAPSHOT_ROOT.is_dir()
        else []
    )
    if snapshot_files:
        present.append(
            f"{UPSTREAM_SNAPSHOT_ROOT.relative_to(REPO_ROOT)}/ ({len(snapshot_files)} files)"
        )
    else:
        absent.append(f"{UPSTREAM_SNAPSHOT_ROOT.relative_to(REPO_ROOT)}/")

    detail = f"present: {present}"
    if absent:
        detail += f"; absent: {absent}"
    return Line(
        label="3. license/provenance artifacts still present",
        source=(
            "direct filesystem read of crates/krites/{LICENSE-MPL-2.0,NOTICE.md,upstream-snapshot/}, "
            "cross-checked against PROVENANCE.toml's non-sovereign rows and against "
            "krites_provenance_lib.render_notice()"
        ),
        count=len(present),
        detail=detail,
    )


def _is_unpointed(row: dict) -> bool:
    # SAFETY: must match check-krites-capability-matrix.py's GATE_TEST_UNPOINTED
    # exactly. A row this file counts as pointed while the checker treats it as
    # unpointed would let the total drop without the pointer ever being resolved.
    value = row.get("gate_test")
    return not isinstance(value, str) or value.strip().lower() in {"", "none"}


def line_4_no_gate_test(rows: list[dict]) -> Line:
    unpointed = [r for r in rows if _is_unpointed(r)]
    have_gate_test = len(rows) - len(unpointed)
    by_category: dict[str, int] = {}
    for row in unpointed:
        category = row.get("category", "<missing>")
        by_category[category] = by_category.get(category, 0) + 1
    return Line(
        label="4. CAPABILITY_MATRIX.toml capabilities with no gate_test pointer",
        source=(
            f"{CAPABILITY_MATRIX_PATH.relative_to(REPO_ROOT)} -- per-row filter on a "
            "syntactically validated 'gate_test' field, with the row set verified against "
            "its own source-derived categories. Existence and ignored state are intentionally "
            "outside this no-cargo count and belong to the required hosted nextest check"
        ),
        count=len(unpointed),
        detail=(
            f"{have_gate_test} of {len(rows)} [[capability]] rows record a well-shaped "
            f"'gate_test' candidate; {len(unpointed)} do not, by category: {by_category}. "
            "This line measures recorded candidates only; the hosted gate owns resolution and "
            "execution, and neither count proves the row's gate sentence is asserted"
        ),
    )


def _gh_issue_status(number: int) -> IssueStatus | None:
    """Return this issue's live state/stateReason/labels/closing references,
    or None if the query could not be completed. WARNING: None must never be
    read as 0 or as resolved by the caller -- it means the state is
    genuinely unknown."""
    try:
        result = subprocess.run(
            [
                "gh",
                "issue",
                "view",
                str(number),
                "--repo",
                GH_REPO,
                "--json",
                "state,stateReason,labels,closedByPullRequestsReferences",
            ],
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        print(
            f"gh issue view {number} --repo {GH_REPO} failed to run: {exc}",
            file=sys.stderr,
        )
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
        print(
            f"gh issue view {number} --repo {GH_REPO} returned unparsable JSON: {exc}",
            file=sys.stderr,
        )
        return None
    labels = tuple(
        sorted(entry.get("name", "") for entry in payload.get("labels") or [])
    )
    # NOTE: gh's closedByPullRequestsReferences nests {name, owner: {login}},
    # not a flattened nameWithOwner -- url is already the fully-qualified,
    # always-correct form and needs no reassembly.
    refs = tuple(
        ref.get("url", f"#{ref.get('number')}")
        for ref in payload.get("closedByPullRequestsReferences") or []
    )
    return IssueStatus(
        state=payload.get("state"),
        state_reason=payload.get("stateReason") or None,
        labels=labels,
        closing_refs=refs,
    )


def _issue_disposition(status: IssueStatus) -> str:
    # WHY a not_planned close is 'unresolved': GitHub's bare CLOSED bit
    # conflates "fixed" with "declined to fix". A wontfix close is exactly
    # the cheapest form of the attack this line exists to catch -- closing
    # a tracked issue without fixing what it names -- and must read
    # identically to still OPEN.
    if status.state != "CLOSED":
        return "unresolved"
    if (status.state_reason or "").upper() != "COMPLETED":
        return "unresolved"
    # A state bit is still the maker grading its own completion.  Require the
    # tracker-owned join to the PR that closed the issue so a bare manual close
    # cannot lower this load-bearing count.  A non-PR completion needs a
    # separately typed proof mechanism before it can count here.
    if not status.closing_refs:
        return "unresolved"
    return "resolved"


def _original_tracked_issue_numbers() -> frozenset[int]:
    try:
        result = subprocess.run(
            [
                "git",
                "-C",
                str(REPO_ROOT),
                "show",
                f"{TRACKED_ISSUES_ANCHOR_COMMIT}:scripts/krites-tethers-remaining.py",
            ],
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        refuse(
            f"could not read the anchor commit {TRACKED_ISSUES_ANCHOR_COMMIT}: {exc}"
        )
        # NOTE: refuse() exits, so this is unreachable and exists only so the
        # type checker sees the branch terminate. Chained from `exc` so that if
        # refuse() is ever changed to return, the original cause survives.
        raise AssertionError("unreachable") from exc
    if result.returncode != 0:
        refuse(
            f"git show {TRACKED_ISSUES_ANCHOR_COMMIT}:scripts/krites-tethers-remaining.py failed -- "
            "this is the immutable floor TRACKED_MECHANISM_ISSUES is checked against; a missing or "
            "unreachable anchor commit means the anti-shrink check cannot run at all, which must fail "
            "loudly rather than silently apply no floor"
        )
    match = re.search(r"TRACKED_MECHANISM_ISSUES\s*=\s*\(([^)]*)\)", result.stdout)
    if not match:
        refuse(
            f"could not locate TRACKED_MECHANISM_ISSUES in the anchor commit "
            f"{TRACKED_ISSUES_ANCHOR_COMMIT}'s copy of this script"
        )
        raise AssertionError("unreachable")
    return frozenset(int(tok) for tok in match.group(1).split(",") if tok.strip())


def _validate_tracked_issue_removals() -> list[int]:
    """Every issue number present in TRACKED_ISSUES_ANCHOR_COMMIT's tuple but
    absent from TRACKED_MECHANISM_ISSUES today must be independently
    verified, via a live gh query, to be CLOSED with stateReason COMPLETED and
    joined to a closing PR -- the only typed proof currently admitted for a
    member leaving this set. Returns
    the numbers whose gh query failed (contributes to line 5's UNKNOWN
    state, same as any other gh outage); refuses outright the moment a
    removed member is confirmed NOT legitimately closed, since that is a
    positively-confirmed tamper, not an unmeasured state."""
    original = _original_tracked_issue_numbers()
    current = set(TRACKED_MECHANISM_ISSUES)
    removed = sorted(original - current)
    failed: list[int] = []
    for number in removed:
        status = _gh_issue_status(number)
        if status is None:
            failed.append(number)
            continue
        if _issue_disposition(status) != "resolved":
            refuse(
                f"#{number} was removed from TRACKED_MECHANISM_ISSUES but GitHub reports "
                f"state={status.state!r} stateReason={status.state_reason!r} "
                f"closing_refs={status.closing_refs!r} -- a member may only leave this "
                "tuple once CLOSED as COMPLETED with a closing PR, never by editing the "
                "tuple down to make line 5 read lower"
            )
    return failed


def line_5_open_mechanism_issues() -> Line:
    removal_query_failed = _validate_tracked_issue_removals()

    statuses: dict[int, IssueStatus | None] = {
        n: _gh_issue_status(n) for n in TRACKED_MECHANISM_ISSUES
    }
    failed = sorted(removal_query_failed)
    failed += sorted(n for n, s in statuses.items() if s is None)
    source = (
        f"gh issue view <N> --repo {GH_REPO} --json state,stateReason,labels,"
        f"closedByPullRequestsReferences, for N in {TRACKED_MECHANISM_ISSUES}; every number present "
        f"in commit {TRACKED_ISSUES_ANCHOR_COMMIT[:12]}'s tuple but absent from it today is verified "
        "CLOSED+COMPLETED with a closing PR before this line runs"
    )
    if failed:
        return Line(
            label="5. issues compromising the provenance mechanism itself, not yet resolved",
            source=source,
            count=None,
            detail=(
                f"gh query failed for issue(s) {sorted(set(failed))} -- state UNKNOWN, never counted "
                "as 0. This line is load-bearing: a hole in the checker cannot be closed by narrowing "
                "what the checker counts, only by fixing the hole -- an unmeasured line must not read "
                "as clean either."
            ),
        )

    unresolved = sorted(
        n
        for n, s in statuses.items()
        if s is not None and _issue_disposition(s) != "resolved"
    )
    no_krites_label = sorted(
        n for n, s in statuses.items() if s is not None and "krites" not in s.labels
    )
    per_issue = {
        n: {
            "state": s.state,
            "state_reason": s.state_reason,
            "closing_refs": list(s.closing_refs),
        }
        for n, s in statuses.items()
        if s is not None
    }
    detail = (
        f"per-issue state: {per_issue}. "
        + (
            "every tracked member carries the 'krites' label"
            if not no_krites_label
            else f"issue(s) {no_krites_label} carry no 'krites' label"
        )
        + " -- live evidence the label is not a suitable substitute for this tuple "
        "(see TRACKED_MECHANISM_ISSUES's module-level comment). Beyond the anchor-commit removal "
        "check, this line's membership stays reviewer-enforced."
    )
    return Line(
        label="5. issues compromising the provenance mechanism itself, not yet resolved",
        source=source,
        count=len(unresolved),
        detail=detail,
    )


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument(
        "--json", action="store_true", help="emit the breakdown as JSON"
    )
    args = parser.parse_args()

    # INVARIANT: no total is emitted unless the sibling validators that
    # actually check these artifacts against source have passed. Run first,
    # before anything below is even loaded.
    _run_sibling_validator(SIBLING_PROVENANCE_CHECK)
    _run_sibling_validator(SIBLING_CAPABILITY_CHECK)

    meta, ledger_rows = _load_ledger()
    capability_rows = _load_capability_rows()

    _validate_ledger_completeness(ledger_rows)
    _validate_capability_matrix_completeness(capability_rows)

    lines = [
        line_1_non_sovereign(ledger_rows),
        line_2_unknown_authorship(ledger_rows),
        line_3_license_artifacts(meta, ledger_rows),
        line_4_no_gate_test(capability_rows),
        line_5_open_mechanism_issues(),
    ]

    unknown = [ln for ln in lines if ln.count is None]
    measured_total = sum(ln.count for ln in lines if ln.count is not None)

    if args.json:
        payload = {
            "necessary_not_sufficient": __doc__.strip(),
            "lines": [
                {
                    "label": ln.label,
                    "source": ln.source,
                    "count": ln.count,
                    "detail": ln.detail,
                }
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
            print(
                f"TOTAL: UNKNOWN ({len(unknown)} of {len(lines)} line(s) could not be measured)"
            )
        else:
            print(f"TOTAL: {measured_total}")

    if unknown:
        return 2
    return 0 if measured_total == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
