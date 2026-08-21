#!/usr/bin/env python3
"""Behavioral tests for scripts/check-tool-versions.py."""

from __future__ import annotations

import importlib.util
import sys
import tempfile
from pathlib import Path

_SCRIPT_PATH = Path(__file__).parent / "check-tool-versions.py"


def _load_checker() -> object:
    spec = importlib.util.spec_from_file_location("check_tool_versions", _SCRIPT_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"failed to load {_SCRIPT_PATH}")
    module = importlib.util.module_from_spec(spec)
    sys.modules["check_tool_versions"] = module
    spec.loader.exec_module(module)
    return module


CHECKER = _load_checker()
_FAILURES: list[str] = []


def expect(condition: bool, msg: str) -> None:
    if not condition:
        _FAILURES.append(msg)


def run_isolated(test_fn: object) -> None:
    with tempfile.TemporaryDirectory() as tmp_str:
        root = Path(tmp_str)
        test_fn(root)


def test_check_tool_passes_when_literal_present(root: Path) -> None:
    site = root / "workflow.yml"
    site.write_text("with:\n  tool: nextest@0.9.143\n", encoding="utf-8")
    errors = CHECKER.check_tool(
        root, "nextest", {"version": "0.9.143", "sites": ["workflow.yml"]}
    )
    expect(errors == [], f"expected no errors, got {errors!r}")


def test_check_tool_fails_when_literal_missing(root: Path) -> None:
    site = root / "workflow.yml"
    site.write_text("with:\n  tool: nextest@0.9.100\n", encoding="utf-8")
    errors = CHECKER.check_tool(
        root, "nextest", {"version": "0.9.143", "sites": ["workflow.yml"]}
    )
    expect(len(errors) == 1, f"expected exactly one error, got {errors!r}")
    expect(
        "0.9.143" in errors[0] if errors else False,
        "error should name the manifest version that was not found",
    )


def test_check_tool_accepts_each_honest_install_spelling(root: Path) -> None:
    host = root / "host.yml"
    image = root / "image.sh"
    host.write_text(
        "cargo install cargo-auditable --locked --version 0.7.4\n",
        encoding="utf-8",
    )
    image.write_text('cargo_auditable_version="0.7.4"\n', encoding="utf-8")
    entry = {"version": "0.7.4", "sites": ["host.yml", "image.sh"]}
    errors = CHECKER.check_tool(root, "cargo-auditable", entry)
    expect(errors == [], f"both auditable install spellings should pass: {errors!r}")


def test_check_tool_accepts_pinned_cargo_mutants(root: Path) -> None:
    site = root / "substance.yml"
    site.write_text(
        "cargo install --locked cargo-mutants --version 27.1.0\n",
        encoding="utf-8",
    )
    errors = CHECKER.check_tool(
        root,
        "cargo-mutants",
        {"version": "27.1.0", "sites": ["substance.yml"]},
    )
    expect(errors == [], f"pinned cargo-mutants install should pass: {errors!r}")


def test_check_tool_fails_when_site_missing(root: Path) -> None:
    errors = CHECKER.check_tool(
        root, "cross", {"version": "0.2.5", "sites": ["does-not-exist.yml"]}
    )
    expect(len(errors) == 1, f"expected one error for a missing site, got {errors!r}")
    expect(
        "does not exist" in errors[0] if errors else False,
        "error should say the site file does not exist",
    )


def test_check_tool_rejects_site_escape(root: Path) -> None:
    with tempfile.NamedTemporaryFile(
        mode="w", encoding="utf-8", dir=root.parent, delete=True
    ) as outside:
        outside.write("with:\n  tool: nextest@0.9.143\n")
        outside.flush()
        outside_path = Path(outside.name)
        link = root / "site-link.yml"
        link.symlink_to(outside_path)
        for site in (
            str(outside_path),
            f"../{outside_path.name}",
            "site-link.yml",
        ):
            errors = CHECKER.check_tool(
                root,
                "nextest",
                {"version": "0.9.143", "sites": [site]},
            )
            expect(
                len(errors) == 1
                and any(
                    fragment in errors[0]
                    for fragment in ("canonical", "escapes", "symlink")
                ),
                f"unsafe tool site {site!r} was accepted: {errors!r}",
            )


def test_check_tool_rejects_unregistered_tool_name(root: Path) -> None:
    errors = CHECKER.check_tool(
        root, "not-a-real-tool", {"version": "1.0.0", "sites": []}
    )
    expect(len(errors) == 1, f"expected one error for an unknown tool, got {errors!r}")
    expect(
        "no match template" in errors[0] if errors else False,
        "error should say no match template is registered",
    )


def test_check_fuzz_nightly_passes_when_date_present(root: Path) -> None:
    site = root / "fuzz.yml"
    site.write_text("toolchain: nightly-2026-08-15\n", encoding="utf-8")
    errors = CHECKER.check_fuzz_nightly(
        root, {"nightly_date": "2026-08-15", "sites": ["fuzz.yml"]}
    )
    expect(errors == [], f"expected no errors, got {errors!r}")


def test_check_fuzz_nightly_fails_when_date_stale(root: Path) -> None:
    site = root / "fuzz.yml"
    site.write_text("toolchain: nightly-2026-01-01\n", encoding="utf-8")
    errors = CHECKER.check_fuzz_nightly(
        root, {"nightly_date": "2026-08-15", "sites": ["fuzz.yml"]}
    )
    expect(len(errors) == 1, f"expected one error for a stale date, got {errors!r}")


def test_check_fuzz_nightly_rejects_site_escape(root: Path) -> None:
    errors = CHECKER.check_fuzz_nightly(
        root,
        {
            "nightly_date": "2026-08-15",
            "sites": [str(_SCRIPT_PATH)],
        },
    )
    expect(
        len(errors) == 1 and "canonical" in errors[0],
        f"fuzz site escape was accepted: {errors!r}",
    )


def main() -> int:
    for test_fn in (
        test_check_tool_passes_when_literal_present,
        test_check_tool_fails_when_literal_missing,
        test_check_tool_accepts_each_honest_install_spelling,
        test_check_tool_accepts_pinned_cargo_mutants,
        test_check_tool_fails_when_site_missing,
        test_check_tool_rejects_site_escape,
        test_check_tool_rejects_unregistered_tool_name,
        test_check_fuzz_nightly_passes_when_date_present,
        test_check_fuzz_nightly_fails_when_date_stale,
        test_check_fuzz_nightly_rejects_site_escape,
    ):
        run_isolated(test_fn)

    if _FAILURES:
        print(f"FAIL: {len(_FAILURES)} assertion(s) failed", file=sys.stderr)
        for failure in _FAILURES:
            print(f"  - {failure}", file=sys.stderr)
        return 1

    print("OK: all check-tool-versions tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
