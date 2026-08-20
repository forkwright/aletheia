#!/usr/bin/env python3
"""Behavioral fixtures for scripts/substance-audit.py."""

from __future__ import annotations

import importlib.util
import json
import os
import shutil
import tempfile
from pathlib import Path
from types import SimpleNamespace

SCRIPT = Path(__file__).with_name("substance-audit.py")
SPEC = importlib.util.spec_from_file_location("substance_audit", SCRIPT)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot import {SCRIPT}")
AUDIT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(AUDIT)

SHA = "a" * 40
FAILURES: list[str] = []
TRUSTED_FIXTURE_PARENT = SCRIPT.parents[1].resolve(strict=True)


def expect(condition: bool, message: str) -> None:
    if not condition:
        FAILURES.append(message)


def _fixture_root(raw_path: str) -> Path:
    """Rebuild TemporaryDirectory's generated name beneath a trusted parent."""
    safe_name = os.path.basename(raw_path)
    candidate = (TRUSTED_FIXTURE_PARENT / safe_name).resolve(strict=True)
    if candidate.parent != TRUSTED_FIXTURE_PARENT or not candidate.is_dir():
        raise ValueError(f"fixture root escaped trusted parent: {raw_path!r}")
    return candidate


def _fixture_path(root: Path, path: Path) -> Path:
    """Return a normalized fixture path whose every component stays under root."""
    safe_root_name = os.path.basename(os.fspath(root))
    resolved_root = (TRUSTED_FIXTURE_PARENT / safe_root_name).resolve(strict=True)
    if (
        resolved_root != root.resolve(strict=True)
        or resolved_root.parent != TRUSTED_FIXTURE_PARENT
    ):
        raise ValueError(f"untrusted fixture root: {root}")
    try:
        relative = path.relative_to(root)
    except ValueError as exc:
        raise ValueError(f"fixture path escapes root: {path}") from exc
    safe_parts: list[str] = []
    for part in relative.parts:
        safe_part = os.path.basename(part)
        if safe_part != part or safe_part in {"", ".", ".."}:
            raise ValueError(f"unsafe fixture path component: {part!r}")
        safe_parts.append(safe_part)
    candidate = resolved_root.joinpath(*safe_parts).resolve(strict=False)
    if resolved_root not in candidate.parents:
        raise ValueError(f"fixture path resolves outside root: {path}")
    return candidate


def _write_text(root: Path, path: Path, value: str) -> None:
    validated = _fixture_path(root, path)
    safe_root_name = os.path.basename(os.fspath(root))
    relative = validated.relative_to(root)
    safe_parts = [os.path.basename(part) for part in relative.parts]
    safe_path = (TRUSTED_FIXTURE_PARENT / safe_root_name).joinpath(*safe_parts)
    if safe_path != validated or any(
        part in {"", ".", ".."} for part in safe_parts
    ):
        raise ValueError(f"fixture write path was not canonical: {path}")
    safe_path.parent.mkdir(parents=True, exist_ok=True)
    safe_path.write_text(value, encoding="utf-8")


def write_repo(root: Path) -> Path:
    _write_text(
        root,
        root / "rust-toolchain.toml",
        '[toolchain]\nchannel = "1.97.1"\n',
    )
    paths = {
        "symbolon": ["crates/symbolon/src"],
        "organon": ["crates/organon/src/sandbox"],
        "episteme": [
            "crates/episteme/src/recall",
            "crates/episteme/src/conflict.rs",
        ],
        "krites": ["crates/krites/src/fixed_rule/algos"],
        "nous": ["crates/nous/src"],
    }
    for crate, required in paths.items():
        crate_root = root / "crates" / crate
        (crate_root / "src").mkdir(parents=True, exist_ok=True)
        _write_text(
            root,
            crate_root / "Cargo.toml",
            f'[package]\nname = "{crate}"\nversion = "1.2.3"\n',
        )
        _write_text(
            root,
            crate_root / "src" / "lib.rs",
            "pub fn live() -> bool { true }\n",
        )
        for raw in required:
            path = root / raw
            if path.suffix:
                path.parent.mkdir(parents=True, exist_ok=True)
                _write_text(root, path, "pub fn critical() {}\n")
            else:
                path.mkdir(parents=True, exist_ok=True)

    policy = root / "scripts" / "substance-audit-policy.toml"
    policy.parent.mkdir(parents=True, exist_ok=True)
    _write_text(
        root,
        policy,
        """schema_version = 1

[tools]
kanon_tag = "v0.13.0"
kanon_commit = "6795565b0ae3368faa0b710608dfeabe1f70fafb"
kanon_version = "0.13.0"
kanon_rust = "1.97.1"
cargo_mutants_version = "27.1.0"

[execution]
mutant_jobs = 4
per_mutant_timeout_seconds = 300
wall_timeout_minutes = 330
job_timeout_minutes = 360
artifact_retention_days = 90

[crates.symbolon]
path = "crates/symbolon"
features = ["keyring"]
critical_paths = ["crates/symbolon/src"]

[crates.organon]
path = "crates/organon"
features = []
critical_paths = ["crates/organon/src/sandbox"]

[crates.episteme]
path = "crates/episteme"
features = ["storage-fjall"]
critical_paths = ["crates/episteme/src/recall", "crates/episteme/src/conflict.rs"]

[crates.krites]
path = "crates/krites"
features = []
critical_paths = ["crates/krites/src/fixed_rule/algos"]

[crates.nous]
path = "crates/nous"
features = []
critical_paths = []
""",
    )
    workflow = root / ".github/workflows/substance-audit.yml"
    workflow.parent.mkdir(parents=True, exist_ok=True)
    safe_workflow = _fixture_path(root, workflow)
    safe_workflow.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(
        SCRIPT.parents[1] / ".github/workflows/substance-audit.yml", safe_workflow
    )
    return _fixture_path(root, policy)


def report(crate: str, *, mutation: str = "PASS", taut: str = "PASS", config: str = "PASS") -> dict[str, object]:
    checks = [
        {"name": "mutation", "result": mutation, "evidence": "fixture"},
        {"name": "tautological_doc", "result": taut, "evidence": "fixture"},
        {"name": "always_default_config", "result": config, "evidence": "fixture"},
    ]
    return {
        "crate_path": f"crates/{crate}",
        "crate_name": crate,
        "checks": checks,
        "pass_count": sum(item["result"] == "PASS" for item in checks),
        "fail_count": sum(item["result"] == "FAIL" for item in checks),
        "needs_human_count": sum(item["result"] == "NEEDS_HUMAN" for item in checks),
    }


def outcomes(crate: str, summary: str = "CaughtMutant", path: str | None = None) -> dict[str, object]:
    counts = {
        "MissedMutant": "missed",
        "CaughtMutant": "caught",
        "Timeout": "timeout",
        "Unviable": "unviable",
        "Success": "success",
    }
    features = {
        "symbolon": ["keyring"],
        "episteme": ["storage-fjall"],
    }.get(crate, [])
    common = [
        "--verbose",
        f"--package={crate}@1.2.3",
        *[f"--features={feature}" for feature in features],
        "--locked",
    ]
    build_status: object = "Success"
    test_status: object | None
    if summary == "Unviable":
        build_status = {"Failure": 101}
        test_status = None
    elif summary == "Timeout":
        test_status = "Timeout"
    elif summary == "CaughtMutant":
        test_status = {"Failure": 101}
    elif summary in ("MissedMutant", "Success"):
        test_status = "Success"
    else:
        test_status = "Other"
    phases: list[dict[str, object]] = [
        {
            "phase": "Build",
            "duration": 1.0,
            "process_status": build_status,
            "argv": ["cargo", "test", "--no-run", *common],
        }
    ]
    if test_status is not None:
        phases.append(
            {
                "phase": "Test",
                "duration": 2.0,
                "process_status": test_status,
                "argv": ["cargo", "test", *common],
            }
        )
    value: dict[str, object] = {
        "outcomes": [
            {
                "scenario": {
                    "Mutant": {
                        "name": f"fixture mutant in {crate}",
                        "package": crate,
                        "file": path or f"crates/{crate}/src/lib.rs",
                        "function": None,
                        "span": {"start": {"line": 1, "column": 1}, "end": {"line": 1, "column": 2}},
                        "replacement": "false",
                        "genre": "FnValue",
                    }
                },
                "summary": summary,
                "phase_results": phases,
                "log_path": "log/1.log",
                "diff_path": "diff/1.diff",
            }
        ],
        "total_mutants": 1,
        "missed": 0,
        "caught": 0,
        "timeout": 0,
        "unviable": 0,
        "success": 0,
        "cargo_mutants_version": "27.1.0",
        "start_time": "2026-08-20T00:00:00Z",
        "end_time": "2026-08-20T00:01:00Z",
    }
    if summary in counts:
        value[counts[summary]] = 1
    return value


def write_json(root: Path, path: Path, value: object) -> None:
    _write_text(root, path, json.dumps(value))


def classify_fixture(
    root: Path,
    policy: Path,
    crate: str,
    *,
    report_value: dict[str, object] | None = None,
    outcomes_value: dict[str, object] | None = None,
    audit_exit: int = 0,
) -> dict[str, object]:
    safe_crate = os.path.basename(crate)
    if safe_crate != crate or safe_crate not in AUDIT.CRATES:
        raise ValueError(f"invalid fixture crate: {crate!r}")
    evidence = _fixture_path(root, root / "evidence" / safe_crate)
    evidence.mkdir(parents=True, exist_ok=True)
    audit_json = evidence / "audit.json"
    outcomes_json = evidence / "mutants.out" / "outcomes.json"
    metadata = evidence / "tool-metadata.json"
    config = evidence / "mutants.toml"
    baseline_exit = evidence / "baseline-exit.txt"
    audit_exit_path = evidence / "audit-exit.txt"
    clean_exit = evidence / "clean-exit.txt"
    write_json(root, audit_json, report_value or report(safe_crate))
    write_json(root, outcomes_json, outcomes_value or outcomes(safe_crate))
    write_json(
        root,
        metadata,
        {
            "kanon_version": "0.13.0",
            "kanon_build_sha": "6795565b0ae3368faa0b710608dfeabe1f70fafb",
            "cargo_mutants_version": "27.1.0",
            "rustc_version": "1.97.1",
            "kanon_binary_sha256": "b" * 64,
        },
    )
    features = {
        "symbolon": ["keyring"],
        "episteme": ["storage-fjall"],
    }.get(crate, [])
    _write_text(root, config, AUDIT.render_mutants_config(features))
    _write_text(root, baseline_exit, "0\n")
    _write_text(root, audit_exit_path, f"{audit_exit}\n")
    _write_text(root, clean_exit, "0\n")
    args = SimpleNamespace(
        repo_root=root,
        policy=policy,
        crate=safe_crate,
        repo_sha=SHA,
        release_pr=6902,
        source_run_id="123",
        source_run_url="https://github.com/forkwright/aletheia/actions/runs/123",
        audit_json=audit_json,
        outcomes_json=outcomes_json,
        audit_exit=audit_exit_path,
        baseline_exit=baseline_exit,
        clean_exit=clean_exit,
        tool_metadata=metadata,
        mutants_config=config,
    )
    return AUDIT.classify(args)


def write_receipts(root: Path, policy: Path, overrides: dict[str, dict[str, object]] | None = None) -> list[Path]:
    paths: list[Path] = []
    for crate in AUDIT.CRATES:
        value = classify_fixture(root, policy, crate)
        if overrides and crate in overrides:
            value.update(overrides[crate])
        path = root / "evidence" / crate / "receipt.json"
        write_json(root, path, value)
        paths.append(path)
    return paths


def aggregate_fixture(
    root: Path,
    policy: Path,
    receipts: list[Path],
    issue_map: dict[str, str] | None = None,
) -> dict[str, object]:
    args = SimpleNamespace(
        repo_root=root,
        policy=policy,
        receipts=receipts,
        repo_sha=SHA,
        release_pr=6902,
        source_run_id="123",
        run_url="https://github.com/forkwright/aletheia/actions/runs/123",
        advisory_issues_json=json.dumps(issue_map or {}),
    )
    return AUDIT.aggregate(args)


def test_fixture_path_containment(root: Path, policy: Path) -> None:
    safe_path = _fixture_path(root, root / "safe-child")
    expect(safe_path.parent == root, "contained fixture path should resolve")

    escape_link = root / "escape-link"
    escape_link.symlink_to(SCRIPT)
    for path in (root / ".." / "escape", SCRIPT, escape_link):
        try:
            _fixture_path(root, path)
        except ValueError:
            continue
        expect(False, f"fixture containment accepted {path}")

    for crate in ("../symbolon", "unknown"):
        try:
            classify_fixture(root, policy, crate)
        except ValueError:
            continue
        expect(False, f"fixture crate validation accepted {crate!r}")


def main() -> int:
    with tempfile.TemporaryDirectory(
        prefix=".substance-audit-", dir=TRUSTED_FIXTURE_PARENT
    ) as tmp:
        root = _fixture_root(tmp)
        policy_path = write_repo(root)
        test_fixture_path_containment(root, policy_path)
        policy, errors = AUDIT.load_policy(policy_path, root)
        expect(not errors, f"valid policy failed: {errors}")
        workflow_errors = AUDIT.validate_workflow_contract(policy, root)
        expect(not workflow_errors, f"valid workflow contract failed: {workflow_errors}")
        workflow_path = root / ".github/workflows/substance-audit.yml"
        workflow_text = workflow_path.read_text(encoding="utf-8")
        _write_text(
            root,
            workflow_path,
            workflow_text.replace(
                "ref: 6795565b0ae3368faa0b710608dfeabe1f70fafb",
                "ref: " + "0" * 40,
                1,
            ),
        )
        expect(
            any("exact Kanon commit" in error for error in AUDIT.validate_workflow_contract(policy, root)),
            "workflow validator accepted the wrong private Kanon commit",
        )
        _write_text(
            root,
            workflow_path,
            workflow_text.replace(
                'CARGO_MUTANTS_OUTPUT="$RESULT_DIR"',
                'CARGO_MUTANTS_OUTPUT="$RESULT_DIR/mutants.out"',
                1,
            ),
        )
        expect(
            any(
                "cargo-mutants output parent" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted a nested cargo-mutants output parent",
        )
        _write_text(
            root,
            workflow_path,
            workflow_text.replace("MUTANT_JOBS: 4", "MUTANT_JOBS: 5", 1),
        )
        expect(
            any(
                "mutation jobs" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted mutation-job drift",
        )
        _write_text(
            root,
            workflow_path,
            workflow_text.replace(
                "check-release-versioning.py verify-comparison",
                "check-release-versioning.py check",
                1,
            ),
        )
        expect(
            any(
                "immutable release comparison" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted removal of immutable comparison validation",
        )
        _write_text(
            root,
            workflow_path,
            workflow_text.replace(
                "compare/${GITHUB_SHA}...${EXPECTED_SHA}",
                "compare/main...${EXPECTED_SHA}",
                1,
            ),
        )
        expect(
            any(
                "immutable comparison route" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted a mutable comparison base",
        )
        _write_text(
            root,
            workflow_path,
            workflow_text.replace(
                '--base-sha "$GITHUB_SHA"',
                '--base-sha "$EXPECTED_SHA"',
                1,
            ),
        )
        expect(
            any(
                "trusted comparison base binding" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted the wrong comparison base binding",
        )
        _write_text(
            root,
            workflow_path,
            workflow_text.replace(
                '--candidate-sha "$EXPECTED_SHA"',
                '--candidate-sha "$GITHUB_SHA"',
                1,
            ),
        )
        expect(
            any(
                "trusted comparison candidate binding" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted the wrong comparison candidate binding",
        )
        _write_text(
            root,
            workflow_path,
            workflow_text.replace(
                '<<<"$compare_json"',
                '<<<"$compare_json" || true',
                1,
            ),
        )
        expect(
            any(
                "immutable comparison step must remain exact" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted suppression of comparison failure",
        )
        _write_text(
            root,
            workflow_path,
            workflow_text.replace(
                "Bind the open Release Please PR to current main\n",
                "Bind the open Release Please PR to current main\n"
                "        continue-on-error: true\n",
                1,
            ),
        )
        expect(
            any(
                "release PR binding step must fail closed" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted continue-on-error on candidate admission",
        )
        _write_text(
            root,
            workflow_path,
            workflow_text.replace(
                "  preflight:\n",
                "  preflight:\n    continue-on-error: true\n",
                1,
            ),
        )
        expect(
            any(
                "preflight admission job must fail closed" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted job-level preflight suppression",
        )
        _write_text(
            root,
            workflow_path,
            workflow_text.replace(
                "- name: Bind the open Release Please PR to current main\n"
                "        env:",
                "- name: Bind the open Release Please PR to current main\n"
                "        if: false\n"
                "        env:",
                1,
            ),
        )
        expect(
            any(
                "preflight steps must not be conditional" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted a skipped candidate-admission step",
        )
        _write_text(
            root,
            workflow_path,
            workflow_text.replace(
                "\nenv:\n  GH_REPO:",
                "\ndefaults:\n  run:\n    shell: bash {0}\n\nenv:\n  GH_REPO:",
                1,
            ),
        )
        expect(
            any(
                "must not override the default run shell" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted a fail-open workflow shell override",
        )
        _write_text(
            root,
            workflow_path,
            workflow_text.replace(
                "  preflight:\n",
                "  preflight:\n"
                "    defaults:\n"
                "      run:\n"
                "        shell: bash {0}\n",
                1,
            ),
        )
        expect(
            any(
                "preflight admission job must fail closed" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted a fail-open preflight shell override",
        )
        _write_text(
            root,
            workflow_path,
            workflow_text.replace(
                '          compare_json="$(gh api "repos/${GH_REPO}/compare/',
                "          cat >/dev/null <<'REVIEW_EOF'\n"
                '          compare_json="$(gh api "repos/${GH_REPO}/compare/',
                1,
            ).replace(
                '            <<<"$compare_json"',
                '            <<<"$compare_json"\n          REVIEW_EOF',
                1,
            ),
        )
        expect(
            any(
                "immutable comparison step must remain exact" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted an inert comparison heredoc",
        )
        candidate_checkout = (
            "          path: release-candidate\n"
            "          persist-credentials: false\n"
            "          ref: ${{ inputs.expected_sha }}"
        )
        _write_text(
            root,
            workflow_path,
            workflow_text.replace(
                candidate_checkout,
                candidate_checkout.replace(
                    "${{ inputs.expected_sha }}", "${{ github.sha }}"
                ),
                1,
            ),
        )
        expect(
            any(
                "must inspect the exact candidate checkout" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted validation of the wrong candidate tree",
        )
        _write_text(
            root,
            workflow_path,
            workflow_text.replace("    needs: [preflight]\n", "", 1),
        )
        expect(
            any(
                "audit job must depend on preflight" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted audit execution without preflight",
        )
        _write_text(
            root,
            workflow_path,
            workflow_text.replace(
                "    if: inputs.source_run_id == ''",
                "    if: always() && inputs.source_run_id == ''",
                1,
            ),
        )
        expect(
            any(
                "must retain the implicit successful-preflight condition" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted audit execution after failed preflight",
        )
        _write_text(
            root,
            workflow_path,
            workflow_text.replace(
                "matrix: ${{ fromJSON(needs.preflight.outputs.matrix) }}",
                "matrix: ${{ fromJSON(inputs.matrix) }}",
                1,
            ),
        )
        expect(
            any(
                "audit matrix must come from preflight" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted an untrusted audit matrix",
        )
        _write_text(
            root,
            workflow_path,
            workflow_text.replace(
                "    permissions:\n      contents: read\n    steps:\n",
                "    permissions:\n"
                "      contents: write\n"
                "      id-token: write\n"
                "    steps:\n",
                1,
            ),
        )
        expect(
            any(
                "audit permissions must remain exact" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted expanded candidate-audit permissions",
        )
        audit_control_checkout = (
            "          path: control\n"
            "          persist-credentials: false\n"
            "          ref: ${{ github.sha }}"
        )
        _write_text(
            root,
            workflow_path,
            workflow_text.replace(
                audit_control_checkout,
                audit_control_checkout.replace(
                    "${{ github.sha }}", "${{ inputs.expected_sha }}"
                ),
                1,
            ),
        )
        expect(
            any(
                "audit control must use the trusted base" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted candidate code as audit control",
        )
        _write_text(
            root,
            workflow_path,
            workflow_text.replace(
                "- name: Scrub private source and credentials before Aletheia execution\n"
                "        run:",
                "- name: Scrub private source and credentials before Aletheia execution\n"
                "        if: false\n"
                "        run:",
                1,
            ),
        )
        expect(
            any(
                "private-source scrub must remain exact" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted candidate execution without private scrub",
        )
        for expression in (
            "${{ secrets.FLEET_REPO_TOKEN }}",
            "${{ secrets['FLEET_REPO_TOKEN'] }}",
            "${{ github['token'] }}",
            "${{ toJSON(secrets) }}",
        ):
            _write_text(
                root,
                workflow_path,
                workflow_text.replace(
                    "- name: Run the exact feature-world baseline and substance audit\n"
                    "        env:\n",
                    "- name: Run the exact feature-world baseline and substance audit\n"
                    "        env:\n"
                    f"          LEAKED_CREDENTIAL: {expression}\n",
                    1,
                ),
            )
            expect(
                any(
                    "must not expose credentials after the private-source scrub"
                    in error
                    for error in AUDIT.validate_workflow_contract(policy, root)
                ),
                "workflow validator accepted post-scrub credential expression "
                f"{expression}",
            )
        private_start = workflow_text.index(
            "      - uses: actions/checkout@"
            "3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1\n"
            "        with:\n"
            "          repository: forkwright/kanon"
        )
        scrub_start = workflow_text.index(
            "      - name: Scrub private source and credentials before "
            "Aletheia execution",
            private_start,
        )
        target_start = workflow_text.index(
            "      - uses: actions/checkout@"
            "3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1\n"
            "        with:\n"
            "          path: target",
            scrub_start,
        )
        private_build_block = workflow_text[private_start:scrub_start]
        scrub_block = workflow_text[scrub_start:target_start]
        _write_text(
            root,
            workflow_path,
            workflow_text[:private_start]
            + scrub_block
            + private_build_block
            + workflow_text[target_start:],
        )
        expect(
            any(
                "audit step order must remain exact" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted private checkout after the scrub boundary",
        )
        _write_text(
            root,
            workflow_path,
            workflow_text.replace(
                "always() && needs.preflight.result == 'success' &&",
                "always() &&",
                1,
            ),
        )
        expect(
            any(
                "aggregate job must require successful preflight" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted aggregation after failed preflight",
        )
        aggregate_start = workflow_text.index("\n  aggregate:")
        aggregate_text = workflow_text[aggregate_start:]
        aggregate_control_checkout = (
            "          persist-credentials: false\n"
            "          ref: ${{ github.sha }}"
        )
        _write_text(
            root,
            workflow_path,
            workflow_text[:aggregate_start]
            + aggregate_text.replace(
                aggregate_control_checkout,
                aggregate_control_checkout.replace(
                    "${{ github.sha }}", "${{ inputs.expected_sha }}"
                ),
                1,
            ),
        )
        expect(
            any(
                "must bind trusted control and exact candidate" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted candidate code as aggregate control",
        )
        aggregate_candidate_checkout = (
            "          path: release-candidate\n"
            "          persist-credentials: false\n"
            "          ref: ${{ inputs.expected_sha }}"
        )
        _write_text(
            root,
            workflow_path,
            workflow_text[:aggregate_start]
            + aggregate_text.replace(
                aggregate_candidate_checkout,
                aggregate_candidate_checkout.replace(
                    "${{ inputs.expected_sha }}", "${{ github.sha }}"
                ),
                1,
            ),
        )
        expect(
            any(
                "must bind trusted control and exact candidate" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted base code as the aggregate candidate",
        )
        _write_text(
            root,
            workflow_path,
            workflow_text.replace(
                "  aggregate:\n",
                "  aggregate:\n    continue-on-error: true\n",
                1,
            ),
        )
        expect(
            any(
                "aggregate job keys must remain exact" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted job-level aggregate failure suppression",
        )
        aggregate_permissions = (
            "    permissions:\n"
            "      actions: read\n"
            "      contents: read\n"
            "      pull-requests: write\n"
        )
        _write_text(
            root,
            workflow_path,
            workflow_text.replace(
                aggregate_permissions,
                "    permissions:\n"
                "      actions: write\n"
                "      contents: write\n"
                "      id-token: write\n"
                "      pull-requests: write\n",
                1,
            ),
        )
        expect(
            any(
                "aggregate permissions must remain exact" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted expanded aggregate permissions",
        )
        enforce_marker = "- name: Enforce release substance policy\n"
        for insertion, label in (
            ("        if: false\n", "skipped"),
            ("        continue-on-error: true\n", "failure-suppressed"),
        ):
            _write_text(
                root,
                workflow_path,
                workflow_text.replace(
                    enforce_marker,
                    enforce_marker + insertion,
                    1,
                ),
            )
            expect(
                any(
                    "final policy enforcement step must remain exact" in error
                    for error in AUDIT.validate_workflow_contract(policy, root)
                ),
                f"workflow validator accepted a {label} final policy decision",
            )
        update_marker = "- name: Rebind the receipt before updating the release PR\n"
        for insertion, label in (
            ("        if: false\n", "skipped"),
            ("        continue-on-error: true\n", "failure-suppressed"),
        ):
            _write_text(
                root,
                workflow_path,
                workflow_text.replace(
                    update_marker,
                    update_marker + insertion,
                    1,
                ),
            )
            expect(
                any(
                    "PR receipt update step must remain exact" in error
                    for error in AUDIT.validate_workflow_contract(policy, root)
                ),
                f"workflow validator accepted a {label} PR receipt update",
            )
        update_index = workflow_text.index(update_marker)
        update_prefix = workflow_text[:update_index]
        update_block = workflow_text[update_index:]
        head_binding = (
            "          test \"$(jq -r '.head.sha' <<<\"$pr_json\")\" = "
            "\"$EXPECTED_SHA\""
        )
        last_binding = update_block.rfind(head_binding)
        expect(last_binding >= 0, "fixture could not find the post-edit head binding")
        if last_binding >= 0:
            _write_text(
                root,
                workflow_path,
                update_prefix
                + update_block[:last_binding]
                + "          true"
                + update_block[last_binding + len(head_binding) :],
            )
            expect(
                any(
                    "PR receipt update step must remain exact" in error
                    for error in AUDIT.validate_workflow_contract(policy, root)
                ),
                "workflow validator accepted removal of the post-edit head binding",
            )
        upload_marker = "- name: Upload the aggregate release receipt\n"
        for insertion, label in (
            ("        if: false\n", "skipped"),
            ("        continue-on-error: true\n", "failure-suppressed"),
        ):
            _write_text(
                root,
                workflow_path,
                workflow_text.replace(
                    upload_marker,
                    upload_marker + insertion,
                    1,
                ),
            )
            expect(
                any(
                    "aggregate receipt upload must remain exact" in error
                    for error in AUDIT.validate_workflow_contract(policy, root)
                ),
                f"workflow validator accepted a {label} aggregate receipt upload",
            )
        _write_text(
            root,
            workflow_path,
            workflow_text
            + "\n  bypass:\n"
            + "    runs-on: ubuntu-latest\n"
            + "    steps:\n"
            + "      - run: echo unexpected\n",
        )
        expect(
            any(
                "must contain exactly preflight, audit, and aggregate jobs" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted an unrecognized sibling job",
        )
        _write_text(
            root,
            workflow_path,
            workflow_text.replace(
                "--base-root . --candidate-root release-candidate",
                "--base-root . --candidate-root release-candidate || true",
                1,
            ),
        )
        expect(
            any(
                "release transition command block must remain exact" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted suppression of release transition failure",
        )
        for release_pr_reference in (
            "$RELEASE_PR",
            "${{ inputs.release_pr }}",
        ):
            _write_text(
                root,
                workflow_path,
                workflow_text.replace(
                    "scripts/check-release-versioning.py verify-comparison",
                    f'gh api "repos/${{GH_REPO}}/pulls/{release_pr_reference}/files"\n'
                    "          scripts/check-release-versioning.py verify-comparison",
                    1,
                ),
            )
            expect(
                any(
                    "mutable release PR files endpoint" in error
                    for error in AUDIT.validate_workflow_contract(policy, root)
                ),
                "workflow validator accepted the mutable PR files endpoint",
            )
        _write_text(
            root,
            workflow_path,
            workflow_text.replace(
                "Rebind the Release Please PR after candidate validation",
                "Candidate validation complete",
                1,
            ),
        )
        expect(
            any(
                "post-validation release PR rebind" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted removal of the post-validation PR rebind",
        )
        rebind_marker = "Rebind the Release Please PR after candidate validation"
        prefix, rebind_block = workflow_text.split(rebind_marker, 1)
        _write_text(
            root,
            workflow_path,
            prefix
            + rebind_marker
            + rebind_block.replace(
                "test \"$(jq -r '.head.sha' <<<\"$pr_json\")\" = \"$EXPECTED_SHA\"",
                "true",
                1,
            ),
        )
        expect(
            any(
                "post-validation rebind lacks candidate SHA binding" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted a named but vacuous post-validation rebind",
        )
        _write_text(root, workflow_path, workflow_text)
        policy_text = policy_path.read_text(encoding="utf-8")
        _write_text(
            root,
            policy_path,
            policy_text.replace('kanon_tag = "v0.13.0"', 'kanon_tag = "v9.9.9"'),
        )
        _, tag_errors = AUDIT.load_policy(policy_path, root)
        expect(
            any("kanon_tag" in error for error in tag_errors),
            "policy validator accepted a tag/version mismatch",
        )
        for replacement, label in (
            ('path = "../symbolon"', "parent traversal"),
            (f'path = "{SCRIPT.parent.as_posix()}"', "absolute path"),
        ):
            _write_text(
                root,
                policy_path,
                policy_text.replace('path = "crates/symbolon"', replacement, 1),
            )
            _, path_errors = AUDIT.load_policy(policy_path, root)
            expect(
                any("repository-relative path" in error for error in path_errors),
                f"policy validator accepted crate {label}: {path_errors}",
            )

        linked_crate = root / "crates" / "linked-symbolon"
        linked_crate.symlink_to(SCRIPT.parent, target_is_directory=True)
        _write_text(
            root,
            policy_path,
            policy_text.replace(
                'path = "crates/symbolon"',
                'path = "crates/linked-symbolon"',
                1,
            ),
        )
        _, symlink_errors = AUDIT.load_policy(policy_path, root)
        expect(
            any("escapes the audited repository" in error for error in symlink_errors),
            f"policy validator accepted a symlinked crate escape: {symlink_errors}",
        )
        linked_crate.unlink()
        _write_text(root, policy_path, policy_text)

        with tempfile.TemporaryDirectory(
            prefix=".substance-external-", dir=TRUSTED_FIXTURE_PARENT
        ) as external_tmp:
            external_root = _fixture_root(external_tmp)
            escaped_source = external_root / "escaped.rs"
            _write_text(
                external_root,
                escaped_source,
                "/// Returns the escaped secret.\npub fn escaped() {}\n",
            )
            source_link = root / "crates/nous/src/escaped.rs"
            source_link.symlink_to(escaped_source)
            escaped_findings, escaped_errors = AUDIT.scan_tautological_docs(
                root, "crates/nous"
            )
            expect(
                any("symlink or escapes" in error for error in escaped_errors),
                f"source scanner accepted a symlink escape: {escaped_errors}",
            )
            expect(
                not any("escaped secret" in item["text"] for item in escaped_findings),
                "source scanner consumed text through a symlink escape",
            )
            source_link.unlink()

            escaped_directory = external_root / "linked-directory"
            _write_text(
                external_root,
                escaped_directory / "escaped.rs",
                "/// Returns the directory escape.\npub fn escaped_dir() {}\n",
            )
            directory_link = root / "crates/nous/src/linked-directory"
            directory_link.symlink_to(escaped_directory, target_is_directory=True)
            directory_findings, directory_errors = AUDIT.scan_tautological_docs(
                root, "crates/nous"
            )
            expect(
                any(
                    "source directory is a symlink or escapes" in error
                    for error in directory_errors
                ),
                "source scanner silently skipped a symlinked directory escape: "
                f"{directory_errors}",
            )
            expect(
                not any(
                    "directory escape" in item["text"]
                    for item in directory_findings
                ),
                "source scanner consumed text through a directory symlink escape",
            )
            directory_link.unlink()

        original_walk = AUDIT.os.walk

        def failing_walk(*_args: object, onerror: object = None, **_kwargs: object) -> object:
            if callable(onerror):
                onerror(PermissionError(13, "permission denied", "sealed"))
            return iter(())

        AUDIT.os.walk = failing_walk
        try:
            _, traversal_errors = AUDIT.scan_tautological_docs(root, "crates/nous")
        finally:
            AUDIT.os.walk = original_walk
        expect(
            any("cannot traverse source directory" in error for error in traversal_errors),
            f"source scanner silently ignored a traversal failure: {traversal_errors}",
        )
        expect(
            AUDIT.matrix(policy)[0]["features"] == "keyring",
            "matrix lost symbolon feature world",
        )
        expect(
            'features = ["storage-fjall"]' in AUDIT.render_mutants_config(["storage-fjall"]),
            "mutants config lost episteme feature world",
        )

        passed = classify_fixture(root, policy_path, "symbolon")
        expect(passed["status"] == "PASS", f"valid receipt failed: {passed['blockers']}")

        absolute_report = report("symbolon")
        absolute_report["crate_path"] = "/old/runner/work/aletheia/target/crates/symbolon"
        absolute = classify_fixture(
            root, policy_path, "symbolon", report_value=absolute_report
        )
        expect(
            absolute["status"] == "PASS",
            f"runner-independent absolute crate path failed: {absolute['blockers']}",
        )

        critical_outcomes = outcomes(
            "symbolon", "MissedMutant", "crates/symbolon/src/lib.rs"
        )
        critical = classify_fixture(
            root,
            policy_path,
            "symbolon",
            report_value=report("symbolon", mutation="FAIL"),
            outcomes_value=critical_outcomes,
            audit_exit=1,
        )
        expect(
            critical["status"] == "BLOCKED"
            and any(item["kind"] == "mutation" for item in critical["blockers"]),
            "critical missed mutant did not block",
        )

        advisory_outcomes = outcomes(
            "organon", "MissedMutant", "crates/organon/src/lib.rs"
        )
        advisory = classify_fixture(
            root,
            policy_path,
            "organon",
            report_value=report("organon", mutation="FAIL"),
            outcomes_value=advisory_outcomes,
            audit_exit=1,
        )
        expect(
            advisory["status"] == "PASS_WITH_ADVISORIES"
            and advisory["advisories"][0]["owner_key"] == "mutation:organon",
            "noncritical missed mutant was not an advisory",
        )

        critical_doc = root / "crates/episteme/src/conflict.rs"
        _write_text(
            root,
            critical_doc,
            "/// Returns the conflict.\npub fn conflict() {}\n",
        )
        taut = classify_fixture(
            root,
            policy_path,
            "episteme",
            report_value=report("episteme", taut="FAIL"),
            audit_exit=1,
        )
        expect(
            taut["status"] == "BLOCKED"
            and any(item["kind"] == "tautological_doc" for item in taut["blockers"]),
            "critical tautological doc did not block",
        )
        _write_text(root, critical_doc, "pub fn conflict() {}\n")

        human = classify_fixture(
            root,
            policy_path,
            "nous",
            report_value=report("nous", mutation="NEEDS_HUMAN"),
        )
        expect(
            human["status"] == "BLOCKED"
            and any(item["kind"] == "needs_human" for item in human["blockers"]),
            "NEEDS_HUMAN did not block",
        )

        missing_phase_field = outcomes("nous")
        del missing_phase_field["outcomes"][0]["phase_results"][0]["duration"]
        malformed_phase = classify_fixture(
            root, policy_path, "nous", outcomes_value=missing_phase_field
        )
        expect(
            malformed_phase["status"] == "BLOCKED"
            and any("phase 0 fields" in item["detail"] for item in malformed_phase["blockers"]),
            "cargo-mutants phase with missing schema fields did not block",
        )

        contradictory_outcome = outcomes("nous", "CaughtMutant")
        contradictory_outcome["outcomes"][0]["phase_results"][1][
            "process_status"
        ] = "Success"
        contradictory = classify_fixture(
            root, policy_path, "nous", outcomes_value=contradictory_outcome
        )
        expect(
            contradictory["status"] == "BLOCKED"
            and any("differs from derived" in item["detail"] for item in contradictory["blockers"]),
            "outcome summary contradictory to phase status did not block",
        )

        wrong_package_outcome = outcomes("nous")
        wrong_package_outcome["outcomes"][0]["phase_results"][0]["argv"][3] = (
            "--package=other@1.2.3"
        )
        wrong_package = classify_fixture(
            root, policy_path, "nous", outcomes_value=wrong_package_outcome
        )
        expect(
            wrong_package["status"] == "BLOCKED"
            and any("argv" in item["detail"] for item in wrong_package["blockers"]),
            "phase selecting the wrong package did not block",
        )

        malformed_report = report("nous")
        malformed_report["checks"][0].pop("evidence")
        missing_evidence = classify_fixture(
            root, policy_path, "nous", report_value=malformed_report
        )
        expect(
            missing_evidence["status"] == "BLOCKED"
            and any("check fields" in item["detail"] for item in missing_evidence["blockers"]),
            "Kanon check without evidence did not block",
        )

        receipts = write_receipts(root, policy_path)
        aggregate = aggregate_fixture(root, policy_path, receipts)
        expect(aggregate["status"] == "PASS", f"valid aggregate failed: {aggregate['blockers']}")

        advisory_value = classify_fixture(
            root,
            policy_path,
            "organon",
            report_value=report("organon", mutation="FAIL"),
            outcomes_value=outcomes(
                "organon", "MissedMutant", "crates/organon/src/lib.rs"
            ),
            audit_exit=1,
        )
        write_json(root, receipts[1], advisory_value)
        unowned = aggregate_fixture(root, policy_path, receipts)
        expect(
            unowned["status"] == "BLOCKED"
            and any("no verified issue owner" in item["detail"] for item in unowned["blockers"]),
            "unowned advisory did not block aggregate",
        )
        owned = aggregate_fixture(
            root,
            policy_path,
            receipts,
            {"mutation:organon": "https://github.com/forkwright/aletheia/issues/123"},
        )
        expect(
            owned["status"] == "PASS_WITH_ADVISORIES",
            f"owned advisory did not pass: {owned['blockers']}",
        )

        plain_receipts = write_receipts(root, policy_path)
        contradictory_receipt = json.loads(
            plain_receipts[0].read_text(encoding="utf-8")
        )
        contradictory_receipt["status"] = "BLOCKED"
        write_json(root, plain_receipts[0], contradictory_receipt)
        contradictory_aggregate = aggregate_fixture(
            root, policy_path, plain_receipts
        )
        expect(
            contradictory_aggregate["status"] == "BLOCKED"
            and any(
                "contradicts derived" in item["detail"]
                for item in contradictory_aggregate["blockers"]
            ),
            "receipt with BLOCKED status and no blockers was accepted",
        )

        plain_receipts = write_receipts(root, policy_path)
        incomplete_receipt = json.loads(plain_receipts[0].read_text(encoding="utf-8"))
        incomplete_receipt["tools"] = None
        incomplete_receipt["evidence_sha256"] = {}
        write_json(root, plain_receipts[0], incomplete_receipt)
        incomplete_aggregate = aggregate_fixture(root, policy_path, plain_receipts)
        expect(
            incomplete_aggregate["status"] == "BLOCKED"
            and any("evidence_sha256" in item["detail"] for item in incomplete_aggregate["blockers"]),
            "receipt missing tools/raw-evidence hashes was accepted",
        )

        plain_receipts = write_receipts(root, policy_path)
        with (plain_receipts[0].parent / "audit.json").open("a", encoding="utf-8") as stream:
            stream.write("\n")
        tampered_aggregate = aggregate_fixture(root, policy_path, plain_receipts)
        expect(
            tampered_aggregate["status"] == "BLOCKED"
            and any("hash mismatch" in item["detail"] for item in tampered_aggregate["blockers"]),
            "raw evidence changed after classification was accepted",
        )

        global_receipts = write_receipts(root, policy_path)
        global_value = classify_fixture(
            root,
            policy_path,
            "symbolon",
            report_value=report("symbolon", config="FAIL"),
            audit_exit=1,
        )
        write_json(root, global_receipts[0], global_value)
        global_disagreement = aggregate_fixture(
            root,
            policy_path,
            global_receipts,
            {
                "always_default_config:workspace": (
                    "https://github.com/forkwright/aletheia/issues/123"
                )
            },
        )
        expect(
            global_disagreement["status"] == "BLOCKED"
            and any(
                "always_default_config results differ" in item["detail"]
                for item in global_disagreement["blockers"]
            ),
            "cross-crate global detector disagreement was collapsed",
        )

        plain_receipts = write_receipts(root, policy_path)
        plain_aggregate = aggregate_fixture(root, policy_path, plain_receipts)
        aggregate_path = root / "aggregate.json"
        write_json(root, aggregate_path, plain_aggregate)
        enforce_args = SimpleNamespace(
            receipt=aggregate_path,
            policy=policy_path,
            repo_sha=SHA,
            release_pr=6902,
            source_run_id="123",
            run_url="https://github.com/forkwright/aletheia/actions/runs/123",
        )
        expect(AUDIT.cmd_enforce(enforce_args) == 0, "valid aggregate did not enforce")

        bare_path = root / "bare-aggregate.json"
        write_json(root, bare_path, {"status": "PASS"})
        bare_args = SimpleNamespace(**{**vars(enforce_args), "receipt": bare_path})
        expect(
            AUDIT.cmd_enforce(bare_args) == 1,
            "bare PASS aggregate bypassed policy enforcement",
        )

        forged = json.loads(aggregate_path.read_text(encoding="utf-8"))
        forged["receipts"]["symbolon"]["status"] = "BLOCKED"
        write_json(root, bare_path, forged)
        expect(
            AUDIT.cmd_enforce(bare_args) == 1,
            "aggregate PASS ignored a BLOCKED crate receipt summary",
        )

        body = "Release notes\n"
        rendered = AUDIT.render_body(body, owned, "c" * 64)
        rerendered = AUDIT.render_body(rendered, owned, "c" * 64)
        expect(rendered == rerendered, "PR body receipt rendering is not idempotent")

    if FAILURES:
        print(f"FAIL: {len(FAILURES)} substance-audit assertions")
        for failure in FAILURES:
            print(f"  - {failure}")
        return 1
    print("OK: substance-audit policy and receipt boundaries hold")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
