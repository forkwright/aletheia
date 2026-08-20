#!/usr/bin/env python3
"""Behavioral fixtures for scripts/substance-audit.py."""

from __future__ import annotations

import importlib.util
import json
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


def expect(condition: bool, message: str) -> None:
    if not condition:
        FAILURES.append(message)


def write_repo(root: Path) -> Path:
    (root / "rust-toolchain.toml").write_text(
        '[toolchain]\nchannel = "1.97.1"\n', encoding="utf-8"
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
        (crate_root / "Cargo.toml").write_text(
            f'[package]\nname = "{crate}"\nversion = "1.2.3"\n',
            encoding="utf-8",
        )
        (crate_root / "src" / "lib.rs").write_text(
            "pub fn live() -> bool { true }\n", encoding="utf-8"
        )
        for raw in required:
            path = root / raw
            if path.suffix:
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("pub fn critical() {}\n", encoding="utf-8")
            else:
                path.mkdir(parents=True, exist_ok=True)

    policy = root / "scripts" / "substance-audit-policy.toml"
    policy.parent.mkdir(parents=True, exist_ok=True)
    policy.write_text(
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
        encoding="utf-8",
    )
    workflow = root / ".github/workflows/substance-audit.yml"
    workflow.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(SCRIPT.parents[1] / ".github/workflows/substance-audit.yml", workflow)
    return policy


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


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value), encoding="utf-8")


def classify_fixture(
    root: Path,
    policy: Path,
    crate: str,
    *,
    report_value: dict[str, object] | None = None,
    outcomes_value: dict[str, object] | None = None,
    audit_exit: int = 0,
) -> dict[str, object]:
    evidence = root / "evidence" / crate
    evidence.mkdir(parents=True, exist_ok=True)
    audit_json = evidence / "audit.json"
    outcomes_json = evidence / "mutants.out" / "outcomes.json"
    metadata = evidence / "tool-metadata.json"
    config = evidence / "mutants.toml"
    baseline_exit = evidence / "baseline-exit.txt"
    audit_exit_path = evidence / "audit-exit.txt"
    clean_exit = evidence / "clean-exit.txt"
    write_json(audit_json, report_value or report(crate))
    write_json(outcomes_json, outcomes_value or outcomes(crate))
    write_json(
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
    config.write_text(AUDIT.render_mutants_config(features), encoding="utf-8")
    baseline_exit.write_text("0\n", encoding="utf-8")
    audit_exit_path.write_text(f"{audit_exit}\n", encoding="utf-8")
    clean_exit.write_text("0\n", encoding="utf-8")
    args = SimpleNamespace(
        repo_root=root,
        policy=policy,
        crate=crate,
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
        write_json(path, value)
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


def main() -> int:
    with tempfile.TemporaryDirectory(prefix="substance-audit-") as tmp:
        root = Path(tmp)
        policy_path = write_repo(root)
        policy, errors = AUDIT.load_policy(policy_path, root)
        expect(not errors, f"valid policy failed: {errors}")
        workflow_errors = AUDIT.validate_workflow_contract(policy, root)
        expect(not workflow_errors, f"valid workflow contract failed: {workflow_errors}")
        workflow_path = root / ".github/workflows/substance-audit.yml"
        workflow_text = workflow_path.read_text(encoding="utf-8")
        workflow_path.write_text(
            workflow_text.replace(
                "ref: 6795565b0ae3368faa0b710608dfeabe1f70fafb",
                "ref: " + "0" * 40,
                1,
            ),
            encoding="utf-8",
        )
        expect(
            any("exact Kanon commit" in error for error in AUDIT.validate_workflow_contract(policy, root)),
            "workflow validator accepted the wrong private Kanon commit",
        )
        workflow_path.write_text(
            workflow_text.replace(
                'CARGO_MUTANTS_OUTPUT="$RESULT_DIR"',
                'CARGO_MUTANTS_OUTPUT="$RESULT_DIR/mutants.out"',
                1,
            ),
            encoding="utf-8",
        )
        expect(
            any(
                "cargo-mutants output parent" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted a nested cargo-mutants output parent",
        )
        workflow_path.write_text(
            workflow_text.replace("MUTANT_JOBS: 4", "MUTANT_JOBS: 5", 1),
            encoding="utf-8",
        )
        expect(
            any(
                "mutation jobs" in error
                for error in AUDIT.validate_workflow_contract(policy, root)
            ),
            "workflow validator accepted mutation-job drift",
        )
        workflow_path.write_text(workflow_text, encoding="utf-8")
        policy_text = policy_path.read_text(encoding="utf-8")
        policy_path.write_text(
            policy_text.replace('kanon_tag = "v0.13.0"', 'kanon_tag = "v9.9.9"'),
            encoding="utf-8",
        )
        _, tag_errors = AUDIT.load_policy(policy_path, root)
        expect(
            any("kanon_tag" in error for error in tag_errors),
            "policy validator accepted a tag/version mismatch",
        )
        policy_path.write_text(policy_text, encoding="utf-8")
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
        critical_doc.write_text("/// Returns the conflict.\npub fn conflict() {}\n", encoding="utf-8")
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
        critical_doc.write_text("pub fn conflict() {}\n", encoding="utf-8")

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
        write_json(receipts[1], advisory_value)
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
        write_json(plain_receipts[0], contradictory_receipt)
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
        write_json(plain_receipts[0], incomplete_receipt)
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
        write_json(global_receipts[0], global_value)
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
        write_json(aggregate_path, plain_aggregate)
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
        write_json(bare_path, {"status": "PASS"})
        bare_args = SimpleNamespace(**{**vars(enforce_args), "receipt": bare_path})
        expect(
            AUDIT.cmd_enforce(bare_args) == 1,
            "bare PASS aggregate bypassed policy enforcement",
        )

        forged = json.loads(aggregate_path.read_text(encoding="utf-8"))
        forged["receipts"]["symbolon"]["status"] = "BLOCKED"
        write_json(bare_path, forged)
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
