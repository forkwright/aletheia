#!/usr/bin/env python3
"""
Given a list of changed files (one per line on stdin, or as positional args),
output the workspace package names that need testing: the directly changed
packages plus all packages that transitively depend on them.

Uses `cargo metadata` to obtain the workspace package list and the full
dependency resolve graph.  Cargo.lock is always present in this repo, so
`cargo metadata` does not fetch anything; it just reads local files.

Usage:
    git diff --name-only origin/main...HEAD | python3 scripts/affected-crates.py
    python3 scripts/affected-crates.py crates/foo/src/lib.rs crates/bar/Cargo.toml
"""
from __future__ import annotations

import json
import pathlib
import subprocess
import sys


def cargo_metadata() -> dict:
    result = subprocess.run(
        ["cargo", "metadata", "--format-version", "1"],
        capture_output=True,
        text=True,
        check=True,
    )
    return json.loads(result.stdout)


def main() -> None:
    changed_files: list[str] = sys.argv[1:] if sys.argv[1:] else sys.stdin.read().split()

    if not changed_files:
        return

    meta = cargo_metadata()
    workspace_root = pathlib.Path(meta["workspace_root"])
    workspace_member_ids: set[str] = set(meta["workspace_members"])

    # Build maps: manifest_dir → package_id, package_id → package_name
    pkg_by_manifest_dir: dict[str, str] = {}
    pkg_name_by_id: dict[str, str] = {}
    for pkg in meta["packages"]:
        manifest_dir = str(pathlib.Path(pkg["manifest_path"]).parent)
        pkg_by_manifest_dir[manifest_dir] = pkg["id"]
        pkg_name_by_id[pkg["id"]] = pkg["name"]

    # Resolve changed files to workspace package IDs
    changed_ids: set[str] = set()
    for f in changed_files:
        fpath = pathlib.Path(f)
        if not fpath.is_absolute():
            fpath = workspace_root / fpath
        fpath_str = str(fpath.resolve()) if fpath.exists() else str(fpath)
        for manifest_dir, pkg_id in pkg_by_manifest_dir.items():
            if fpath_str.startswith(manifest_dir + "/") or fpath_str == manifest_dir:
                changed_ids.add(pkg_id)

    if not changed_ids:
        return

    # Build reverse dependency map (workspace packages only)
    # rev_deps[A] = {B, C} means B and C depend on A
    rev_deps: dict[str, set[str]] = {mid: set() for mid in workspace_member_ids}
    for node in meta["resolve"]["nodes"]:
        if node["id"] not in workspace_member_ids:
            continue
        for dep in node["deps"]:
            dep_id = dep["pkg"]
            if dep_id in rev_deps:
                rev_deps[dep_id].add(node["id"])

    # BFS: collect all workspace packages that transitively depend on changed ones
    affected: set[str] = changed_ids & workspace_member_ids
    queue: list[str] = list(affected)
    while queue:
        pkg_id = queue.pop()
        for rdep_id in rev_deps.get(pkg_id, set()):
            if rdep_id not in affected:
                affected.add(rdep_id)
                queue.append(rdep_id)

    # Print one package name per line, sorted for stable output
    for pkg_id in sorted(affected):
        if pkg_id in pkg_name_by_id:
            print(pkg_name_by_id[pkg_id])


if __name__ == "__main__":
    main()
