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
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import tomllib

LOGGER = logging.getLogger("check-release-versioning")
ROOT_RELEASE_PACKAGE = "."
ROOT_CARGO_PATH = "Cargo.toml"
ROOT_CARGO_JSONPATH = "$.workspace.package.version"
ROOT_LOCK_PATH = "Cargo.lock"
ROOT_LOCK_JSONPATH = "$.package[?(!@.source)].version"
CHANGELOG_PATH = "CHANGELOG.md"
PROSKENION_LOCK_PATH = "crates/theatron/proskenion/Cargo.lock"
PROSKENION_LOCK_PACKAGES = ("koina", "skene")
RELEASE_VERSION_PATHS = (
    ".release-please-manifest.json",
    ROOT_LOCK_PATH,
    ROOT_CARGO_PATH,
    PROSKENION_LOCK_PATH,
)
RELEASE_TRANSITION_PATHS = tuple(
    sorted((*RELEASE_VERSION_PATHS, CHANGELOG_PATH))
)
PROBE_VERSION = "999.999.999-release-versioning-check"
SEMVER_RE = re.compile(
    r"^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$"
)
COMMIT_SHA_RE = re.compile(r"^[0-9a-f]{40}$")
SECTION_RE = re.compile(r"^\s*\[([^\]]+)\]\s*(?:#.*)?$")
VERSION_LINE_RE = re.compile(r'^(\s*version\s*=\s*)"([^"]*)"([^\r\n]*)(\r?\n)?$')
PACKAGE_HEADER_RE = re.compile(r"(?m)^\s*\[\[package\]\]\s*(?:#.*)?(?:\r?\n|$)")
LOCK_VERSION_RE = re.compile(
    r'(?m)^(\s*version\s*=\s*)"[^"]*"([^\r\n]*)(\r?\n|$)'
)


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

    errors: list[str] = []
    if config.get("draft") is not True:
        errors.append(
            "release-please-config.json: draft must be true so artifacts are "
            "validated before a release becomes public"
        )
    if config.get("force-tag-creation") is not True:
        errors.append(
            "release-please-config.json: force-tag-creation must be true so the "
            "draft release has an exact build ref"
        )

    packages = config.get("packages")
    if not isinstance(packages, dict):
        errors.append("release-please-config.json: missing object at packages")
        return errors

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

    required_updates = (
        (ROOT_CARGO_PATH, ROOT_CARGO_JSONPATH),
        (ROOT_LOCK_PATH, ROOT_LOCK_JSONPATH),
        *(
            (
                PROSKENION_LOCK_PATH,
                f"$.package[?(@.name.value == '{package}')].version",
            )
            for package in PROSKENION_LOCK_PACKAGES
        ),
    )
    for path, jsonpath in required_updates:
        has_required_update = any(
            isinstance(extra_file, dict)
            and extra_file.get("type") == "toml"
            and extra_file.get("path") == path
            and extra_file.get("jsonpath") == jsonpath
            for extra_file in extra_files
        )
        if not has_required_update:
            errors.append(
                "release-please-config.json: packages['.'].extra-files must update "
                f"{path} at {jsonpath}"
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


def check_release_lock_versions(repo_root: Path, expected_version: str) -> list[str]:
    errors: list[str] = []
    for relative, selected_names in (
        (ROOT_LOCK_PATH, None),
        (PROSKENION_LOCK_PATH, set(PROSKENION_LOCK_PACKAGES)),
    ):
        path = repo_root / relative
        try:
            lock = load_toml(path)
        except OSError as exc:
            errors.append(f"{relative}: failed to read: {exc}")
            continue
        except tomllib.TOMLDecodeError as exc:
            errors.append(f"{relative}: invalid TOML: {exc}")
            continue
        packages = lock.get("package")
        if not isinstance(packages, list):
            errors.append(f"{relative}: package inventory must be an array")
            continue
        selected = [
            package
            for package in packages
            if isinstance(package, dict)
            and (
                "source" not in package
                if selected_names is None
                else package.get("name") in selected_names
            )
        ]
        if not selected:
            errors.append(f"{relative}: release-owned lock packages are missing")
            continue
        if selected_names is not None:
            observed_names = [package.get("name") for package in selected]
            if sorted(observed_names) != sorted(selected_names):
                errors.append(
                    f"{relative}: expected one lock package for each of "
                    f"{sorted(selected_names)!r}, found {observed_names!r}"
                )
        for package in selected:
            if package.get("version") != expected_version:
                errors.append(
                    f"{relative}: package {package.get('name')!r} version "
                    f"{package.get('version')!r} does not match workspace "
                    f"version {expected_version!r}"
                )
    return errors


def validate_static_release_metadata(
    repo_root: Path, require_manifest_alignment: bool
) -> CheckReport:
    errors, package_count = check_workspace_members(repo_root)

    version, _workspace, version_errors = workspace_version(repo_root)
    errors.extend(version_errors)
    errors.extend(check_release_please_config(repo_root))
    if version is not None:
        errors.extend(check_release_lock_versions(repo_root, version))
    if require_manifest_alignment and version is not None:
        errors.extend(check_release_please_manifest(repo_root, version))

    return CheckReport(errors=errors, workspace_package_count=package_count)


def copy_release_metadata(src_root: Path, dst_root: Path) -> list[str]:
    errors: list[str] = []
    for relative in (
        ROOT_CARGO_PATH,
        ROOT_LOCK_PATH,
        PROSKENION_LOCK_PATH,
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


def check_release_transition(base_root: Path, candidate_root: Path) -> list[str]:
    """Prove a Release Please head changed only release-owned metadata."""

    errors: list[str] = []
    base_version, _base_workspace, base_errors = workspace_version(base_root)
    candidate_version, _candidate_workspace, candidate_errors = workspace_version(
        candidate_root
    )
    errors.extend(f"base {error}" for error in base_errors)
    errors.extend(f"candidate {error}" for error in candidate_errors)
    if base_version is None or candidate_version is None:
        return errors
    if base_version == candidate_version:
        errors.append("release candidate did not change the workspace version")
        return errors

    with tempfile.TemporaryDirectory(prefix="aletheia-release-transition-") as tmp_str:
        expected_root = Path(tmp_str)
        copy_errors = copy_release_metadata(base_root, expected_root)
        errors.extend(f"base {error}" for error in copy_errors)
        if not copy_errors:
            try:
                bump_version(expected_root, candidate_version)
            except ReleaseVersioningError as error:
                errors.append(f"cannot render expected release metadata: {error}")
            else:
                for relative in RELEASE_VERSION_PATHS:
                    expected = expected_root / relative
                    candidate = candidate_root / relative
                    try:
                        matches = expected.read_bytes() == candidate.read_bytes()
                    except OSError as error:
                        errors.append(f"{relative}: cannot compare transition: {error}")
                        continue
                    if not matches:
                        errors.append(
                            f"{relative}: release candidate differs beyond the "
                            "canonical version update"
                        )

    try:
        base_changelog = (base_root / CHANGELOG_PATH).read_text(encoding="utf-8")
        candidate_changelog = (candidate_root / CHANGELOG_PATH).read_text(
            encoding="utf-8"
        )
    except (OSError, UnicodeError) as error:
        errors.append(f"{CHANGELOG_PATH}: cannot compare transition: {error}")
        return errors

    header = "# Changelog\n\n"
    if not base_changelog.startswith(header) or not candidate_changelog.startswith(
        header
    ):
        errors.append(f"{CHANGELOG_PATH}: canonical header is missing")
        return errors
    base_history = base_changelog[len(header) :]
    candidate_body = candidate_changelog[len(header) :]
    if not base_history or not candidate_body.endswith(base_history):
        errors.append(
            f"{CHANGELOG_PATH}: release candidate must preserve the complete prior history"
        )
        return errors
    new_section = candidate_body[: -len(base_history)]
    expected_heading = (
        f"## [{candidate_version}](https://github.com/forkwright/aletheia/compare/"
        f"v{base_version}...v{candidate_version})"
    )
    if not new_section.startswith(expected_heading) or not new_section.strip():
        errors.append(
            f"{CHANGELOG_PATH}: release candidate lacks the expected {base_version} -> "
            f"{candidate_version} section"
        )
    return errors


def check_release_comparison(
    comparison: Any, base_sha: str, candidate_sha: str
) -> list[str]:
    """Validate GitHub's immutable comparison for one Release Please commit."""

    errors: list[str] = []
    if not COMMIT_SHA_RE.fullmatch(base_sha):
        errors.append("comparison base SHA must be lowercase 40-hex")
    if not COMMIT_SHA_RE.fullmatch(candidate_sha):
        errors.append("comparison candidate SHA must be lowercase 40-hex")
    if errors:
        return errors
    if not isinstance(comparison, dict):
        return ["release comparison must be a JSON object"]

    if comparison.get("status") != "ahead":
        errors.append("release candidate must be ahead of the current main commit")
    for key, expected in (("ahead_by", 1), ("behind_by", 0), ("total_commits", 1)):
        value = comparison.get(key)
        if type(value) is not int or value != expected:
            errors.append(f"release comparison {key} must equal {expected}")

    base_commit = comparison.get("base_commit")
    if not isinstance(base_commit, dict) or base_commit.get("sha") != base_sha:
        errors.append("release comparison base_commit does not match the trusted main SHA")
    merge_base = comparison.get("merge_base_commit")
    if not isinstance(merge_base, dict) or merge_base.get("sha") != base_sha:
        errors.append("release comparison merge base does not match the trusted main SHA")
    commits = comparison.get("commits")
    if (
        not isinstance(commits, list)
        or len(commits) != 1
        or not isinstance(commits[0], dict)
        or commits[0].get("sha") != candidate_sha
    ):
        errors.append("release comparison must contain only the expected candidate commit")

    files = comparison.get("files")
    if not isinstance(files, list):
        errors.append("release comparison files must be a JSON array")
        return errors
    if len(files) != len(RELEASE_TRANSITION_PATHS):
        errors.append(
            "release comparison must contain exactly "
            f"{len(RELEASE_TRANSITION_PATHS)} changed files"
        )

    filenames: list[str] = []
    for index, item in enumerate(files):
        if not isinstance(item, dict):
            errors.append(f"release comparison file {index} must be a JSON object")
            continue
        filename = item.get("filename")
        if not isinstance(filename, str):
            errors.append(f"release comparison file {index} lacks a string filename")
        else:
            filenames.append(filename)
        if item.get("status") != "modified" or "previous_filename" in item:
            errors.append(
                "release comparison files must be modified in place, never added, "
                "removed, copied, or renamed"
            )
    if sorted(filenames) != list(RELEASE_TRANSITION_PATHS):
        errors.append(
            "release comparison changed paths must be exactly: "
            + ", ".join(RELEASE_TRANSITION_PATHS)
        )
    return errors


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


def render_lock_versions(
    path: Path, version: str, selected_names: set[str] | None
) -> str:
    try:
        text = path.read_text(encoding="utf-8")
        lock = tomllib.loads(text)
    except (OSError, tomllib.TOMLDecodeError) as exc:
        raise ReleaseVersioningError(f"{path}: failed to load: {exc}") from exc
    packages = lock.get("package")
    if not isinstance(packages, list):
        raise ReleaseVersioningError(f"{path}: package inventory must be an array")
    headers = list(PACKAGE_HEADER_RE.finditer(text))
    if len(headers) != len(packages):
        raise ReleaseVersioningError(
            f"{path}: parsed {len(packages)} packages but found "
            f"{len(headers)} package sections"
        )

    selected_indexes = [
        index
        for index, package in enumerate(packages)
        if isinstance(package, dict)
        and (
            "source" not in package
            if selected_names is None
            else package.get("name") in selected_names
        )
    ]
    if not selected_indexes:
        raise ReleaseVersioningError(f"{path}: release-owned lock packages are missing")
    if selected_names is not None:
        observed = [packages[index].get("name") for index in selected_indexes]
        if sorted(observed) != sorted(selected_names):
            raise ReleaseVersioningError(
                f"{path}: expected one package for each {sorted(selected_names)!r}, "
                f"found {observed!r}"
            )

    selected_set = set(selected_indexes)
    chunks = [text[: headers[0].start()]]
    for index, header in enumerate(headers):
        end = headers[index + 1].start() if index + 1 < len(headers) else len(text)
        block = text[header.start() : end]
        if index in selected_set:
            if len(LOCK_VERSION_RE.findall(block)) != 1:
                raise ReleaseVersioningError(
                    f"{path}: package {packages[index].get('name')!r} must have "
                    "exactly one version line"
                )
            block = LOCK_VERSION_RE.sub(
                lambda match: (
                    f'{match.group(1)}"{version}"'
                    f"{match.group(2)}{match.group(3) or ''}"
                ),
                block,
                count=1,
            )
        chunks.append(block)
    return "".join(chunks)


def validate_version(version: str) -> None:
    if not SEMVER_RE.fullmatch(version):
        raise ReleaseVersioningError(f"invalid version format: {version}")


def version_from_tag(tag: str) -> str:
    if not tag.startswith("v"):
        raise ReleaseVersioningError(
            f"release tag must start with 'v', received {tag!r}"
        )
    version = tag[1:]
    validate_version(version)
    return version


def contained_release_binary(repo_root: Path, binary: Path) -> tuple[Path | None, str | None]:
    """Resolve an executable candidate without permitting a repository escape."""
    try:
        root = repo_root.resolve(strict=True)
        lexical = binary if binary.is_absolute() else root / binary
        lexical = lexical.absolute()
        resolved = lexical.resolve(strict=True)
    except OSError as exc:
        return None, f"release binary {binary}: cannot be resolved: {exc}"
    if resolved == root or not resolved.is_relative_to(root):
        return None, f"release binary {binary}: path escapes the repository"
    if resolved != lexical:
        return None, f"release binary {binary}: symlinks are not permitted"
    if not resolved.is_file():
        return None, f"release binary {binary}: path is not a regular file"
    return resolved, None


def check_release_identity(
    repo_root: Path, tag: str, binary: Path | None = None
) -> list[str]:
    errors = validate_static_release_metadata(
        repo_root, require_manifest_alignment=True
    ).errors
    try:
        tag_version = version_from_tag(tag)
    except ReleaseVersioningError as exc:
        errors.append(str(exc))
        return errors

    workspace_release, _workspace, version_errors = workspace_version(repo_root)
    errors.extend(version_errors)
    if workspace_release is not None and tag_version != workspace_release:
        errors.append(
            f"release tag version {tag_version!r} does not match workspace "
            f"version {workspace_release!r}"
        )

    if binary is None or errors:
        return errors

    resolved_binary, binary_error = contained_release_binary(repo_root, binary)
    if binary_error is not None or resolved_binary is None:
        errors.append(binary_error or f"release binary {binary}: invalid path")
        return errors

    try:
        result = subprocess.run(
            [str(resolved_binary), "--version"],
            cwd=repo_root,
            text=True,
            capture_output=True,
            check=False,
            timeout=30,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        errors.append(f"release binary {binary}: failed to read version: {exc}")
        return errors

    if result.returncode != 0:
        detail = (result.stderr or result.stdout).strip()
        errors.append(
            f"release binary {binary}: --version exited {result.returncode}"
            + (f": {detail}" if detail else "")
        )
        return errors

    observed = result.stdout.strip()
    expected = f"aletheia {tag_version}"
    if observed != expected:
        errors.append(
            f"release binary {binary}: --version returned {observed!r}, "
            f"expected {expected!r}"
        )
    return errors


def bump_version(repo_root: Path, version: str) -> None:
    validate_version(version)

    report = validate_static_release_metadata(
        repo_root, require_manifest_alignment=False
    )
    if report.errors:
        raise ReleaseVersioningError("; ".join(report.errors))

    root_lock = render_lock_versions(repo_root / ROOT_LOCK_PATH, version, None)
    proskenion_lock = render_lock_versions(
        repo_root / PROSKENION_LOCK_PATH,
        version,
        set(PROSKENION_LOCK_PACKAGES),
    )

    replace_workspace_version_line(repo_root / ROOT_CARGO_PATH, version)
    update_release_please_manifest(repo_root, version)
    try:
        (repo_root / ROOT_LOCK_PATH).write_text(root_lock, encoding="utf-8")
        (repo_root / PROSKENION_LOCK_PATH).write_text(
            proskenion_lock, encoding="utf-8"
        )
    except OSError as exc:
        raise ReleaseVersioningError(f"failed to write release lockfiles: {exc}") from exc

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

    verify_parser = subcommands.add_parser(
        "verify-release",
        help="verify the tag, release metadata, and optional binary agree",
    )
    verify_parser.add_argument("--tag", required=True, help="release tag (vX.Y.Z)")
    verify_parser.add_argument(
        "--binary",
        type=Path,
        help="built aletheia binary whose --version output must match the tag",
    )

    transition_parser = subcommands.add_parser(
        "verify-transition",
        help="verify a Release Please candidate is a canonical metadata-only update",
    )
    transition_parser.add_argument("--base-root", type=Path, required=True)
    transition_parser.add_argument("--candidate-root", type=Path, required=True)

    comparison_parser = subcommands.add_parser(
        "verify-comparison",
        help="verify the immutable GitHub comparison for a Release Please candidate",
    )
    comparison_parser.add_argument("--base-sha", required=True)
    comparison_parser.add_argument("--candidate-sha", required=True)

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

    if command == "verify-release":
        errors = check_release_identity(repo_root, args.tag, args.binary)
        if errors:
            LOGGER.error("release identity check failed:")
            for error in errors:
                LOGGER.error("  - %s", error)
            return 1
        LOGGER.info("Release identity matches %s", args.tag)
        return 0

    if command == "verify-transition":
        errors = check_release_transition(
            args.base_root.resolve(), args.candidate_root.resolve()
        )
        if errors:
            LOGGER.error("release transition check failed:")
            for error in errors:
                LOGGER.error("  - %s", error)
            return 1
        LOGGER.info("Release Please transition is a canonical metadata-only update")
        return 0

    if command == "verify-comparison":
        try:
            comparison = json.load(sys.stdin)
        except (UnicodeError, json.JSONDecodeError) as exc:
            LOGGER.error("release comparison is not valid JSON: %s", exc)
            return 1
        errors = check_release_comparison(
            comparison, args.base_sha, args.candidate_sha
        )
        if errors:
            LOGGER.error("release comparison check failed:")
            for error in errors:
                LOGGER.error("  - %s", error)
            return 1
        LOGGER.info("Release Please comparison is an exact one-commit metadata update")
        return 0

    LOGGER.error("error: unknown command %s", command)
    return 1


if __name__ == "__main__":
    logging.basicConfig(format="%(message)s", level=logging.INFO, stream=sys.stderr)
    raise SystemExit(main())
