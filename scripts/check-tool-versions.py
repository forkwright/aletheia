#!/usr/bin/env python3
"""Validate release/security/gate tool pins against .github/tool-versions.sh."""

from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / ".github" / "tool-versions.sh"
INSTALL_ACTION_SHA = "bffeee26d4db9be238a4ea78d8826604ebcb594d"

REQUIRED_KEYS = {
    "CARGO_NEXTEST_VERSION",
    "CARGO_AUDIT_VERSION",
    "CARGO_FUZZ_VERSION",
    "CROSS_VERSION",
    "CARGO_CYCLONEDX_VERSION",
    "CARGO_AUDITABLE_VERSION",
    "UV_VERSION",
}

WORKFLOW_EXPECTATIONS = {
    ".github/workflows/gate-attestation.yml": [
        ". .github/tool-versions.sh",
        "tool: cargo-nextest@${{ steps.tool-versions.outputs.cargo-nextest }}",
    ],
    ".github/workflows/security.yml": [
        ". .github/tool-versions.sh",
        "tool: cargo-audit@${{ steps.tool-versions.outputs.cargo-audit }}",
    ],
    ".github/workflows/llm-freshness.yml": [
        ". .github/tool-versions.sh",
        "version: ${{ steps.tool-versions.outputs.uv }}",
    ],
    ".github/workflows/fuzz.yml": [
        ". .github/tool-versions.sh",
        'cargo install cargo-fuzz --version "${CARGO_FUZZ_VERSION}" --locked',
    ],
    ".github/workflows/release.yml": [
        ". .github/tool-versions.sh",
        "tool: cross@${{ steps.tool-versions.outputs.cross }}",
        "tool: cargo-auditable@${{ steps.tool-versions.outputs.cargo-auditable }}",
        "tool: cargo-cyclonedx@${{ steps.tool-versions.outputs.cargo-cyclonedx }}",
        "release_tool_versions_manifest=.github/tool-versions.sh",
        "tool_cargo-auditable=",
    ],
    ".github/workflows/desktop.yml": [
        ". .github/tool-versions.sh",
        "tool: cargo-nextest@${{ steps.tool-versions.outputs.cargo-nextest }}",
    ],
    ".github/workflows/online-tests.yml": [
        ". .github/tool-versions.sh",
        "tool: cargo-nextest@${{ steps.tool-versions.outputs.cargo-nextest }}",
    ],
}


def fail(message: str) -> None:
    print(f"check-tool-versions: {message}", file=sys.stderr)
    sys.exit(1)


def parse_manifest() -> dict[str, str]:
    versions: dict[str, str] = {}
    for line_number, raw_line in enumerate(MANIFEST.read_text(encoding="utf-8").splitlines(), 1):
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        if "=" not in line:
            fail(f"{MANIFEST}:{line_number}: expected KEY=VALUE")
        key, value = line.split("=", 1)
        if key in versions:
            fail(f"{MANIFEST}:{line_number}: duplicate key {key}")
        if not re.fullmatch(r"[A-Z0-9_]+", key):
            fail(f"{MANIFEST}:{line_number}: invalid key {key!r}")
        if not re.fullmatch(r"\d+\.\d+\.\d+", value):
            fail(f"{MANIFEST}:{line_number}: {key} must be an exact x.y.z version")
        versions[key] = value

    missing = REQUIRED_KEYS - versions.keys()
    if missing:
        fail(f"{MANIFEST}: missing keys: {', '.join(sorted(missing))}")
    return versions


def workflow_files() -> list[Path]:
    return sorted((ROOT / ".github" / "workflows").glob("*.yml"))


def check_no_floating_installs() -> None:
    files = [
        *workflow_files(),
        ROOT / "scripts" / "generate-sbom.sh",
        ROOT / "Cross.toml",
    ]
    combined = "\n".join(path.read_text(encoding="utf-8") for path in files)

    if re.search(r"taiki-e/install-action@v\d+(?:\s|$)", combined):
        fail("taiki-e/install-action must be pinned by SHA, not a moving vN tag")
    if re.search(r'version:\s*["\']latest["\']', combined):
        fail('tool setup must not request version: "latest"')
    if re.search(r"--version\s+\^", combined):
        fail("cargo install must not use semver ranges for release/security tooling")

    cargo_install = re.compile(
        r"cargo install (cargo-fuzz|cross|cargo-auditable|cargo-cyclonedx)\b[^\n]*"
    )
    for path in files:
        text = path.read_text(encoding="utf-8")
        for match in cargo_install.finditer(text):
            command = match.group(0)
            if "--version" not in command:
                fail(f"{path.relative_to(ROOT)}: unversioned install: {command}")


def check_consumers_reference_manifest() -> None:
    for relative_path, expected_strings in WORKFLOW_EXPECTATIONS.items():
        text = (ROOT / relative_path).read_text(encoding="utf-8")
        for expected in expected_strings:
            if expected not in text:
                fail(f"{relative_path}: missing expected manifest wiring: {expected}")

    script_text = (ROOT / "scripts" / "generate-sbom.sh").read_text(encoding="utf-8")
    if ".github/tool-versions.sh" not in script_text:
        fail("scripts/generate-sbom.sh must read .github/tool-versions.sh")
    if 'cargo install cargo-cyclonedx --version "${CARGO_CYCLONEDX_VERSION}" --locked' not in script_text:
        fail("scripts/generate-sbom.sh must install the manifest-pinned cargo-cyclonedx")


def check_literal_pins(versions: dict[str, str]) -> None:
    cross_text = (ROOT / "Cross.toml").read_text(encoding="utf-8")
    expected = (
        "cargo install cargo-auditable --locked --version "
        f"{versions['CARGO_AUDITABLE_VERSION']}"
    )
    if expected not in cross_text:
        fail("Cross.toml cargo-auditable pin does not match .github/tool-versions.sh")

    install_action_uses = re.findall(
        r"uses:\s*taiki-e/install-action@([0-9a-f]{40})(?:\s+# v2)?",
        "\n".join(path.read_text(encoding="utf-8") for path in workflow_files()),
    )
    if not install_action_uses:
        fail("no taiki-e/install-action uses found")
    wrong_refs = sorted({ref for ref in install_action_uses if ref != INSTALL_ACTION_SHA})
    if wrong_refs:
        fail(f"unexpected taiki-e/install-action SHA(s): {', '.join(wrong_refs)}")


def main() -> None:
    versions = parse_manifest()
    check_no_floating_installs()
    check_consumers_reference_manifest()
    check_literal_pins(versions)


if __name__ == "__main__":
    main()
