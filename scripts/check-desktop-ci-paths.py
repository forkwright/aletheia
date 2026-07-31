#!/usr/bin/env python3
"""Verify the Desktop workflow's path filter covers proskenion's path-dep closure.

WHY: proskenion is excluded from the root workspace, so root CI never compiles it.
The Desktop workflow is the only pull_request job that does, and it is path-filtered.
Any path dependency of proskenion that is missing from that filter can therefore take
a breaking change with a fully green board, surfacing only in the tag-triggered
release build.
"""

from __future__ import annotations

import sys
import tomllib
import logging
from pathlib import Path

import yaml

DEPENDENCY_TABLES = ("dependencies", "dev-dependencies", "build-dependencies")
WORKFLOW = Path(".github/workflows/desktop.yml")
ROOT_MANIFEST = Path("crates/theatron/proskenion/Cargo.toml")
LOGGER = logging.getLogger("check-desktop-ci-paths")


def load_toml(path: Path) -> dict:
    with path.open("rb") as fh:
        return tomllib.load(fh)


def path_dep_dirs(manifest: Path, repo_root: Path) -> set[Path]:
    """Directories of every crate reachable from `manifest` by path dependency."""
    seen: set[Path] = set()
    queue = [manifest]
    while queue:
        current = queue.pop()
        data = load_toml(current)
        tables = [data.get(table, {}) for table in DEPENDENCY_TABLES]
        workspace = data.get("workspace", {})
        tables.append(workspace.get("dependencies", {}))
        for table in tables:
            for dep in table.values():
                if not isinstance(dep, dict) or "path" not in dep:
                    continue
                dep_dir = (current.parent / dep["path"]).resolve()
                relative = dep_dir.relative_to(repo_root)
                if relative in seen:
                    continue
                seen.add(relative)
                queue.append(dep_dir / "Cargo.toml")
    return seen


def trigger_paths(workflow: dict) -> dict[str, list[str]]:
    # WHY: `on` is parsed by PyYAML as the boolean True (YAML 1.1 truthy key).
    triggers = workflow.get("on", workflow.get(True, {}))
    return {
        event: spec["paths"]
        for event, spec in triggers.items()
        if isinstance(spec, dict) and "paths" in spec
    }


def covers(pattern: str, directory: Path) -> bool:
    if not pattern.endswith("/**"):
        return False
    prefix = Path(pattern[: -len("/**")])
    return directory == prefix or prefix in directory.parents


def main() -> int:
    repo_root = Path(__file__).resolve().parents[1]
    workflow = yaml.safe_load((repo_root / WORKFLOW).read_text(encoding="utf-8"))
    events = trigger_paths(workflow)

    errors: list[str] = []
    if not events:
        errors.append(f"{WORKFLOW}: no triggers declare a `paths` filter")

    dependencies = path_dep_dirs(repo_root / ROOT_MANIFEST, repo_root)
    required = dependencies | {ROOT_MANIFEST.parent}
    for event, patterns in sorted(events.items()):
        for directory in sorted(required):
            if not any(covers(pattern, directory) for pattern in patterns):
                errors.append(
                    f"{event}: no path filter covers {directory}/, "
                    "a path dependency of proskenion"
                )

    if errors:
        LOGGER.error("desktop CI path filter check failed:")
        for error in errors:
            LOGGER.error("  - %s", error)
        LOGGER.error(
            "Add the crate directory to every `paths:` list in %s, or the Desktop "
            "job will not run when it changes.",
            WORKFLOW,
        )
        return 1

    LOGGER.info(
        "desktop CI path filter covers all %d proskenion path dependencies",
        len(required),
    )
    return 0


if __name__ == "__main__":
    logging.basicConfig(format="%(message)s", level=logging.INFO, stream=sys.stderr)
    raise SystemExit(main())
