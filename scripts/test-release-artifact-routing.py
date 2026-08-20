#!/usr/bin/env python3
"""Negative fixtures for scripts/check-release-artifact-routing.py."""

from __future__ import annotations

import importlib.util
import json
import shutil
import sys
import tempfile
from pathlib import Path

SCRIPT_PATH = Path(__file__).parent / "check-release-artifact-routing.py"
REPO_ROOT = SCRIPT_PATH.parents[1]
SPEC = importlib.util.spec_from_file_location("check_release_routing", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"failed to load {SCRIPT_PATH}")
CHECKER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECKER)

FAILURES: list[str] = []


def expect(condition: bool, message: str) -> None:
    if not condition:
        FAILURES.append(message)


def copy_fixture(root: Path) -> None:
    for relative in (
        CHECKER.RELEASE_PLEASE,
        CHECKER.RELEASE,
        CHECKER.GATE,
        CHECKER.SECURITY,
        CHECKER.CONFIG,
        CHECKER.DEPLOY,
        CHECKER.CROSS_CONFIG,
        CHECKER.CROSS_INSTALLER,
        *CHECKER.CONSUMER_DOCS,
    ):
        destination = root / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(REPO_ROOT / relative, destination)


def mutate_text(root: Path, relative: Path, old: str, new: str) -> None:
    path = root / relative
    text = path.read_text(encoding="utf-8")
    if old not in text:
        raise RuntimeError(f"fixture mutation source not found in {relative}: {old!r}")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


def run_mutation(mutator: object) -> list[str]:
    with tempfile.TemporaryDirectory(prefix="aletheia-release-routing-") as tmp:
        root = Path(tmp)
        copy_fixture(root)
        mutator(root)
        return CHECKER.check_repo(root)


def test_live_contract_passes() -> None:
    errors = CHECKER.check_repo(REPO_ROOT)
    expect(not errors, f"live release contract should pass: {errors}")


def test_text_mutations_fail() -> None:
    cases = (
        (
            "missing tagged dispatch",
            CHECKER.RELEASE_PLEASE,
            "gh workflow run release.yml",
            "gh workflow run missing.yml",
            "tagged dispatch lacks gh workflow run release.yml",
        ),
        (
            "cancellable handoff",
            CHECKER.RELEASE_PLEASE,
            "cancel-in-progress: false",
            "cancel-in-progress: true",
            "must queue, not cancel",
        ),
        (
            "dropped pending control run",
            CHECKER.RELEASE_PLEASE,
            "queue: max",
            "queue: one",
            "retain every pending run",
        ),
        (
            "feature-ref Release Please dispatch",
            CHECKER.RELEASE_PLEASE,
            "on:\n  push:\n    branches: [main]",
            "on:\n  push:\n    branches: [main]\n  workflow_dispatch:",
            "triggered only by main push",
        ),
        (
            "core-only SemVer dispatch",
            CHECKER.RELEASE_PLEASE,
            r"(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?",
            "",
            "tagged dispatch lacks",
        ),
        (
            "custom Release Please token",
            CHECKER.RELEASE_PLEASE,
            "manifest-file: .release-please-manifest.json",
            (
                "manifest-file: .release-please-manifest.json\n"
                "          token: ${{ secrets.PAT }}"
            ),
            "custom Release Please token",
        ),
        (
            "duplicate YAML key",
            CHECKER.RELEASE_PLEASE,
            "cancel-in-progress: false",
            "cancel-in-progress: false\n      cancel-in-progress: false",
            "duplicate key",
        ),
        (
            "missing manual tag fallback",
            CHECKER.RELEASE,
            'tags: ["v*"]',
            'tags: ["release-*"]',
            "manual v* tag fallback is missing",
        ),
        (
            "reusable release shortcut",
            CHECKER.RELEASE,
            "workflow_dispatch:\n    inputs:",
            "workflow_call:\n    inputs:",
            "reusable-call route",
        ),
        (
            "wrong checkout ref",
            CHECKER.RELEASE,
            "ref: ${{ inputs.release_sha || github.sha }}",
            "ref: ${{ github.sha }}",
            "checkout is not bound",
        ),
        (
            "wrong canonical gate owner",
            CHECKER.RELEASE,
            "uses: ./.github/workflows/gate-attestation.yml",
            "uses: ./.github/workflows/other-gate.yml",
            "does not call the gate owner",
        ),
        (
            "unbound security call",
            CHECKER.RELEASE,
            "release_sha: ${{ inputs.release_sha || github.sha }}",
            "release_sha: ${{ inputs.release_sha }}",
            "canonical-security is not bound",
        ),
        (
            "early publication",
            CHECKER.RELEASE,
            "needs: [build, sbom]",
            "needs: [build]",
            "wait for build and SBOM",
        ),
        (
            "native unlocked build",
            CHECKER.RELEASE,
            "cargo auditable build --locked",
            "cargo auditable build",
            "artifact build lacks cargo auditable build --locked",
        ),
        (
            "prerelease identity dropped",
            CHECKER.RELEASE,
            '--prerelease="$expected_prerelease"',
            '--prerelease=false',
            "draft prerelease identity lacks",
        ),
        (
            "host-side cross auditable",
            CHECKER.RELEASE,
            "cross build --locked",
            "cross auditable build --locked",
            "cross auditable runs on the host",
        ),
        (
            "missing auditable decode",
            CHECKER.RELEASE,
            'rust-audit-info "$binary"',
            'printf "{}" > "$RUNNER_TEMP/auditable-info.json"',
            "artifact build lacks rust-audit-info",
        ),
        (
            "directory-mode binary SBOM",
            CHECKER.RELEASE,
            "file: ${{ steps.artifact.outputs.bin }}",
            "path: ${{ steps.artifact.outputs.bin }}",
            "must scan the binary as a file",
        ),
        (
            "public release asset mutation",
            CHECKER.RELEASE,
            (
                "if [[ \"$(jq -r '.isDraft' <<<\"$release_json\")\" != \"true\" ]]; then\n"
                "            echo \"::error::${RELEASE_TAG} is public; refusing to mutate its assets\""
            ),
            (
                "if [[ \"$(jq -r '.published' <<<\"$release_json\")\" != \"false\" ]]; then\n"
                "            echo \"::error::${RELEASE_TAG} is public; refusing to mutate its assets\""
            ),
            "draft upload lacks a pre-mutation jq -r",
        ),
        (
            "implicit gh repository",
            CHECKER.RELEASE,
            "GH_REPO: ${{ github.repository }}",
            "GH_REPO: ''",
            "GitHub CLI calls lack an explicit repository",
        ),
        (
            "TruffleHog event head",
            CHECKER.SECURITY,
            "head: ${{ inputs.release_sha }}",
            "head: ${{ github.sha }}",
            "TruffleHog release scan is not bound",
        ),
        (
            "mutable TruffleHog runtime",
            CHECKER.SECURITY,
            "version: 3.97.0",
            "version: latest",
            "TruffleHog runtime is not pinned",
        ),
        (
            "mutable Gitleaks runtime",
            CHECKER.SECURITY,
            "GITLEAKS_VERSION: 8.24.3",
            "GITLEAKS_VERSION: latest",
            "Gitleaks runtime is not pinned",
        ),
        (
            "event-name release discriminator",
            CHECKER.SECURITY,
            "if: inputs.release_sha != '' && github.event_name == 'push'",
            "if: github.event_name == 'workflow_call'",
            "Gitleaks release-history scan is missing",
        ),
        (
            "self-only Gitleaks range",
            CHECKER.SECURITY,
            '--log-opts="--full-history ${RELEASE_SHA}"',
            '--log-opts="${RELEASE_SHA}^..${RELEASE_SHA}"',
            "release-history scan lacks --full-history",
        ),
        (
            "shallow secret history",
            CHECKER.SECURITY,
            (
                "with:\n          fetch-depth: 0\n          persist-credentials: false\n"
                "          ref: ${{ inputs.release_sha || github.sha }}"
            ),
            (
                "with:\n          fetch-depth: 1\n          persist-credentials: false\n"
                "          ref: ${{ inputs.release_sha || github.sha }}"
            ),
            "TruffleHog release scan lacks full history",
        ),
        (
            "missing runtime shallow guard",
            CHECKER.SECURITY,
            "git rev-parse --is-shallow-repository",
            "git rev-parse --show-toplevel",
            "release-history scan lacks --is-shallow-repository",
        ),
        (
            "Cross pre-build list",
            CHECKER.CROSS_CONFIG,
            'pre-build = "./scripts/install-cargo-auditable-cross.sh"',
            'pre-build = ["./scripts/install-cargo-auditable-cross.sh"]',
            "pre-build must be the scalar copied-script form",
        ),
        (
            "manual all-tags push",
            Path("docs/RELEASING.md"),
            "git push origin main\ngit push origin refs/tags/v0.11.0",
            "git push origin main --tags",
            "manual release pushes an unbounded tag set",
        ),
        (
            "multi-asset consumer glob",
            CHECKER.DEPLOY,
            '--pattern "$versioned_asset"',
            '--pattern "${asset_name}-*"',
            "multi-asset glob",
        ),
        (
            "download fallback to source build",
            CHECKER.DEPLOY,
            'die "Download requested for ${DOWNLOAD_VERSION}; refusing an unrequested source build"',
            'log "Download failed; proceeding with local build"\n        BUILD=true',
            "explicit release download silently source-builds",
        ),
        (
            "draft download accepted",
            CHECKER.DEPLOY,
            'gh release view "$version"',
            'gh release inspect "$version"',
            "exact verified download lacks",
        ),
        (
            "rollback mode loss",
            CHECKER.DEPLOY,
            'install -m 0755 -- "$backup" "$rollback_tmp"',
            'cp -- "$backup" "$rollback_tmp"',
            "exact verified download lacks",
        ),
        (
            "deploy service-state probe loss",
            CHECKER.DEPLOY,
            "service_state=$(probe_service_state)",
            "service_state=inactive",
            "exact verified download lacks",
        ),
        (
            "rollback candidate validation loss",
            CHECKER.DEPLOY,
            'smoke_test "$rollback_tmp"',
            'test -x "$rollback_tmp"',
            "exact verified download lacks",
        ),
        (
            "tag used as asset version",
            Path("README.md"),
            'VERSION="${TAG#v}"',
            'VERSION="$TAG"',
            "README.md: install contract lacks",
        ),
    )

    for label, relative, old, new, diagnostic in cases:
        def mutate(root: Path, *, path: Path = relative, before: str = old, after: str = new) -> None:
            mutate_text(root, path, before, after)

        errors = run_mutation(mutate)
        expect(
            any(diagnostic in error for error in errors),
            f"{label} should fail with {diagnostic!r}: {errors}",
        )


def test_public_release_config_fails() -> None:
    def mutate(root: Path) -> None:
        path = root / CHECKER.CONFIG
        config = json.loads(path.read_text(encoding="utf-8"))
        config["draft"] = False
        path.write_text(json.dumps(config), encoding="utf-8")

    errors = run_mutation(mutate)
    expect(
        any("draft must be true" in error for error in errors),
        f"public release config should fail: {errors}",
    )


def main() -> int:
    for test in (
        test_live_contract_passes,
        test_text_mutations_fail,
        test_public_release_config_fails,
    ):
        test()

    if FAILURES:
        print(f"FAIL: {len(FAILURES)} assertion(s) failed", file=sys.stderr)
        for failure in FAILURES:
            print(f"  - {failure}", file=sys.stderr)
        return 1
    print("OK: all release artifact routing tests passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
