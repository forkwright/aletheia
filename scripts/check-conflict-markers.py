#!/usr/bin/env python3
"""Reject tracked files containing unresolved merge-conflict markers."""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

# WHY this check exists: an abandoned merge left `crates/krites/CLAUDE.md` carrying
# `<<<<<<< HEAD` / `||||||| <sha>` / `>>>>>>> <branch>` in the working tree and all three stages in
# the index, with no MERGE_HEAD to signal a merge was in progress. HEAD's copy was clean, so nothing
# was committed — but a routine `git add -A` would have committed conflict markers into the file
# every agent entering that crate reads first. Nothing in the repo detected it: the pre-commit hook
# guards `instance/` paths only, and no workflow scans for markers.
#
# WHY a marker can be a false positive: documentation about merge conflicts legitimately quotes these
# strings. The allowlist below is for those, and each entry states why.

REPO_ROOT = Path(__file__).resolve().parent.parent

# A marker only counts at the start of a line, which is where git writes it.
MARKERS = (
    re.compile(r"^<{7}(?: |$)"),
    re.compile(r"^={7}$"),
    re.compile(r"^>{7}(?: |$)"),
    re.compile(r"^\|{7}(?: |$)"),
)

# Paths permitted to contain marker-shaped lines, with the reason. Keep this empty unless a file
# genuinely documents conflict resolution.
ALLOWLIST: dict[str, str] = {}

BINARY_HINT = b"\x00"


def tracked_files() -> list[str]:
    out = subprocess.run(
        ["git", "ls-files", "-z"], cwd=REPO_ROOT, capture_output=True, check=True
    )
    return [p for p in out.stdout.decode("utf-8", "replace").split("\0") if p]


def unmerged_paths() -> list[str]:
    """Index entries at stage 1/2/3 — a conflict that was never resolved."""
    out = subprocess.run(
        ["git", "ls-files", "-u", "--format=%(path)"],
        cwd=REPO_ROOT,
        capture_output=True,
        check=False,
    )
    if out.returncode != 0:
        return []
    return sorted({p for p in out.stdout.decode("utf-8", "replace").splitlines() if p})


def main() -> int:
    failures: list[str] = []

    # An unmerged index entry is a defect on its own: it means a merge was abandoned, and the next
    # `git add -A` commits whatever the working tree holds — markers included.
    for path in unmerged_paths():
        failures.append(f"{path}: unresolved index entry (stage 1/2/3) — a merge was abandoned here")

    for rel in tracked_files():
        if rel in ALLOWLIST:
            continue
        full = REPO_ROOT / rel
        try:
            raw = full.read_bytes()
        except OSError:
            continue
        if BINARY_HINT in raw[:8192]:
            continue
        text = raw.decode("utf-8", "replace")
        for lineno, line in enumerate(text.splitlines(), 1):
            if any(m.match(line) for m in MARKERS):
                failures.append(f"{rel}:{lineno}: conflict marker: {line[:60]}")
                break

    if failures:
        print("conflict-marker check FAILED:", file=sys.stderr)
        for f in failures:
            print(f"  - {f}", file=sys.stderr)
        print(
            "\nResolve the merge and stage the result. If a file legitimately documents conflict\n"
            "resolution, add it to ALLOWLIST in this script with the reason.",
            file=sys.stderr,
        )
        return 1

    print(f"conflict-marker check passed: {len(tracked_files())} tracked files, no markers, index clean")
    return 0


if __name__ == "__main__":
    sys.exit(main())
