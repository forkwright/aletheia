#!/usr/bin/env python3
"""Verify proskenion's standalone theatron pins match the root workspace."""

from __future__ import annotations

import sys
import tomllib
import logging
from pathlib import Path


THEATRON_DEPS = ("bathron", "gramma", "skeue", "themelion")
PIN_KEYS = ("git", "tag", "rev", "branch", "features", "default-features")
LOGGER = logging.getLogger("check-proskenion-pins")


def load_toml(path: Path) -> dict:
    with path.open("rb") as fh:
        return tomllib.load(fh)


def workspace_deps(manifest: Path) -> dict:
    data = load_toml(manifest)
    return data.get("workspace", {}).get("dependencies", {})


def normalized_pin(dep: object) -> dict:
    if not isinstance(dep, dict):
        return {"value": dep}
    return {key: dep.get(key) for key in PIN_KEYS if key in dep}


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

    if errors:
        LOGGER.error("proskenion theatron pin check failed:")
        for error in errors:
            LOGGER.error("  - %s", error)
        LOGGER.error(
            "Update crates/theatron/proskenion/Cargo.toml to mirror the root "
            "[workspace.dependencies] pins."
        )
        return 1

    LOGGER.info("proskenion theatron pins match root workspace")
    return 0


if __name__ == "__main__":
    logging.basicConfig(format="%(message)s", level=logging.INFO, stream=sys.stderr)
    raise SystemExit(main())
