#!/usr/bin/env python3
"""Generate and verify CRATE-INDEX.toml's dependency-graph fields from Cargo.toml.

`depends_on`, `used_by`, and `dev_depends_on` were hand-maintained and drifted
from the real workspace graph in ~17 ways (aletheia#5574): missing prod deps,
phantom indirect deps, and prod/dev-only confusion (a crate reachable only
through `[dev-dependencies]` listed as if `[dependencies]` pulled it too).
Cargo.toml is the canonical graph; this generator derives the three
structural fields from it and rewrites only those fields in place, byte-exact
everywhere else -- `layer`, `purpose`, and `[crates.X.features]` stay
hand-authored, since neither is mechanically derivable from the manifest
graph alone.

A crate's real package name can differ from its directory (`crates/daemon`
builds as `oikonomos`, `crates/eval` as `dokimion`) -- every workspace member
manifest is read once to resolve `path = "../x"` dependency entries to the
package name they actually name, rather than trusting the directory segment.

Usage:
    python3 scripts/generate-crate-index.py
    python3 scripts/generate-crate-index.py --check

--check exits 0 when the committed file's structural fields match freshly
derived output and non-zero with a diff-shaped explanation.
"""

from __future__ import annotations

import argparse
import re
import sys
import tomllib
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
ROOT_MANIFEST = REPO_ROOT / "Cargo.toml"
INDEX_PATH = REPO_ROOT / "CRATE-INDEX.toml"

# WHY: matches a top-level `[crates.NAME]` header only -- NOT the
# `[crates.NAME.features]` sub-table, which shares the `crates.NAME` prefix
# but must never have its lines mistaken for the parent section's.
SECTION_RE = re.compile(r"^\[crates\.([A-Za-z0-9_-]+)\]$")
SUBSECTION_RE = re.compile(r"^\[crates\.[A-Za-z0-9_-]+\.")
FIELD_RE = re.compile(r"^(depends_on|used_by|dev_depends_on) = \[.*\]$")


def load_toml(path: Path) -> dict:
    with path.open("rb") as fh:
        return tomllib.load(fh)


def workspace_member_dirs() -> list[Path]:
    data = load_toml(ROOT_MANIFEST)
    members = data.get("workspace", {}).get("members", [])
    return [REPO_ROOT / member for member in members]


def path_deps(table: dict) -> set[str]:
    """Path-valued dependency entries in one `[dependencies]`-shaped table."""
    names: set[str] = set()
    for value in table.values():
        if isinstance(value, dict) and "path" in value:
            names.add(str(value["path"]))
    return names


def manifest_path_deps(manifest: dict) -> tuple[set[str], set[str]]:
    """Returns (normal_path_deps, dev_path_deps) as raw `path = "..."` strings,
    pooled across the default dependency tables and any `[target.'cfg(...)'.*]`
    tables -- a target-gated path dependency is still a real workspace edge."""
    normal = path_deps(manifest.get("dependencies", {}))
    dev = path_deps(manifest.get("dev-dependencies", {}))
    for target_table in manifest.get("target", {}).values():
        normal |= path_deps(target_table.get("dependencies", {}))
        dev |= path_deps(target_table.get("dev-dependencies", {}))
    return normal, dev


def derive_graph() -> dict[str, dict[str, list[str]]]:
    """Returns {crate_name: {"depends_on": [...], "used_by": [...], "dev_depends_on": [...]}}."""
    member_dirs = workspace_member_dirs()

    # Resolve every member's declared package name once, keyed by its manifest
    # directory, so a raw `path = "../x"` string can be turned into the name
    # the target actually publishes under.
    name_by_dir: dict[Path, str] = {}
    for member_dir in member_dirs:
        manifest = load_toml(member_dir / "Cargo.toml")
        name_by_dir[member_dir.resolve()] = manifest["package"]["name"]

    depends_on: dict[str, set[str]] = {name: set() for name in name_by_dir.values()}
    dev_depends_on: dict[str, set[str]] = {name: set() for name in name_by_dir.values()}

    for member_dir in member_dirs:
        resolved_dir = member_dir.resolve()
        crate_name = name_by_dir[resolved_dir]
        manifest = load_toml(member_dir / "Cargo.toml")
        normal_raw, dev_raw = manifest_path_deps(manifest)

        def resolve(raw_paths: set[str], *, allow_self: bool) -> set[str]:
            resolved: set[str] = set()
            for raw in raw_paths:
                dep_dir = (member_dir / raw).resolve()
                dep_name = name_by_dir.get(dep_dir)
                if dep_name is not None and (allow_self or dep_name != crate_name):
                    resolved.add(dep_name)
            return resolved

        # WHY normal deps exclude self: a genuine build-graph self-edge would
        # be a cycle Cargo itself refuses to resolve, so it cannot occur here.
        depends_on[crate_name] = resolve(normal_raw, allow_self=False)
        # WHY dev deps ALLOW self: `path = "."` in `[dev-dependencies]` is a
        # real, common Cargo idiom -- a crate depending on its own
        # feature-gated surface (e.g. `test-support`) for its own test
        # binaries (see crates/taxis/Cargo.toml). Excluding it would silently
        # drop information the manifest actually states.
        #
        # WHY subtract depends_on beyond that: a crate reachable through BOTH
        # tables (a dev-only feature flag on an otherwise-normal dep) is
        # already a real prod edge -- dev_depends_on exists to name the deps
        # that are ONLY reachable in a dev/test build, matching the field's
        # use elsewhere in this file (e.g. crates.agora, crates.thesauros,
        # crates.diaporeia).
        dev_depends_on[crate_name] = resolve(dev_raw, allow_self=True) - depends_on[crate_name]

    used_by: dict[str, set[str]] = {name: set() for name in name_by_dir.values()}
    for consumer, deps in depends_on.items():
        for dep in deps:
            used_by[dep].add(consumer)

    return {
        name: {
            "depends_on": sorted(depends_on[name]),
            "used_by": sorted(used_by[name]),
            "dev_depends_on": sorted(dev_depends_on[name]),
        }
        for name in name_by_dir.values()
    }


def format_array(values: list[str]) -> str:
    if not values:
        return "[]"
    return "[" + ", ".join(f'"{v}"' for v in values) + "]"


def rewrite(index_text: str, graph: dict[str, dict[str, list[str]]]) -> tuple[str, list[str]]:
    """Replaces the three structural-field lines inside each top-level
    `[crates.NAME]` block, leaving every other line -- including
    `[crates.NAME.features]` sub-tables -- untouched. Returns (new_text,
    names present in the file but absent from the derived graph, or vice
    versa -- surfaced by the caller as a hard failure rather than silently
    skipped)."""
    lines = index_text.splitlines(keepends=True)
    out: list[str] = []
    current: str | None = None
    seen: set[str] = set()

    for line in lines:
        stripped = line.rstrip("\n")
        section_match = SECTION_RE.match(stripped)
        if section_match:
            current = section_match.group(1)
            seen.add(current)
            out.append(line)
            continue
        if SUBSECTION_RE.match(stripped) or stripped.startswith("["):
            current = None
            out.append(line)
            continue

        field_match = FIELD_RE.match(stripped) if current else None
        if field_match and current in graph:
            field = field_match.group(1)
            newline = "\n" if line.endswith("\n") else ""
            out.append(f"{field} = {format_array(graph[current][field])}{newline}")
        else:
            out.append(line)

    missing_from_file = sorted(set(graph) - seen)
    extra_in_file = sorted(seen - set(graph))
    problems = []
    if missing_from_file:
        problems.append(f"workspace crates absent from CRATE-INDEX.toml: {missing_from_file}")
    if extra_in_file:
        problems.append(f"CRATE-INDEX.toml sections for non-workspace-members: {extra_in_file}")
    return "".join(out), problems


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check",
        action="store_true",
        help="exit non-zero if CRATE-INDEX.toml's structural fields differ from the derived Cargo graph",
    )
    args = parser.parse_args()

    graph = derive_graph()
    current_text = INDEX_PATH.read_text(encoding="utf-8")
    new_text, problems = rewrite(current_text, graph)

    if problems:
        for problem in problems:
            print(f"ERROR: {problem}", file=sys.stderr)
        return 1

    if args.check:
        if new_text != current_text:
            print(
                "ERROR: CRATE-INDEX.toml's depends_on/used_by/dev_depends_on fields "
                "are out of date with the Cargo.toml workspace graph.\n"
                "Run the generator to update it:\n"
                "  python3 scripts/generate-crate-index.py",
                file=sys.stderr,
            )
            return 1
        print("OK: CRATE-INDEX.toml dependency-graph fields match the Cargo.toml workspace graph")
        return 0

    if new_text != current_text:
        INDEX_PATH.write_text(new_text, encoding="utf-8")
        print(f"Updated {INDEX_PATH.relative_to(REPO_ROOT)}")
    else:
        print(f"{INDEX_PATH.relative_to(REPO_ROOT)} already matches the Cargo.toml workspace graph")
    return 0


if __name__ == "__main__":
    sys.exit(main())
