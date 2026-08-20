#!/usr/bin/env python3
"""Behavioral tests for scripts/check-release-versioning.py."""

from __future__ import annotations

import importlib.util
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

import tomllib

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
    (root / "crates" / "theatron" / "proskenion").mkdir(parents=True)
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
    (root / "Cargo.lock").write_text(
        """\
version = 4

[[package]]
name = "fixture-app"
version = "1.2.3"

[[package]]
name = "fixture-lib"
version = "1.2.3"

[[package]]
name = "serde"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
""",
        encoding="utf-8",
    )
    (root / "crates" / "theatron" / "proskenion" / "Cargo.lock").write_text(
        """\
version = 4

[[package]]
name = "koina"
version = "1.2.3"

[[package]]
name = "proskenion"
version = "0.13.1"

[[package]]
name = "skene"
version = "1.2.3"
""",
        encoding="utf-8",
    )
    (root / "release-please-config.json").write_text(
        json.dumps(
            {
                "release-type": "simple",
                "draft": True,
                "force-tag-creation": True,
                "packages": {
                    ".": {
                        "extra-files": [
                            {
                                "type": "toml",
                                "path": "Cargo.toml",
                                "jsonpath": "$.workspace.package.version",
                            },
                            {
                                "type": "toml",
                                "path": "Cargo.lock",
                                "jsonpath": "$.package[?(!@.source)].version",
                            },
                            {
                                "type": "toml",
                                "path": "crates/theatron/proskenion/Cargo.lock",
                                "jsonpath": "$.package[?(@.name.value == 'koina')].version",
                            },
                            {
                                "type": "toml",
                                "path": "crates/theatron/proskenion/Cargo.lock",
                                "jsonpath": "$.package[?(@.name.value == 'skene')].version",
                            },
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
    (root / "CHANGELOG.md").write_text(
        "# Changelog\n\n## [1.2.3](https://example.invalid/1.2.3)\n",
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


def lock_versions(root: Path, relative: str) -> dict[str, str]:
    data = tomllib.loads((root / relative).read_text(encoding="utf-8"))
    return {package["name"]: package["version"] for package in data["package"]}


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
        json.dumps(
            {
                "release-type": "simple",
                "draft": True,
                "force-tag-creation": True,
                "packages": {".": {"extra-files": []}},
            }
        ),
        encoding="utf-8",
    )

    report = CHECKER.check_repo(root, probe_bump_tool=False)
    expect(
        any("$.workspace.package.version" in error for error in report.errors),
        f"missing release-please Cargo.toml updater should fail: {report.errors}",
    )


def test_check_rejects_public_release_before_artifacts(root: Path) -> None:
    config_path = root / "release-please-config.json"
    config = json.loads(config_path.read_text(encoding="utf-8"))
    config["draft"] = False
    config["force-tag-creation"] = False
    config_path.write_text(json.dumps(config), encoding="utf-8")

    report = CHECKER.check_repo(root, probe_bump_tool=False)
    expect(
        any("draft must be true" in error for error in report.errors),
        f"public release should fail: {report.errors}",
    )
    expect(
        any("force-tag-creation must be true" in error for error in report.errors),
        f"missing immutable draft tag should fail: {report.errors}",
    )


def test_release_identity_binds_tag_metadata_and_binary(root: Path) -> None:
    binary = root / "aletheia"
    binary.write_text(
        "#!/usr/bin/env sh\nprintf 'aletheia 1.2.3\\n'\n",
        encoding="utf-8",
    )
    binary.chmod(0o755)

    errors = CHECKER.check_release_identity(root, "v1.2.3", binary)
    expect(not errors, f"matching release identity should pass: {errors}")

    errors = CHECKER.check_release_identity(root, "v9.9.9", binary)
    expect(
        any("does not match workspace" in error for error in errors),
        f"tag/workspace mismatch should fail: {errors}",
    )

    binary.write_text(
        "#!/usr/bin/env sh\nprintf 'aletheia 1.2.4\\n'\n",
        encoding="utf-8",
    )
    errors = CHECKER.check_release_identity(root, "v1.2.3", binary)
    expect(
        any("--version returned" in error for error in errors),
        f"binary/version mismatch should fail: {errors}",
    )

    with tempfile.TemporaryDirectory(dir=root.parent) as external_tmp:
        marker = root / "outside-binary-ran"
        outside = Path(external_tmp) / "aletheia"
        outside.write_text(
            "#!/usr/bin/env sh\n"
            f"printf ran > '{marker}'\n"
            "printf 'aletheia 1.2.3\\n'\n",
            encoding="utf-8",
        )
        outside.chmod(0o755)
        for candidate in (outside, root / "outside-link"):
            if candidate != outside:
                candidate.symlink_to(outside)
            errors = CHECKER.check_release_identity(root, "v1.2.3", candidate)
            expect(
                any(
                    fragment in error
                    for error in errors
                    for fragment in ("escapes the repository", "symlinks")
                ),
                f"unsafe release binary path was accepted: {candidate}: {errors}",
            )
            expect(
                not marker.exists(),
                f"unsafe release binary executed before rejection: {candidate}",
            )

    invalid_root = root / "invalid-repository"
    invalid_root.mkdir()
    invalid_marker = root / "invalid-metadata-binary-ran"
    invalid_binary = invalid_root / "aletheia"
    invalid_binary.write_text(
        "#!/usr/bin/env sh\n"
        f"printf ran > '{invalid_marker}'\n"
        "printf 'aletheia 1.2.3\\n'\n",
        encoding="utf-8",
    )
    invalid_binary.chmod(0o755)
    errors = CHECKER.check_release_identity(
        invalid_root, "v1.2.3", invalid_binary
    )
    expect(errors, "invalid repository metadata should fail release identity")
    expect(
        not invalid_marker.exists(),
        "release binary executed despite invalid repository metadata",
    )


def test_bump_updates_all_version_owners(root: Path) -> None:
    CHECKER.bump_version(root, "2.0.0")

    expect(root_version(root) == "2.0.0", "bump should update workspace version")
    expect(
        manifest_version(root) == "2.0.0",
        "bump should update release-please manifest",
    )
    root_lock = lock_versions(root, "Cargo.lock")
    expect(
        root_lock["fixture-app"] == root_lock["fixture-lib"] == "2.0.0",
        "bump should update every source-free root lock package",
    )
    expect(
        root_lock["serde"] == "1.0.0",
        "bump should not update registry packages",
    )
    proskenion_lock = lock_versions(
        root, "crates/theatron/proskenion/Cargo.lock"
    )
    expect(
        proskenion_lock["koina"] == proskenion_lock["skene"] == "2.0.0",
        "bump should update proskenion's two workspace-version path packages",
    )
    expect(
        proskenion_lock["proskenion"] == "0.13.1",
        "bump should preserve proskenion's independent version",
    )
    member = tomllib.loads(
        (root / "crates" / "lib" / "Cargo.toml").read_text(encoding="utf-8")
    )
    expect(
        member["package"]["version"] == {"workspace": True},
        "bump should leave member crates inheriting the workspace version",
    )


def test_check_rejects_stale_lock_version(root: Path) -> None:
    path = root / "Cargo.lock"
    path.write_text(
        path.read_text(encoding="utf-8").replace(
            'name = "fixture-lib"\nversion = "1.2.3"',
            'name = "fixture-lib"\nversion = "1.2.2"',
        ),
        encoding="utf-8",
    )
    report = CHECKER.check_repo(root, probe_bump_tool=False)
    expect(
        any("fixture-lib" in error and "does not match" in error for error in report.errors),
        f"stale root lock version should fail: {report.errors}",
    )


def test_release_transition_allows_only_canonical_metadata(root: Path) -> None:
    candidate = root / "candidate"
    shutil.copytree(root, candidate)
    CHECKER.bump_version(candidate, "1.3.0")
    old_history = (root / "CHANGELOG.md").read_text(encoding="utf-8").removeprefix(
        "# Changelog\n\n"
    )
    (candidate / "CHANGELOG.md").write_text(
        "# Changelog\n\n"
        "## [1.3.0](https://github.com/forkwright/aletheia/compare/v1.2.3...v1.3.0) "
        "(2026-08-20)\n\n* release note\n\n"
        + old_history,
        encoding="utf-8",
    )

    errors = CHECKER.check_release_transition(root, candidate)
    expect(not errors, f"canonical release transition should pass: {errors}")

    lock = candidate / "Cargo.lock"
    original = lock.read_text(encoding="utf-8")
    lock.write_text(
        original.replace(
            'name = "serde"\nversion = "1.0.0"',
            'name = "serde"\nversion = "9.9.9"',
        ),
        encoding="utf-8",
    )
    errors = CHECKER.check_release_transition(root, candidate)
    expect(
        any("Cargo.lock" in error and "beyond" in error for error in errors),
        f"dependency mutation should fail transition: {errors}",
    )
    lock.write_text(original, encoding="utf-8")

    (candidate / "CHANGELOG.md").write_text(
        "# Changelog\n\n## [1.3.0](https://example.invalid/rewrite)\n",
        encoding="utf-8",
    )
    errors = CHECKER.check_release_transition(root, candidate)
    expect(
        any("preserve the complete prior history" in error for error in errors),
        f"changelog rewrite should fail transition: {errors}",
    )


def test_release_comparison_rejects_extra_newline_and_rename_paths(
    _root: Path,
) -> None:
    base_sha = "a" * 40
    candidate_sha = "b" * 40
    valid_files = [
        {"filename": path, "status": "modified"}
        for path in CHECKER.RELEASE_TRANSITION_PATHS
    ]

    def comparison(files: list[dict[str, object]]) -> dict[str, object]:
        return {
            "status": "ahead",
            "ahead_by": 1,
            "behind_by": 0,
            "total_commits": 1,
            "base_commit": {"sha": base_sha},
            "merge_base_commit": {"sha": base_sha},
            "commits": [{"sha": candidate_sha}],
            "files": files,
        }

    errors = CHECKER.check_release_comparison(
        comparison(valid_files), base_sha, candidate_sha
    )
    expect(not errors, f"exact immutable release comparison should pass: {errors}")

    errors = CHECKER.check_release_comparison([], base_sha, candidate_sha)
    expect(
        any("JSON object" in error for error in errors),
        f"non-object comparison should fail: {errors}",
    )
    malformed_files = comparison(valid_files)
    malformed_files["files"] = [*valid_files[:-1], "not-a-file-object"]
    errors = CHECKER.check_release_comparison(
        malformed_files, base_sha, candidate_sha
    )
    expect(
        any("must be a JSON object" in error for error in errors),
        f"malformed comparison file entry should fail: {errors}",
    )

    malformed_json = subprocess.run(
        [
            sys.executable,
            str(_SCRIPT_PATH),
            "verify-comparison",
            "--base-sha",
            base_sha,
            "--candidate-sha",
            candidate_sha,
        ],
        input="{",
        text=True,
        capture_output=True,
        check=False,
    )
    expect(
        malformed_json.returncode != 0
        and "not valid JSON" in malformed_json.stderr,
        "verify-comparison CLI accepted malformed JSON input",
    )

    cases = {
        "ordinary sixth file": [
            *valid_files,
            {"filename": "crates/aletheia/src/escape.rs", "status": "modified"},
        ],
        "newline-bearing sixth file": [
            *valid_files,
            {"filename": "Cargo.toml\nCHANGELOG.md", "status": "modified"},
        ],
        "renamed release file": [
            {
                "filename": CHECKER.RELEASE_TRANSITION_PATHS[0],
                "previous_filename": "renamed-owner",
                "status": "renamed",
            },
            *valid_files[1:],
        ],
    }
    for label, files in cases.items():
        errors = CHECKER.check_release_comparison(
            comparison(files), base_sha, candidate_sha
        )
        expect(errors, f"{label} must fail immutable comparison validation")

    wrong_commit = comparison(valid_files)
    wrong_commit["commits"] = [{"sha": "c" * 40}]
    errors = CHECKER.check_release_comparison(
        wrong_commit, base_sha, candidate_sha
    )
    expect(
        any("expected candidate commit" in error for error in errors),
        f"comparison candidate SHA drift should fail: {errors}",
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
        test_check_rejects_public_release_before_artifacts,
        test_release_identity_binds_tag_metadata_and_binary,
        test_bump_updates_all_version_owners,
        test_check_rejects_stale_lock_version,
        test_release_transition_allows_only_canonical_metadata,
        test_release_comparison_rejects_extra_newline_and_rename_paths,
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
