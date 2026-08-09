#!/usr/bin/env python3
"""Assert every independently-resolving Cargo manifest has a tracked lockfile."""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

# WHY: a manifest that declares [workspace] resolves its own dependency graph, independent of the
# root lock. If its Cargo.lock is untracked, CI re-resolves that graph against live crates.io on
# every run -- so a required check becomes non-deterministic against an input no one in this repo
# controls, and an upstream publish can turn every open PR red with no commit here.
#
# That is not hypothetical. `fuzz/Cargo.lock` was gitignored (inherited from cargo-fuzz's template,
# which suits a scratch directory and not a required gate). A fresh resolve dropped the feature
# unification supplying zune-jpeg's `log` feature, its `warn!` collapsed to a no-op macro, and
# "macro expansion ends with an incomplete expression" blocked seven PRs at once -- while main
# stayed green, because that job runs only on pull_request and main never exercises it.
#
# The green-main detail is why this check exists rather than a note somewhere: the signal that would
# normally reveal the breakage is structurally incapable of seeing it.

REPO_ROOT = Path(__file__).resolve().parent.parent


def tracked(path: Path) -> bool:
    rel = path.relative_to(REPO_ROOT)
    result = subprocess.run(
        ["git", "ls-files", "--error-unmatch", str(rel)],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
    )
    return result.returncode == 0


def workspace_manifests() -> list[Path]:
    listing = subprocess.run(
        ["git", "ls-files", "*Cargo.toml"],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        check=True,
    )
    found = []
    for line in listing.stdout.splitlines():
        manifest = REPO_ROOT / line
        try:
            body = manifest.read_text(encoding="utf-8")
        except OSError:
            continue
        # A [workspace] table -- with or without members -- makes this manifest a resolve root.
        if any(l.strip() == "[workspace]" for l in body.splitlines()):
            found.append(manifest)
    return found


def main() -> int:
    failures = []
    manifests = workspace_manifests()

    for manifest in manifests:
        lock = manifest.parent / "Cargo.lock"
        rel = manifest.relative_to(REPO_ROOT)
        if not lock.exists():
            failures.append(f"{rel}: declares [workspace] but has no Cargo.lock beside it")
        elif not tracked(lock):
            failures.append(
                f"{rel}: declares [workspace] but its Cargo.lock is NOT tracked by git "
                f"-- CI will re-resolve this graph from crates.io on every run"
            )

    if failures:
        print("workspace-lock check FAILED:", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        print(
            "\nTrack the lockfile (`git add -f <path>/Cargo.lock`) and drop any ignore rule "
            "covering it. A required check must resolve the same graph every time; dependency "
            "updates then arrive as reviewable commits instead of silent re-resolves.",
            file=sys.stderr,
        )
        return 1

    print(
        f"workspace-lock check passed: {len(manifests)} resolve roots, all locks tracked "
        f"({', '.join(str(m.relative_to(REPO_ROOT)) for m in manifests)})"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
