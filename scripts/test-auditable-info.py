#!/usr/bin/env python3
"""Negative fixtures for scripts/check-auditable-info.py."""

from __future__ import annotations

import importlib.util
import json
import os
import sys
import tempfile
from copy import deepcopy
from pathlib import Path

SCRIPT = Path(__file__).parent / "check-auditable-info.py"
SPEC = importlib.util.spec_from_file_location("check_auditable_info", SCRIPT)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot import {SCRIPT}")
CHECKER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECKER)

VERSION = "1.2.3"
GOOD = {
    "format": 1,
    "packages": [
        {
            "name": "aletheia",
            "version": VERSION,
            "source": "Local",
            "root": True,
            "dependencies": [1, 2, 3],
        },
        {"name": "serde", "version": "1.0.0", "source": "CratesIo"},
        {
            "name": "tokio",
            "version": "1.2.0",
            "source": "CratesIo",
            "kind": "runtime",
        },
        {
            "name": "cc",
            "version": "1.0.0",
            "source": "CratesIo",
            "kind": "build",
        },
    ],
}
FAILURES: list[str] = []


def expect(condition: bool, message: str) -> None:
    if not condition:
        FAILURES.append(message)


def run(value: object) -> list[str]:
    return CHECKER.check_info(value, VERSION)


def expect_error(value: object, fragment: str) -> None:
    errors = run(value)
    if not any(fragment in error for error in errors):
        FAILURES.append(f"expected {fragment!r}, got {errors}")


def check_sboms(*, include_tokio: bool) -> list[str]:
    components = [
        {"name": "aletheia", "version": VERSION},
        {"name": "serde", "version": "1.0.0"},
    ]
    packages = [
        {"name": "aletheia", "versionInfo": VERSION},
        {"name": "serde", "versionInfo": "1.0.0"},
    ]
    if include_tokio:
        components.append({"name": "tokio", "version": "1.2.0"})
        packages.append({"name": "tokio", "versionInfo": "1.2.0"})
    return CHECKER.check_info(
        GOOD,
        VERSION,
        cyclonedx={"components": components},
        spdx={"packages": packages},
    )


def check_duplicate_runtime_identity() -> list[str]:
    value = deepcopy(GOOD)
    value["packages"].append(
        {
            "name": "serde",
            "version": "1.0.0",
            "source": {"Git": "https://example.invalid/serde"},
            "kind": "runtime",
        }
    )
    value["packages"][0]["dependencies"].append(4)
    return CHECKER.check_info(
        value,
        VERSION,
        cyclonedx={
            "components": [
                {"name": "aletheia", "version": VERSION},
                {"name": "serde", "version": "1.0.0"},
                {"name": "tokio", "version": "1.2.0"},
            ]
        },
        spdx={
            "packages": [
                {"name": "aletheia", "versionInfo": VERSION},
                {"name": "serde", "versionInfo": "1.0.0"},
                {"name": "tokio", "versionInfo": "1.2.0"},
            ]
        },
    )


def test_cli_json_must_remain_beneath_invocation_directory() -> None:
    with tempfile.TemporaryDirectory(prefix="auditable-info-cli-") as tmp:
        root = Path(tmp)
        allowed = root / "allowed"
        allowed.mkdir()
        with tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", dir=allowed, delete=False
        ) as inside_handle:
            json.dump(GOOD, inside_handle)
            inside = Path(inside_handle.name)
        with tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", dir=root, delete=False
        ) as outside_handle:
            json.dump(GOOD, outside_handle)
            outside = Path(outside_handle.name)
        (allowed / "escape-link").symlink_to(outside)

        original_cwd = Path.cwd()
        try:
            os.chdir(allowed)
            decoded = CHECKER._contained_cli_json(inside.name)
            expect(decoded == GOOD, "contained CLI JSON should decode")
            for value in ("../" + outside.name, str(outside), "escape-link", "."):
                try:
                    CHECKER._contained_cli_json(value)
                except CHECKER.argparse.ArgumentTypeError:
                    continue
                FAILURES.append(f"unsafe CLI JSON path was accepted: {value}")
        finally:
            os.chdir(original_cwd)


def main() -> int:
    if run(GOOD):
        FAILURES.append("valid decoded dependency graph failed")

    no_root = deepcopy(GOOD)
    no_root["packages"][0].pop("root")
    expect_error(no_root, "one root, found 0")

    two_roots = deepcopy(GOOD)
    two_roots["packages"][1]["root"] = True
    expect_error(two_roots, "one root, found 2")

    wrong_version = deepcopy(GOOD)
    wrong_version["packages"][0]["version"] = "9.9.9"
    expect_error(wrong_version, "root version")

    out_of_range = deepcopy(GOOD)
    out_of_range["packages"][0]["dependencies"] = [99]
    expect_error(out_of_range, "out of range")

    cycle = deepcopy(GOOD)
    cycle["packages"][1]["dependencies"] = [0]
    expect_error(cycle, "contains a cycle")

    expect_error({"packages": []}, "root and dependencies")

    root_only = {"packages": [deepcopy(GOOD["packages"][0])]}
    root_only["packages"][0]["dependencies"] = []
    expect_error(root_only, "root and dependencies")

    unreachable = deepcopy(GOOD)
    unreachable["packages"][0]["dependencies"] = []
    expect_error(unreachable, "unreachable from the root")

    errors = check_sboms(include_tokio=True)
    if errors:
        FAILURES.append(f"complete SBOM inventories failed: {errors}")

    errors = check_sboms(include_tokio=False)
    if not (
        any("CycloneDX SBOM omits" in error and "tokio@1.2.0" in error for error in errors)
        and any("SPDX SBOM omits" in error and "tokio@1.2.0" in error for error in errors)
    ):
        FAILURES.append(f"missing runtime SBOM package was accepted: {errors}")

    errors = check_duplicate_runtime_identity()
    if not (
        any("CycloneDX SBOM omits" in error and "serde@1.0.0 x1" in error for error in errors)
        and any("SPDX SBOM omits" in error and "serde@1.0.0 x1" in error for error in errors)
    ):
        FAILURES.append(f"duplicate runtime identity was collapsed: {errors}")

    unknown_kind = deepcopy(GOOD)
    unknown_kind["packages"][3]["kind"] = "mystery"
    expect_error(unknown_kind, "unsupported dependency kind")

    test_cli_json_must_remain_beneath_invocation_directory()

    if FAILURES:
        print(f"FAIL: {len(FAILURES)} auditable-info assertions", file=sys.stderr)
        for failure in FAILURES:
            print(f"  - {failure}", file=sys.stderr)
        return 1
    print("OK: auditable dependency decoder rejects false evidence")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
