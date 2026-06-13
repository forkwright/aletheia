#!/usr/bin/env python3
"""Verify every in-repo package is explicitly marked private."""

from __future__ import annotations

import logging
import sys
import tomllib
from pathlib import Path


LOGGER = logging.getLogger("check-cargo-publish-policy")
SKIP_DIRS = {".direnv", ".git", "target"}


def manifest_paths(repo_root: Path) -> list[Path]:
    paths: list[Path] = []
    for path in repo_root.rglob("Cargo.toml"):
        relative = path.relative_to(repo_root)
        if any(part in SKIP_DIRS for part in relative.parts):
            continue
        paths.append(path)
    return sorted(paths)


def load_toml(path: Path) -> dict:
    with path.open("rb") as fh:
        return tomllib.load(fh)


def main() -> int:
    repo_root = Path(__file__).resolve().parents[1]
    errors: list[str] = []
    checked = 0

    for manifest in manifest_paths(repo_root):
        data = load_toml(manifest)
        package = data.get("package")
        if not isinstance(package, dict):
            continue

        checked += 1
        if package.get("publish") is not False:
            relative = manifest.relative_to(repo_root)
            name = package.get("name", "<unknown>")
            errors.append(f"{relative}: package {name!r} must set publish = false")

    if errors:
        LOGGER.error("cargo publish policy check failed:")
        for error in errors:
            LOGGER.error("  - %s", error)
        return 1

    LOGGER.info("cargo publish policy check passed for %s packages", checked)
    return 0


if __name__ == "__main__":
    logging.basicConfig(format="%(message)s", level=logging.INFO, stream=sys.stderr)
    raise SystemExit(main())
