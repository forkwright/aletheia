#!/usr/bin/env python3
"""Validate automation PR gate policy for CI workflow YAML."""

from pathlib import Path
import sys
import tomllib

import yaml


ROOT = Path(__file__).resolve().parents[1]

# WHY: maps a kanon.toml [gate].stages entry to the substring a full-gate-build
# step's run command must contain, so the check stays data-driven against the
# shared gate contract instead of a second hardcoded stage list.
STAGE_COMMAND_HINTS = {
    "fmt": "cargo fmt",
    "check": "cargo check",
    "clippy": "cargo clippy",
    "nextest": "cargo nextest",
}

AUTOMATION_LOGINS = ("dependabot[bot]", "release-please[bot]")


def load_workflow(path: str) -> dict:
    workflow_path = ROOT / path
    with workflow_path.open(encoding="utf-8") as handle:
        data = yaml.safe_load(handle)
    if not isinstance(data, dict):
        raise SystemExit(f"{path}: expected a workflow mapping")
    return data


def job_step_text(job: dict) -> str:
    """Concatenate every step's run/if/name/uses/env text for substring checks."""
    chunks = [str(job.get("if", "")), str(job.get("env", ""))]
    for step in job.get("steps", []):
        chunks.append(str(step.get("name", "")))
        chunks.append(str(step.get("if", "")))
        chunks.append(str(step.get("run", "")))
        chunks.append(str(step.get("uses", "")))
        chunks.append(str(step.get("env", "")))
    return "\n".join(chunks)


def named_step(workflow: dict, job: str, name: str) -> dict | None:
    for step in workflow["jobs"][job].get("steps", []):
        if step.get("name") == name:
            return step
    return None


def main() -> int:
    errors: list[str] = []

    gate = load_workflow(".github/workflows/gate-attestation.yml")
    # #6421: gate-attestation is a hybrid — check-trailer is the fast stamp-trust path
    # (local `kanon gate --stamp` Gate-Passed trailer), full-gate-build is the CI-build
    # fallback for trailer-less PRs (re-running the exact kanon.toml [gate].stages),
    # and gate aggregates both. Dependency security stays gated for bots by the cargo
    # audit/deny jobs (which must not waive Dependabot — enforced below). Validate the
    # 3-job shape and that no job silently short-circuits the contract.
    gate_jobs = gate.get("jobs", {})

    check_trailer = gate_jobs.get("check-trailer")
    if check_trailer is None:
        errors.append("gate-attestation.yml must define a check-trailer job")
    else:
        text = job_step_text(check_trailer)
        for login in AUTOMATION_LOGINS:
            if login not in text:
                errors.append(f"check-trailer must waive trusted automation login {login}")
        if "release-please--branches--" not in text:
            errors.append("check-trailer must waive release-please branch-shaped PRs")
        if "Gate-Passed:" not in text:
            errors.append("check-trailer must verify the Gate-Passed trailer")
        if "exit 1" in text:
            errors.append(
                "check-trailer must never exit 1 — a missing trailer is a normal "
                "outcome routed to full-gate-build, not a check-trailer failure"
            )
        outputs = check_trailer.get("outputs", {})
        if "found" not in outputs:
            errors.append("check-trailer must expose an outputs.found")

    full_gate_build = gate_jobs.get("full-gate-build")
    if full_gate_build is None:
        errors.append("gate-attestation.yml must define a full-gate-build job")
    else:
        needs = full_gate_build.get("needs", [])
        needs = [needs] if isinstance(needs, str) else needs
        if "check-trailer" not in needs:
            errors.append("full-gate-build must need check-trailer")
        job_if = str(full_gate_build.get("if", ""))
        if "check-trailer.outputs.found" not in job_if:
            errors.append(
                "full-gate-build must be gated on needs.check-trailer.outputs.found "
                "(skip when a trailer was already found)"
            )
        text = job_step_text(full_gate_build)
        try:
            kanon_toml = tomllib.loads((ROOT / "kanon.toml").read_text(encoding="utf-8"))
            stages = kanon_toml.get("gate", {}).get("stages", [])
        except FileNotFoundError:
            stages = []
        if not stages:
            errors.append("kanon.toml [gate].stages must be non-empty to validate full-gate-build against")
        for stage in stages:
            hint = STAGE_COMMAND_HINTS.get(stage)
            if hint is None:
                errors.append(f"no STAGE_COMMAND_HINTS entry for kanon.toml gate stage '{stage}'")
                continue
            if hint not in text:
                errors.append(
                    f"full-gate-build must run a step covering kanon.toml gate stage "
                    f"'{stage}' ({hint})"
                )
        if "FLEET_REPO_TOKEN" not in text:
            errors.append("full-gate-build must configure FLEET_REPO_TOKEN for private fleet deps")

    gate_job = gate_jobs.get("gate")
    if gate_job is None:
        errors.append("gate-attestation.yml must define a gate aggregator job")
    else:
        needs = gate_job.get("needs", [])
        needs = [needs] if isinstance(needs, str) else needs
        for required_need in ("check-trailer", "full-gate-build"):
            if required_need not in needs:
                errors.append(f"gate aggregator must need {required_need}")
        if str(gate_job.get("if", "")).strip() != "always()":
            errors.append("gate aggregator must run unconditionally (if: always()) to aggregate both paths")
        text = job_step_text(gate_job)
        for login in AUTOMATION_LOGINS:
            if login not in text:
                errors.append(f"gate aggregator must waive trusted automation login {login}")
        if "check-trailer.outputs.found" not in text:
            errors.append("gate aggregator must check needs.check-trailer.outputs.found")
        if "full-gate-build.result" not in text:
            errors.append("gate aggregator must check needs.full-gate-build.result")
        if "exit 1" not in text:
            errors.append("gate aggregator must fail closed (exit 1) when neither path passed")

    security = load_workflow(".github/workflows/security.yml")
    cargo_deny = security["jobs"]["cargo-deny"]
    if "dependabot[bot]" in str(cargo_deny.get("if", "")):
        errors.append("security cargo-deny job must not skip Dependabot PRs")

    for job_name, job in security["jobs"].items():
        for step in job.get("steps", []):
            run = str(step.get("run", ""))
            # WHY: dependabot-triggered runs never receive repo secrets, so a hard-fail
            # here made every bot PR permanently red with NO supply-chain scan at all
            # (the opposite of the original private-deps intent; fleet git deps are
            # public now and fetch anonymously). The step may skip — but only LOUDLY:
            # a silent exit 0 is still forbidden.
            if "FLEET_REPO_TOKEN" in run and "exit 0" in run:
                if "skipping credential setup" not in run:
                    errors.append(
                        f"{job_name} credential setup exits 0 silently when "
                        "FLEET_REPO_TOKEN is missing — must announce the skip "
                        '("skipping credential setup") or fail'
                    )

    auto_merge = load_workflow(".github/workflows/dependabot-auto-merge.yml")
    wait_step = named_step(auto_merge, "auto-merge", "Wait for CI checks to pass")
    if wait_step is None:
        errors.append("dependabot auto-merge is missing the CI wait step")
    else:
        wait_run = str(wait_step.get("run", ""))
        # WHY(#6421): "gate" is the hybrid gate-attestation aggregator job's check
        # name (renamed from the reusable-delegation-derived "gate-attestation" /
        # "gate / gate-attestation") — keep this in sync with the gate job's `name:`.
        for required in ("gate", "cargo deny", "cargo audit", "osv"):
            if required not in wait_run:
                errors.append(
                    "dependabot auto-merge must require real verification check: "
                    f"{required}"
                )
        if "gh pr checks" not in wait_run or "--watch" not in wait_run:
            errors.append("dependabot auto-merge must wait for PR checks")
        if "Required CI checks did not pass" in wait_run and "exit 0" in wait_run:
            errors.append("dependabot auto-merge must fail closed on failed checks")

    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1

    print("Automation PR gate policy valid")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
