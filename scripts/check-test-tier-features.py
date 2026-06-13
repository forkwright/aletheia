#!/usr/bin/env python3
"""Validate workspace test-tier feature relationships."""

from __future__ import annotations

import sys
import tomllib
from pathlib import Path


def load_toml(path: Path) -> dict:
    with path.open("rb") as fh:
        return tomllib.load(fh)


def workspace_manifest_paths(repo_root: Path) -> list[Path]:
    root_manifest = repo_root / "Cargo.toml"
    workspace = load_toml(root_manifest).get("workspace", {})
    paths: list[Path] = []
    seen: set[Path] = set()

    for member in workspace.get("members", []):
        for crate_dir in sorted(repo_root.glob(member)):
            manifest = crate_dir / "Cargo.toml"
            if manifest.exists() and manifest not in seen:
                seen.add(manifest)
                paths.append(manifest)

    return paths


def main() -> int:
    repo_root = Path(__file__).resolve().parents[1]
    errors: list[str] = []
    checked = 0

    for manifest in workspace_manifest_paths(repo_root):
        data = load_toml(manifest)
        package_name = data.get("package", {}).get("name", str(manifest))
        features = data.get("features", {})
        test_core = features.get("test-core", [])

        if not test_core:
            continue

        checked += 1
        test_full = features.get("test-full")
        if test_full is None:
            errors.append(f"{package_name}: non-empty test-core but missing test-full")
            continue

        if "test-core" not in test_full:
            rel_manifest = manifest.relative_to(repo_root)
            errors.append(
                f"{package_name}: {rel_manifest} has non-empty test-core, "
                "but test-full does not include test-core"
            )

    if errors:
        print("test tier feature check failed:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        print(
            "Fix by adding test-full = [\"test-core\", ...] or making the "
            "exception explicit in this checker.",
            file=sys.stderr,
        )
        return 1

    print(f"test tier feature check passed for {checked} crate(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
