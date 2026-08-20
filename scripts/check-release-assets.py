#!/usr/bin/env python3
"""Validate the complete staged Aletheia release asset set."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path

SEMVER_TAG_RE = re.compile(
    r"^v(?P<version>[0-9]+\.[0-9]+\.[0-9]+"
    r"(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?)$"
)
PLATFORM_ARTIFACTS = (
    "aletheia-linux-x86_64",
    "aletheia-macos-aarch64",
)
WORKSPACE_SBOMS = (
    "aletheia-sbom.spdx.json",
    "bom.cdx.json",
)


def expected_assets(tag: str) -> set[str]:
    match = SEMVER_TAG_RE.fullmatch(tag)
    if match is None:
        raise ValueError(f"release tag must have vX.Y.Z form, received {tag!r}")
    version = match.group("version")

    expected = set(WORKSPACE_SBOMS)
    for artifact in PLATFORM_ARTIFACTS:
        binary = f"{artifact}-{version}"
        tarball = f"{binary}.tar.gz"
        expected.update(
            {
                binary,
                f"{binary}.sha256",
                tarball,
                f"{tarball}.sha256",
                f"{binary}.cdx.json",
                f"{binary}.spdx.json",
                f"{binary}.provenance.intoto.jsonl",
                f"{binary}.cdx.intoto.jsonl",
                f"{binary}.spdx.intoto.jsonl",
            }
        )
    return expected


def _check_checksum(directory: Path, checksum_path: Path) -> list[str]:
    errors: list[str] = []
    try:
        line = checksum_path.read_text(encoding="utf-8").strip()
    except OSError as exc:
        return [f"{checksum_path.name}: failed to read checksum: {exc}"]

    fields = line.split(maxsplit=1)
    if (
        len(fields) != 2
        or len(fields[0]) != 64
        or any(character not in "0123456789abcdef" for character in fields[0])
    ):
        return [f"{checksum_path.name}: malformed SHA-256 record"]
    digest, recorded_name = fields
    recorded_name = recorded_name.removeprefix("*")

    subject_name = checksum_path.name.removesuffix(".sha256")
    if recorded_name != subject_name:
        errors.append(
            f"{checksum_path.name}: names {recorded_name!r}, expected {subject_name!r}"
        )
        return errors

    subject = directory / subject_name
    try:
        observed = hashlib.sha256(subject.read_bytes()).hexdigest()
    except OSError as exc:
        errors.append(f"{subject_name}: failed to hash: {exc}")
        return errors
    if observed != digest:
        errors.append(
            f"{checksum_path.name}: digest {digest} does not match {observed}"
        )
    return errors


def _check_sbom(path: Path, version: str) -> list[str]:
    try:
        with path.open(encoding="utf-8") as handle:
            document = json.load(handle)
    except (OSError, json.JSONDecodeError) as exc:
        return [f"{path.name}: invalid JSON: {exc}"]
    if not isinstance(document, dict):
        return [f"{path.name}: SBOM root must be an object"]
    if path.name.endswith((".cdx.json", "bom.cdx.json")):
        errors: list[str] = []
        if document.get("bomFormat") != "CycloneDX":
            return [f"{path.name}: missing CycloneDX bomFormat"]
        if not isinstance(document.get("specVersion"), str):
            return [f"{path.name}: missing CycloneDX specVersion"]
        if not isinstance(document.get("serialNumber"), str):
            return [f"{path.name}: missing CycloneDX serialNumber"]
        components = document.get("components")
        if not isinstance(components, list) or not components:
            errors.append(f"{path.name}: CycloneDX components are empty")
            components = []
        metadata = document.get("metadata")
        candidates = list(components)
        if isinstance(metadata, dict) and isinstance(metadata.get("component"), dict):
            candidates.append(metadata["component"])
        roots = [
            component
            for component in candidates
            if isinstance(component, dict)
            and component.get("name") == "aletheia"
            and component.get("version") == version
        ]
        if not roots:
            errors.append(
                f"{path.name}: CycloneDX does not identify aletheia {version}"
            )
        dependencies = [
            component
            for component in components
            if isinstance(component, dict)
            and not (
                component.get("name") == "aletheia"
                and component.get("version") == version
            )
        ]
        if not dependencies:
            errors.append(f"{path.name}: CycloneDX has no non-root dependencies")
        root_refs = {
            component.get("bom-ref")
            for component in roots
            if isinstance(component.get("bom-ref"), str)
        }
        dependency_refs = {
            component.get("bom-ref")
            for component in dependencies
            if isinstance(component.get("bom-ref"), str)
        }
        graph = document.get("dependencies")
        graph_proves_edge = isinstance(graph, list) and any(
            isinstance(edge, dict)
            and edge.get("ref") in root_refs
            and isinstance(edge.get("dependsOn"), list)
            and bool(set(edge["dependsOn"]) & dependency_refs)
            for edge in graph
        )
        if not root_refs or not dependency_refs or not graph_proves_edge:
            errors.append(
                f"{path.name}: CycloneDX lacks a root-to-dependency graph edge"
            )
        return errors
    if document.get("spdxVersion") != "SPDX-2.3":
        return [f"{path.name}: expected SPDX-2.3 document"]
    if document.get("SPDXID") != "SPDXRef-DOCUMENT":
        return [f"{path.name}: missing SPDX document identity"]
    packages = document.get("packages")
    if not isinstance(packages, list) or not packages:
        return [f"{path.name}: SPDX packages are empty"]
    roots = [
        package
        for package in packages
        if isinstance(package, dict)
        and package.get("name") == "aletheia"
        and package.get("versionInfo") == version
    ]
    errors = []
    if not roots:
        errors.append(f"{path.name}: SPDX does not identify aletheia {version}")
    dependencies = [package for package in packages if package not in roots]
    if not dependencies:
        errors.append(f"{path.name}: SPDX has no non-root dependencies")
    root_ids = {
        package.get("SPDXID")
        for package in roots
        if isinstance(package.get("SPDXID"), str)
    }
    dependency_ids = {
        package.get("SPDXID")
        for package in dependencies
        if isinstance(package, dict) and isinstance(package.get("SPDXID"), str)
    }
    relationships = document.get("relationships")
    graph_proves_edge = isinstance(relationships, list) and any(
        isinstance(relationship, dict)
        and (
            (
                relationship.get("spdxElementId") in root_ids
                and relationship.get("relationshipType") == "DEPENDS_ON"
                and relationship.get("relatedSpdxElement") in dependency_ids
            )
            or (
                relationship.get("spdxElementId") in dependency_ids
                and relationship.get("relationshipType") == "DEPENDENCY_OF"
                and relationship.get("relatedSpdxElement") in root_ids
            )
        )
        for relationship in relationships
    )
    if not root_ids or not dependency_ids or not graph_proves_edge:
        errors.append(f"{path.name}: SPDX lacks a root-to-dependency graph edge")
    return errors


def _check_json_lines(path: Path) -> list[str]:
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except OSError as exc:
        return [f"{path.name}: failed to read JSONL: {exc}"]
    if not lines:
        return [f"{path.name}: attestation bundle is empty"]

    errors: list[str] = []
    for line_number, line in enumerate(lines, start=1):
        try:
            json.loads(line)
        except json.JSONDecodeError as exc:
            errors.append(f"{path.name}:{line_number}: invalid JSON: {exc}")
    return errors


def check_assets(directory: Path, tag: str) -> list[str]:
    try:
        expected = expected_assets(tag)
        version = SEMVER_TAG_RE.fullmatch(tag).group("version")
    except ValueError as exc:
        return [str(exc)]

    if not directory.is_dir():
        return [f"release asset directory does not exist: {directory}"]

    actual = {path.name for path in directory.iterdir() if path.is_file()}
    errors = [f"missing release asset: {name}" for name in sorted(expected - actual)]
    errors.extend(
        f"unexpected release asset: {name}" for name in sorted(actual - expected)
    )

    for name in sorted(expected & actual):
        path = directory / name
        if path.is_symlink():
            errors.append(f"{name}: symlinks are not release assets")
            continue
        if path.stat().st_size == 0:
            errors.append(f"{name}: release asset is empty")
            continue
        if name.endswith(".sha256"):
            errors.extend(_check_checksum(directory, path))
        elif name.endswith(".jsonl"):
            errors.extend(_check_json_lines(path))
        elif name.endswith(".json"):
            errors.extend(_check_sbom(path, version))
    return errors


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--tag", required=True)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    errors = check_assets(args.directory.resolve(), args.tag)
    if errors:
        for error in errors:
            print(f"release-assets: {error}", file=sys.stderr)
        return 1
    print(f"release-assets: complete ({len(expected_assets(args.tag))} assets)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
