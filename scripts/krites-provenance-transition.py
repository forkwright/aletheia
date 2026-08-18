#!/usr/bin/env python3
"""Apply a status transition (derived -> dual, or dual -> sovereign; see
kanon/projects/aletheia/phases/05g-krites-overhaul/PROVENANCE-LEDGER.md
"Transitions"), or clear a sovereign row's authorship `method`, to named
PROVENANCE.toml rows, and re-render NOTICE.md. Exactly one of --to/--set-method is
required per invocation — they are deliberately not combinable: a status transition
tells us nothing about how a file was originally written (a fresh dual -> sovereign
row always enters at method='unknown'), so recording HOW is always a separate,
deliberate act with its own evidence, never a side effect of moving status.

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

    python3 scripts/krites-provenance-transition.py \\
        --set-method from_behavioral_oracle --evidence 3d0c035eedda8c476bb6d9b71dbdd1f5c336377c \\
        runtime/hnsw_sovereign/types.rs runtime/hnsw_sovereign/graph.rs ...

    python3 scripts/krites-provenance-transition.py \\
        --set-method from_spec_derived_siblings --evidence '#6879' \\
        --consulted fts/tokenizer/remove_long.rs,fts/tokenizer/stemmer.rs \\
        fts/tokenizer/stop_word_filter/sovereign/mod.rs

    python3 scripts/krites-provenance-transition.py \\
        --set-method unknown \\
        some/sovereign/row.rs

--soak-commits is required for --to dual (added to the current
`git rev-list --count origin/main` to produce an absolute
soak_expires_at_commit_count, per the ledger header's own note that the field
is absolute, not relative). It is rejected for --to sovereign, where the
correct value is always 0 (soak is over).

--set-method requires --evidence (a GitHub PR/issue reference '#NNNN', a git
commit SHA, or a spec path 'spec:<path>') for every value except 'unknown',
where --evidence is rejected — 'unknown' by definition has nothing to point
at, and a value + a placeholder evidence string would be indistinguishable
from a real one to every downstream reader. This is the ONLY sanctioned way
to clear a row's 'unknown' authorship method — PROVENANCE.toml's own header
says rows are generated and must not be hand-edited, and method is no
exception: krites_provenance_lib.validate_rows re-checks the evidence shape
on write regardless, so a hand-edit that skips this script fails the next
gate run rather than silently landing.

--consulted takes a comma-separated list of ledger paths the author read while
writing (relative to crates/krites/src/, like every other path in the ledger)
and is REQUIRED for the two spec-class methods, which are the values that make
a claim about what was NOT read (aletheia#6879). Pass an empty string to record
"none" against them. It is rejected with --set-method unknown for the same
reason --evidence is, and is optional elsewhere — omitted, it leaves whatever
the row already records, since re-recording a method does not change what its
author read. krites_provenance_lib.consulted_errors runs on the write path via
dump_ledger, so a list that contradicts the method never reaches the file.
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
    METHODS,
    NOTICE_PATH,
    REPO_ROOT,
    SPEC_CLASS_METHODS,
    UPSTREAM_SNAPSHOT_DIR,
    LedgerError,
    dump_ledger,
    ledger_source_path,
    parse_ledger,
    render_notice,
    sync_exhibit_a,
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
    for derived/dual rows today.

    WHY method always resets to 'unknown' here (#6797-followup): a status
    transition is not an authorship record. The dual-era row's method was
    always 'none' (method is only meaningful on sovereign rows), so there is
    no prior method value to carry forward the way upstream_path is carried
    forward as replaced_upstream_path — the row is entering sovereign for the
    first time and genuinely has no recorded method yet. Defaulting to
    anything but 'unknown' here would be exactly the fiat-clean-value mistake
    this field exists to end. Call --set-method separately, with real
    evidence, if the wave PR that completed this transition also knows how
    the file was written."""
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
    # SAFETY(#5956): the notice leaves with the lineage claim. A row entering sovereign
    # asserts the file is aletheia's own expression; a retained MPL Exhibit A header would
    # keep telling every recipient the opposite, and would encumber aletheia's own work with
    # an obligation the ledger says it does not carry. sync_exhibit_a removes only the block
    # this tooling generated — a notice inherited from the replaced copy (upstream's own
    # header) is not this script's to delete, and check_exhibit_a_notices reports it instead,
    # because deleting someone else's copyright header silently is the one direction that
    # must never be automatic.
    sync_exhibit_a(ledger_source_path(KRITES_SRC, row["path"]), "sovereign")
    row["method"] = "unknown"
    row["method_evidence"] = "none"
    # NOTE(#6879): consulted travels with method for the same reason — a row entering
    # sovereign has no recorded reading list yet, and [] here means "nothing recorded",
    # not a verified "nothing read". --set-method with --consulted is what records one.
    row["consulted"] = []


def apply_set_method(row: dict, method: str, evidence: str, consulted: list[str] | None = None) -> None:
    """Mutate a sovereign row's method/method_evidence/consulted in place.

    WHY this is the ONLY sanctioned mutator: PROVENANCE.toml's header says rows
    are generated and must not be hand-edited, and a hand-edit that skips this
    script fails the next gate run anyway (krites_provenance_lib.validate_rows
    re-checks the method/method_evidence shape on every write, and
    consulted_errors the sibling rule). Routing through here just makes that the
    first failure encountered rather than the last.

    consulted=None leaves the row's existing list alone (defaulting to [] on a
    row that predates the field): re-recording a method does not change what its
    author read, and silently clearing the list would drop the only part of the
    claim a checker can reach (aletheia#6879)."""
    row["method"] = method
    row["method_evidence"] = evidence
    if consulted is not None:
        row["consulted"] = consulted
    else:
        row.setdefault("consulted", [])


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
    parser.add_argument("--to", choices=["dual", "sovereign"], default=None)
    parser.add_argument("--soak-commits", type=int, default=None, help="required for --to dual")
    parser.add_argument("--main-ref", default="origin/main")
    parser.add_argument(
        "--set-method",
        choices=METHODS,
        default=None,
        help="clear/re-record a sovereign row's authorship method; mutually exclusive with --to",
    )
    parser.add_argument(
        "--evidence",
        default=None,
        help="required with --set-method unless the value is 'unknown' — a '#NNNN' PR/issue "
        "ref, a commit SHA, or 'spec:<path>'",
    )
    parser.add_argument(
        "--consulted",
        default=None,
        help="comma-separated ledger paths read while writing (relative to crates/krites/src/); "
        f"required for {' and '.join(SPEC_CLASS_METHODS)}, rejected with --set-method unknown, "
        "optional elsewhere. Pass an empty string to record none",
    )
    parser.add_argument("paths", nargs="+")
    args = parser.parse_args()

    if (args.to is None) == (args.set_method is None):
        parser.error("exactly one of --to or --set-method is required")

    if args.to == "dual" and args.soak_commits is None:
        parser.error("--to dual requires --soak-commits")
    if args.to == "sovereign" and args.soak_commits is not None:
        parser.error("--to sovereign takes soak_expires_at_commit_count=0 always; do not pass --soak-commits")
    if args.to is not None and args.consulted is not None:
        parser.error("--consulted belongs with --set-method; a status transition records no authorship")
    if args.set_method is not None:
        if args.set_method == "unknown":
            if args.evidence is not None:
                parser.error("--set-method unknown must not carry --evidence — 'unknown' has nothing to point at")
            if args.consulted is not None:
                parser.error("--set-method unknown must not carry --consulted — 'unknown' means no record exists")
        elif args.evidence is None:
            parser.error(f"--set-method {args.set_method!r} requires --evidence")
        # SAFETY(#6879): the spec-class values assert what was NOT read. Recording one
        # without stating what WAS read leaves the assertion unfalsifiable, which is the
        # defect 'method' itself was built to end, reproduced one level up.
        if args.set_method in SPEC_CLASS_METHODS and args.consulted is None:
            parser.error(
                f"--set-method {args.set_method!r} requires --consulted — it claims what the "
                "author did not read, so what they DID read is the only checkable part of it. "
                "Pass --consulted '' to record that nothing was consulted"
            )

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

    if args.to is not None:
        # SAFETY(#5956): every path is validated before ANY is applied. apply_to_sovereign
        # now writes to source files (it removes the Exhibit A notice), so validating inside
        # the apply loop would leave the first half of a batch stamped and the rest not when
        # a later path turns out to be an illegal transition — a half-applied batch that the
        # ledger, unwritten, gives no record of.
        illegal = [(p, by_path[p]["status"]) for p in args.paths if (by_path[p]["status"], args.to) not in ALLOWED_TRANSITIONS]
        if illegal:
            for p, prior in illegal:
                print(
                    f"error: {p}: illegal transition {prior!r} -> {args.to!r} "
                    f"(allowed: {sorted(ALLOWED_TRANSITIONS)})",
                    file=sys.stderr,
                )
            return 1
        for p in args.paths:
            row = by_path[p]
            row["status"] = args.to
            if args.to == "dual":
                row["soak_expires_at_commit_count"] = target_expiry
            else:
                apply_to_sovereign(row)
    else:
        evidence = "none" if args.set_method == "unknown" else args.evidence
        consulted = None if args.consulted is None else [c.strip() for c in args.consulted.split(",") if c.strip()]
        if args.set_method == "unknown":
            consulted = []
        for p in args.paths:
            row = by_path[p]
            if row["status"] != "sovereign":
                print(
                    f"error: {p}: method only applies to sovereign rows (status={row['status']!r})",
                    file=sys.stderr,
                )
                return 1
            apply_set_method(row, args.set_method, evidence, consulted)

    try:
        LEDGER_PATH.write_text(dump_ledger(meta, rows))
    except LedgerError as exc:
        print(f"error: refusing to write an invalid ledger: {exc}", file=sys.stderr)
        return 1
    NOTICE_PATH.write_text(render_notice(meta, rows))

    if args.to is not None:
        print(f"transitioned {len(args.paths)} row(s) to {args.to!r}")
    else:
        print(f"set method={args.set_method!r} on {len(args.paths)} row(s)")
    for p in args.paths:
        print(f"  {p}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
