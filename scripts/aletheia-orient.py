#!/usr/bin/env python3
"""Print public-safe repo facts for a fresh agent or reviewer.

Sources every fact from the repo itself -- CRATE-INDEX.toml (crates, layers,
feature flags), _llm/api.toml (CLI commands), and a freshness check against
Cargo.toml (CRATE-INDEX.toml drift) and _llm/manifest.toml (L3 index staleness).
No private planning, no fleet-internal inventory, no hand-maintained status
text: every section here is either read from a generated/CI-gated file or
computed on the spot.

Usage:
    python3 scripts/aletheia-orient.py
"""

from __future__ import annotations

import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parent.parent
CRATE_INDEX = REPO_ROOT / "CRATE-INDEX.toml"
API_TOML = REPO_ROOT / "_llm" / "api.toml"
LLM_MANIFEST = REPO_ROOT / "_llm" / "manifest.toml"


def load_toml(path: Path) -> dict[str, Any]:
    with path.open("rb") as fh:
        return tomllib.load(fh)


def section(title: str) -> None:
    print(f"\n== {title} ==")


def orient_crates(index: dict[str, Any]) -> None:
    crates: dict[str, Any] = index.get("crates", {})
    section(f"Crates ({len(crates)}) -- source: CRATE-INDEX.toml")
    by_layer: dict[str, list[str]] = {}
    for name, meta in crates.items():
        by_layer.setdefault(meta.get("layer", "?"), []).append(name)
    for layer in sorted(by_layer):
        names = ", ".join(sorted(by_layer[layer]))
        print(f"  {layer}: {names}")


def orient_features(index: dict[str, Any]) -> None:
    crates: dict[str, Any] = index.get("crates", {})
    gated = {name: meta["features"] for name, meta in crates.items() if meta.get("features")}
    total = sum(len(features) for features in gated.values())
    section(f"Feature flags ({total} across {len(gated)} crates) -- source: CRATE-INDEX.toml")
    for name in sorted(gated):
        print(f"  {name}:")
        for flag, desc in gated[name].items():
            print(f"    {flag} -- {desc}")


def orient_commands() -> None:
    section("Key CLI commands -- source: _llm/api.toml")
    if not API_TOML.exists():
        print(f"  MISSING: {API_TOML.relative_to(REPO_ROOT)}")
        return
    data = load_toml(API_TOML)
    commands = data.get("cli", {}).get("command", [])
    for cmd in commands:
        name = cmd.get("name", "?")
        desc = cmd.get("description", "")
        print(f"  {name:<20} {desc}")
    note = data.get("cli", {}).get("note")
    if note:
        print(f"  ({note})")


def check_crate_index_freshness() -> bool:
    result = subprocess.run(
        [sys.executable, str(REPO_ROOT / "scripts" / "generate-crate-index.py"), "--check"],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    return result.returncode == 0


def orient_freshness() -> None:
    section("Generated-doc freshness")
    if LLM_MANIFEST.exists():
        manifest = load_toml(LLM_MANIFEST)
        generated_at = manifest.get("generated_at", "?")
        print(f"  _llm/manifest.toml present (generated_at={generated_at})")
    else:
        print(
            "  _llm/manifest.toml absent (gitignored, generated) -- run "
            "`uv run scripts/llm-extract-l3.py` to materialize L3 + manifest"
        )

    if check_crate_index_freshness():
        print("  CRATE-INDEX.toml matches the Cargo.toml workspace graph")
    else:
        print(
            "  STALE: CRATE-INDEX.toml has drifted from Cargo.toml -- run "
            "scripts/generate-crate-index.py"
        )


def main() -> None:
    index = load_toml(CRATE_INDEX)
    print("Aletheia orientation")
    print("=====================")
    print("Read docs/GOLDEN-PATH.md first, then docs/HARNESS-LIFECYCLE.md for the")
    print("canonical nine-stage agent-work loop. AGENTS.md covers build/test/lint")
    print("commands and where to add things.")
    orient_crates(index)
    orient_features(index)
    orient_commands()
    orient_freshness()


if __name__ == "__main__":
    main()
