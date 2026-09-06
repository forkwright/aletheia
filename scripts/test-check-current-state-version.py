#!/usr/bin/env python3
"""Behavioral tests for scripts/check-current-state-version.py (#4034)."""

from __future__ import annotations

import importlib.util
import sys
import tempfile
from pathlib import Path

_SCRIPT_PATH = Path(__file__).parent / "check-current-state-version.py"


def _load_checker() -> object:
    spec = importlib.util.spec_from_file_location(
        "check_current_state_version", _SCRIPT_PATH
    )
    if spec is None or spec.loader is None:
        raise RuntimeError(f"failed to load {_SCRIPT_PATH}")
    module = importlib.util.module_from_spec(spec)
    sys.modules["check_current_state_version"] = module
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


def _write_state(root: Path, version: str) -> Path:
    state = root / "current_state.toml"
    state.write_text(
        f'[state]\nphase = "pre-1.0"\nversion = "{version}"\n',
        encoding="utf-8",
    )
    return state


def _write_manifest(root: Path, version: str) -> Path:
    manifest = root / "manifest.json"
    manifest.write_text(f'{{".": "{version}"}}\n', encoding="utf-8")
    return manifest


def test_check_passes_when_versions_match(root: Path) -> None:
    state = _write_state(root, "0.45.0")
    manifest = _write_manifest(root, "0.45.0")
    errors = CHECKER.check(state, manifest)
    expect(errors == [], f"expected no errors, got {errors!r}")


def test_check_fails_when_versions_mismatch(root: Path) -> None:
    # This reproduces the exact defect the issue reported: current_state.toml
    # stuck at 0.37.0 while the release manifest had already moved to 0.43.0
    # (later 0.45.0). The check must reject that drift.
    state = _write_state(root, "0.37.0")
    manifest = _write_manifest(root, "0.43.0")
    errors = CHECKER.check(state, manifest)
    expect(len(errors) == 1, f"expected exactly one error, got {errors!r}")
    expect(
        "0.37.0" in errors[0] and "0.43.0" in errors[0] if errors else False,
        f"error should name both the stale and current versions: {errors!r}",
    )


def test_check_fails_when_state_missing_version(root: Path) -> None:
    state = root / "current_state.toml"
    state.write_text('[state]\nphase = "pre-1.0"\n', encoding="utf-8")
    manifest = _write_manifest(root, "0.45.0")
    errors = CHECKER.check(state, manifest)
    expect(len(errors) == 1, f"expected exactly one error, got {errors!r}")
    expect(
        "missing or invalid" in errors[0] if errors else False,
        f"error should say the version field is missing: {errors!r}",
    )


def test_check_fails_when_manifest_missing_root_entry(root: Path) -> None:
    state = _write_state(root, "0.45.0")
    manifest = root / "manifest.json"
    manifest.write_text('{"crates/foo": "1.0.0"}\n', encoding="utf-8")
    errors = CHECKER.check(state, manifest)
    expect(len(errors) == 1, f"expected exactly one error, got {errors!r}")
    expect(
        'missing or invalid' in errors[0] if errors else False,
        f"error should say the manifest '.' entry is missing: {errors!r}",
    )


def test_check_fails_when_state_file_absent(root: Path) -> None:
    state = root / "does-not-exist.toml"
    manifest = _write_manifest(root, "0.45.0")
    errors = CHECKER.check(state, manifest)
    expect(len(errors) == 1, f"expected exactly one error, got {errors!r}")


def main() -> int:
    for test_fn in (
        test_check_passes_when_versions_match,
        test_check_fails_when_versions_mismatch,
        test_check_fails_when_state_missing_version,
        test_check_fails_when_manifest_missing_root_entry,
        test_check_fails_when_state_file_absent,
    ):
        run_isolated(test_fn)

    if _FAILURES:
        print(f"FAIL: {len(_FAILURES)} assertion(s) failed", file=sys.stderr)
        for failure in _FAILURES:
            print(f"  - {failure}", file=sys.stderr)
        return 1

    print("OK: all check-current-state-version tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
