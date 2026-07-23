#!/usr/bin/env python3
"""Validate automation PR gate policy for CI workflow YAML."""

from pathlib import Path
import sys
import tomllib

import yaml


ROOT = Path(__file__).resolve().parents[1]

# WHY: maps a kanon.toml [gate].stages entry to the hybrid-gate `with:` input
# name and substring that input's command string must contain, so the check
# stays data-driven against the shared gate contract instead of a second
# hardcoded stage list.
STAGE_INPUT_HINTS = {
    "fmt": ("fmt_cmd", "cargo fmt"),
    "check": ("check_cmd", "cargo check"),
    "clippy": ("clippy_cmd", "cargo clippy"),
    "nextest": ("nextest_cmd", "cargo nextest"),
}


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
    # #6421/#6433/kanon#2522: gate-attestation delegates the check-trailer/
    # full-gate-build hybrid mechanism to the fleet-shared hybrid-gate.yml
    # reusable workflow — one fact, one place: the trailer/build/trusted-
    # automation-waiver logic is validated once, centrally, not re-derived
    # per caller repo. This repo's own gate-attestation.yml supplies only:
    # the delegated job's `with:` commands, any repo-local always-on
    # coverage jobs the reusable has no equivalent for (#6433:
    # gate-coverage-scripts, gate-coverage-compile-checks), and a `gate`
    # aggregator that must depend on and check the result of every OTHER job
    # in this file — generic on job name/count, not a hardcoded pair, so a
    # newly added coverage job (or a future rename) can never silently
    # orphan itself from the required check the way #6433 did.
    #
    # WHY the owning repo is NOT checked: the reusable is currently hosted
    # publicly at forkwright/.github (GitHub cannot resolve a workflow_call
    # reference into forkwright/kanon — private, personal-account-owned —
    # for other-repo callers; kanon#2522). It moves back to forkwright/kanon
    # if/when kanon goes public. Matching only the reusable's own path
    # segment keeps this validator correct across that move without a
    # second edit here.
    gate_jobs = gate.get("jobs", {})

    def find_hybrid_gate_job(jobs: dict) -> tuple[str, dict] | None:
        for job_id, job in jobs.items():
            uses = str(job.get("uses", ""))
            if "/.github/workflows/hybrid-gate.yml" in uses:
                return job_id, job
        return None

    hybrid = find_hybrid_gate_job(gate_jobs)
    if hybrid is None:
        errors.append(
            "gate-attestation.yml must delegate to the fleet-shared "
            "hybrid-gate.yml reusable workflow (a job with uses: "
            "<owner>/<repo>/.github/workflows/hybrid-gate.yml@...)"
        )
    else:
        hybrid_job_id, hybrid_job = hybrid
        with_block = hybrid_job.get("with", {}) or {}

        try:
            kanon_toml = tomllib.loads((ROOT / "kanon.toml").read_text(encoding="utf-8"))
            stages = kanon_toml.get("gate", {}).get("stages", [])
        except FileNotFoundError:
            stages = []
        if not stages:
            errors.append(
                "kanon.toml [gate].stages must be non-empty to validate the "
                "hybrid-gate job's with: block against"
            )
        for stage in stages:
            hint = STAGE_INPUT_HINTS.get(stage)
            if hint is None:
                errors.append(f"no STAGE_INPUT_HINTS entry for kanon.toml gate stage '{stage}'")
                continue
            input_name, hint_text = hint
            if hint_text not in str(with_block.get(input_name, "")):
                errors.append(
                    f"hybrid-gate job's with.{input_name} must cover kanon.toml "
                    f"gate stage '{stage}' ({hint_text})"
                )

        # WHY: Cargo.toml resolves a git dependency on forkwright/theatron —
        # needs_fleet_repo_token must be true or full-gate-build can't
        # authenticate that fetch on a trailer-less PR.
        cargo_toml_text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
        if "github.com/forkwright" in cargo_toml_text and with_block.get("needs_fleet_repo_token") is not True:
            errors.append(
                "hybrid-gate job's with.needs_fleet_repo_token must be true — "
                "Cargo.toml resolves a forkwright git dependency"
            )

    gate_job = gate_jobs.get("gate")
    if gate_job is None:
        errors.append("gate-attestation.yml must define a gate aggregator job")
    else:
        needs = gate_job.get("needs", [])
        needs = [needs] if isinstance(needs, str) else needs
        text = job_step_text(gate_job)

        other_job_ids = [j for j in gate_jobs if j != "gate"]
        if not other_job_ids:
            errors.append("gate-attestation.yml must define at least one job besides the gate aggregator")
        for job_id in other_job_ids:
            if job_id not in needs:
                errors.append(f"gate aggregator must need '{job_id}'")
            if f"needs.{job_id}.result" not in text:
                errors.append(f"gate aggregator must check needs.{job_id}.result")

        if str(gate_job.get("if", "")).strip() != "always()":
            errors.append("gate aggregator must run unconditionally (if: always()) to aggregate every dependency")
        if "exit 1" not in text:
            errors.append("gate aggregator must fail closed (exit 1) when any dependency did not succeed")
        # WHY(kanon#2522): the dependabot[bot]/release-please[bot] trusted-
        # automation waiver for the trailer/build path now lives inside
        # forkwright/kanon's hybrid-gate.yml (validated once, centrally, for
        # every adopting repo) — no longer re-checked in this repo's own
        # aggregator text. gate-coverage-scripts/gate-coverage-compile-checks
        # were never part of that waiver (enforced above via the generic
        # every-other-job check) and still hold for every PR.

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
