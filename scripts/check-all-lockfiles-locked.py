#!/usr/bin/env python3
"""Validate every committed Cargo lockfile with --locked (#7148).

PR #7139 raised Krites to itertools 0.15.0 in its manifest and root lock but
left fuzz/Cargo.lock on itertools 0.14.0 -- the fuzz graph enables Krites
through mneme-engine, so that secondary lockfile no longer satisfied its
manifest. The fuzz gate command did not pass --locked, so cargo silently
repaired the committed lockfile during CI instead of proving the checked-in
artifact was coherent; PR #7147 then regenerated the stale entry
incidentally, masking the gap rather than closing it.

This check enumerates every [workspace]-declaring manifest the same way
scripts/check-workspace-locks.py already does (loaded from there rather than
re-derived, so the two never drift apart the way the fuzz lock itself did),
and for each one with a committed Cargo.lock, runs the manifest's owning
resolve command with --locked. Cargo's own --locked flag is the fail-on-
rewrite mechanism: it errors instead of silently updating the lock when the
manifest and lock disagree.
"""

from __future__ import annotations

import importlib.util
import logging
import subprocess
import sys
from pathlib import Path

LOGGER = logging.getLogger("check-all-lockfiles-locked")

REPO_ROOT = Path(__file__).resolve().parent.parent

# WHY fuzz alone needs a different command: it is a bin-only crate (no_main
# fuzz targets) whose gate purpose was always "does this compile", not "does
# this resolve" -- this check replaces the fuzz job's pre-existing unlocked
# `cargo check --manifest-path fuzz/Cargo.toml --bins` outright, so it must
# keep that compile coverage, just with --locked added. Every other resolve
# root gets the cheaper, non-compiling `cargo metadata --locked`, which is
# sufficient to prove the committed lock still satisfies the manifest.
FUZZ_MANIFEST_RELATIVE = Path("fuzz") / "Cargo.toml"


def _workspace_locks_checker() -> object:
    """Load scripts/check-workspace-locks.py's own manifest enumeration.

    Loaded via importlib rather than a normal import because the source
    file's name (hyphenated) is not a valid Python module identifier --
    mirrors scripts/release_asset_inventory.py's load of
    check-release-assets.py. "Every Cargo.toml declaring [workspace]" is one
    fact; it already lives in workspace_manifests() there, so a release-time
    lockfile check can load it from the same place instead of re-deriving it
    a third time.
    """
    path = REPO_ROOT / "scripts" / "check-workspace-locks.py"
    spec = importlib.util.spec_from_file_location("check_workspace_locks", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load manifest enumeration from {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


workspace_manifests = _workspace_locks_checker().workspace_manifests


def command_for(manifest: Path, repo_root: Path = REPO_ROOT) -> list[str]:
    rel = manifest.relative_to(repo_root)
    if rel == FUZZ_MANIFEST_RELATIVE:
        return ["cargo", "check", "--manifest-path", str(rel), "--bins", "--locked"]
    return ["cargo", "metadata", "--locked", "--manifest-path", str(rel)]


def check_manifest(
    manifest: Path, repo_root: Path = REPO_ROOT, run=subprocess.run
) -> str | None:
    """Run manifest's --locked resolve command; return a failure message, or None."""
    lock = manifest.parent / "Cargo.lock"
    rel = manifest.relative_to(repo_root)
    if not lock.exists():
        # scripts/check-workspace-locks.py already fails a missing or
        # untracked lock; this check has nothing further to prove without one.
        return None

    command = command_for(manifest, repo_root)
    result = run(command, cwd=repo_root, capture_output=True, text=True, check=False)
    if result.returncode != 0:
        return (
            f"{rel}: `{' '.join(command)}` failed -- the committed Cargo.lock "
            f"does not satisfy the manifest (cargo would rewrite it):\n"
            f"{(result.stderr or '').strip()}"
        )
    return None


def main() -> int:
    manifests = workspace_manifests()
    failures: list[str] = []
    for manifest in manifests:
        failure = check_manifest(manifest)
        if failure:
            failures.append(failure)

    if failures:
        LOGGER.error("lockfile check FAILED:")
        for failure in failures:
            LOGGER.error("  - %s", failure)
        LOGGER.error(
            "\nRegenerate the stale lockfile locally (a plain `cargo build` or "
            "`cargo check` in that manifest's directory, without --locked) and "
            "commit the result alongside whatever manifest change caused the "
            "drift."
        )
        return 1

    LOGGER.info(
        "all %d committed lockfile(s) satisfy their manifests under --locked",
        len(manifests),
    )
    return 0


if __name__ == "__main__":
    logging.basicConfig(format="%(message)s", level=logging.INFO, stream=sys.stderr)
    raise SystemExit(main())
