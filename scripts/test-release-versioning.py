#!/usr/bin/env python3
"""Behavioral tests for scripts/check-release-versioning.py."""

from __future__ import annotations

import importlib.util
import json
import shutil
import sys
import tempfile
import tomllib
from pathlib import Path


_SCRIPT_PATH = Path(__file__).parent / "check-release-versioning.py"


def _load_checker() -> object:
    spec = importlib.util.spec_from_file_location("release_versioning", _SCRIPT_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"failed to load {_SCRIPT_PATH}")
    module = importlib.util.module_from_spec(spec)
    sys.modules["release_versioning"] = module
    spec.loader.exec_module(module)
    return module


CHECKER = _load_checker()
_FAILURES: list[str] = []


def expect(condition: bool, msg: str) -> None:
    if not condition:
        _FAILURES.append(msg)


def write_fixture_repo(root: Path) -> None:
    (root / "crates" / "app").mkdir(parents=True)
    (root / "crates" / "lib").mkdir(parents=True)
    (root / "scripts").mkdir()
    (root / "Cargo.toml").write_text(
        """\
[workspace]
resolver = "2"
members = [
    "crates/app",
    "crates/lib",
]

[workspace.package]
version = "1.2.3"
edition = "2024"
""",
        encoding="utf-8",
    )
    (root / "crates" / "app" / "Cargo.toml").write_text(
        """\
[package]
name = "fixture-app"
version.workspace = true
edition.workspace = true
publish = false
""",
        encoding="utf-8",
    )
    (root / "crates" / "lib" / "Cargo.toml").write_text(
        """\
[package]
name = "fixture-lib"
version.workspace = true
edition.workspace = true
publish = false
""",
        encoding="utf-8",
    )
    (root / "release-please-config.json").write_text(
        json.dumps(
            {
                "release-type": "simple",
                "packages": {
                    ".": {
                        "extra-files": [
                            {
                                "type": "toml",
                                "path": "Cargo.toml",
                                "jsonpath": "$.workspace.package.version",
                            }
                        ]
                    }
                },
            }
        ),
        encoding="utf-8",
    )
    (root / ".release-please-manifest.json").write_text(
        '{".":"1.2.3"}\n',
        encoding="utf-8",
    )
    shutil.copy2(_SCRIPT_PATH, root / "scripts" / "check-release-versioning.py")
    (root / "scripts" / "bump-version.sh").write_text(
        """\
#!/usr/bin/env bash
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
exec python3 "${REPO_ROOT}/scripts/check-release-versioning.py" bump "$@"
""",
        encoding="utf-8",
    )
    (root / "scripts" / "bump-version.sh").chmod(0o755)


def root_version(root: Path) -> str:
    data = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    return data["workspace"]["package"]["version"]


def manifest_version(root: Path) -> str:
    return json.loads((root / ".release-please-manifest.json").read_text())["."]


def test_check_accepts_workspace_version_owner(root: Path) -> None:
    report = CHECKER.check_repo(root)
    expect(not report.errors, f"valid fixture should pass: {report.errors}")
    expect(report.workspace_package_count == 2, "fixture should check two packages")


def test_check_rejects_hardcoded_member_version(root: Path) -> None:
    manifest = root / "crates" / "lib" / "Cargo.toml"
    manifest.write_text(
        """\
[package]
name = "fixture-lib"
version = "0.1.0"
edition.workspace = true
publish = false
""",
        encoding="utf-8",
    )

    report = CHECKER.check_repo(root, probe_bump_tool=False)
    expect(
        any("hardcoded version" in error for error in report.errors),
        f"hardcoded member version should fail: {report.errors}",
    )


def test_check_rejects_release_please_without_workspace_update(root: Path) -> None:
    (root / "release-please-config.json").write_text(
        json.dumps({"release-type": "simple", "packages": {".": {"extra-files": []}}}),
        encoding="utf-8",
    )

    report = CHECKER.check_repo(root, probe_bump_tool=False)
    expect(
        any("$.workspace.package.version" in error for error in report.errors),
        f"missing release-please Cargo.toml updater should fail: {report.errors}",
    )


def test_bump_updates_all_version_owners(root: Path) -> None:
    CHECKER.bump_version(root, "2.0.0")

    expect(root_version(root) == "2.0.0", "bump should update workspace version")
    expect(
        manifest_version(root) == "2.0.0",
        "bump should update release-please manifest",
    )
    member = tomllib.loads(
        (root / "crates" / "lib" / "Cargo.toml").read_text(encoding="utf-8")
    )
    expect(
        member["package"]["version"] == {"workspace": True},
        "bump should leave member crates inheriting the workspace version",
    )


def run_isolated(test_fn: object) -> None:
    with tempfile.TemporaryDirectory() as tmp_str:
        root = Path(tmp_str)
        write_fixture_repo(root)
        test_fn(root)


def main() -> int:
    for test_fn in (
        test_check_accepts_workspace_version_owner,
        test_check_rejects_hardcoded_member_version,
        test_check_rejects_release_please_without_workspace_update,
        test_bump_updates_all_version_owners,
    ):
        run_isolated(test_fn)

    if _FAILURES:
        print(f"FAIL: {len(_FAILURES)} assertion(s) failed", file=sys.stderr)
        for failure in _FAILURES:
            print(f"  - {failure}", file=sys.stderr)
        return 1

    print("OK: all release versioning tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
