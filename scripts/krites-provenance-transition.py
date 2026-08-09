#!/usr/bin/env python3
"""Apply a status transition (derived -> dual, or dual -> sovereign; see
kanon/projects/aletheia/phases/05g-krites-overhaul/PROVENANCE-LEDGER.md
"Transitions") to named PROVENANCE.toml rows, and re-render NOTICE.md.

WHY this exists as a script rather than a hand-edit: PROVENANCE.toml's own
header says "do not hand-edit rows" — that norm exists to stop the ledger
drifting from measured reality (verbatim_pct, upstream_path). A status
transition is a different kind of edit: a deliberate lifecycle decision, not
a measurement. Routing it through parse_ledger/dump_ledger keeps it
type-checked (illegal status values, duplicate rows, the sovereign/
verbatim_pct cross-check, the sovereign-path naming rule) instead of a raw
TOML text edit that could silently corrupt an unrelated field. This is
infrastructure every wave's land-dark PR needs (PROVENANCE-LEDGER.md's
three-PR landing discipline), not specific to any one wave —
scripts/measure-krites-provenance.py regenerates verbatim_pct/upstream_path
but never asserts a status past 'derived' or 'sovereign' on its own, by
design (see its load_graduated_status()).

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
    LEDGER_PATH,
    NOTICE_PATH,
    REPO_ROOT,
    LedgerError,
    dump_ledger,
    parse_ledger,
    render_notice,
)


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
            row["soak_expires_at_commit_count"] = 0
            row["verbatim_pct"] = 0.0
            row["upstream_path"] = "none"

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
