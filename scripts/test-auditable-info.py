#!/usr/bin/env python3
"""Negative fixtures for scripts/check-auditable-info.py."""

from __future__ import annotations

import importlib.util
import json
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


def run(value: object) -> list[str]:
    with tempfile.TemporaryDirectory(prefix="auditable-info-") as tmp:
        path = Path(tmp) / "info.json"
        path.write_text(json.dumps(value), encoding="utf-8")
        return CHECKER.check_info(path, VERSION)


def expect_error(value: object, fragment: str) -> None:
    errors = run(value)
    if not any(fragment in error for error in errors):
        FAILURES.append(f"expected {fragment!r}, got {errors}")


def check_sboms(*, include_tokio: bool) -> list[str]:
    with tempfile.TemporaryDirectory(prefix="auditable-info-sbom-") as tmp:
        root = Path(tmp)
        info = root / "info.json"
        cdx = root / "binary.cdx.json"
        spdx = root / "binary.spdx.json"
        info.write_text(json.dumps(GOOD), encoding="utf-8")
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
        cdx.write_text(json.dumps({"components": components}), encoding="utf-8")
        spdx.write_text(json.dumps({"packages": packages}), encoding="utf-8")
        return CHECKER.check_info(
            info, VERSION, cyclonedx=cdx, spdx=spdx
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
    with tempfile.TemporaryDirectory(prefix="auditable-info-sbom-") as tmp:
        root = Path(tmp)
        info = root / "info.json"
        cdx = root / "binary.cdx.json"
        spdx = root / "binary.spdx.json"
        info.write_text(json.dumps(value), encoding="utf-8")
        cdx.write_text(
            json.dumps(
                {
                    "components": [
                        {"name": "aletheia", "version": VERSION},
                        {"name": "serde", "version": "1.0.0"},
                        {"name": "tokio", "version": "1.2.0"},
                    ]
                }
            ),
            encoding="utf-8",
        )
        spdx.write_text(
            json.dumps(
                {
                    "packages": [
                        {"name": "aletheia", "versionInfo": VERSION},
                        {"name": "serde", "versionInfo": "1.0.0"},
                        {"name": "tokio", "versionInfo": "1.2.0"},
                    ]
                }
            ),
            encoding="utf-8",
        )
        return CHECKER.check_info(info, VERSION, cyclonedx=cdx, spdx=spdx)


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

    if FAILURES:
        print(f"FAIL: {len(FAILURES)} auditable-info assertions", file=sys.stderr)
        for failure in FAILURES:
            print(f"  - {failure}", file=sys.stderr)
        return 1
    print("OK: auditable dependency decoder rejects false evidence")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
