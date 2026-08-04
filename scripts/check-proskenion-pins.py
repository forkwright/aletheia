#!/usr/bin/env python3
"""Verify proskenion's standalone theatron pins match the root workspace."""

from __future__ import annotations

import sys
import tomllib
import logging
from pathlib import Path


THEATRON_DEPS = ("bathron", "gramma", "skeue", "themelion")
PIN_KEYS = ("git", "tag", "rev", "branch", "features", "default-features")
# WHY: koina and skene are root-workspace members that proskenion consumes by
# path across the workspace boundary, so their locked version is the root
# workspace version. proskenion's own entry is versioned independently and must
# not be compared against it.
ROOT_MEMBER_DEPS = ("koina", "skene")
LOGGER = logging.getLogger("check-proskenion-pins")


def load_toml(path: Path) -> dict:
    with path.open("rb") as fh:
        return tomllib.load(fh)


def workspace_deps(manifest: Path) -> dict:
    data = load_toml(manifest)
    return data.get("workspace", {}).get("dependencies", {})


def workspace_version(manifest: Path) -> str | None:
    data = load_toml(manifest)
    return data.get("workspace", {}).get("package", {}).get("version")


def locked_versions(lockfile: Path) -> dict[str, str]:
    data = load_toml(lockfile)
    return {
        package["name"]: package["version"]
        for package in data.get("package", [])
        if "name" in package and "version" in package
    }


def normalized_pin(dep: object) -> dict:
    if not isinstance(dep, dict):
        return {"value": dep}
    return {key: dep.get(key) for key in PIN_KEYS if key in dep}


def lock_errors(repo_root: Path, root_manifest: Path) -> list[str]:
    """Check proskenion's lockfile records the current root workspace version.

    WHY: release-please patches the root Cargo.toml and root Cargo.lock, but
    proskenion is a separate workspace with its own lockfile that no release
    step rewrites. Left unchecked it drifts behind every release until
    `cargo --locked` can no longer resolve the workspace.
    """
    lockfile = repo_root / "crates" / "theatron" / "proskenion" / "Cargo.lock"
    root_version = workspace_version(root_manifest)
    if root_version is None:
        return ["root Cargo.toml: missing [workspace.package] version"]

    locked = locked_versions(lockfile)
    errors: list[str] = []
    for dep_name in ROOT_MEMBER_DEPS:
        if dep_name not in locked:
            errors.append(f"{dep_name}: missing from proskenion Cargo.lock")
            continue
        if locked[dep_name] != root_version:
            errors.append(
                f"{dep_name}: proskenion Cargo.lock records {locked[dep_name]}, "
                f"root workspace is {root_version}"
            )
    return errors


def main() -> int:
    repo_root = Path(__file__).resolve().parents[1]
    root_manifest = repo_root / "Cargo.toml"
    proskenion_manifest = repo_root / "crates" / "theatron" / "proskenion" / "Cargo.toml"

    root_deps = workspace_deps(root_manifest)
    proskenion_deps = workspace_deps(proskenion_manifest)
    errors: list[str] = []
    root_missing = {dep_name for dep_name in THEATRON_DEPS if dep_name not in root_deps}
    proskenion_missing = {
        dep_name for dep_name in THEATRON_DEPS if dep_name not in proskenion_deps
    }

    for dep_name in THEATRON_DEPS:
        if dep_name in root_missing:
            errors.append(f"{dep_name}: missing from root [workspace.dependencies]")
            continue
        if dep_name in proskenion_missing:
            errors.append(f"{dep_name}: missing from proskenion [workspace.dependencies]")
            continue

        root_pin = root_deps[dep_name]
        proskenion_pin = proskenion_deps[dep_name]
        root_norm = normalized_pin(root_pin)
        proskenion_norm = normalized_pin(proskenion_pin)
        if root_norm != proskenion_norm:
            errors.append(
                f"{dep_name}: root pin {root_norm!r} != proskenion pin {proskenion_norm!r}"
            )

    errors.extend(lock_errors(repo_root, root_manifest))

    if errors:
        LOGGER.error("proskenion theatron pin check failed:")
        for error in errors:
            LOGGER.error("  - %s", error)
        LOGGER.error(
            "Update crates/theatron/proskenion/Cargo.toml to mirror the root "
            "[workspace.dependencies] pins, and crates/theatron/proskenion/"
            "Cargo.lock to record the root [workspace.package] version."
        )
        return 1

    LOGGER.info("proskenion theatron pins match root workspace")
    return 0


if __name__ == "__main__":
    logging.basicConfig(format="%(message)s", level=logging.INFO, stream=sys.stderr)
    raise SystemExit(main())
