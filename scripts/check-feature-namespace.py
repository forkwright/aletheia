#!/usr/bin/env python3
"""Fail if a workspace feature name cannot resolve under cargo's CLI feature rules.

cargo-auditable (release.yml's build job) collects each compiled crate's
dependency tree by running `cargo metadata --format-version 1 --locked
--features <names>` with that crate's enabled features as unqualified names.
Cargo applies an unqualified CLI feature name to every workspace member: a
member defining the feature gets it, a member lacking it is skipped, but a
member holding an optional dependency of the same name with the implicit
feature suppressed by `dep:` syntax is a hard resolver error ("package X
does not have feature Y"). aletheia#6958: koina's `fjall` feature collided
with episteme's `dep:fjall`, so every release build died inside
cargo-auditable's metadata call and four draft releases shipped zero assets.

This gate replays the same resolution cargo-auditable performs, once per
workspace feature name, so the ambiguity class fails PR CI instead of a
tagged release. `--filter-platform` is omitted deliberately: it prunes the
emitted dependency tree to one target platform and does not change feature
validation, which is platform-independent.
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
from typing import Any

ANSI = re.compile(r"\x1b\[[0-9;]*m")


def workspace_feature_names(no_deps_metadata: dict[str, Any]) -> list[str]:
    """Union of feature names defined by workspace members, minus `default`.

    cargo-auditable never passes `default` (it maps to implicit
    default-features handling), so the name is out of scope here.
    """
    members = set(no_deps_metadata["workspace_members"])
    names = {
        feature
        for pkg in no_deps_metadata["packages"]
        if pkg["id"] in members
        for feature in pkg.get("features", {})
    }
    names.discard("default")
    return sorted(names)


def cargo_metadata(extra_args: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["cargo", "metadata", "--format-version", "1", *extra_args],
        capture_output=True,
        text=True,
        check=False,
    )


def failure_detail(stderr: str) -> str:
    """First cargo error line, ANSI-stripped, for a failed metadata run."""
    for line in stderr.splitlines():
        plain = ANSI.sub("", line).strip()
        if plain.startswith("error"):
            return plain
    for line in stderr.splitlines():
        plain = ANSI.sub("", line).strip()
        if plain:
            return plain
    return "(cargo metadata failed with no stderr output)"


def main() -> int:
    listing = cargo_metadata(["--no-deps"])
    if listing.returncode != 0:
        print(
            f"feature namespace check could not list the workspace: "
            f"{failure_detail(listing.stderr)}",
            file=sys.stderr,
        )
        return 1
    names = workspace_feature_names(json.loads(listing.stdout))

    failures: list[tuple[str, str]] = []
    for name in names:
        proc = cargo_metadata(["--locked", "--features", name])
        if proc.returncode != 0:
            failures.append((name, failure_detail(proc.stderr)))

    if failures:
        print(
            "feature namespace check failed: these feature names do not resolve "
            "when applied workspace-wide (the resolution cargo-auditable performs "
            "for every crate a release build compiles):",
            file=sys.stderr,
        )
        for name, detail in failures:
            print(f"  - {name}: {detail}", file=sys.stderr)
        return 1

    print(f"feature namespace clean: {len(names)} feature names resolve workspace-wide")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
