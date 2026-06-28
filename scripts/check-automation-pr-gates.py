#!/usr/bin/env python3
"""Validate automation PR gate policy for CI workflow YAML."""

from pathlib import Path
import sys

import yaml


ROOT = Path(__file__).resolve().parents[1]


def load_workflow(path: str) -> dict:
    workflow_path = ROOT / path
    with workflow_path.open(encoding="utf-8") as handle:
        data = yaml.safe_load(handle)
    if not isinstance(data, dict):
        raise SystemExit(f"{path}: expected a workflow mapping")
    return data


def named_step(workflow: dict, job: str, name: str) -> dict | None:
    for step in workflow["jobs"][job].get("steps", []):
        if step.get("name") == name:
            return step
    return None


def main() -> int:
    errors: list[str] = []

    gate = load_workflow(".github/workflows/gate-attestation.yml")
    gate_steps = gate["jobs"]["gate-attestation"].get("steps", [])
    for step in gate_steps:
        if step.get("name") == "Pass trusted automation PRs":
            errors.append("gate-attestation must not have a bot-author pass step")

        condition = str(step.get("if", ""))
        if "dependabot[bot]" in condition or "release-please[bot]" in condition:
            errors.append(
                "gate-attestation step conditions must not skip Dependabot or "
                "release-please authors"
            )

    gate_step_names = {step.get("name") for step in gate_steps}
    for required in (
        "Configure git credentials for private fleet deps",
        "cargo fmt --check",
        "cargo clippy",
        "cargo nextest run",
    ):
        if required not in gate_step_names:
            errors.append(f"gate-attestation is missing required step: {required}")

    security = load_workflow(".github/workflows/security.yml")
    cargo_deny = security["jobs"]["cargo-deny"]
    if "dependabot[bot]" in str(cargo_deny.get("if", "")):
        errors.append("security cargo-deny job must not skip Dependabot PRs")

    for job_name, job in security["jobs"].items():
        for step in job.get("steps", []):
            run = str(step.get("run", ""))
            if "FLEET_REPO_TOKEN" in run and "exit 0" in run:
                errors.append(
                    f"{job_name} credential setup exits successfully when "
                    "FLEET_REPO_TOKEN is missing"
                )

    auto_merge = load_workflow(".github/workflows/dependabot-auto-merge.yml")
    wait_step = named_step(auto_merge, "auto-merge", "Wait for CI checks to pass")
    if wait_step is None:
        errors.append("dependabot auto-merge is missing the CI wait step")
    else:
        wait_run = str(wait_step.get("run", ""))
        for required in ("gate-attestation", "cargo deny", "cargo audit", "osv"):
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
