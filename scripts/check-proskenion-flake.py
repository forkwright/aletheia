#!/usr/bin/env python3
"""Verify direnv and flake wiring stays pointed at proskenion."""

from __future__ import annotations

import logging
import sys
import tomllib
from pathlib import Path


LOGGER = logging.getLogger("check-proskenion-flake")
MANIFEST = Path("crates/theatron/proskenion/Cargo.toml")
STALE_TOKENS = (
    ".#desktop",
    "aletheia-desktop",
    "theatron-desktop",
)


def load_toml(path: Path) -> dict:
    with path.open("rb") as fh:
        return tomllib.load(fh)


def main() -> int:
    repo_root = Path(__file__).resolve().parents[1]
    manifest_path = repo_root / MANIFEST
    manifest = load_toml(manifest_path)
    package = manifest["package"]
    package_name = package["name"]
    package_version = package["version"]

    flake = (repo_root / "flake.nix").read_text(encoding="utf-8")
    envrc = (repo_root / ".envrc").read_text(encoding="utf-8")
    errors: list[str] = []

    if package_name != "proskenion":
        errors.append(f"{MANIFEST}: expected package name 'proskenion', found {package_name!r}")

    if "use flake .#proskenion" not in envrc:
        errors.append(".envrc must use the proskenion flake shell")

    for token in STALE_TOKENS:
        if token in flake or token in envrc:
            errors.append(f"stale desktop flake token remains: {token}")

    required_flake_fragments = (
        "./crates/theatron/proskenion/Cargo.toml",
        "proskenionName = proskenionPackage.name;",
        "proskenionVersion = proskenionPackage.version;",
        'proskenionCargoArgs = "--manifest-path crates/theatron/proskenion/Cargo.toml -p ${proskenionName}";',
        "pname = proskenionName;",
        "version = proskenionVersion;",
    )
    for fragment in required_flake_fragments:
        if fragment not in flake:
            errors.append(f"flake.nix missing expected proskenion wiring: {fragment}")

    if package_version in flake:
        errors.append(
            f"flake.nix must derive proskenion version from {MANIFEST}, not hardcode {package_version}"
        )

    if errors:
        LOGGER.error("proskenion flake check failed:")
        for error in errors:
            LOGGER.error("  - %s", error)
        return 1

    LOGGER.info("proskenion flake wiring matches %s", MANIFEST)
    return 0


if __name__ == "__main__":
    logging.basicConfig(format="%(message)s", level=logging.INFO, stream=sys.stderr)
    raise SystemExit(main())
