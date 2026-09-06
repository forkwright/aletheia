#!/usr/bin/env python3
"""Verify _llm/current_state.toml's [state].version matches the release manifest (#4034).

_llm/current_state.toml is the de facto cold-start STATE doc for aletheia:
_llm/README.md injects it into every session. Its `[state].version` field is
free text with no write path of its own, so it drifts from the real release
version silently — twice already (v0.22.0 claimed at 0.27.0, then 0.37.0
claimed at 0.43.0/0.45.0). `.release-please-manifest.json` is the canonical,
release-please-maintained version for the root package; this check makes the
two agree mechanically instead of by memory.
"""

from __future__ import annotations

import json
import logging
import sys
from pathlib import Path

import tomllib

LOGGER = logging.getLogger("check-current-state-version")


def read_state_version(state_path: Path) -> str:
    with state_path.open("rb") as fh:
        data = tomllib.load(fh)
    version = data.get("state", {}).get("version")
    if not isinstance(version, str) or not version:
        raise ValueError(f"{state_path}: missing or invalid [state].version")
    return version


def read_manifest_version(manifest_path: Path) -> str:
    with manifest_path.open(encoding="utf-8") as fh:
        data = json.load(fh)
    version = data.get(".")
    if not isinstance(version, str) or not version:
        raise ValueError(f"{manifest_path}: missing or invalid \".\" entry")
    return version


def check(state_path: Path, manifest_path: Path) -> list[str]:
    errors: list[str] = []
    try:
        state_version = read_state_version(state_path)
    except (OSError, ValueError, tomllib.TOMLDecodeError) as error:
        return [f"{state_path}: {error}"]
    try:
        manifest_version = read_manifest_version(manifest_path)
    except (OSError, ValueError, json.JSONDecodeError) as error:
        return [f"{manifest_path}: {error}"]

    if state_version != manifest_version:
        errors.append(
            f"{state_path} [state].version is {state_version!r} but "
            f"{manifest_path} is {manifest_version!r}. Update "
            f"_llm/current_state.toml's [state].version to match the "
            f"release manifest."
        )
    return errors


def main() -> int:
    repo_root = Path(__file__).resolve().parents[1]
    state_path = repo_root / "_llm" / "current_state.toml"
    manifest_path = repo_root / ".release-please-manifest.json"

    errors = check(state_path, manifest_path)
    if errors:
        LOGGER.error("current-state-version check failed:")
        for error in errors:
            LOGGER.error("  - %s", error)
        return 1

    LOGGER.info(
        "_llm/current_state.toml [state].version matches .release-please-manifest.json"
    )
    return 0


if __name__ == "__main__":
    logging.basicConfig(format="%(message)s", level=logging.INFO, stream=sys.stderr)
    raise SystemExit(main())
