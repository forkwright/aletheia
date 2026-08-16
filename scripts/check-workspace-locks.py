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

# WHY the same enumeration also gates dependabot: a resolve root is invisible to a dependabot scan
# rooted anywhere else, so an excluded workspace's dependencies are simply never updated -- silently,
# because a missing row produces no error, only an absence of PRs. Measured on this repo: cargo
# dependabot covered `/` alone while `crates/theatron/proskenion` (the shipped desktop frontend) and
# `fuzz` each carried their own lock. Both are security-relevant surfaces and neither had ever been
# bumped.
#
# It is asserted HERE rather than in a second script because "the set of independently-resolving
# manifests" is one fact, and it already lives in `workspace_manifests()` below. A separate checker
# would have to re-derive it and would drift.

REPO_ROOT = Path(__file__).resolve().parent.parent
DEPENDABOT = REPO_ROOT / ".github" / "dependabot.yml"


def dependabot_cargo_directories() -> set[str] | None:
    """Directories the cargo ecosystem is configured to scan, or None if unreadable."""
    try:
        import yaml

        config = yaml.safe_load(DEPENDABOT.read_text(encoding="utf-8"))
    except (OSError, ImportError, yaml.YAMLError):
        return None
    if not isinstance(config, dict):
        return None

    covered: set[str] = set()
    for entry in config.get("updates") or []:
        if not isinstance(entry, dict) or entry.get("package-ecosystem") != "cargo":
            continue
        # Both spellings are valid; `directories` is the plural form and takes a list.
        if isinstance(entry.get("directory"), str):
            covered.add(entry["directory"].rstrip("/") or "/")
        for directory in entry.get("directories") or []:
            if isinstance(directory, str):
                covered.add(directory.rstrip("/") or "/")
    return covered


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

    covered = dependabot_cargo_directories()
    if covered is None:
        failures.append(
            ".github/dependabot.yml could not be read or parsed -- dependency-update coverage "
            "cannot be verified, so it is treated as absent rather than assumed present"
        )
    else:
        for manifest in manifests:
            rel = manifest.parent.relative_to(REPO_ROOT).as_posix()
            wanted = "/" if rel == "." else f"/{rel}"
            if wanted not in covered:
                failures.append(
                    f"{manifest.relative_to(REPO_ROOT)}: resolves its own dependency graph but "
                    f"`{wanted}` is not in dependabot.yml's cargo `directories` -- nothing here "
                    f"is ever updated, and the absence of PRs looks exactly like being current"
                )

    if failures:
        print("workspace-lock check FAILED:", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        # WHY two remedies, printed only when earned: they fix different faults, and a message
        # naming the wrong one sends the reader to edit a file that is already correct.
        if any("Cargo.lock" in f and "dependabot" not in f for f in failures):
            print(
                "\nUntracked lock: track it (`git add -f <path>/Cargo.lock`) and drop any ignore "
                "rule covering it. A required check must resolve the same graph every time; "
                "dependency updates then arrive as reviewable commits instead of silent "
                "re-resolves.",
                file=sys.stderr,
            )
        if any("dependabot" in f for f in failures):
            print(
                "\nUncovered resolve root: add its path to the cargo entry's `directories` list in "
                "`.github/dependabot.yml`. Until then that manifest's dependencies are frozen, and "
                "the symptom is silence -- no failing check, no stale-dependency warning, just an "
                "ecosystem that never opens a PR.",
                file=sys.stderr,
            )
        return 1

    print(
        f"workspace-lock check passed: {len(manifests)} resolve roots, all locks tracked and all "
        f"covered by dependabot ({', '.join(str(m.relative_to(REPO_ROOT)) for m in manifests)})"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
