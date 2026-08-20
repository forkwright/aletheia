#!/usr/bin/env python3
"""Behavioral tests for scripts/check-release-tarball.py."""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import io
import os
import stat
import sys
import tarfile
import tempfile
from pathlib import Path

SCRIPT_PATH = Path(__file__).parent / "check-release-tarball.py"
SPEC = importlib.util.spec_from_file_location("check_release_tarball", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"failed to load {SCRIPT_PATH}")
CHECKER = importlib.util.module_from_spec(SPEC)
sys.modules["check_release_tarball"] = CHECKER
SPEC.loader.exec_module(CHECKER)

VERSION = "1.2.3"
TARGET = "x86_64-unknown-linux-musl"
SOURCE_SHA = "0123456789abcdef0123456789abcdef01234567"
FAILURES: list[str] = []


def expect(condition: bool, message: str) -> None:
    if not condition:
        FAILURES.append(message)


def _write_fixture(root: Path) -> Path:
    package = root / f"aletheia-{VERSION}"
    for relative in CHECKER.REQUIRED_PATHS:
        if relative == "PACKAGE-MANIFEST.txt":
            continue
        path = package / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(f"fixture:{relative}\n", encoding="utf-8")
    (package / "aletheia").chmod(0o755)
    _write_manifest(package)
    return package


def _write_manifest(package: Path) -> None:
    lines = [
        "Aletheia binary package manifest",
        f"version={VERSION}",
        f"target={TARGET}",
        f"source_commit={SOURCE_SHA}",
        f"features={CHECKER.FEATURES}",
        "rustc_version=rustc fixture",
        "cross_version=fixture",
        f"provenance_asset=aletheia-linux-x86_64-{VERSION}.provenance.intoto.jsonl",
        f"cyclonedx_sbom_asset=aletheia-linux-x86_64-{VERSION}.cdx.json",
        f"spdx_sbom_asset=aletheia-linux-x86_64-{VERSION}.spdx.json",
        "",
        "sha256 mode bytes path",
    ]
    manifest = package / "PACKAGE-MANIFEST.txt"
    for path in sorted(p for p in package.rglob("*") if p.is_file()):
        data = path.read_bytes()
        mode = stat.S_IMODE(path.stat().st_mode)
        relative = path.relative_to(package).as_posix()
        lines.append(
            f"{hashlib.sha256(data).hexdigest()} {mode:04o} {len(data)} {relative}"
        )
    manifest.write_text("\n".join(lines) + "\n", encoding="utf-8")


def _archive(package: Path, destination: Path) -> None:
    with tarfile.open(destination, "w:gz") as archive:
        archive.add(package, arcname=package.name)


def _run_fixture(mutator: object | None = None) -> list[str]:
    with tempfile.TemporaryDirectory(prefix="aletheia-tarball-") as tmp:
        root = Path(tmp)
        package = _write_fixture(root)
        if mutator is not None:
            mutator(package)
        tarball = root / "fixture.tar.gz"
        _archive(package, tarball)
        standalone = root / "aletheia-linux-x86_64-1.2.3"
        standalone.write_bytes((package / "aletheia").read_bytes())
        return CHECKER.check_tarball(
            tarball, VERSION, TARGET, SOURCE_SHA, standalone
        )


def test_valid_tarball_passes() -> None:
    errors = _run_fixture()
    expect(not errors, f"valid fixture should pass: {errors}")


def test_stale_manifest_content_fails() -> None:
    def mutate(package: Path) -> None:
        (package / "README.md").write_text("changed after manifest\n", encoding="utf-8")

    errors = _run_fixture(mutate)
    expect(
        any("digest mismatch for README.md" in error for error in errors),
        f"stale digest should fail: {errors}",
    )
    expect(
        any("size mismatch for README.md" in error for error in errors),
        f"stale size should fail: {errors}",
    )


def test_missing_required_file_fails() -> None:
    errors = _run_fixture(lambda package: (package / "SECURITY.md").unlink())
    expect(
        any("missing aletheia-1.2.3/SECURITY.md" in error for error in errors),
        f"missing required file should fail: {errors}",
    )


def test_stale_manifest_mode_fails() -> None:
    def mutate(package: Path) -> None:
        (package / "aletheia").chmod(0o644)

    errors = _run_fixture(mutate)
    expect(
        any("mode mismatch for aletheia" in error for error in errors),
        f"stale mode should fail: {errors}",
    )


def test_self_consistent_non_executable_binary_fails() -> None:
    def mutate(package: Path) -> None:
        (package / "aletheia").chmod(0o644)
        _write_manifest(package)

    errors = _run_fixture(mutate)
    expect(
        any("packaged aletheia mode is 0644" in error for error in errors),
        f"non-executable binary should fail independently of manifest: {errors}",
    )


def test_wrong_source_commit_fails() -> None:
    with tempfile.TemporaryDirectory(prefix="aletheia-tarball-") as tmp:
        root = Path(tmp)
        package = _write_fixture(root)
        tarball = root / "fixture.tar.gz"
        _archive(package, tarball)
        wrong_sha = "f" * 40
        standalone = root / "aletheia-linux-x86_64-1.2.3"
        standalone.write_bytes((package / "aletheia").read_bytes())
        errors = CHECKER.check_tarball(
            tarball, VERSION, TARGET, wrong_sha, standalone
        )
    expect(
        any("source_commit" in error and wrong_sha in error for error in errors),
        f"wrong source commit should fail: {errors}",
    )


def test_empty_package_root_fails() -> None:
    with tempfile.TemporaryDirectory(prefix="aletheia-tarball-") as tmp:
        root = Path(tmp)
        package = root / f"aletheia-{VERSION}"
        package.mkdir()
        tarball = root / "fixture.tar.gz"
        _archive(package, tarball)
        standalone = root / "aletheia-linux-x86_64-1.2.3"
        standalone.write_bytes(b"fixture")
        errors = CHECKER.check_tarball(
            tarball, VERSION, TARGET, SOURCE_SHA, standalone
        )
    expect(
        any("missing aletheia-1.2.3/aletheia" in error for error in errors),
        f"empty package root should fail required paths: {errors}",
    )


def test_embedded_binary_must_equal_standalone_asset() -> None:
    with tempfile.TemporaryDirectory(prefix="aletheia-tarball-") as tmp:
        root = Path(tmp)
        package = _write_fixture(root)
        tarball = root / "fixture.tar.gz"
        _archive(package, tarball)
        standalone = root / "aletheia-linux-x86_64-1.2.3"
        standalone.write_bytes(b"different standalone binary")
        errors = CHECKER.check_tarball(
            tarball, VERSION, TARGET, SOURCE_SHA, standalone
        )
    expect(
        any("does not equal the standalone" in error for error in errors),
        f"tar/standalone binary mismatch should fail: {errors}",
    )


def test_noncanonical_member_alias_fails() -> None:
    with tempfile.TemporaryDirectory(prefix="aletheia-tarball-") as tmp:
        root = Path(tmp)
        package = _write_fixture(root)
        tarball = root / "fixture.tar.gz"
        with tarfile.open(tarball, "w:gz") as archive:
            archive.add(package, arcname=package.name)
            alias = tarfile.TarInfo(f"{package.name}/./README.md")
            payload = b"alias\n"
            alias.size = len(payload)
            alias.mode = 0o644
            archive.addfile(alias, io.BytesIO(payload))
        standalone = root / "aletheia-linux-x86_64-1.2.3"
        standalone.write_bytes((package / "aletheia").read_bytes())
        errors = CHECKER.check_tarball(
            tarball, VERSION, TARGET, SOURCE_SHA, standalone
        )
    expect(
        any("non-canonical archive member" in error for error in errors),
        f"normalized path alias should fail: {errors}",
    )


def test_cli_files_must_remain_beneath_invocation_directory() -> None:
    with tempfile.TemporaryDirectory(prefix="aletheia-tarball-cli-") as tmp:
        parent = Path(tmp)
        allowed = parent / "allowed"
        allowed.mkdir()
        inside = allowed / "inside"
        inside.write_bytes(b"inside")
        outside = parent / "outside"
        outside.write_bytes(b"outside")
        escape_link = allowed / "escape-link"
        escape_link.symlink_to(outside)
        original_cwd = Path.cwd()
        try:
            os.chdir(allowed)
            expect(
                CHECKER._contained_cli_file("inside") == inside.resolve(),
                "contained CLI file should resolve",
            )
            for value in ("../outside", str(outside), "escape-link", "."):
                try:
                    CHECKER._contained_cli_file(value)
                except argparse.ArgumentTypeError:
                    continue
                expect(False, f"CLI containment accepted {value!r}")
        finally:
            os.chdir(original_cwd)


def main() -> int:
    for test in (
        test_valid_tarball_passes,
        test_stale_manifest_content_fails,
        test_missing_required_file_fails,
        test_stale_manifest_mode_fails,
        test_self_consistent_non_executable_binary_fails,
        test_wrong_source_commit_fails,
        test_empty_package_root_fails,
        test_embedded_binary_must_equal_standalone_asset,
        test_noncanonical_member_alias_fails,
        test_cli_files_must_remain_beneath_invocation_directory,
    ):
        test()

    if FAILURES:
        print(f"FAIL: {len(FAILURES)} assertion(s) failed", file=sys.stderr)
        for failure in FAILURES:
            print(f"  - {failure}", file=sys.stderr)
        return 1
    print("OK: all release tarball tests passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
