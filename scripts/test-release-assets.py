#!/usr/bin/env python3
"""Behavioral tests for scripts/check-release-assets.py."""

from __future__ import annotations

import hashlib
import importlib.util
import sys
import tempfile
from pathlib import Path

SCRIPT_PATH = Path(__file__).parent / "check-release-assets.py"
SPEC = importlib.util.spec_from_file_location("check_release_assets", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"failed to load {SCRIPT_PATH}")
CHECKER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECKER)

TAG = "v1.2.3"
FAILURES: list[str] = []


def expect(condition: bool, message: str) -> None:
    if not condition:
        FAILURES.append(message)


def write_valid_assets(root: Path) -> None:
    expected = CHECKER.expected_assets(TAG)
    for name in expected:
        path = root / name
        if name.endswith(".sha256"):
            continue
        if name.endswith(".jsonl"):
            payload = b"{}\n"
        elif name.endswith((".cdx.json", "bom.cdx.json")):
            payload = (
                b'{"bomFormat":"CycloneDX","specVersion":"1.6",'
                b'"serialNumber":"urn:uuid:fixture",'
                b'"metadata":{"component":{"name":"aletheia",'
                b'"version":"1.2.3","bom-ref":"pkg:aletheia"}},'
                b'"components":[{"name":"serde","version":"1.0.0",'
                b'"bom-ref":"pkg:serde"}],'
                b'"dependencies":[{"ref":"pkg:aletheia",'
                b'"dependsOn":["pkg:serde"]}]}\n'
            )
        elif name.endswith(".spdx.json"):
            payload = (
                b'{"spdxVersion":"SPDX-2.3","SPDXID":"SPDXRef-DOCUMENT",'
                b'"packages":[{"name":"aletheia","versionInfo":"1.2.3",'
                b'"SPDXID":"SPDXRef-aletheia"},{"name":"serde",'
                b'"versionInfo":"1.0.0","SPDXID":"SPDXRef-serde"}],'
                b'"relationships":[{"spdxElementId":"SPDXRef-aletheia",'
                b'"relationshipType":"DEPENDS_ON",'
                b'"relatedSpdxElement":"SPDXRef-serde"}]}\n'
            )
        else:
            payload = f"fixture:{name}\n".encode()
        path.write_bytes(payload)

    for name in expected:
        if not name.endswith(".sha256"):
            continue
        subject_name = name.removesuffix(".sha256")
        digest = hashlib.sha256((root / subject_name).read_bytes()).hexdigest()
        (root / name).write_text(f"{digest}  {subject_name}\n", encoding="utf-8")


def run_test(test: object) -> None:
    with tempfile.TemporaryDirectory(prefix="aletheia-release-assets-") as tmp:
        root = Path(tmp)
        write_valid_assets(root)
        test(root)


def test_complete_set_passes(root: Path) -> None:
    errors = CHECKER.check_assets(root, TAG)
    expect(not errors, f"complete set should pass: {errors}")


def test_missing_and_unexpected_assets_fail(root: Path) -> None:
    missing = min(CHECKER.expected_assets(TAG))
    (root / missing).unlink()
    (root / "surprise.bin").write_bytes(b"surprise")
    errors = CHECKER.check_assets(root, TAG)
    expect(
        f"missing release asset: {missing}" in errors,
        f"missing asset should fail: {errors}",
    )
    expect(
        "unexpected release asset: surprise.bin" in errors,
        f"unexpected asset should fail: {errors}",
    )


def test_checksum_mismatch_fails(root: Path) -> None:
    subject = root / "aletheia-linux-x86_64-1.2.3"
    subject.write_bytes(b"changed")
    errors = CHECKER.check_assets(root, TAG)
    expect(
        any("digest" in error and subject.name in error for error in errors),
        f"checksum mismatch should fail: {errors}",
    )


def test_malformed_attestation_fails(root: Path) -> None:
    bundle = root / "aletheia-linux-x86_64-1.2.3.provenance.intoto.jsonl"
    bundle.write_text("not json\n", encoding="utf-8")
    errors = CHECKER.check_assets(root, TAG)
    expect(
        any(bundle.name in error and "invalid JSON" in error for error in errors),
        f"malformed attestation should fail: {errors}",
    )


def test_semantically_empty_sbom_fails(root: Path) -> None:
    sbom = root / "aletheia-linux-x86_64-1.2.3.cdx.json"
    sbom.write_text("{}\n", encoding="utf-8")
    errors = CHECKER.check_assets(root, TAG)
    expect(
        any(sbom.name in error and "bomFormat" in error for error in errors),
        f"semantically empty SBOM should fail: {errors}",
    )


def test_shaped_but_dependency_empty_sboms_fail(root: Path) -> None:
    cdx = root / "aletheia-linux-x86_64-1.2.3.cdx.json"
    cdx.write_text(
        '{"bomFormat":"CycloneDX","specVersion":"1.6",'
        '"serialNumber":"urn:uuid:fixture","components":[]}\n',
        encoding="utf-8",
    )
    spdx = root / "aletheia-linux-x86_64-1.2.3.spdx.json"
    spdx.write_text(
        '{"spdxVersion":"SPDX-2.3","SPDXID":"SPDXRef-DOCUMENT",'
        '"packages":[]}\n',
        encoding="utf-8",
    )
    errors = CHECKER.check_assets(root, TAG)
    expect(
        any(cdx.name in error and "components are empty" in error for error in errors),
        f"empty CycloneDX components should fail: {errors}",
    )
    expect(
        any(spdx.name in error and "packages are empty" in error for error in errors),
        f"empty SPDX packages should fail: {errors}",
    )


def test_root_only_sboms_fail(root: Path) -> None:
    cdx = root / "aletheia-linux-x86_64-1.2.3.cdx.json"
    cdx.write_text(
        '{"bomFormat":"CycloneDX","specVersion":"1.6",'
        '"serialNumber":"urn:uuid:fixture",'
        '"metadata":{"component":{"name":"aletheia",'
        '"version":"1.2.3","bom-ref":"pkg:aletheia"}},'
        '"components":[],"dependencies":[]}\n',
        encoding="utf-8",
    )
    spdx = root / "aletheia-linux-x86_64-1.2.3.spdx.json"
    spdx.write_text(
        '{"spdxVersion":"SPDX-2.3","SPDXID":"SPDXRef-DOCUMENT",'
        '"packages":[{"name":"aletheia","versionInfo":"1.2.3",'
        '"SPDXID":"SPDXRef-aletheia"}],"relationships":[]}\n',
        encoding="utf-8",
    )
    errors = CHECKER.check_assets(root, TAG)
    expect(
        any(cdx.name in error and "no non-root dependencies" in error for error in errors),
        f"root-only CycloneDX should fail: {errors}",
    )
    expect(
        any(spdx.name in error and "no non-root dependencies" in error for error in errors),
        f"root-only SPDX should fail: {errors}",
    )


def test_spdx_dependency_of_orientation_passes(root: Path) -> None:
    spdx = root / "aletheia-linux-x86_64-1.2.3.spdx.json"
    spdx.write_text(
        '{"spdxVersion":"SPDX-2.3","SPDXID":"SPDXRef-DOCUMENT",'
        '"packages":[{"name":"aletheia","versionInfo":"1.2.3",'
        '"SPDXID":"SPDXRef-aletheia"},{"name":"serde",'
        '"versionInfo":"1.0.0","SPDXID":"SPDXRef-serde"}],'
        '"relationships":[{"spdxElementId":"SPDXRef-serde",'
        '"relationshipType":"DEPENDENCY_OF",'
        '"relatedSpdxElement":"SPDXRef-aletheia"}]}\n',
        encoding="utf-8",
    )
    errors = CHECKER.check_assets(root, TAG)
    expect(
        not errors,
        f"Syft DEPENDENCY_OF orientation should pass: {errors}",
    )


def main() -> int:
    for test in (
        test_complete_set_passes,
        test_missing_and_unexpected_assets_fail,
        test_checksum_mismatch_fails,
        test_malformed_attestation_fails,
        test_semantically_empty_sbom_fails,
        test_shaped_but_dependency_empty_sboms_fail,
        test_root_only_sboms_fail,
        test_spdx_dependency_of_orientation_passes,
    ):
        run_test(test)

    if FAILURES:
        print(f"FAIL: {len(FAILURES)} assertion(s) failed", file=sys.stderr)
        for failure in FAILURES:
            print(f"  - {failure}", file=sys.stderr)
        return 1
    print("OK: all release asset tests passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
