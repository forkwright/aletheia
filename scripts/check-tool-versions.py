#!/usr/bin/env python3
"""Verify pinned CI tool versions match .github/tool-versions.toml (#4945).

.github/tool-versions.toml is the SSOT for release/security/gate tool
versions; each entry's `sites` list names the workflow/script files that must
carry a literal matching that version. This has no write path — bump the
manifest and every site together, then this check confirms they agree.
"""

from __future__ import annotations

import sys
import tomllib
import logging
from pathlib import Path


LOGGER = logging.getLogger("check-tool-versions")

# WHY a fixed per-tool template rather than a manifest-declared pattern: the
# call-site shape (an install-action `tool:` value, a `cargo install --version`
# flag, a shell variable assignment) is a property of HOW each tool is
# installed, not of its version — six tools, six shapes, not worth a
# templating DSL in the TOML for a set this size.
TOOL_MATCH_TEMPLATES: dict[str, str] = {
    "nextest": "nextest@{version}",
    "cargo-audit": "cargo-audit@{version}",
    "cargo-fuzz": "cargo-fuzz --locked --version {version}",
    "cross": "cross --locked --version {version}",
    "cargo-cyclonedx": 'CARGO_CYCLONEDX_VERSION="{version}"',
    "uv": 'version: "{version}"',
}


def load_toml(path: Path) -> dict:
    with path.open("rb") as fh:
        return tomllib.load(fh)


def check_tool(repo_root: Path, name: str, entry: dict) -> list[str]:
    template = TOOL_MATCH_TEMPLATES.get(name)
    if template is None:
        return [f"{name}: no match template registered in check-tool-versions.py"]

    version = entry.get("version")
    sites = entry.get("sites", [])
    if not version:
        return [f"{name}: manifest entry missing 'version'"]
    if not sites:
        return [f"{name}: manifest entry missing 'sites'"]

    needle = template.format(version=version)
    errors: list[str] = []
    for site in sites:
        site_path = repo_root / site
        if not site_path.is_file():
            errors.append(f"{name}: site {site} does not exist")
            continue
        content = site_path.read_text(encoding="utf-8")
        if needle not in content:
            errors.append(
                f"{name}: {site} does not contain the pinned literal {needle!r} "
                f"(manifest says {version})"
            )
    return errors


def check_fuzz_nightly(repo_root: Path, entry: dict) -> list[str]:
    date = entry.get("nightly_date")
    sites = entry.get("sites", [])
    if not date:
        return ["fuzz: manifest [fuzz] section missing 'nightly_date'"]

    needle = f"nightly-{date}"
    errors: list[str] = []
    for site in sites:
        site_path = repo_root / site
        if not site_path.is_file():
            errors.append(f"fuzz: site {site} does not exist")
            continue
        content = site_path.read_text(encoding="utf-8")
        if needle not in content:
            errors.append(
                f"fuzz: {site} does not contain the pinned literal {needle!r} "
                f"(manifest says nightly_date = {date!r})"
            )
    return errors


def main() -> int:
    repo_root = Path(__file__).resolve().parents[1]
    manifest_path = repo_root / ".github" / "tool-versions.toml"
    manifest = load_toml(manifest_path)

    errors: list[str] = []
    for name, entry in manifest.get("tools", {}).items():
        errors.extend(check_tool(repo_root, name, entry))

    fuzz_entry = manifest.get("fuzz")
    if fuzz_entry:
        errors.extend(check_fuzz_nightly(repo_root, fuzz_entry))

    if errors:
        LOGGER.error("tool-versions check failed:")
        for error in errors:
            LOGGER.error("  - %s", error)
        LOGGER.error(
            "Update .github/tool-versions.toml and every site listed under it "
            "together — this check has no write path."
        )
        return 1

    LOGGER.info("all pinned tool versions match .github/tool-versions.toml")
    return 0


if __name__ == "__main__":
    logging.basicConfig(format="%(message)s", level=logging.INFO, stream=sys.stderr)
    raise SystemExit(main())
