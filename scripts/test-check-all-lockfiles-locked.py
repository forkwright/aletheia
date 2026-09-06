#!/usr/bin/env python3
"""Behavioral tests for scripts/check-all-lockfiles-locked.py (#7148)."""

from __future__ import annotations

import importlib.util
import sys
import tempfile
from pathlib import Path

_SCRIPT_PATH = Path(__file__).parent / "check-all-lockfiles-locked.py"


def _load_checker() -> object:
    spec = importlib.util.spec_from_file_location(
        "check_all_lockfiles_locked", _SCRIPT_PATH
    )
    if spec is None or spec.loader is None:
        raise RuntimeError(f"failed to load {_SCRIPT_PATH}")
    module = importlib.util.module_from_spec(spec)
    sys.modules["check_all_lockfiles_locked"] = module
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


class _FakeResult:
    def __init__(self, returncode: int, stderr: str = "") -> None:
        self.returncode = returncode
        self.stderr = stderr


def test_reuses_workspace_locks_enumeration_function() -> None:
    # This is the "write once" requirement (#7148): a second, drifted
    # re-derivation of "every [workspace] manifest" is exactly the class of
    # bug this issue exists to close. Each importlib load produces its own
    # module (so object identity across two loads is not meaningful), but
    # the function's __code__ still names the file it was compiled from --
    # assert that origin is check-workspace-locks.py, not a copy inlined
    # here.
    origin = Path(CHECKER.workspace_manifests.__code__.co_filename)
    expect(
        origin.name == "check-workspace-locks.py",
        f"check-all-lockfiles-locked.py must reuse check-workspace-locks.py's "
        f"workspace_manifests(), not re-derive it (loaded from {origin})",
    )


def test_command_for_fuzz_manifest_adds_locked_to_check(root: Path) -> None:
    fuzz_manifest = root / "fuzz" / "Cargo.toml"
    (root / "fuzz").mkdir()
    fuzz_manifest.write_text("[workspace]\n", encoding="utf-8")
    command = CHECKER.command_for(fuzz_manifest, repo_root=root)
    expect(
        command == ["cargo", "check", "--manifest-path", "fuzz/Cargo.toml", "--bins", "--locked"],
        f"unexpected fuzz command: {command!r}",
    )


def test_command_for_non_fuzz_manifest_uses_metadata(root: Path) -> None:
    manifest = root / "Cargo.toml"
    manifest.write_text("[workspace]\n", encoding="utf-8")
    command = CHECKER.command_for(manifest, repo_root=root)
    expect(
        command == ["cargo", "metadata", "--locked", "--manifest-path", "Cargo.toml"],
        f"unexpected default command: {command!r}",
    )


def test_check_manifest_skips_when_lock_absent(root: Path) -> None:
    manifest = root / "Cargo.toml"
    manifest.write_text("[workspace]\n", encoding="utf-8")
    calls: list[list[str]] = []

    def recording_run(command, **_kwargs):
        calls.append(command)
        return _FakeResult(0)

    error = CHECKER.check_manifest(manifest, repo_root=root, run=recording_run)
    expect(error is None, f"expected no error when lock is absent, got {error!r}")
    expect(calls == [], "cargo should never be invoked when no Cargo.lock exists")


def test_check_manifest_fails_when_cargo_rejects_locked(root: Path) -> None:
    # This reproduces the reported defect: a secondary manifest and its
    # lock disagree, and --locked must surface that as a failure rather
    # than cargo silently repairing the lock.
    manifest = root / "fuzz" / "Cargo.toml"
    (root / "fuzz").mkdir()
    manifest.write_text("[workspace]\n", encoding="utf-8")
    (root / "fuzz" / "Cargo.lock").write_text("# stale\n", encoding="utf-8")

    def failing_run(command, **_kwargs):
        return _FakeResult(
            101,
            stderr="error: the lock file fuzz/Cargo.lock needs to be updated "
            "but --locked was passed to prevent this",
        )

    error = CHECKER.check_manifest(manifest, repo_root=root, run=failing_run)
    expect(error is not None, "expected a failure when cargo rejects --locked")
    expect(
        "needs to be updated" in error if error else False,
        f"error should surface cargo's own message: {error!r}",
    )


def test_check_manifest_passes_when_cargo_accepts_locked(root: Path) -> None:
    manifest = root / "Cargo.toml"
    manifest.write_text("[workspace]\n", encoding="utf-8")
    (root / "Cargo.lock").write_text("# ok\n", encoding="utf-8")

    def passing_run(command, **_kwargs):
        return _FakeResult(0)

    error = CHECKER.check_manifest(manifest, repo_root=root, run=passing_run)
    expect(error is None, f"expected no error, got {error!r}")


def main() -> int:
    test_reuses_workspace_locks_enumeration_function()
    for test_fn in (
        test_command_for_fuzz_manifest_adds_locked_to_check,
        test_command_for_non_fuzz_manifest_uses_metadata,
        test_check_manifest_skips_when_lock_absent,
        test_check_manifest_fails_when_cargo_rejects_locked,
        test_check_manifest_passes_when_cargo_accepts_locked,
    ):
        run_isolated(test_fn)

    if _FAILURES:
        print(f"FAIL: {len(_FAILURES)} assertion(s) failed", file=sys.stderr)
        for failure in _FAILURES:
            print(f"  - {failure}", file=sys.stderr)
        return 1

    print("OK: all check-all-lockfiles-locked tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
