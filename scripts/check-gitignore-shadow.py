#!/usr/bin/env python3
"""Reject a tracked file that an ignore rule also matches.

WHY(#5229-adjacent, found via the release substance audit): git does not apply
ignore rules to files already in the index, so a tracked-and-ignored file looks
fine to every everyday command -- `git status` is clean, `git check-ignore`
without `--no-index` reports it as not ignored, and it commits and diffs
normally. The discrepancy is invisible until a tool walks the tree by ignore
rules instead of by the index, at which point the file silently is not there.

That is not hypothetical here. `crates/koina/data/model-seed.toml` was matched by
a broad `data/` rule with per-path negations that never covered it. cargo-mutants
copies the workspace gitignore-filtered, so the file was absent from every copied
tree; `koina/build.rs` reads it and failed with `NotFound`; every mutant for
episteme, nous and krites was therefore unviable and the symbolon and organon
baselines exited 101. All five receipts of the first-ever Release Substance Audit
came back BLOCKED, and the visible failure named three missing advisory issues --
the least important blocker -- while the real cause sat one `.gitignore` line
away.

The invariant is simple enough to state and cheap enough to check: if a file is
worth tracking, no ignore rule should claim it.
"""

from __future__ import annotations

import subprocess
import sys


def shadowed_paths() -> list[str]:
    """Return tracked paths that an ignore rule would exclude.

    `--no-index` is the whole point: without it git suppresses the answer for
    tracked files, which is exactly the blindness being tested for.
    """
    tracked = subprocess.run(
        ["git", "ls-files", "-z"],
        capture_output=True,
        check=True,
    ).stdout

    probe = subprocess.run(
        ["git", "check-ignore", "--no-index", "--stdin", "-z"],
        input=tracked,
        capture_output=True,
        check=False,
    )
    # check-ignore exits 1 when nothing matches, which is the success case here.
    if probe.returncode not in (0, 1):
        sys.stderr.write(probe.stderr.decode("utf-8", "replace"))
        raise SystemExit(f"git check-ignore failed with {probe.returncode}")

    return [p for p in probe.stdout.decode("utf-8", "replace").split("\0") if p]


def main() -> int:
    shadowed = shadowed_paths()
    if not shadowed:
        print("check-gitignore-shadow: no tracked file is matched by an ignore rule")
        return 0

    print(
        "check-gitignore-shadow: these files are tracked AND matched by an ignore rule.",
        file=sys.stderr,
    )
    print(
        "Any tool that walks the tree by ignore rules rather than by the index "
        "(cargo-mutants, container builds, filtered archives) will omit them "
        "silently:",
        file=sys.stderr,
    )
    for path in shadowed:
        rule = subprocess.run(
            ["git", "check-ignore", "-v", "--no-index", path],
            capture_output=True,
            check=False,
        ).stdout.decode("utf-8", "replace").strip()
        print(f"  {rule or path}", file=sys.stderr)
    print(
        "\nAdd a negation for the path, or drop the rule if the file is meant to "
        "be tracked.",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
