#!/usr/bin/env python3
"""CI gate: PROVENANCE.toml completeness, NOTICE.md sync, no derived-row growth,
status-sequence, soak expiry, offline verbatim recompute."""

from __future__ import annotations

import argparse
import pathlib
import subprocess
import sys
import tomllib

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from krites_provenance_lib import (  # noqa: E402
    ALLOWED_TRANSITIONS,
    KRITES_SRC,
    LEDGER_PATH,
    NOTICE_PATH,
    REPO_ROOT,
    UPSTREAM_SNAPSHOT_DIR,
    LedgerError,
    iter_src_files,
    parse_ledger,
    render_notice,
    verbatim_pct,
)


def fail(message: str) -> None:
    print(f"::error::krites-provenance: {message}", file=sys.stderr)


class BaseRefError(RuntimeError):
    """The requested base ref cannot be resolved at all — distinct from a
    resolved ref that simply predates the ledger (a genuine bootstrap)."""


def ref_exists(ref: str) -> bool:
    result = subprocess.run(
        ["git", "-C", str(REPO_ROOT), "rev-parse", "--verify", "--quiet", f"{ref}^{{commit}}"],
        capture_output=True,
        text=True,
    )
    return result.returncode == 0


def git_show(ref: str, path: str) -> str | None:
    result = subprocess.run(
        ["git", "-C", str(REPO_ROOT), "show", f"{ref}:{path}"],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        return None
    return result.stdout


def git_commit_count(ref: str) -> int | None:
    result = subprocess.run(
        ["git", "-C", str(REPO_ROOT), "rev-list", "--count", ref],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        return None
    stdout = result.stdout.strip()
    return int(stdout) if stdout.isdigit() else None


def load_base_rows(base_ref: str) -> list[dict] | None:
    """Returns the base ref's ledger rows, or None only when base_ref
    resolves but genuinely has no PROVENANCE.toml yet (the ledger's actual
    first landing).

    SAFETY(P4): fails closed. The prior version treated ANY nonzero `git
    show` exit — including an unresolvable ref, e.g. `--base-ref
    origin/does-not-exist` — as a bootstrap commit and returned [], silently
    passing the growth check. ref_exists() is checked FIRST and separately,
    so an unresolvable ref now raises instead of masquerading as bootstrap.
    """
    if not ref_exists(base_ref):
        raise BaseRefError(
            f"base ref {base_ref!r} does not resolve to a commit — cannot verify the "
            "no-derived-growth or status-sequence invariants against it; refusing to treat an "
            "unresolvable ref as a bootstrap commit (fail closed, not fail open)"
        )
    base_text = git_show(base_ref, "crates/krites/PROVENANCE.toml")
    if base_text is None:
        print(
            f"krites-provenance: {base_ref} resolves but has no PROVENANCE.toml — "
            "skipping growth/sequence checks (bootstrap commit)"
        )
        return None
    _, base_rows = parse_ledger(base_text)
    return base_rows


def check_completeness(rows: list[dict]) -> list[str]:
    ledger_paths = {row["path"] for row in rows}
    src_paths = set(iter_src_files())
    missing = sorted(src_paths - ledger_paths)
    stale = sorted(ledger_paths - src_paths)
    errors = []
    if missing:
        errors.append(
            "files under crates/krites/src/ with no PROVENANCE.toml row: " + ", ".join(missing)
        )
    if stale:
        errors.append(
            "PROVENANCE.toml rows for files that no longer exist: " + ", ".join(stale)
        )
    return errors


def check_notice_sync(meta: dict, rows: list[dict]) -> list[str]:
    expected = render_notice(meta, rows)
    actual = NOTICE_PATH.read_text()
    if expected != actual:
        return ["NOTICE.md is out of sync with PROVENANCE.toml — run scripts/measure-krites-provenance.py or scripts/render-krites-notice.py and commit the result"]
    return []


def check_no_derived_growth(rows: list[dict], base_rows: list[dict] | None) -> list[str]:
    """kanon/projects/aletheia/phases/05g-krites-overhaul/PROVENANCE-LEDGER.md
    "Kill criterion — ledger creep": a row already known to the ledger must
    never regress TO 'derived'. A path with no base-ref row at all is not a
    regression — it is either wave 0's initial population or a completeness
    fix closing an undercount (P3: fts/README.md and gen_stopwords.py sat
    outside the ledger with no row of any status to regress from) — so only
    a base-ref path whose status was something other than 'derived' and is
    now 'derived' counts."""
    if base_rows is None:
        return []
    base_status = {r["path"]: r["status"] for r in base_rows}
    current_derived = {r["path"] for r in rows if r["status"] == "derived"}
    backslid = sorted(
        path
        for path in current_derived
        if path in base_status and base_status[path] != "derived"
    )
    if backslid:
        return [
            "ledger row(s) regressed TO 'derived' relative to the base commit — a file may only "
            "be marked derived by wave 0's initial population, never afterward "
            "(PROVENANCE-LEDGER.md \"Kill criterion — ledger creep\"): " + ", ".join(backslid)
        ]
    return []


def check_status_sequence(rows: list[dict], base_rows: list[dict] | None) -> list[str]:
    """SAFETY(P1): the second half of the anti-backslide fix. The
    sovereign/verbatim_pct cross-check in validate_rows catches a bypass
    that flips status while leaving verbatim_pct as evidence; this check
    catches the sneakier variant that zeroes verbatim_pct too — a direct
    derived -> sovereign jump is illegal independent of what any other
    field says, because the only forcing function for real disuse (the
    dual/soak window) never ran."""
    if base_rows is None:
        return []
    base_status = {r["path"]: r["status"] for r in base_rows}
    errors = []
    for row in rows:
        path = row["path"]
        prior = base_status.get(path)
        if prior is None or prior == row["status"]:
            continue
        if (prior, row["status"]) not in ALLOWED_TRANSITIONS:
            errors.append(
                f"{path}: illegal status transition {prior!r} -> {row['status']!r} — the only "
                "legal path out of 'derived' is derived -> dual -> sovereign "
                "(PROVENANCE-LEDGER.md \"Transitions\"); a direct "
                f"{prior!r} -> {row['status']!r} jump is not permitted in one PR"
            )
    return errors


def check_soak_expiry(rows: list[dict], commit_count: int | None) -> list[str]:
    """kanon/projects/aletheia/phases/05g-krites-overhaul/PROVENANCE-LEDGER.md
    "Soak fuse"'s forcing function: a 'dual' row cannot soak forever by
    neglect. soak_expires_at_commit_count is an ABSOLUTE target — the count
    of commits reachable from origin/main (see krites_provenance_lib.py's
    ledger header NOTE for why origin/main, not HEAD: on a PR, HEAD includes
    the PR's own unmerged commits, which have not landed on main and would
    over-count the window)."""
    dual_rows = [r for r in rows if r["status"] == "dual"]
    if not dual_rows:
        return []
    if commit_count is None:
        return [
            "could not determine the current commit count on main (git rev-list --count "
            "origin/main failed) — cannot evaluate soak expiry for dual row(s): "
            + ", ".join(r["path"] for r in dual_rows)
        ]
    errors = []
    for row in dual_rows:
        expiry = row["soak_expires_at_commit_count"]
        if expiry <= 0:
            errors.append(
                f"{row['path']}: status=dual requires a positive soak_expires_at_commit_count "
                f"(0 means 'not in dual' per the ledger header note); got {expiry}"
            )
        elif commit_count >= expiry:
            errors.append(
                f"{row['path']}: dual soak window expired — current commit count on main "
                f"({commit_count}) has reached soak_expires_at_commit_count ({expiry}); flip to "
                "sovereign or delete the module (PROVENANCE-LEDGER.md \"Soak fuse\"), or extend "
                "the window with an explicit, reviewable ledger edit"
            )
    return errors


def check_verbatim_recompute(rows: list[dict]) -> list[str]:
    """P6: when the offline upstream snapshot (crates/krites/upstream-snapshot/
    cozo-core-src/, vendored by wave0/drift-metric) is present, recompute
    every derived row's verbatim_pct from it and fail if the stored ledger
    value has drifted — the check that makes the published numbers
    self-verifying instead of trusted-forever. Skips (does not fail) when
    the snapshot is absent so this branch has no landing-order dependency
    on wave0/drift-metric.

    WHY dual is included: a 'dual' row's file is still, physically, the
    unmodified CozoDB-lineage copy soaking before deletion
    (kanon/projects/aletheia/phases/05g-krites-overhaul/PROVENANCE-LEDGER.md
    "Statuses") — it carries a real upstream_path the same as a 'derived'
    row, and drifting silently during the soak window is exactly the
    failure this check exists to catch. Only 'sovereign' rows
    (upstream_path == 'none', nothing to recompute against) are exempt."""
    if not UPSTREAM_SNAPSHOT_DIR.is_dir():
        print("krites-provenance: no upstream-snapshot/ present — skipping offline verbatim_pct recompute")
        return []
    errors = []
    for row in rows:
        if row["status"] not in ("derived", "dual"):
            continue
        snapshot_path = UPSTREAM_SNAPSHOT_DIR / row["upstream_path"]
        if not snapshot_path.is_file():
            errors.append(
                f"{row['path']}: upstream-snapshot/ is present but has no {row['upstream_path']} "
                "— snapshot is incomplete relative to PROVENANCE.toml"
            )
            continue
        local_text = (KRITES_SRC / row["path"]).read_text(errors="replace")
        upstream_text = snapshot_path.read_text(errors="replace")
        recomputed = verbatim_pct(local_text, upstream_text)
        if recomputed != row["verbatim_pct"]:
            errors.append(
                f"{row['path']}: stored verbatim_pct {row['verbatim_pct']} does not match offline "
                f"recomputation {recomputed} against upstream-snapshot/ — run "
                "scripts/measure-krites-provenance.py and commit the result"
            )
    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-ref", default="origin/main")
    parser.add_argument("--main-ref", default="origin/main")
    args = parser.parse_args()

    if not LEDGER_PATH.exists():
        fail(f"missing {LEDGER_PATH}")
        return 1

    try:
        meta, rows = parse_ledger(LEDGER_PATH.read_text())
    except (tomllib.TOMLDecodeError, LedgerError) as exc:
        fail(f"could not parse {LEDGER_PATH}: {exc}")
        return 1

    try:
        base_rows = load_base_rows(args.base_ref)
    except (BaseRefError, tomllib.TOMLDecodeError, LedgerError) as exc:
        fail(str(exc))
        return 1

    errors: list[str] = []
    errors += check_completeness(rows)
    errors += check_notice_sync(meta, rows)
    errors += check_no_derived_growth(rows, base_rows)
    errors += check_status_sequence(rows, base_rows)
    errors += check_soak_expiry(rows, git_commit_count(args.main_ref))
    errors += check_verbatim_recompute(rows)

    if errors:
        for err in errors:
            fail(err)
        return 1

    print(f"krites-provenance: clean ({len(rows)} rows)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
