#!/usr/bin/env python3
"""Negative fixtures for scripts/check-release-artifact-routing.py."""

from __future__ import annotations

import importlib.util
import json
import os
import shutil
import sys
import tempfile
from copy import deepcopy
from pathlib import Path

SCRIPT_PATH = Path(__file__).parent / "check-release-artifact-routing.py"
REPO_ROOT = SCRIPT_PATH.parents[1]
TRUSTED_FIXTURE_PARENT = REPO_ROOT.resolve(strict=True)
SPEC = importlib.util.spec_from_file_location("check_release_routing", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"failed to load {SCRIPT_PATH}")
CHECKER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECKER)

FAILURES: list[str] = []


def expect(condition: bool, message: str) -> None:
    if not condition:
        FAILURES.append(message)


def fixture_root(raw_path: str) -> Path:
    safe_name = os.path.basename(raw_path)
    candidate = (TRUSTED_FIXTURE_PARENT / safe_name).resolve(strict=True)
    if candidate.parent != TRUSTED_FIXTURE_PARENT or not candidate.is_dir():
        raise ValueError(f"fixture root escaped trusted parent: {raw_path!r}")
    return candidate


def fixture_path(root: Path, relative: Path) -> Path:
    if relative.is_absolute() or not relative.parts:
        raise ValueError(f"fixture path must be repository-relative: {relative}")
    safe_root_name = os.path.basename(os.fspath(root))
    safe_parts = [os.path.basename(part) for part in relative.parts]
    if any(
        safe != original or safe in {"", ".", ".."}
        for safe, original in zip(safe_parts, relative.parts, strict=True)
    ):
        raise ValueError(f"unsafe fixture path: {relative}")
    candidate = (TRUSTED_FIXTURE_PARENT / safe_root_name).joinpath(*safe_parts)
    if candidate.parent != root and root not in candidate.parents:
        raise ValueError(f"fixture path escaped its root: {relative}")
    return candidate


def copy_fixture(root: Path) -> None:
    for relative in (
        CHECKER.RELEASE_PLEASE,
        CHECKER.RELEASE,
        CHECKER.RELEASE_HEALTH,
        CHECKER.GATE,
        CHECKER.SECURITY,
        CHECKER.CONFIG,
        CHECKER.DEPLOY,
        CHECKER.CROSS_CONFIG,
        CHECKER.CROSS_INSTALLER,
        *CHECKER.CONSUMER_DOCS,
    ):
        destination = fixture_path(root, relative)
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(REPO_ROOT / relative, destination)


def mutate_text(root: Path, relative: Path, old: str, new: str) -> None:
    path = fixture_path(root, relative)
    text = path.read_text(encoding="utf-8")
    if old not in text:
        raise RuntimeError(f"fixture mutation source not found in {relative}: {old!r}")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


def run_mutation(mutator: object) -> list[str]:
    with tempfile.TemporaryDirectory(
        prefix=".release-routing-", dir=TRUSTED_FIXTURE_PARENT
    ) as tmp:
        root = fixture_root(tmp)
        copy_fixture(root)
        mutator(root)
        return CHECKER.check_repo(root)


def test_live_contract_passes() -> None:
    errors = CHECKER.check_repo(REPO_ROOT)
    expect(not errors, f"live release contract should pass: {errors}")


def test_text_mutations_fail() -> None:
    cases = (
        (
            "non-terminal release outcome",
            CHECKER.RELEASE,
            "if: ${{ always() }}",
            "if: ${{ success() }}",
            "closed schema",
        ),
        (
            "inert outcome comment",
            CHECKER.RELEASE,
            "run: scripts/check-release-outcome.py --attempts 6 --retry-seconds 10",
            "run: true # scripts/check-release-outcome.py --attempts 6 --retry-seconds 10",
            "closed schema",
        ),
        (
            "quoted outcome command",
            CHECKER.RELEASE,
            "run: scripts/check-release-outcome.py --attempts 6 --retry-seconds 10",
            'run: "\"scripts/check-release-outcome.py\" --attempts 6 --retry-seconds 10"',
            "failed to load",
        ),
        (
            "indirect outcome command",
            CHECKER.RELEASE,
            "run: scripts/check-release-outcome.py --attempts 6 --retry-seconds 10",
            "run: bash -c 'scripts/check-release-outcome.py --attempts 6 --retry-seconds 10'",
            "closed schema",
        ),
        (
            "reordered outcome arguments",
            CHECKER.RELEASE,
            "run: scripts/check-release-outcome.py --attempts 6 --retry-seconds 10",
            "run: scripts/check-release-outcome.py --retry-seconds 10 --attempts 6",
            "closed schema",
        ),
        (
            "outcome command missing an argument",
            CHECKER.RELEASE,
            "run: scripts/check-release-outcome.py --attempts 6 --retry-seconds 10",
            "run: scripts/check-release-outcome.py --attempts 6",
            "closed schema",
        ),
        (
            "outcome command has a second command",
            CHECKER.RELEASE,
            "run: scripts/check-release-outcome.py --attempts 6 --retry-seconds 10",
            "run: scripts/check-release-outcome.py --attempts 6 --retry-seconds 10 && true",
            "closed schema",
        ),
        (
            "alternate outcome invocation",
            CHECKER.RELEASE,
            "run: scripts/check-release-outcome.py --attempts 6 --retry-seconds 10",
            "run: python3 scripts/check-release-outcome.py --attempts 6 --retry-seconds 10",
            "closed schema",
        ),
        (
            "missing scheduled reconciliation",
            CHECKER.RELEASE_HEALTH,
            '- cron: "43 6 * * *"',
            '- cron: "43 7 * * *"',
            "closed schema",
        ),
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
            'rust-audit-info "$binary" > "$audit_info"',
            'printf "{}" > "$audit_info"',
            "auditable evidence handoff is not exact",
        ),
        (
            "unsafe fixed auditable output",
            CHECKER.RELEASE,
            'audit_dir=$(mktemp -d "$GITHUB_WORKSPACE/.release-audit.XXXXXX")',
            'audit_dir="$GITHUB_WORKSPACE"',
            "auditable evidence handoff is not exact",
        ),
        (
            "unexported auditable evidence",
            CHECKER.RELEASE,
            "printf 'AUDITABLE_INFO=%s\\n' \"$audit_info\" >> \"$GITHUB_ENV\"",
            "printf 'AUDITABLE_INFO=%s\\n' \"$audit_info\"",
            "auditable evidence handoff is not exact",
        ),
        (
            "external auditable evidence",
            CHECKER.RELEASE,
            'audit_info="$audit_dir/auditable-info.json"',
            'audit_info="$RUNNER_TEMP/auditable-info.json"',
            "auditable evidence handoff is not exact",
        ),
        (
            "wrong auditable evidence export",
            CHECKER.RELEASE,
            "printf 'AUDITABLE_INFO=%s\\n' \"$audit_info\" >> \"$GITHUB_ENV\"",
            "printf 'AUDITABLE_INFO=%s\\n' \"$binary\" >> \"$GITHUB_ENV\"",
            "auditable evidence handoff is not exact",
        ),
        (
            "step-local auditable evidence override",
            CHECKER.RELEASE,
            "env:\n          VERSION: ${{ steps.version.outputs.version }}\n        run: |\n          scripts/check-auditable-info.py",
            "env:\n          VERSION: ${{ steps.version.outputs.version }}\n          AUDITABLE_INFO: $BINARY\n        run: |\n          scripts/check-auditable-info.py",
            "binary SBOM comparison lacks the exact evidence handoff",
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
            "cross installer redirect downgrade",
            CHECKER.CROSS_INSTALLER,
            "curl --proto '=https' --proto-redir '=https'",
            "curl",
            "cross-image install lacks",
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
            "release download redirect downgrade",
            CHECKER.DEPLOY,
            "curl --proto '=https' --proto-redir '=https'",
            "curl",
            "downloads must remain HTTPS-only",
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


def test_observer_workflows_have_no_execution_extension_points() -> None:
    release = CHECKER.yaml.safe_load((REPO_ROOT / CHECKER.RELEASE).read_text())
    health = CHECKER.yaml.safe_load((REPO_ROOT / CHECKER.RELEASE_HEALTH).read_text())
    for mutate in (
        lambda candidate: candidate["jobs"]["release-outcome"]["steps"][0].clear(),
        lambda candidate: candidate["jobs"]["release-outcome"]["steps"].append({"run": "curl https://example.invalid"}),
        lambda candidate: next(step for step in candidate["jobs"]["release-outcome"]["steps"] if step.get("name") == "Report the release outcome").update({"if": "${{ false }}"}),
        lambda candidate: next(step for step in candidate["jobs"]["release-outcome"]["steps"] if step.get("name") == "Report the release outcome").update({"continue-on-error": True}),
        lambda candidate: next(step for step in candidate["jobs"]["release-outcome"]["steps"] if step.get("name") == "Report the release outcome").update({"working-directory": "missing"}),
        lambda candidate: candidate["jobs"]["release-outcome"].update({"shell": "bash {0}"}),
        lambda candidate: candidate["jobs"]["release-outcome"].update({"container": "evil:latest"}),
        lambda candidate: candidate["jobs"]["release-outcome"].update({"continue-on-error": True}),
        lambda candidate: candidate["jobs"]["release-outcome"].update({"timeout-minutes": 11}),
    ):
        candidate = deepcopy(release)
        mutate(candidate)
        errors = CHECKER._check_release(candidate)
        expect(any("closed schema" in error for error in errors), f"outcome mutation accepted: {errors}")
    for mutate in (
        lambda candidate: candidate["jobs"]["audit"]["steps"][0].clear(),
        lambda candidate: candidate["jobs"]["audit"]["steps"].append({"uses": "evil/action@deadbeef"}),
        lambda candidate: next(step for step in candidate["jobs"]["audit"]["steps"] if step.get("name") == "Reconcile tags against releases").update({"run": "true # scripts/check-release-health.py --grace-hours 12"}),
        lambda candidate: next(step for step in candidate["jobs"]["audit"]["steps"] if step.get("name") == "Reconcile tags against releases").update({"if": "${{ false }}"}),
        lambda candidate: next(step for step in candidate["jobs"]["audit"]["steps"] if step.get("name") == "Reconcile tags against releases").update({"continue-on-error": True}),
        lambda candidate: next(step for step in candidate["jobs"]["audit"]["steps"] if step.get("name") == "Reconcile tags against releases").update({"working-directory": "missing"}),
        lambda candidate: candidate["jobs"]["audit"].update({"env": {"PATH": "bad"}}),
        lambda candidate: candidate["jobs"]["audit"].update({"container": "evil:latest"}),
    ):
        candidate = deepcopy(health)
        mutate(candidate)
        errors = CHECKER._check_release_health(candidate)
        expect(any("closed schema" in error for error in errors), f"health mutation accepted: {errors}")


def main() -> int:
    for test in (
        test_live_contract_passes,
        test_text_mutations_fail,
        test_public_release_config_fails,
        test_observer_workflows_have_no_execution_extension_points,
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
