#!/usr/bin/env python3
"""Guard and update Aletheia's release version ownership."""

from __future__ import annotations

import argparse
import json
import logging
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
from dataclasses import dataclass
from pathlib import Path


LOGGER = logging.getLogger("check-release-versioning")
ROOT_RELEASE_PACKAGE = "."
ROOT_CARGO_PATH = "Cargo.toml"
ROOT_CARGO_JSONPATH = "$.workspace.package.version"
PROBE_VERSION = "999.999.999-release-versioning-check"
SEMVER_RE = re.compile(
    r"^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$"
)
SECTION_RE = re.compile(r"^\s*\[([^\]]+)\]\s*(?:#.*)?$")
VERSION_LINE_RE = re.compile(r'^(\s*version\s*=\s*)"([^"]*)"([^\r\n]*)(\r?\n)?$')


@dataclass
class CheckReport:
    errors: list[str]
    workspace_package_count: int


class ReleaseVersioningError(RuntimeError):
    """Raised when release version metadata cannot be updated safely."""


def load_toml(path: Path) -> dict:
    with path.open("rb") as fh:
        return tomllib.load(fh)


def load_json(path: Path) -> object:
    with path.open(encoding="utf-8") as fh:
        return json.load(fh)


def workspace_member_manifest_paths(
    repo_root: Path, workspace: dict
) -> tuple[list[Path], list[str]]:
    members = workspace.get("members")
    if not isinstance(members, list) or not all(
        isinstance(member, str) for member in members
    ):
        return [], ["Cargo.toml: [workspace].members must be a list of strings"]

    excludes = workspace.get("exclude", [])
    if not isinstance(excludes, list) or not all(
        isinstance(exclude, str) for exclude in excludes
    ):
        return [], [
            "Cargo.toml: [workspace].exclude must be a list of strings when present"
        ]
    excluded = set(excludes)

    errors: list[str] = []
    manifests: set[Path] = set()
    for member in members:
        if member in excluded:
            continue

        matched_dirs = (
            sorted(repo_root.glob(member))
            if any(ch in member for ch in "*?[")
            else [repo_root / member]
        )
        if not matched_dirs:
            errors.append(
                f"Cargo.toml: workspace member pattern {member!r} matched no paths"
            )
            continue

        for member_dir in matched_dirs:
            relative_dir = member_dir.relative_to(repo_root).as_posix()
            if relative_dir in excluded:
                continue

            manifest = member_dir / "Cargo.toml"
            if not manifest.is_file():
                errors.append(
                    f"Cargo.toml: workspace member {relative_dir!r} has no Cargo.toml"
                )
                continue
            manifests.add(manifest)

    return sorted(manifests), errors


def workspace_version(repo_root: Path) -> tuple[str | None, dict | None, list[str]]:
    cargo_path = repo_root / ROOT_CARGO_PATH
    try:
        cargo = load_toml(cargo_path)
    except OSError as exc:
        return None, None, [f"{ROOT_CARGO_PATH}: failed to read: {exc}"]
    except tomllib.TOMLDecodeError as exc:
        return None, None, [f"{ROOT_CARGO_PATH}: invalid TOML: {exc}"]

    workspace = cargo.get("workspace")
    if not isinstance(workspace, dict):
        return None, None, [f"{ROOT_CARGO_PATH}: missing [workspace] table"]

    package = workspace.get("package")
    if not isinstance(package, dict):
        return None, workspace, [
            f"{ROOT_CARGO_PATH}: missing [workspace.package] table"
        ]

    version = package.get("version")
    if not isinstance(version, str) or not version:
        return None, workspace, [
            f"{ROOT_CARGO_PATH}: [workspace.package].version must be a string"
        ]

    return version, workspace, []


def check_workspace_members(repo_root: Path) -> tuple[list[str], int]:
    version, workspace, errors = workspace_version(repo_root)
    if version is None or workspace is None:
        return errors, 0

    manifests, manifest_errors = workspace_member_manifest_paths(repo_root, workspace)
    errors.extend(manifest_errors)

    package_count = 0
    for manifest in manifests:
        relative = manifest.relative_to(repo_root).as_posix()
        try:
            data = load_toml(manifest)
        except OSError as exc:
            errors.append(f"{relative}: failed to read: {exc}")
            continue
        except tomllib.TOMLDecodeError as exc:
            errors.append(f"{relative}: invalid TOML: {exc}")
            continue

        package = data.get("package")
        if not isinstance(package, dict):
            continue

        package_count += 1
        package_name = package.get("name", "<unknown>")
        package_version = package.get("version")
        if package_version != {"workspace": True}:
            if isinstance(package_version, str):
                detail = f"declares hardcoded version {package_version!r}"
            else:
                detail = f"declares version metadata {package_version!r}"
            errors.append(
                f"{relative}: package {package_name!r} {detail}; use version.workspace = true"
            )

    return errors, package_count


def check_release_please_config(repo_root: Path) -> list[str]:
    path = repo_root / "release-please-config.json"
    try:
        config = load_json(path)
    except OSError as exc:
        return [f"release-please-config.json: failed to read: {exc}"]
    except json.JSONDecodeError as exc:
        return [f"release-please-config.json: invalid JSON: {exc}"]

    if not isinstance(config, dict):
        return ["release-please-config.json: root value must be an object"]

    packages = config.get("packages")
    if not isinstance(packages, dict):
        return ["release-please-config.json: missing object at packages"]

    errors: list[str] = []
    package_keys = set(packages.keys())
    if package_keys != {ROOT_RELEASE_PACKAGE}:
        errors.append(
            "release-please-config.json: packages must contain only the root "
            f"{ROOT_RELEASE_PACKAGE!r} release owner; found {sorted(package_keys)!r}"
        )

    root_package = packages.get(ROOT_RELEASE_PACKAGE)
    if not isinstance(root_package, dict):
        errors.append("release-please-config.json: packages['.'] must be an object")
        return errors

    extra_files = root_package.get("extra-files")
    if not isinstance(extra_files, list):
        errors.append(
            "release-please-config.json: packages['.'].extra-files must be a list"
        )
        return errors

    required_update = {
        "type": "toml",
        "path": ROOT_CARGO_PATH,
        "jsonpath": ROOT_CARGO_JSONPATH,
    }
    has_required_update = any(
        isinstance(extra_file, dict)
        and all(extra_file.get(key) == value for key, value in required_update.items())
        for extra_file in extra_files
    )
    if not has_required_update:
        errors.append(
            "release-please-config.json: packages['.'].extra-files must update "
            f"{ROOT_CARGO_PATH} at {ROOT_CARGO_JSONPATH}"
        )

    return errors


def check_release_please_manifest(repo_root: Path, expected_version: str) -> list[str]:
    path = repo_root / ".release-please-manifest.json"
    try:
        manifest = load_json(path)
    except OSError as exc:
        return [f".release-please-manifest.json: failed to read: {exc}"]
    except json.JSONDecodeError as exc:
        return [f".release-please-manifest.json: invalid JSON: {exc}"]

    if not isinstance(manifest, dict):
        return [".release-please-manifest.json: root value must be an object"]

    errors: list[str] = []
    package_keys = set(manifest.keys())
    if package_keys != {ROOT_RELEASE_PACKAGE}:
        errors.append(
            ".release-please-manifest.json: packages must contain only the root "
            f"{ROOT_RELEASE_PACKAGE!r} release owner; found {sorted(package_keys)!r}"
        )

    manifest_version = manifest.get(ROOT_RELEASE_PACKAGE)
    if manifest_version != expected_version:
        errors.append(
            ".release-please-manifest.json: root version "
            f"{manifest_version!r} does not match workspace version {expected_version!r}"
        )

    return errors


def validate_static_release_metadata(
    repo_root: Path, require_manifest_alignment: bool
) -> CheckReport:
    errors, package_count = check_workspace_members(repo_root)

    version, _workspace, version_errors = workspace_version(repo_root)
    errors.extend(version_errors)
    errors.extend(check_release_please_config(repo_root))
    if require_manifest_alignment and version is not None:
        errors.extend(check_release_please_manifest(repo_root, version))

    return CheckReport(errors=errors, workspace_package_count=package_count)


def copy_release_metadata(src_root: Path, dst_root: Path) -> list[str]:
    errors: list[str] = []
    for relative in (
        ROOT_CARGO_PATH,
        "release-please-config.json",
        ".release-please-manifest.json",
        "scripts/bump-version.sh",
        "scripts/check-release-versioning.py",
    ):
        src = src_root / relative
        dst = dst_root / relative
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(src, dst)

    _version, workspace, version_errors = workspace_version(src_root)
    if workspace is None:
        return version_errors
    errors.extend(version_errors)

    manifests, manifest_errors = workspace_member_manifest_paths(src_root, workspace)
    errors.extend(manifest_errors)
    for manifest in manifests:
        relative = manifest.relative_to(src_root)
        dst = dst_root / relative
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(manifest, dst)

    return errors


def check_bump_tool_probe(repo_root: Path) -> list[str]:
    with tempfile.TemporaryDirectory(prefix="aletheia-release-versioning-") as tmp_str:
        tmp_root = Path(tmp_str)
        errors = copy_release_metadata(repo_root, tmp_root)
        if errors:
            return errors

        try:
            result = subprocess.run(
                [str(tmp_root / "scripts" / "bump-version.sh"), PROBE_VERSION],
                cwd=tmp_root,
                text=True,
                capture_output=True,
                check=False,
            )
        except OSError as exc:
            return [f"scripts/bump-version.sh: probe bump failed to run: {exc}"]

        if result.returncode != 0:
            detail = (result.stderr or result.stdout).strip()
            suffix = f": {detail}" if detail else ""
            return [f"scripts/bump-version.sh: probe bump failed{suffix}"]

        version, _workspace, version_errors = workspace_version(tmp_root)
        errors.extend(version_errors)
        if version != PROBE_VERSION:
            errors.append(
                f"scripts/bump-version.sh: probe left workspace version at {version!r}"
            )

        manifest_errors = check_release_please_manifest(tmp_root, PROBE_VERSION)
        errors.extend(
            f"scripts/bump-version.sh: probe {error}" for error in manifest_errors
        )
        return errors


def check_repo(repo_root: Path, probe_bump_tool: bool = True) -> CheckReport:
    report = validate_static_release_metadata(
        repo_root, require_manifest_alignment=True
    )
    if not report.errors and probe_bump_tool:
        report.errors.extend(check_bump_tool_probe(repo_root))
    return report


def replace_workspace_version_line(cargo_path: Path, version: str) -> None:
    try:
        lines = cargo_path.read_text(encoding="utf-8").splitlines(keepends=True)
    except OSError as exc:
        raise ReleaseVersioningError(f"{cargo_path}: failed to read: {exc}") from exc

    inside_workspace_package = False
    for index, line in enumerate(lines):
        section = SECTION_RE.match(line)
        if section:
            inside_workspace_package = section.group(1).strip() == "workspace.package"
            continue

        if not inside_workspace_package:
            continue

        version_line = VERSION_LINE_RE.match(line)
        if version_line:
            lines[index] = (
                f'{version_line.group(1)}"{version}"'
                f"{version_line.group(3)}{version_line.group(4) or ''}"
            )
            try:
                cargo_path.write_text("".join(lines), encoding="utf-8")
            except OSError as exc:
                raise ReleaseVersioningError(
                    f"{cargo_path}: failed to write: {exc}"
                ) from exc
            return

    raise ReleaseVersioningError(
        f"{cargo_path}: could not find {ROOT_CARGO_JSONPATH} to update"
    )


def update_release_please_manifest(repo_root: Path, version: str) -> None:
    path = repo_root / ".release-please-manifest.json"
    try:
        manifest = load_json(path)
    except (OSError, json.JSONDecodeError) as exc:
        raise ReleaseVersioningError(
            f".release-please-manifest.json: failed to load: {exc}"
        ) from exc

    if not isinstance(manifest, dict):
        raise ReleaseVersioningError(
            ".release-please-manifest.json: root value must be an object"
        )

    package_keys = set(manifest.keys())
    if package_keys != {ROOT_RELEASE_PACKAGE}:
        raise ReleaseVersioningError(
            ".release-please-manifest.json: expected only root release owner "
            f"{ROOT_RELEASE_PACKAGE!r}, found {sorted(package_keys)!r}"
        )

    manifest[ROOT_RELEASE_PACKAGE] = version
    try:
        path.write_text(
            json.dumps(manifest, sort_keys=True, separators=(",", ":")) + "\n",
            encoding="utf-8",
        )
    except OSError as exc:
        raise ReleaseVersioningError(
            f".release-please-manifest.json: failed to write: {exc}"
        ) from exc


def validate_version(version: str) -> None:
    if not SEMVER_RE.fullmatch(version):
        raise ReleaseVersioningError(f"invalid version format: {version}")


def bump_version(repo_root: Path, version: str) -> None:
    validate_version(version)

    report = validate_static_release_metadata(
        repo_root, require_manifest_alignment=False
    )
    if report.errors:
        raise ReleaseVersioningError("; ".join(report.errors))

    replace_workspace_version_line(repo_root / ROOT_CARGO_PATH, version)
    update_release_please_manifest(repo_root, version)

    final_report = validate_static_release_metadata(
        repo_root, require_manifest_alignment=True
    )
    if final_report.errors:
        raise ReleaseVersioningError("; ".join(final_report.errors))


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Check or update release version ownership metadata."
    )
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=Path(__file__).resolve().parents[1],
        help="repository root to inspect",
    )
    subcommands = parser.add_subparsers(dest="command")

    subcommands.add_parser("check", help="verify release version metadata")
    bump_parser = subcommands.add_parser("bump", help="update owned release versions")
    bump_parser.add_argument("version", help="new semantic version")

    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    command = args.command or "check"
    repo_root = args.repo_root.resolve()

    if command == "check":
        report = check_repo(repo_root)
        if report.errors:
            LOGGER.error("release versioning check failed:")
            for error in report.errors:
                LOGGER.error("  - %s", error)
            return 1

        LOGGER.info(
            "release versioning check passed for %s workspace packages",
            report.workspace_package_count,
        )
        return 0

    if command == "bump":
        try:
            bump_version(repo_root, args.version)
        except ReleaseVersioningError as exc:
            LOGGER.error("error: %s", exc)
            return 1

        LOGGER.info("Bumped workspace version to %s", args.version)
        LOGGER.info(
            "Verify: scripts/check-release-versioning.py && "
            "cargo metadata --format-version 1 | jq "
            "'.packages[] | select(.name | startswith(\"aletheia\")) | .version'"
        )
        return 0

    LOGGER.error("error: unknown command %s", command)
    return 1


if __name__ == "__main__":
    logging.basicConfig(format="%(message)s", level=logging.INFO, stream=sys.stderr)
    raise SystemExit(main())
