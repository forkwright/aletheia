#!/usr/bin/env python3
"""Validate automation PR gate policy for CI workflow YAML."""

import sys
from pathlib import Path

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


def workflow_run_references(workflows_dir: Path) -> list[tuple[str, str]]:
    """Every (referencing file, referenced workflow NAME) across `on.workflow_run`.

    WHY(#6806) this is checked: `workflow_run.workflows` matches on a workflow's `name:`
    field, as a plain string. Rename the target and the reference silently stops firing
    -- no error, no warning, just a trigger that never runs again. That is the same
    shape as every other defect this file guards: absent rather than red.
    """
    references = []
    for path in sorted(workflows_dir.glob("*.yml")):
        try:
            data = yaml.safe_load(path.read_text(encoding="utf-8"))
        except (OSError, UnicodeError, yaml.YAMLError):
            continue
        if not isinstance(data, dict):
            continue
        # PyYAML parses a bare `on:` key as the boolean True.
        triggers = data.get("on") or data.get(True) or {}
        if not isinstance(triggers, dict):
            continue
        run_on = triggers.get("workflow_run") or {}
        if not isinstance(run_on, dict):
            continue
        for name in run_on.get("workflows") or []:
            references.append((path.name, str(name)))
    return references


def declared_workflow_names(workflows_dir: Path) -> set[str]:
    names = set()
    for path in sorted(workflows_dir.glob("*.yml")):
        try:
            data = yaml.safe_load(path.read_text(encoding="utf-8"))
        except (OSError, UnicodeError, yaml.YAMLError):
            continue
        if isinstance(data, dict) and isinstance(data.get("name"), str):
            names.add(data["name"])
    return names


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


def pull_request_paths(workflow: dict) -> list[str] | None:
    """The workflow's pull_request `paths:` filter, or None when unfiltered.

    WHY: `on:` parses to the boolean True for `on: pull_request`, and PyYAML
    reads the bare key `on` as True as well, so both shapes are normalised
    here rather than at each call site.
    """
    triggers = workflow.get("on", workflow.get(True, {}))
    if not isinstance(triggers, dict):
        return None
    pull_request = triggers.get("pull_request")
    if not isinstance(pull_request, dict):
        return None
    paths = pull_request.get("paths")
    return paths if isinstance(paths, list) else None


def unfiltered_pr_workflows() -> dict[str, dict]:
    """Workflows whose pull_request trigger carries no `paths:` filter."""
    unfiltered = {}
    for path in sorted((ROOT / ".github" / "workflows").glob("*.yml")):
        relative = f".github/workflows/{path.name}"
        workflow = load_workflow(relative)
        triggers = workflow.get("on", workflow.get(True, {}))
        if not isinstance(triggers, dict) or "pull_request" not in triggers:
            continue
        if pull_request_paths(workflow) is None:
            unfiltered[relative] = workflow
    return unfiltered


def workflow_run_text(workflow: dict) -> str:
    return "\n".join(job_step_text(job) for job in workflow.get("jobs", {}).values())


# The two keys whose value is the derived test inventory. Named once: they appeared
# four times each, and a fifth spelling would have diverged silently.
SONAR_EXCLUSIONS_KEY = "sonar.exclusions"
SONAR_TEST_INCLUSIONS_KEY = "sonar.test.inclusions"
SONAR_SCOPE_KEYS = (SONAR_EXCLUSIONS_KEY, SONAR_TEST_INCLUSIONS_KEY)
SONAR_SOURCES_KEY = "sonar.sources"
SONAR_TESTS_KEY = "sonar.tests"
SONAR_PROPERTIES = ROOT / ".sonarcloud.properties"


def load_properties(path: Path) -> dict[str, str]:
    """Read the deliberately simple key=value Sonar configuration."""
    properties: dict[str, str] = {}
    for line_number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        if "=" not in line:
            raise ValueError(f"{path}:{line_number}: expected key=value")
        key, value = (part.strip() for part in line.split("=", 1))
        if not key or key in properties:
            raise ValueError(f"{path}:{line_number}: invalid or duplicate key {key!r}")
        properties[key] = value
    return properties


def test_fixture_paths() -> list[str]:
    """Return Python and shell test fixtures for Sonar test scope."""
    scripts = ROOT / "scripts"
    nested_tests = scripts / "tests"
    return sorted(
        path.relative_to(ROOT).as_posix()
        for path in scripts.rglob("*")
        if path.is_file()
        and (
            path.suffix in {".py", ".sh"}
            and (
                path.name.startswith(("test-", "test_"))
                or path.is_relative_to(nested_tests)
            )
        )
    )


def derived_sonar_scope(inventory: list[str]) -> str:
    """The two scope lines `.sonarcloud.properties` must carry, ready to paste.

    WHY this exists: the inventory is already DERIVED here, by `test_fixture_paths`.
    Everything downstream is a mechanical transcription of a comma-joined list that a
    human retypes each time a test file is added, and gets wrong. The check knew the
    right answer well enough to print it in its failure; it just would not print it in
    a form anyone could use.

    WHY it prints rather than writes: a writer would have to build its target path from
    this file's own location, and a tool that rewrites a repository file is a larger
    thing to trust than one that prints two lines. Redirecting straight back into the
    file it reads would also truncate it first. Two lines on stdout compose with any
    editor and cannot destroy anything.
    """
    joined = ",".join(inventory)
    return "\n".join(f"{key}={joined}" for key in SONAR_SCOPE_KEYS)


def main() -> int:
    if "--print-sonar-scope" in sys.argv[1:]:
        print(derived_sonar_scope(test_fixture_paths()))
        return 0

    errors: list[str] = []

    workflows_dir = ROOT / ".github" / "workflows"
    declared = declared_workflow_names(workflows_dir)
    for referrer, referenced in workflow_run_references(workflows_dir):
        if referenced not in declared:
            errors.append(
                f"{referrer}: on.workflow_run references a workflow named "
                f"{referenced!r}, which no workflow declares. `workflow_run` matches on "
                "the `name:` field as a plain string, so this trigger will never fire."
            )

    # Automatic Analysis ignores sonar-project.properties and requires its
    # source/test roots to be literal paths. Keep the guarded scope keys and
    # explicit test inventory complete and disjoint so a broad filter cannot
    # hide production or let fixture-only path mutation affect the rating.
    sonar_path = SONAR_PROPERTIES
    try:
        sonar = load_properties(sonar_path)
    except (OSError, UnicodeError, ValueError) as error:
        errors.append(f"cannot read Automatic Analysis scope: {error}")
    else:
        expected_keys = {SONAR_SOURCES_KEY, SONAR_TESTS_KEY, *SONAR_SCOPE_KEYS}
        if set(sonar) != expected_keys:
            errors.append(
                ".sonarcloud.properties must contain exactly the guarded scope "
                f"keys (expected {sorted(expected_keys)!r})"
            )
        expected_tests = test_fixture_paths()
        configured_tests = sorted(
            filter(None, sonar.get(SONAR_TEST_INCLUSIONS_KEY, "").split(","))
        )
        source_exclusions = sorted(
            filter(None, sonar.get(SONAR_EXCLUSIONS_KEY, "").split(","))
        )
        if sonar.get(SONAR_SOURCES_KEY) != ".":
            errors.append(f".sonarcloud.properties {SONAR_SOURCES_KEY} must be '.'")
        if sonar.get(SONAR_TESTS_KEY) != "scripts":
            errors.append(f".sonarcloud.properties {SONAR_TESTS_KEY} must be 'scripts'")
        if configured_tests != expected_tests:
            errors.append(
                ".sonarcloud.properties sonar.test.inclusions must exactly "
                f"inventory test fixtures (expected {expected_tests!r})"
                " -- `scripts/check-automation-pr-gates.py --print-sonar-scope`"
                " emits both lines ready to paste, rather than transcribing by hand"
            )
        if source_exclusions != expected_tests:
            errors.append(
                ".sonarcloud.properties sonar.exclusions must exactly mirror "
                "sonar.test.inclusions so source and test scopes are disjoint"
                " -- `--print-sonar-scope` emits both"
            )
        if any(
            wildcard in sonar.get(key, "")
            for key in (SONAR_SOURCES_KEY, SONAR_TESTS_KEY, *SONAR_SCOPE_KEYS)
            for wildcard in ("*", "?")
        ):
            errors.append(
                ".sonarcloud.properties must use literal Automatic Analysis paths, "
                "not wildcards"
            )

    # WHY(root-manifest coverage): a check that validates a file no PR-time
    # trigger watches is a check that does not run. check-proskenion-pins.py
    # compares proskenion's theatron pins against the ROOT Cargo.toml, but its
    # only PR-time home was desktop.yml, which is paths-filtered to the
    # proskenion/skene subtrees — so re-pinning theatron in the root workspace
    # drifted the manifests with nothing watching, and the break surfaced on a
    # later, unrelated PR. Any such cross-cutting validator must run from a
    # workflow whose pull_request trigger is unfiltered.
    ROOT_MANIFEST_VALIDATORS = ("scripts/check-proskenion-pins.py",)
    unfiltered = unfiltered_pr_workflows()
    unfiltered_text = "\n".join(
        workflow_run_text(workflow) for workflow in unfiltered.values()
    )
    for validator in ROOT_MANIFEST_VALIDATORS:
        if validator not in unfiltered_text:
            errors.append(
                f"{validator} reads the root Cargo.toml but runs only from "
                "paths-filtered workflows — it must run from a workflow whose "
                "pull_request trigger has no paths: filter (unfiltered today: "
                f"{', '.join(sorted(unfiltered)) or 'none'})"
            )

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
        _hybrid_job_id, hybrid_job = hybrid
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
            if (
                "FLEET_REPO_TOKEN" in run
                and "exit 0" in run
                and "skipping credential setup" not in run
            ):
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
