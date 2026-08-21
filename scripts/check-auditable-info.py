#!/usr/bin/env python3
"""Validate decoded cargo-auditable dependency data for a release binary."""

from __future__ import annotations

import argparse
import json
import sys
from collections import Counter
from pathlib import Path


def _contained_cli_json(value: str) -> object:
    """Decode one JSON file beneath the invocation directory."""
    allowed_root = Path.cwd().resolve(strict=True)
    try:
        candidate = (allowed_root / value).resolve(strict=True)
    except OSError as exc:
        raise argparse.ArgumentTypeError(f"invalid file {value!r}: {exc}") from exc
    if allowed_root not in candidate.parents:
        raise argparse.ArgumentTypeError(
            f"{value!r} resolves outside invocation directory {allowed_root}"
        )
    if not candidate.is_file():
        raise argparse.ArgumentTypeError(f"{value!r} is not a regular file")

    handle = argparse.FileType("r", encoding="utf-8")(str(candidate))
    try:
        return json.load(handle)
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise argparse.ArgumentTypeError(f"unreadable JSON {value!r}: {exc}") from exc
    finally:
        handle.close()


def _sbom_inventory(
    document: object, kind: str
) -> tuple[Counter[tuple[str, str]], list[str]]:
    errors: list[str] = []
    if not isinstance(document, dict):
        return Counter(), [f"{kind} SBOM root must be an object"]
    if kind == "CycloneDX":
        raw_packages = document.get("components")
        if not isinstance(raw_packages, list):
            return Counter(), ["CycloneDX SBOM components must be an array"]
        metadata = document.get("metadata")
        packages = list(raw_packages)
        if isinstance(metadata, dict) and isinstance(metadata.get("component"), dict):
            packages.append(metadata["component"])
        version_key = "version"
    else:
        packages = document.get("packages")
        if not isinstance(packages, list):
            return Counter(), ["SPDX SBOM packages must be an array"]
        version_key = "versionInfo"
    inventory: Counter[tuple[str, str]] = Counter()
    for index, package in enumerate(packages):
        if not isinstance(package, dict):
            errors.append(f"{kind} package {index} must be an object")
            continue
        name = package.get("name")
        version = package.get(version_key)
        if isinstance(name, str) and name and isinstance(version, str) and version:
            inventory[(name, version)] += 1
    return inventory, errors


def check_info(
    value: object,
    expected_version: str,
    cyclonedx: object | None = None,
    spdx: object | None = None,
) -> list[str]:
    errors: list[str] = []
    if not isinstance(value, dict):
        return ["decoded audit data root must be an object"]
    packages = value.get("packages")
    if not isinstance(packages, list) or len(packages) < 2:
        return ["decoded audit data must contain a root and dependencies"]

    roots: list[int] = []
    edges: list[list[int]] = []
    for index, package in enumerate(packages):
        if not isinstance(package, dict):
            errors.append(f"package {index} must be an object")
            edges.append([])
            continue
        for key in ("name", "version"):
            if not isinstance(package.get(key), str) or not package[key]:
                errors.append(f"package {index} has invalid {key}")
        source = package.get("source")
        if not isinstance(source, (str, dict)):
            errors.append(f"package {index} has invalid source")
        kind = package.get("kind", "runtime")
        if kind not in ("runtime", "build"):
            errors.append(f"package {index} has unsupported dependency kind {kind!r}")
        if package.get("root") is True:
            roots.append(index)
        raw_dependencies = package.get("dependencies", [])
        if not isinstance(raw_dependencies, list) or any(
            not isinstance(item, int) or isinstance(item, bool)
            for item in raw_dependencies
        ):
            errors.append(f"package {index} dependencies must be integer indices")
            edges.append([])
            continue
        dependencies = list(raw_dependencies)
        if len(set(dependencies)) != len(dependencies):
            errors.append(f"package {index} has duplicate dependency indices")
        for dependency in dependencies:
            if dependency < 0 or dependency >= len(packages):
                errors.append(
                    f"package {index} dependency index {dependency} is out of range"
                )
        edges.append(
            [dependency for dependency in dependencies if 0 <= dependency < len(packages)]
        )

    if len(roots) != 1:
        errors.append(f"decoded audit data must contain one root, found {len(roots)}")
    else:
        root = packages[roots[0]]
        if isinstance(root, dict):
            if root.get("name") != "aletheia":
                errors.append(f"audit root is {root.get('name')!r}, expected 'aletheia'")
            if root.get("version") != expected_version:
                errors.append(
                    f"audit root version is {root.get('version')!r}, "
                    f"expected {expected_version!r}"
                )

    if len(roots) == 1:
        reachable: set[int] = set()
        pending = [roots[0]]
        while pending:
            node = pending.pop()
            if node in reachable:
                continue
            reachable.add(node)
            pending.extend(edges[node])
        if len(reachable) != len(packages):
            errors.append(
                "decoded dependency graph contains packages unreachable from the root"
            )

    state = [0] * len(edges)

    def visit(node: int) -> None:
        if state[node] == 1:
            errors.append("decoded dependency graph contains a cycle")
            return
        if state[node] == 2:
            return
        state[node] = 1
        for dependency in edges[node]:
            visit(dependency)
        state[node] = 2

    for node in range(len(edges)):
        if state[node] == 0:
            visit(node)

    expected_inventory = Counter(
        (package["name"], package["version"])
        for package in packages
        if isinstance(package, dict)
        and package.get("kind", "runtime") == "runtime"
        and isinstance(package.get("name"), str)
        and isinstance(package.get("version"), str)
    )
    for sbom, kind in ((cyclonedx, "CycloneDX"), (spdx, "SPDX")):
        if sbom is None:
            continue
        inventory, inventory_errors = _sbom_inventory(sbom, kind)
        errors.extend(inventory_errors)
        missing = expected_inventory - inventory
        if missing:
            rendered = ", ".join(
                f"{name}@{version} x{count}"
                for (name, version), count in sorted(missing.items())
            )
            errors.append(
                f"{kind} SBOM omits decoded runtime packages: {rendered}"
            )
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("json", type=_contained_cli_json)
    parser.add_argument("version")
    parser.add_argument("--cyclonedx", type=_contained_cli_json)
    parser.add_argument("--spdx", type=_contained_cli_json)
    args = parser.parse_args()
    errors = check_info(
        args.json,
        args.version,
        cyclonedx=args.cyclonedx,
        spdx=args.spdx,
    )
    if errors:
        for error in errors:
            print(f"auditable-info: {error}", file=sys.stderr)
        return 1
    print("auditable-info: decoded dependency graph verified")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
