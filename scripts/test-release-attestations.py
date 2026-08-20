#!/usr/bin/env python3
"""Behavioral tests for scripts/check-release-attestations.py."""

from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
import tempfile
from pathlib import Path

SCRIPT_PATH = Path(__file__).parent / "check-release-attestations.py"
SPEC = importlib.util.spec_from_file_location("check_release_attestations", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"failed to load {SCRIPT_PATH}")
CHECKER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECKER)

TAG = "v1.2.3"
SHA = "0123456789abcdef0123456789abcdef01234567"
REPO = "forkwright/aletheia"
FAILURES: list[str] = []


def expect(condition: bool, message: str) -> None:
    if not condition:
        FAILURES.append(message)


def write_fixture(root: Path) -> None:
    for artifact in CHECKER.PLATFORM_ARTIFACTS:
        binary = root / f"{artifact}-1.2.3"
        binary.write_bytes(b"binary")
        for stem, payload in (
            ("cdx", {"bomFormat": "CycloneDX", "components": []}),
            ("spdx", {"spdxVersion": "SPDX-2.3", "packages": []}),
        ):
            (root / f"{artifact}-1.2.3.{stem}.json").write_text(
                json.dumps(payload), encoding="utf-8"
            )
        for stem in ("provenance", "cdx", "spdx"):
            (root / f"{artifact}-1.2.3.{stem}.intoto.jsonl").write_text(
                "{}\n", encoding="utf-8"
            )


def fake_runner(
    command: list[str], *, mismatch: bool = False
) -> subprocess.CompletedProcess[str]:
    predicate_type = command[command.index("--predicate-type") + 1]
    binary = Path(command[3])
    if predicate_type == CHECKER.CYCLONEDX_TYPE:
        predicate = json.loads(Path(f"{binary}.cdx.json").read_text())
    elif predicate_type == CHECKER.SPDX_TYPE:
        predicate = json.loads(Path(f"{binary}.spdx.json").read_text())
    else:
        predicate = {"builder": "fixture"}
    if mismatch and predicate_type == CHECKER.CYCLONEDX_TYPE:
        predicate = {"bomFormat": "wrong"}
    stdout = json.dumps(
        [
            {
                "verificationResult": {
                    "statement": {
                        "predicateType": predicate_type,
                        "predicate": predicate,
                    }
                }
            }
        ]
    )
    return subprocess.CompletedProcess(command, 0, stdout=stdout, stderr="")


def test_all_bundles_and_policy_flags_pass(root: Path) -> None:
    commands: list[list[str]] = []

    def runner(command: list[str]) -> subprocess.CompletedProcess[str]:
        commands.append(command)
        return fake_runner(command)

    errors = CHECKER.check_attestations(root, TAG, SHA, REPO, runner=runner)
    expect(not errors, f"valid bundles should pass: {errors}")
    expect(len(commands) == 6, f"all six bundles should be verified: {commands}")
    for command in commands:
        expect("--bundle" in command, f"offline bundle must be bound: {command}")
        expect(
            command[command.index("--source-digest") + 1] == SHA,
            f"source SHA must be bound: {command}",
        )
        expect(
            command[command.index("--source-ref") + 1] == f"refs/tags/{TAG}",
            f"source tag must be bound: {command}",
        )
        expect(
            command[command.index("--signer-workflow") + 1]
            == "github.com/forkwright/aletheia/.github/workflows/release.yml",
            f"signer workflow must be bound: {command}",
        )
        expect(
            "--deny-self-hosted-runners" in command,
            f"hosted builder policy must be enforced: {command}",
        )


def test_signed_sbom_mismatch_fails(root: Path) -> None:
    errors = CHECKER.check_attestations(
        root,
        TAG,
        SHA,
        REPO,
        runner=lambda command: fake_runner(command, mismatch=True),
    )
    expect(
        any("signed predicate does not equal" in error for error in errors),
        f"signed/released SBOM mismatch should fail: {errors}",
    )


def test_failed_signature_fails(root: Path) -> None:
    def runner(command: list[str]) -> subprocess.CompletedProcess[str]:
        return subprocess.CompletedProcess(command, 1, stdout="", stderr="bad signature")

    errors = CHECKER.check_attestations(root, TAG, SHA, REPO, runner=runner)
    expect(
        len(errors) == 6 and all("cryptographic verification failed" in e for e in errors),
        f"signature failure should fail every bundle: {errors}",
    )


def run_test(test: object) -> None:
    with tempfile.TemporaryDirectory(prefix="aletheia-attestations-") as tmp:
        root = Path(tmp)
        write_fixture(root)
        test(root)


def main() -> int:
    for test in (
        test_all_bundles_and_policy_flags_pass,
        test_signed_sbom_mismatch_fails,
        test_failed_signature_fails,
    ):
        run_test(test)

    if FAILURES:
        print(f"FAIL: {len(FAILURES)} assertion(s) failed", file=sys.stderr)
        for failure in FAILURES:
            print(f"  - {failure}", file=sys.stderr)
        return 1
    print("OK: all release attestation tests passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
