#!/usr/bin/env python3
"""Apply a PLAN.md §2 status transition (derived -> dual, or dual ->
sovereign) to named PROVENANCE.toml rows, and re-render NOTICE.md.

WHY this exists as a script rather than a hand-edit: PROVENANCE.toml's own
header says "do not hand-edit rows" — that norm exists to stop the ledger
drifting from measured reality (verbatim_pct, upstream_path). A status
transition is a different kind of edit: a deliberate lifecycle decision, not
a measurement. Routing it through parse_ledger/dump_ledger keeps it
type-checked (illegal status values, duplicate rows, the sovereign/
verbatim_pct cross-check) instead of a raw TOML text edit that could silently
corrupt an unrelated field. This is infrastructure every wave's land-dark PR
needs (PLAN.md §2's three-PR discipline), not specific to any one wave —
scripts/measure-krites-provenance.py regenerates verbatim_pct/upstream_path
but never asserts a status past 'derived' or 'sovereign' on its own, by
design (see its load_graduated_status()).

--to sovereign does not zero the row's measurement (aletheia#6656: it used
to, which erased the upstream_path link and replaced a real verbatim_pct with
an unmeasured 0.0 — a "sovereign" transition that certified nothing rather
than recording that the file was checked). See apply_to_sovereign() below:
the dual-era upstream_path is retained as replaced_upstream_path and
verbatim_pct is recomputed fresh against it, so the row keeps proving the
independence claim instead of being measured once, at 'dual' entry, and
trusted forever after.

Usage:
    python3 scripts/krites-provenance-transition.py --to dual \\
        --soak-commits 300 \\
        fts/tokenizer/stop_word_filter/derived/mod.rs \\
        fts/tokenizer/stop_word_filter/derived/gen_stopwords.py \\
        ...

    python3 scripts/krites-provenance-transition.py --to sovereign \\
        fts/tokenizer/stop_word_filter/derived/mod.rs \\
        ...

--soak-commits is required for --to dual (added to the current
`git rev-list --count origin/main` to produce an absolute
soak_expires_at_commit_count, per the ledger header's own note that the field
is absolute, not relative). It is rejected for --to sovereign, where the
correct value is always 0 (soak is over).
"""

from __future__ import annotations

import argparse
import pathlib
import subprocess
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from krites_provenance_lib import (  # noqa: E402
    ALLOWED_TRANSITIONS,
    KRITES_SRC,
    LEDGER_PATH,
    NOTICE_PATH,
    REPO_ROOT,
    UPSTREAM_SNAPSHOT_DIR,
    LedgerError,
    dump_ledger,
    parse_ledger,
    render_notice,
    verbatim_pct,
)


def apply_to_sovereign(row: dict) -> None:
    """Mutate a 'dual' row in place into its 'sovereign' shape.

    WHY this is a separate, pure(ish) function: it used to be three lines
    inline in main() that wrote soak_expires_at_commit_count = 0,
    verbatim_pct = 0.0, upstream_path = 'none' unconditionally (aletheia#6656)
    — a status flip that DESTROYED the row's only measurement instead of
    recording one, discarding the upstream_path link entirely. That made a
    dual -> sovereign transition indistinguishable, to every downstream
    check, from a file that had never been measured at all. This function
    instead retains the dual-era upstream_path as replaced_upstream_path
    (the field check-krites-provenance.py's check_status_sequence now
    requires to reappear unchanged) and recomputes verbatim_pct fresh
    against the offline upstream snapshot when it is available, rather than
    trusting whatever number the row happened to carry at transition time —
    a wave PR can edit source in the same commit that flips status, and a
    stale reused number would silently misreport that edit.

    Falls back to keeping the row's existing (dual-era) verbatim_pct only
    when the snapshot is unavailable for this specific path — deliberately
    not a hard failure, since scripts/measure-krites-provenance.py's P6
    fetch_upstream() has the same offline-preferred/network-fallback
    behavior and this script has no network fetch of its own to match it.
    check-krites-provenance.py's check_verbatim_recompute will catch any
    resulting drift the moment the snapshot is present, exactly as it does
    for derived/dual rows today."""
    prior_upstream_path = row["upstream_path"]
    row["soak_expires_at_commit_count"] = 0
    row["replaced_upstream_path"] = prior_upstream_path
    snapshot_path = UPSTREAM_SNAPSHOT_DIR / prior_upstream_path
    if snapshot_path.is_file():
        local_text = (KRITES_SRC / row["path"]).read_text(errors="replace")
        upstream_text = snapshot_path.read_text(errors="replace")
        row["verbatim_pct"] = verbatim_pct(local_text, upstream_text)
    else:
        print(
            f"warning: {row['path']}: upstream-snapshot/ has no {prior_upstream_path} — "
            "keeping the dual-era verbatim_pct rather than recomputing; "
            "check-krites-provenance.py will catch drift once the snapshot is present",
            file=sys.stderr,
        )
    row["upstream_path"] = "none"


def git_commit_count(ref: str) -> int:
    result = subprocess.run(
        ["git", "-C", str(REPO_ROOT), "rev-list", "--count", ref],
        capture_output=True,
        text=True,
        check=True,
    )
    return int(result.stdout.strip())


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--to", required=True, choices=["dual", "sovereign"])
    parser.add_argument("--soak-commits", type=int, default=None, help="required for --to dual")
    parser.add_argument("--main-ref", default="origin/main")
    parser.add_argument("paths", nargs="+")
    args = parser.parse_args()

    if args.to == "dual" and args.soak_commits is None:
        parser.error("--to dual requires --soak-commits")
    if args.to == "sovereign" and args.soak_commits is not None:
        parser.error("--to sovereign takes soak_expires_at_commit_count=0 always; do not pass --soak-commits")

    try:
        meta, rows = parse_ledger(LEDGER_PATH.read_text())
    except LedgerError as exc:
        print(f"error: could not parse {LEDGER_PATH}: {exc}", file=sys.stderr)
        return 1

    by_path = {r["path"]: r for r in rows}
    missing = [p for p in args.paths if p not in by_path]
    if missing:
        print(f"error: not in ledger: {', '.join(missing)}", file=sys.stderr)
        return 1

    if args.to == "dual":
        target_expiry = git_commit_count(args.main_ref) + args.soak_commits

    for p in args.paths:
        row = by_path[p]
        prior = row["status"]
        if (prior, args.to) not in ALLOWED_TRANSITIONS:
            print(
                f"error: {p}: illegal transition {prior!r} -> {args.to!r} "
                f"(allowed: {sorted(ALLOWED_TRANSITIONS)})",
                file=sys.stderr,
            )
            return 1
        row["status"] = args.to
        if args.to == "dual":
            row["soak_expires_at_commit_count"] = target_expiry
        else:
            apply_to_sovereign(row)

    try:
        LEDGER_PATH.write_text(dump_ledger(meta, rows))
    except LedgerError as exc:
        print(f"error: refusing to write an invalid ledger: {exc}", file=sys.stderr)
        return 1
    NOTICE_PATH.write_text(render_notice(meta, rows))

    print(f"transitioned {len(args.paths)} row(s) to {args.to!r}")
    for p in args.paths:
        print(f"  {p}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
