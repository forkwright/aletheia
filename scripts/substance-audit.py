#!/usr/bin/env python3
"""Policy, receipt, and PR-body tooling for the release substance audit.

The expensive mutation runs belong to the maintainer-dispatched hosted
workflow.  This module keeps the release decision deterministic: it validates
the checked-in policy, classifies raw cargo-mutants outcomes by path, records
every tautological-doc location (Kanon's evidence is intentionally sampled),
and requires an owned issue for every non-blocking advisory class.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import math
import os
import re
import sys
from pathlib import Path, PurePosixPath
from typing import Any

import tomllib
import yaml

SCHEMA_VERSION = 1
CRATES = ("symbolon", "organon", "episteme", "krites", "nous")
CHECK_NAMES = {"mutation", "tautological_doc", "always_default_config"}
CHECK_ORDER = ("mutation", "tautological_doc", "always_default_config")
RECEIPT_STATUSES = {"PASS", "PASS_WITH_ADVISORIES", "BLOCKED"}
OUTCOME_COUNT_KEYS = {
    "CaughtMutant",
    "MissedMutant",
    "Timeout",
    "Unviable",
    "Success",
    "Failure",
}
EVIDENCE_FILES = {
    "audit_json": "audit.json",
    "outcomes_json": "mutants.out/outcomes.json",
    "tool_metadata": "tool-metadata.json",
    "mutants_config": "mutants.toml",
    "audit_exit": "audit-exit.txt",
    "baseline_exit": "baseline-exit.txt",
    "clean_exit": "clean-exit.txt",
}
MUTANT_GENRES = {
    "FnValue",
    "BinaryOperator",
    "UnaryOperator",
    "MatchArm",
    "MatchArmGuard",
    "StructField",
}
TAUTOLOGICAL_PREFIXES = (
    "/// Returns the ",
    "/// Gets the ",
    "/// Sets the ",
    "/// Returns a ",
    "/// Get the ",
    "/// Set the ",
)
SHA_RE = re.compile(r"^[0-9a-f]{40}$")
ISSUE_RE = re.compile(r"^https://github\.com/forkwright/aletheia/issues/([1-9][0-9]*)$")
RUN_URL_RE = re.compile(
    r"^https://github\.com/forkwright/aletheia/actions/runs/([1-9][0-9]*)$"
)
START_MARKER = "<!-- substance-audit-receipt:start -->"
END_MARKER = "<!-- substance-audit-receipt:end -->"


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def read_json(path: Path) -> tuple[Any | None, str | None]:
    try:
        return json.loads(path.read_text(encoding="utf-8")), None
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        return None, f"cannot read JSON {path}: {error}"


def read_exit(path: Path, label: str) -> tuple[int | None, str | None]:
    try:
        value = int(path.read_text(encoding="utf-8").strip())
    except (OSError, UnicodeError, ValueError) as error:
        return None, f"cannot read {label} exit status from {path}: {error}"
    return value, None


def resolve_repo_relative(
    repo_root: Path, raw: Any, label: str
) -> tuple[Path | None, str | None]:
    """Resolve one canonical, non-symlinked path beneath the audited tree."""
    if not isinstance(raw, str) or not raw or raw != raw.strip() or "\\" in raw:
        return None, f"{label} must be a canonical repository-relative path"
    pure = PurePosixPath(raw)
    if (
        pure.is_absolute()
        or not pure.parts
        or ".." in pure.parts
        or pure.as_posix() != raw
    ):
        return None, f"{label} must be a canonical repository-relative path: {raw!r}"
    try:
        root = repo_root.resolve(strict=True)
        lexical = root.joinpath(*pure.parts)
        resolved = lexical.resolve(strict=True)
    except OSError as error:
        return None, f"{label} cannot be resolved: {raw!r}: {error}"
    if resolved == root or not resolved.is_relative_to(root):
        return None, f"{label} escapes the audited repository: {raw!r}"
    if resolved != lexical:
        return None, f"{label} contains a symlink: {raw!r}"
    return resolved, None


def load_policy(path: Path, repo_root: Path) -> tuple[dict[str, Any], list[str]]:
    errors: list[str] = []
    try:
        policy = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, tomllib.TOMLDecodeError) as error:
        return {}, [f"cannot read policy {path}: {error}"]

    if policy.get("schema_version") != SCHEMA_VERSION:
        errors.append(f"policy schema_version must be {SCHEMA_VERSION}")
    tools = policy.get("tools")
    execution = policy.get("execution")
    crates = policy.get("crates")
    if not isinstance(tools, dict):
        errors.append("policy [tools] table is missing")
        tools = {}
    if not isinstance(execution, dict):
        errors.append("policy [execution] table is missing")
        execution = {}
    if not isinstance(crates, dict):
        errors.append("policy [crates] table is missing")
        crates = {}

    commit = tools.get("kanon_commit")
    if not isinstance(commit, str) or not SHA_RE.fullmatch(commit):
        errors.append("tools.kanon_commit must be a lowercase 40-hex SHA")
    for key in (
        "kanon_tag",
        "kanon_version",
        "kanon_rust",
        "cargo_mutants_version",
    ):
        if not isinstance(tools.get(key), str) or not tools[key]:
            errors.append(f"tools.{key} must be a nonempty string")
    if isinstance(tools.get("kanon_version"), str) and tools.get(
        "kanon_tag"
    ) != f"v{tools['kanon_version']}":
        errors.append("tools.kanon_tag must equal 'v' + tools.kanon_version")

    for key in (
        "mutant_jobs",
        "per_mutant_timeout_seconds",
        "wall_timeout_minutes",
        "job_timeout_minutes",
        "artifact_retention_days",
    ):
        if not isinstance(execution.get(key), int) or execution[key] <= 0:
            errors.append(f"execution.{key} must be a positive integer")
    if (
        isinstance(execution.get("wall_timeout_minutes"), int)
        and isinstance(execution.get("job_timeout_minutes"), int)
        and execution["wall_timeout_minutes"] >= execution["job_timeout_minutes"]
    ):
        errors.append("wall timeout must be shorter than the enclosing job timeout")

    if set(crates) != set(CRATES):
        errors.append(
            "policy crates must be exactly " + ", ".join(CRATES)
        )
    for crate in CRATES:
        entry = crates.get(crate)
        if not isinstance(entry, dict):
            errors.append(f"crates.{crate} table is missing")
            continue
        crate_path = entry.get("path")
        features = entry.get("features")
        critical_paths = entry.get("critical_paths")
        if not isinstance(crate_path, str) or not crate_path:
            errors.append(f"crates.{crate}.path must be a nonempty string")
            continue
        resolved_crate, crate_path_error = resolve_repo_relative(
            repo_root, crate_path, f"crates.{crate}.path"
        )
        if crate_path_error is not None:
            errors.append(crate_path_error)
        elif resolved_crate is None or not resolved_crate.is_dir() or not (
            resolved_crate / "Cargo.toml"
        ).is_file():
            errors.append(f"crates.{crate}.path is not a crate: {crate_path}")
        if not isinstance(features, list) or not all(
            isinstance(feature, str) and feature for feature in features
        ):
            errors.append(f"crates.{crate}.features must be a string list")
        if not isinstance(critical_paths, list) or not all(
            isinstance(item, str) and item for item in critical_paths
        ):
            errors.append(f"crates.{crate}.critical_paths must be a string list")
            continue
        for raw in critical_paths:
            candidate = PurePosixPath(raw)
            resolved_critical, critical_error = resolve_repo_relative(
                repo_root, raw, f"crates.{crate} critical path"
            )
            if critical_error is not None:
                errors.append(critical_error)
                continue
            if not candidate.is_relative_to(PurePosixPath(crate_path)):
                errors.append(
                    f"crates.{crate} critical path escapes its crate: {raw}"
                )
            elif resolved_crate is not None and (
                resolved_critical is None
                or not resolved_critical.is_relative_to(resolved_crate)
            ):
                errors.append(
                    f"crates.{crate} critical path resolves outside its crate: {raw}"
                )

    toolchain_path = repo_root / "rust-toolchain.toml"
    try:
        toolchain = tomllib.loads(toolchain_path.read_text(encoding="utf-8"))
        channel = toolchain["toolchain"]["channel"]
    except (OSError, UnicodeError, tomllib.TOMLDecodeError, KeyError, TypeError) as error:
        errors.append(f"cannot read Rust toolchain identity: {error}")
    else:
        if channel != tools.get("kanon_rust"):
            errors.append(
                f"Kanon Rust {tools.get('kanon_rust')!r} differs from Aletheia {channel!r}"
            )
    return policy, errors


def validate_workflow_contract(policy: dict[str, Any], repo_root: Path) -> list[str]:
    workflow = repo_root / ".github/workflows/substance-audit.yml"
    try:
        text = workflow.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as error:
        return [f"cannot read {workflow}: {error}"]
    tools = policy.get("tools", {})
    execution = policy.get("execution", {})
    required = {
        "trusted manual trigger": "workflow_dispatch:",
        "owned repository guard": 'GITHUB_REPOSITORY" != "forkwright/aletheia',
        "private Kanon repository": "repository: forkwright/kanon",
        "exact Kanon commit": f"ref: {tools.get('kanon_commit')}",
        "non-persistent private checkout": "persist-credentials: false",
        "pinned Kanon Rust": f"toolchain: {tools.get('kanon_rust')}",
        "locked private build": "cargo install --locked --path private-kanon/crates/pragma",
        "private-source scrub": 'rm -rf -- private-kanon "$CARGO_HOME/git"',
        "exact cargo-mutants install": (
            "cargo install --locked cargo-mutants --version "
            f"{tools.get('cargo_mutants_version')}"
        ),
        "mutation jobs": f"MUTANT_JOBS: {execution.get('mutant_jobs')}",
        "per-mutant timeout": (
            f"MUTANT_TIMEOUT_SECONDS: {execution.get('per_mutant_timeout_seconds')}"
        ),
        "external wall timeout": (
            f"WALL_TIMEOUT_MINUTES: {execution.get('wall_timeout_minutes')}"
        ),
        "matrix job timeout": f"timeout-minutes: {execution.get('job_timeout_minutes')}",
        "retention": f"retention-days: {execution.get('artifact_retention_days')}",
        "canonical release transition": (
            "check-release-versioning.py verify-transition"
        ),
        "immutable release comparison": (
            "check-release-versioning.py verify-comparison"
        ),
        "immutable comparison route": (
            "compare/${GITHUB_SHA}...${EXPECTED_SHA}"
        ),
        "trusted comparison base binding": (
            '--base-sha "$GITHUB_SHA"'
        ),
        "trusted comparison candidate binding": (
            '--candidate-sha "$EXPECTED_SHA"'
        ),
        "post-validation release PR rebind": (
            "Rebind the Release Please PR after candidate validation"
        ),
        "cargo-mutants output parent": 'CARGO_MUTANTS_OUTPUT="$RESULT_DIR"',
        "raw outcomes classifier": "--outcomes-json \"$RESULT_DIR/mutants.out/outcomes.json\"",
        "prior-receipt replay": "run-id: ${{ inputs.source_run_id }}",
        "prior-run workflow path binding": (
            "test \"$(jq -r '.path' <<<\"$run_json\")\" = "
            "\".github/workflows/substance-audit.yml\""
        ),
        "prior-run source SHA binding": (
            "test \"$(jq -r '.head_sha' <<<\"$run_json\")\" = \"$GITHUB_SHA\""
        ),
        "candidate-root receipt reclassification": (
            "--repo-root release-candidate aggregate"
        ),
        "advisory verification failure propagation": 'exit "$status"',
        "final policy enforcement": "substance-audit.py enforce \\",
        "external enforce SHA binding": '--repo-sha "$EXPECTED_SHA"',
    }
    errors = [
        f"substance workflow lacks {label}"
        for label, needle in required.items()
        if needle not in text
    ]
    for forbidden in ("pull_request:", "schedule:"):
        if forbidden in text:
            errors.append(f"substance workflow must not expose {forbidden.rstrip(':')} trigger")
    if re.search(r"pulls/[^\"']*/files(?:\?[^\"']*)?", text):
        errors.append("substance workflow must not use the mutable release PR files endpoint")
    try:
        workflow_data = yaml.safe_load(text)
    except yaml.YAMLError as error:
        errors.append(f"substance workflow is not valid YAML: {error}")
        workflow_data = {}
    if not isinstance(workflow_data, dict):
        errors.append("substance workflow must be a mapping")
        workflow_data = {}
    if "defaults" in workflow_data:
        errors.append("substance workflow must not override the default run shell")
    if workflow_data.get("env") != {"GH_REPO": "${{ github.repository }}"}:
        errors.append("substance workflow must bind GH_REPO to the current repository")
    if workflow_data.get("permissions") != {"contents": "read"}:
        errors.append("substance workflow root permissions must remain read-only")
    jobs = workflow_data.get("jobs", {})
    if not isinstance(jobs, dict):
        errors.append("substance workflow jobs must be a mapping")
        jobs = {}
    if set(jobs) != {"preflight", "audit", "aggregate"}:
        errors.append(
            "substance workflow must contain exactly preflight, audit, and aggregate jobs"
        )
    preflight_job = jobs.get("preflight", {})
    if not isinstance(preflight_job, dict):
        errors.append("substance workflow preflight job must be a mapping")
        preflight_job = {}
    if set(preflight_job) != {
        "runs-on",
        "timeout-minutes",
        "permissions",
        "outputs",
        "steps",
    }:
        errors.append("substance workflow preflight job keys must remain exact")
    expected_preflight_fields = {
        "runs-on": "ubuntu-latest",
        "timeout-minutes": 10,
        "permissions": {
            "actions": "read",
            "contents": "read",
            "pull-requests": "read",
        },
        "outputs": {"matrix": "${{ steps.policy.outputs.matrix }}"},
    }
    for key, expected in expected_preflight_fields.items():
        if preflight_job.get(key) != expected:
            errors.append(f"substance workflow preflight {key} must remain exact")
    for forbidden in ("if", "continue-on-error", "defaults"):
        if forbidden in preflight_job:
            errors.append(
                "substance workflow preflight admission job must fail closed; "
                f"found {forbidden}"
            )
    preflight_steps = preflight_job.get("steps", [])
    if not isinstance(preflight_steps, list):
        errors.append("substance workflow preflight steps must be a list")
        preflight_steps = []
    for step in preflight_steps:
        if not isinstance(step, dict):
            errors.append("substance workflow preflight step must be a mapping")
            continue
        for forbidden in ("if", "continue-on-error", "working-directory"):
            if forbidden in step:
                errors.append(
                    "substance workflow preflight steps must not be conditional or "
                    f"suppress failures; found {forbidden}"
                )

    step_order = [
        ("name", step["name"])
        if isinstance(step, dict) and "name" in step
        else ("uses", step.get("uses"))
        if isinstance(step, dict)
        else ("invalid", None)
        for step in preflight_steps
    ]
    expected_step_order = [
        ("name", "Require trusted main dispatch inputs"),
        (
            "uses",
            "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
        ),
        ("name", "Validate policy and derive the five-crate matrix"),
        ("name", "Bind the open Release Please PR to current main"),
        ("name", "Validate the immutable release comparison"),
        (
            "uses",
            "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
        ),
        ("name", "Validate the exact Release Please metadata transition"),
        ("name", "Rebind the Release Please PR after candidate validation"),
    ]
    if step_order != expected_step_order:
        errors.append("substance workflow preflight step order must remain exact")

    exact_run_contracts = {
        "Require trusted main dispatch inputs": (
            {
                "name": "Require trusted main dispatch inputs",
                "env": {
                    "EXPECTED_SHA": "${{ inputs.expected_sha }}",
                    "RELEASE_PR": "${{ inputs.release_pr }}",
                    "SOURCE_RUN_ID": "${{ inputs.source_run_id }}",
                },
            },
            "12d947aa9af2c3ed946b799893965a6ec57695413a99568426e432ffc117fce4",
        ),
        "Validate policy and derive the five-crate matrix": (
            {
                "name": "Validate policy and derive the five-crate matrix",
                "id": "policy",
            },
            "c42d90fb27d4a7eda186ce8eba5f7baf98ed14c8a5d8a8c258ec208dc056a81d",
        ),
        "Bind the open Release Please PR to current main": (
            {
                "name": "Bind the open Release Please PR to current main",
                "env": {
                    "EXPECTED_SHA": "${{ inputs.expected_sha }}",
                    "GH_TOKEN": "${{ secrets.GITHUB_TOKEN }}",
                    "RELEASE_PR": "${{ inputs.release_pr }}",
                    "SOURCE_RUN_ID": "${{ inputs.source_run_id }}",
                },
            },
            "8919efc82158655befbca71222f5a553bb87a40a14d31feb5ce573d8ad658dc4",
        ),
        "Rebind the Release Please PR after candidate validation": (
            {
                "name": "Rebind the Release Please PR after candidate validation",
                "env": {
                    "EXPECTED_SHA": "${{ inputs.expected_sha }}",
                    "GH_TOKEN": "${{ secrets.GITHUB_TOKEN }}",
                    "RELEASE_PR": "${{ inputs.release_pr }}",
                },
            },
            "fa8b3efb360532f56b9f8da321a02f2365288d13d0d64ebf957daf832b3feebc",
        ),
    }
    for name, (expected_metadata, expected_run_sha256) in exact_run_contracts.items():
        matches = [
            step
            for step in preflight_steps
            if isinstance(step, dict) and step.get("name") == name
        ]
        if len(matches) != 1:
            errors.append(f"substance workflow must contain one exact {name!r} step")
            continue
        step = matches[0]
        run = step.get("run")
        run_sha256 = (
            hashlib.sha256(run.encode("utf-8")).hexdigest()
            if isinstance(run, str)
            else ""
        )
        metadata = {key: value for key, value in step.items() if key != "run"}
        if metadata != expected_metadata or run_sha256 != expected_run_sha256:
            errors.append(f"substance workflow {name!r} step must remain exact")

    main_checkouts = [
        step
        for step in preflight_steps
        if isinstance(step, dict)
        and step.get("uses")
        == "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1"
        and isinstance(step.get("with"), dict)
        and "path" not in step["with"]
    ]
    expected_main_checkout = {
        "uses": "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
        "with": {
            "persist-credentials": False,
            "ref": "${{ github.sha }}",
        },
    }
    if main_checkouts != [expected_main_checkout]:
        errors.append("substance workflow preflight must inspect the exact trusted base")

    comparison_name = "Validate the immutable release comparison"
    comparison_steps = [
        step
        for step in preflight_steps
        if isinstance(step, dict) and step.get("name") == comparison_name
    ]
    expected_comparison_step = {
        "name": comparison_name,
        "env": {
            "EXPECTED_SHA": "${{ inputs.expected_sha }}",
            "GH_TOKEN": "${{ secrets.GITHUB_TOKEN }}",
        },
        "shell": "bash",
        "run": (
            'compare_json="$(gh api "repos/${GH_REPO}/compare/'
            '${GITHUB_SHA}...${EXPECTED_SHA}")"\n'
            "exec scripts/check-release-versioning.py verify-comparison \\\n"
            '  --base-sha "$GITHUB_SHA" --candidate-sha "$EXPECTED_SHA" \\\n'
            '  <<<"$compare_json"\n'
        ),
    }
    if comparison_steps != [expected_comparison_step]:
        errors.append(
            "substance workflow immutable comparison step must remain exact and "
            "fail closed"
        )

    candidate_checkouts = [
        step
        for step in preflight_steps
        if isinstance(step, dict)
        and step.get("uses")
        == "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1"
        and isinstance(step.get("with"), dict)
        and step["with"].get("path") == "release-candidate"
    ]
    expected_candidate_checkout = {
        "uses": "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
        "with": {
            "path": "release-candidate",
            "persist-credentials": False,
            "ref": "${{ inputs.expected_sha }}",
        },
    }
    if candidate_checkouts != [expected_candidate_checkout]:
        errors.append(
            "substance workflow preflight must inspect the exact candidate checkout"
        )

    audit_job = jobs.get("audit", {})
    if not isinstance(audit_job, dict):
        errors.append("substance workflow audit job must be a mapping")
        audit_job = {}
    if audit_job.get("needs") != ["preflight"]:
        errors.append("substance workflow audit job must depend on preflight")
    if audit_job.get("if") != "inputs.source_run_id == ''":
        errors.append(
            "substance workflow audit job must retain the implicit successful-"
            "preflight condition"
        )
    if set(audit_job) != {
        "if",
        "needs",
        "permissions",
        "runs-on",
        "steps",
        "strategy",
        "timeout-minutes",
    }:
        errors.append("substance workflow audit job keys must remain exact")
    expected_audit_fields = {
        "runs-on": "ubuntu-latest",
        "timeout-minutes": 360,
        "permissions": {"contents": "read"},
        "strategy": {
            "fail-fast": False,
            "matrix": "${{ fromJSON(needs.preflight.outputs.matrix) }}",
        },
    }
    for key, expected in expected_audit_fields.items():
        if audit_job.get(key) != expected:
            errors.append(f"substance workflow audit {key} must remain exact")
    strategy = audit_job.get("strategy", {})
    if not isinstance(strategy, dict) or strategy.get("matrix") != (
        "${{ fromJSON(needs.preflight.outputs.matrix) }}"
    ):
        errors.append("substance workflow audit matrix must come from preflight")
    audit_candidate_checkouts = [
        step
        for step in audit_job.get("steps", [])
        if isinstance(step, dict)
        and step.get("uses")
        == "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1"
        and isinstance(step.get("with"), dict)
        and step["with"].get("path") == "target"
    ]
    expected_audit_candidate_checkout = {
        "uses": "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
        "with": {
            "path": "target",
            "persist-credentials": False,
            "ref": "${{ inputs.expected_sha }}",
        },
    }
    if audit_candidate_checkouts != [expected_audit_candidate_checkout]:
        errors.append("substance workflow audit must execute the exact candidate SHA")
    audit_control_checkouts = [
        step
        for step in audit_job.get("steps", [])
        if isinstance(step, dict)
        and step.get("uses")
        == "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1"
        and isinstance(step.get("with"), dict)
        and step["with"].get("path") == "control"
    ]
    expected_audit_control_checkout = {
        "uses": "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
        "with": {
            "path": "control",
            "persist-credentials": False,
            "ref": "${{ github.sha }}",
        },
    }
    if audit_control_checkouts != [expected_audit_control_checkout]:
        errors.append("substance workflow audit control must use the trusted base")
    private_checkouts = [
        step
        for step in audit_job.get("steps", [])
        if isinstance(step, dict)
        and step.get("uses")
        == "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1"
        and isinstance(step.get("with"), dict)
        and step["with"].get("path") == "private-kanon"
    ]
    expected_private_checkout = {
        "uses": "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
        "with": {
            "repository": "forkwright/kanon",
            "ref": "6795565b0ae3368faa0b710608dfeabe1f70fafb",
            "token": "${{ secrets.FLEET_REPO_TOKEN }}",
            "path": "private-kanon",
            "persist-credentials": False,
            "fetch-depth": 1,
        },
    }
    if private_checkouts != [expected_private_checkout]:
        errors.append("substance workflow private Kanon checkout must remain exact")
    audit_steps = audit_job.get("steps", [])
    audit_step_order = [
        ("name", step["name"])
        if isinstance(step, dict) and "name" in step
        else ("uses", step.get("uses"))
        if isinstance(step, dict)
        else ("invalid", None)
        for step in audit_steps
    ]
    expected_audit_step_order = [
        (
            "uses",
            "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
        ),
        ("name", "Require private Kanon read credential"),
        (
            "uses",
            "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
        ),
        (
            "uses",
            "dtolnay/rust-toolchain@631a55b12751854ce901bb631d5902ceb48146f7",
        ),
        ("name", "Build and verify the exact private Kanon CLI"),
        ("name", "Scrub private source and credentials before Aletheia execution"),
        (
            "uses",
            "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
        ),
        ("name", "Install the pinned mutation runner"),
        ("name", "Record exact tool identities"),
        ("name", "Run the exact feature-world baseline and substance audit"),
        ("name", "Classify the complete raw evidence"),
        ("name", "Upload public audit evidence only"),
    ]
    if audit_step_order != expected_audit_step_order:
        errors.append("substance workflow audit step order must remain exact")
    scrub_name = "Scrub private source and credentials before Aletheia execution"
    scrub_steps = [
        (index, step)
        for index, step in enumerate(audit_steps)
        if isinstance(step, dict) and step.get("name") == scrub_name
    ]
    target_indices = [
        index
        for index, step in enumerate(audit_steps)
        if step == expected_audit_candidate_checkout
    ]
    if len(scrub_steps) != 1 or len(target_indices) != 1:
        errors.append("substance workflow must scrub private material before checkout")
    else:
        scrub_index, scrub_step = scrub_steps[0]
        scrub_run = scrub_step.get("run")
        scrub_sha256 = (
            hashlib.sha256(scrub_run.encode("utf-8")).hexdigest()
            if isinstance(scrub_run, str)
            else ""
        )
        if (
            set(scrub_step) != {"name", "run"}
            or scrub_sha256
            != "cf11f2194cde175536f14f0e6772969183c89d6e3038041afaad0948ea9482ea"
            or scrub_index >= target_indices[0]
        ):
            errors.append(
                "substance workflow private-source scrub must remain exact and "
                "precede candidate checkout"
            )
        for step in audit_steps[scrub_index + 1 :]:
            serialized = json.dumps(step, sort_keys=True)
            if re.search(r"\b(?:secrets|github)\b", serialized):
                errors.append(
                    "substance workflow must not expose credentials after the "
                    "private-source scrub"
                )
                break

    aggregate_job = jobs.get("aggregate", {})
    if not isinstance(aggregate_job, dict):
        errors.append("substance workflow aggregate job must be a mapping")
        aggregate_job = {}
    expected_aggregate_if = (
        "always() && needs.preflight.result == 'success' && "
        "(needs.audit.result != 'cancelled' || inputs.source_run_id != '')"
    )
    if aggregate_job.get("needs") != ["preflight", "audit"]:
        errors.append("substance workflow aggregate job must depend on preflight and audit")
    if aggregate_job.get("if") != expected_aggregate_if:
        errors.append("substance workflow aggregate job must require successful preflight")
    if set(aggregate_job) != {
        "if",
        "needs",
        "permissions",
        "runs-on",
        "steps",
        "timeout-minutes",
    }:
        errors.append("substance workflow aggregate job keys must remain exact")
    expected_aggregate_fields = {
        "runs-on": "ubuntu-latest",
        "timeout-minutes": 15,
        "permissions": {
            "actions": "read",
            "contents": "read",
            "pull-requests": "write",
        },
    }
    for key, expected in expected_aggregate_fields.items():
        if aggregate_job.get(key) != expected:
            errors.append(f"substance workflow aggregate {key} must remain exact")
    aggregate_steps = aggregate_job.get("steps", [])
    if not isinstance(aggregate_steps, list):
        errors.append("substance workflow aggregate steps must be a list")
        aggregate_steps = []
    aggregate_step_order = [
        ("name", step["name"])
        if isinstance(step, dict) and "name" in step
        else ("uses", step.get("uses"))
        if isinstance(step, dict)
        else ("invalid", None)
        for step in aggregate_steps
    ]
    expected_aggregate_step_order = [
        (
            "uses",
            "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
        ),
        (
            "uses",
            "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
        ),
        ("name", "Download this run's per-crate receipts"),
        ("name", "Download prior per-crate receipts for issue adjudication"),
        ("name", "Verify every advisory owner is an open Aletheia issue"),
        ("name", "Aggregate exact-five receipts"),
        ("name", "Rebind the receipt before updating the release PR"),
        ("name", "Upload the aggregate release receipt"),
        ("name", "Enforce release substance policy"),
    ]
    if aggregate_step_order != expected_aggregate_step_order:
        errors.append("substance workflow aggregate step order must remain exact")

    expected_aggregate_control_checkout = {
        "uses": "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
        "with": {
            "persist-credentials": False,
            "ref": "${{ github.sha }}",
        },
    }
    expected_aggregate_candidate_checkout = {
        "uses": "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
        "with": {
            "path": "release-candidate",
            "persist-credentials": False,
            "ref": "${{ inputs.expected_sha }}",
        },
    }
    if len(aggregate_steps) < 2 or aggregate_steps[:2] != [
        expected_aggregate_control_checkout,
        expected_aggregate_candidate_checkout,
    ]:
        errors.append(
            "substance workflow aggregate must bind trusted control and exact candidate"
        )

    update_name = "Rebind the receipt before updating the release PR"
    update_steps = [
        step
        for step in aggregate_steps
        if isinstance(step, dict) and step.get("name") == update_name
    ]
    expected_update_metadata = {
        "name": update_name,
        "env": {
            "EXPECTED_SHA": "${{ inputs.expected_sha }}",
            "GH_TOKEN": "${{ secrets.GITHUB_TOKEN }}",
            "RELEASE_PR": "${{ inputs.release_pr }}",
        },
    }
    if len(update_steps) != 1:
        errors.append("substance workflow must contain one exact PR receipt update step")
    else:
        update_step = update_steps[0]
        update_run = update_step.get("run")
        update_sha256 = (
            hashlib.sha256(update_run.encode("utf-8")).hexdigest()
            if isinstance(update_run, str)
            else ""
        )
        update_metadata = {
            key: value for key, value in update_step.items() if key != "run"
        }
        if (
            update_metadata != expected_update_metadata
            or update_sha256
            != "be8f42dc8dfa1870988fcd7bfa20ca54446ffb2674f7dc36e69d72afd44a41e2"
        ):
            errors.append("substance workflow PR receipt update step must remain exact")

    upload_name = "Upload the aggregate release receipt"
    upload_steps = [
        step
        for step in aggregate_steps
        if isinstance(step, dict) and step.get("name") == upload_name
    ]
    expected_aggregate_upload = {
        "name": upload_name,
        "uses": "actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a",
        "with": {
            "name": "substance-aggregate-${{ inputs.expected_sha }}",
            "path": (
                "${{ runner.temp }}/substance-receipt.json\n"
                "${{ runner.temp }}/pr-body-updated.md\n"
            ),
            "if-no-files-found": "error",
            "retention-days": 90,
        },
    }
    if upload_steps != [expected_aggregate_upload]:
        errors.append("substance workflow aggregate receipt upload must remain exact")

    enforce_name = "Enforce release substance policy"
    enforce_steps = [
        step
        for step in aggregate_steps
        if isinstance(step, dict) and step.get("name") == enforce_name
    ]
    expected_enforce_metadata = {
        "name": enforce_name,
        "env": {
            "EXPECTED_SHA": "${{ inputs.expected_sha }}",
            "RELEASE_PR": "${{ inputs.release_pr }}",
            "SOURCE_RUN_ID": "${{ inputs.source_run_id }}",
        },
    }
    if len(enforce_steps) != 1:
        errors.append("substance workflow must contain one final policy enforcement step")
    else:
        enforce_step = enforce_steps[0]
        enforce_run = enforce_step.get("run")
        enforce_sha256 = (
            hashlib.sha256(enforce_run.encode("utf-8")).hexdigest()
            if isinstance(enforce_run, str)
            else ""
        )
        enforce_metadata = {
            key: value for key, value in enforce_step.items() if key != "run"
        }
        if (
            aggregate_steps[-1] is not enforce_step
            or enforce_metadata != expected_enforce_metadata
            or enforce_sha256
            != "a9ac0e4a65e7bc76424dea95b5716d335c57d9ae4c1bb114cc08fb30920418c3"
        ):
            errors.append(
                "substance workflow final policy enforcement step must remain exact"
            )
    preflight_index = text.find("\n  preflight:")
    audit_job_index = text.find("\n  audit:", preflight_index)
    if preflight_index < 0 or audit_job_index < 0:
        errors.append("substance workflow lacks the preflight admission job")
        preflight_block = ""
    else:
        preflight_block = text[preflight_index:audit_job_index]
        for forbidden in ("continue-on-error:", "set +e", "set +o errexit", "||"):
            if forbidden in preflight_block:
                errors.append(
                    "substance workflow preflight admission job must fail closed; "
                    f"found {forbidden}"
                )
    bind_marker = "Bind the open Release Please PR to current main"
    bind_index = text.find(bind_marker)
    bind_end = text.find("\n      - uses:", bind_index)
    if bind_index < 0 or bind_end < 0:
        errors.append("substance workflow lacks the release PR binding step")
    else:
        bind_block = text[bind_index:bind_end]
        for forbidden in ("continue-on-error:", "set +e", "set +o errexit"):
            if forbidden in bind_block:
                errors.append(
                    "substance workflow release PR binding step must fail closed; "
                    f"found {forbidden}"
                )
    transition_marker = "- name: Validate the exact Release Please metadata transition"
    transition_index = text.find(transition_marker)
    rebind_marker = "- name: Rebind the Release Please PR after candidate validation"
    rebind_index = text.find(rebind_marker)
    audit_index = text.find("\n  audit:", rebind_index)
    if not 0 <= transition_index < rebind_index < audit_index:
        errors.append(
            "substance workflow must rebind the Release Please PR after candidate "
            "validation and before audit execution"
        )
    else:
        transition_lines = [
            line.strip()
            for line in text[transition_index:rebind_index].splitlines()
            if line.strip()
        ]
        expected_transition_lines = [
            transition_marker,
            "run: >-",
            "scripts/check-release-versioning.py verify-transition",
            "--base-root . --candidate-root release-candidate",
        ]
        if transition_lines != expected_transition_lines:
            errors.append(
                "substance workflow release transition command block must remain "
                "exact and fail closed"
            )
        rebind_block = text[rebind_index:audit_index]
        rebind_required = {
            "live PR read": (
                'pr_json="$(gh api "repos/${GH_REPO}/pulls/${RELEASE_PR}")"'
            ),
            "open state": "test \"$(jq -r '.state' <<<\"$pr_json\")\" = \"open\"",
            "same-repository head": (
                "test \"$(jq -r '.head.repo.full_name' <<<\"$pr_json\")\" = \"$GH_REPO\""
            ),
            "release branch": (
                "test \"$(jq -r '.head.ref' <<<\"$pr_json\")\" = "
                '"release-please--branches--main"'
            ),
            "candidate SHA": (
                "test \"$(jq -r '.head.sha' <<<\"$pr_json\")\" = \"$EXPECTED_SHA\""
            ),
            "main base": (
                "test \"$(jq -r '.base.ref' <<<\"$pr_json\")\" = \"main\""
            ),
            "trusted base SHA": (
                "test \"$(jq -r '.base.sha' <<<\"$pr_json\")\" = \"$GITHUB_SHA\""
            ),
        }
        for label, needle in rebind_required.items():
            if needle not in rebind_block:
                errors.append(
                    f"substance workflow post-validation rebind lacks {label} binding"
                )
    upload_blocks = re.findall(
        r"uses: actions/upload-artifact@.*?(?=\n\s*- name:|\n\s*- uses:|\n\s{2}[a-zA-Z_-]+:|\Z)",
        text,
        flags=re.DOTALL,
    )
    for block in upload_blocks:
        if "private-kanon" in block or "kanon-root" in block:
            errors.append("substance workflow uploads private Kanon material")
    if text.count('pr_json="$(gh api "repos/${GH_REPO}/pulls/${RELEASE_PR}")"') < 3:
        errors.append("substance workflow lacks a post-edit release PR rebind")
    return errors


def matrix(policy: dict[str, Any]) -> list[dict[str, str]]:
    result: list[dict[str, str]] = []
    for crate in CRATES:
        entry = policy["crates"][crate]
        result.append(
            {
                "crate": crate,
                "path": entry["path"],
                "features": ",".join(entry["features"]),
            }
        )
    return result


def render_mutants_config(features: list[str]) -> str:
    quoted = ", ".join(json.dumps(feature) for feature in features)
    return (
        "# Generated for one hosted substance-audit matrix leg; never commit.\n"
        f"features = [{quoted}]\n"
        'additional_cargo_args = ["--locked"]\n'
        "gitignore = true\n"
    )


def is_critical(path: str, critical_paths: list[str]) -> bool:
    candidate = PurePosixPath(path)
    return any(
        candidate == PurePosixPath(root)
        or candidate.is_relative_to(PurePosixPath(root))
        for root in critical_paths
    )


def normalize_mutant_path(raw: str, crate_path: str) -> str | None:
    candidate = PurePosixPath(raw.replace("\\", "/"))
    if candidate.is_absolute() or ".." in candidate.parts:
        return None
    crate = PurePosixPath(crate_path)
    if candidate == crate or candidate.is_relative_to(crate):
        return candidate.as_posix()
    if candidate.parts and candidate.parts[0] == "src":
        return (crate / candidate).as_posix()
    return None


def read_crate_version(repo_root: Path, crate_path: str) -> tuple[str | None, str | None]:
    resolved_crate, path_error = resolve_repo_relative(
        repo_root, crate_path, "crate path"
    )
    if path_error is not None or resolved_crate is None:
        return None, path_error or "crate path could not be resolved"
    try:
        crate_manifest = tomllib.loads(
            (resolved_crate / "Cargo.toml").read_text(encoding="utf-8")
        )
        raw_version = crate_manifest["package"]["version"]
        if isinstance(raw_version, str) and raw_version:
            return raw_version, None
        if isinstance(raw_version, dict) and raw_version.get("workspace") is True:
            workspace_manifest = tomllib.loads(
                (repo_root / "Cargo.toml").read_text(encoding="utf-8")
            )
            workspace_version = workspace_manifest["workspace"]["package"]["version"]
            if isinstance(workspace_version, str) and workspace_version:
                return workspace_version, None
    except (OSError, UnicodeError, tomllib.TOMLDecodeError, KeyError, TypeError) as error:
        return None, f"cannot resolve crate version for {crate_path}: {error}"
    return None, f"cannot resolve crate version for {crate_path}"


def scan_tautological_docs(repo_root: Path, crate_path: str) -> tuple[list[dict[str, Any]], list[str]]:
    findings: list[dict[str, Any]] = []
    errors: list[str] = []
    resolved_root = repo_root.resolve(strict=True)
    resolved_crate, path_error = resolve_repo_relative(
        resolved_root, crate_path, "crate path"
    )
    if path_error is not None or resolved_crate is None:
        return [], [path_error or "crate path could not be resolved"]
    lexical_src = resolved_crate / "src"
    try:
        src = lexical_src.resolve(strict=True)
    except OSError as error:
        return [], [f"cannot resolve source root for {crate_path}: {error}"]
    if (
        lexical_src.is_symlink()
        or src != lexical_src
        or not src.is_dir()
        or not src.is_relative_to(resolved_crate)
    ):
        return [], [f"crate source root is unsafe or missing: {crate_path}/src"]
    source_paths: list[Path] = []

    def record_walk_error(error: OSError) -> None:
        location = error.filename or os.fspath(src)
        errors.append(f"cannot traverse source directory {location}: {error}")

    for directory, dirnames, filenames in os.walk(
        src, followlinks=False, onerror=record_walk_error
    ):
        parent = Path(directory)
        dirnames.sort()
        for dirname in list(dirnames):
            child = parent / dirname
            try:
                resolved_child = child.resolve(strict=True)
            except OSError as error:
                errors.append(f"cannot resolve source directory {child}: {error}")
                dirnames.remove(dirname)
                continue
            if child.is_symlink() or not resolved_child.is_relative_to(src):
                errors.append(
                    f"source directory is a symlink or escapes its crate: {child}"
                )
                dirnames.remove(dirname)
        source_paths.extend(
            parent / filename for filename in sorted(filenames) if filename.endswith(".rs")
        )

    for path in source_paths:
        try:
            resolved_path = path.resolve(strict=True)
        except OSError as error:
            errors.append(f"cannot resolve {path}: {error}")
            continue
        if path.is_symlink() or not resolved_path.is_relative_to(src):
            errors.append(f"source path is a symlink or escapes its crate: {path}")
            continue
        try:
            text = resolved_path.read_text(encoding="utf-8")
        except (OSError, UnicodeError) as error:
            errors.append(f"cannot read {resolved_path}: {error}")
            continue
        relative = resolved_path.relative_to(resolved_root).as_posix()
        for lineno, line in enumerate(text.splitlines(), 1):
            stripped = line.lstrip()
            if stripped.startswith(TAUTOLOGICAL_PREFIXES):
                findings.append(
                    {"path": relative, "line": lineno, "text": stripped.strip()}
                )
    return findings, errors


def finding(kind: str, detail: str, **extra: Any) -> dict[str, Any]:
    return {"kind": kind, "detail": detail, **extra}


def parse_timestamp(value: Any, label: str) -> tuple[dt.datetime | None, str | None]:
    if not isinstance(value, str) or not value:
        return None, f"cargo-mutants {label} is missing"
    try:
        parsed = dt.datetime.fromisoformat(value)
    except ValueError as error:
        return None, f"cargo-mutants {label} is invalid: {error}"
    if parsed.tzinfo is None:
        return None, f"cargo-mutants {label} lacks a timezone"
    return parsed, None


def process_status_kind(value: Any) -> tuple[str | None, str | None]:
    if value in ("Success", "Timeout", "Other"):
        return str(value), None
    if isinstance(value, dict) and len(value) == 1:
        kind, code = next(iter(value.items()))
        if kind in ("Failure", "Signalled") and isinstance(code, int) and not isinstance(
            code, bool
        ):
            if code == 0:
                return None, f"{kind} exit code must be nonzero"
            return kind, None
    return None, f"unsupported process_status {value!r}"


def valid_span(value: Any) -> bool:
    if not isinstance(value, dict) or set(value) != {"start", "end"}:
        return False
    for point in (value["start"], value["end"]):
        if not isinstance(point, dict) or set(point) != {"line", "column"}:
            return False
        if any(
            not isinstance(point[key], int)
            or isinstance(point[key], bool)
            or point[key] < 1
            for key in ("line", "column")
        ):
            return False
    return True


def validate_phase_results(
    phases: Any,
    expected_features: list[str],
    expected_package: str,
    expected_version: str,
    mutant_name: str,
) -> tuple[str | None, list[str]]:
    errors: list[str] = []
    if not isinstance(phases, list) or not phases:
        return None, [f"{mutant_name} has no phase command evidence"]

    names: list[str] = []
    statuses: list[str] = []
    for index, phase in enumerate(phases):
        if not isinstance(phase, dict):
            errors.append(f"{mutant_name} phase {index} is not an object")
            continue
        expected_keys = {"phase", "duration", "process_status", "argv"}
        if set(phase) != expected_keys:
            errors.append(
                f"{mutant_name} phase {index} fields {sorted(phase)} differ from "
                f"{sorted(expected_keys)}"
            )
        phase_name = phase.get("phase")
        if phase_name not in ("Build", "Test"):
            errors.append(f"{mutant_name} has unsupported phase {phase_name!r}")
        else:
            names.append(phase_name)

        duration = phase.get("duration")
        if (
            not isinstance(duration, (int, float))
            or isinstance(duration, bool)
            or not math.isfinite(duration)
            or duration < 0
        ):
            errors.append(f"{mutant_name} phase {index} has invalid duration")

        status, status_error = process_status_kind(phase.get("process_status"))
        if status_error:
            errors.append(f"{mutant_name} phase {index}: {status_error}")
        elif status is not None:
            statuses.append(status)

        argv = phase.get("argv")
        if not isinstance(argv, list) or not argv or not all(
            isinstance(arg, str) and arg for arg in argv
        ):
            errors.append(f"{mutant_name} phase {index} has malformed argv")
            continue
        if PurePosixPath(argv[0].replace("\\", "/")).name != "cargo":
            errors.append(f"{mutant_name} phase {index} was not run by cargo")
        expected_tail = ["test"]
        if phase_name == "Build":
            expected_tail.append("--no-run")
        expected_tail.extend(
            [
                "--verbose",
                f"--package={expected_package}@{expected_version}",
                *(f"--features={feature}" for feature in expected_features),
                "--locked",
            ]
        )
        if argv[1:] != expected_tail:
            errors.append(
                f"{mutant_name} {phase_name} argv {argv[1:]!r} differs from "
                f"{expected_tail!r}"
            )

    if names not in (["Build"], ["Build", "Test"]):
        errors.append(f"{mutant_name} has impossible phase sequence {names}")
    if len(statuses) != len(phases):
        return None, errors
    if names == ["Build", "Test"] and statuses[0] != "Success":
        errors.append(f"{mutant_name} ran Test after a non-successful Build")
    if names == ["Build"] and statuses[0] == "Success":
        errors.append(f"{mutant_name} stopped after a successful Build")
    if errors:
        return None, errors

    if any(status == "Timeout" for status in statuses):
        return "Timeout", []
    if statuses[0] == "Failure":
        return "Unviable", []
    if len(names) == 2 and statuses[-1] == "Failure":
        return "CaughtMutant", []
    if len(names) == 2 and statuses[-1] == "Success":
        return "MissedMutant", []
    if statuses[-1] == "Success":
        return "Success", []
    return "Failure", []


def classify(args: argparse.Namespace) -> dict[str, Any]:
    repo_root = args.repo_root.resolve()
    policy, policy_errors = load_policy(args.policy, repo_root)
    blockers = [finding("policy", error) for error in policy_errors]
    advisories: list[dict[str, Any]] = []
    global_advisories: list[dict[str, Any]] = []
    hashes: dict[str, str] = {}
    if policy_errors or args.crate not in CRATES:
        if args.crate not in CRATES:
            blockers.append(finding("crate", f"unsupported crate {args.crate!r}"))
        entry: dict[str, Any] = {"path": "", "critical_paths": []}
    else:
        entry = policy["crates"][args.crate]
    crate_version, crate_version_error = read_crate_version(
        repo_root, entry.get("path", "")
    )
    if crate_version_error:
        blockers.append(finding("crate", crate_version_error))

    for label, path in (
        ("audit_json", args.audit_json),
        ("outcomes_json", args.outcomes_json),
        ("tool_metadata", args.tool_metadata),
        ("mutants_config", args.mutants_config),
        ("audit_exit", args.audit_exit),
        ("baseline_exit", args.baseline_exit),
        ("clean_exit", args.clean_exit),
    ):
        if path.is_file():
            hashes[label] = sha256_file(path)
        else:
            blockers.append(finding("missing_evidence", f"missing {label}: {path}"))

    if args.mutants_config.is_file() and not policy_errors:
        try:
            actual_config = args.mutants_config.read_text(encoding="utf-8")
        except (OSError, UnicodeError) as error:
            blockers.append(finding("mutants_config", f"cannot read config: {error}"))
        else:
            expected_config = render_mutants_config(entry.get("features", []))
            if actual_config != expected_config:
                blockers.append(
                    finding("mutants_config", "ephemeral cargo-mutants config differs from policy")
                )

    baseline_exit, error = read_exit(args.baseline_exit, "baseline")
    if error:
        blockers.append(finding("baseline", error))
    elif baseline_exit != 0:
        blockers.append(finding("baseline", f"baseline tests exited {baseline_exit}"))

    clean_exit, error = read_exit(args.clean_exit, "clean-tree")
    if error:
        blockers.append(finding("clean_tree", error))
    elif clean_exit != 0:
        blockers.append(finding("clean_tree", "audit changed the checked-out release tree"))

    audit_exit, error = read_exit(args.audit_exit, "Kanon audit")
    if error:
        blockers.append(finding("kanon_exit", error))
    elif audit_exit not in (0, 1):
        blockers.append(finding("kanon_exit", f"Kanon audit exited {audit_exit}"))

    metadata, metadata_error = read_json(args.tool_metadata)
    if metadata_error:
        blockers.append(finding("tool_identity", metadata_error))
    elif isinstance(metadata, dict) and not policy_errors:
        expected_tools = {
            "kanon_version": policy["tools"]["kanon_version"],
            "kanon_build_sha": policy["tools"]["kanon_commit"],
            "cargo_mutants_version": policy["tools"]["cargo_mutants_version"],
            "rustc_version": policy["tools"]["kanon_rust"],
        }
        expected_tool_keys = {*expected_tools, "kanon_binary_sha256"}
        if set(metadata) != expected_tool_keys:
            blockers.append(
                finding(
                    "tool_identity",
                    f"tool fields {sorted(metadata)} differ from "
                    f"{sorted(expected_tool_keys)}",
                )
            )
        for key, expected in expected_tools.items():
            if metadata.get(key) != expected:
                blockers.append(
                    finding(
                        "tool_identity",
                        f"{key}={metadata.get(key)!r}, expected {expected!r}",
                    )
                )
        binary_sha = metadata.get("kanon_binary_sha256")
        if not isinstance(binary_sha, str) or not re.fullmatch(r"[0-9a-f]{64}", binary_sha):
            blockers.append(finding("tool_identity", "Kanon binary SHA256 is missing"))
    elif metadata is not None:
        blockers.append(finding("tool_identity", "tool metadata must be an object"))

    report, report_error = read_json(args.audit_json)
    checks: dict[str, dict[str, Any]] = {}
    if report_error:
        blockers.append(finding("kanon_report", report_error))
    elif not isinstance(report, dict):
        blockers.append(finding("kanon_report", "Kanon report must be an object"))
    else:
        expected_report_keys = {
            "crate_path",
            "crate_name",
            "checks",
            "pass_count",
            "fail_count",
            "needs_human_count",
        }
        if set(report) != expected_report_keys:
            blockers.append(
                finding(
                    "kanon_report",
                    f"report fields {sorted(report)} differ from "
                    f"{sorted(expected_report_keys)}",
                )
            )
        if report.get("crate_name") != args.crate:
            blockers.append(
                finding(
                    "kanon_report",
                    f"crate_name={report.get('crate_name')!r}, expected {args.crate!r}",
                )
            )
        raw_crate_path = report.get("crate_path")
        if not isinstance(raw_crate_path, str):
            blockers.append(finding("kanon_report", "crate_path is missing"))
        else:
            reported = Path(raw_crate_path)
            expected_parts = PurePosixPath(entry.get("path", "")).parts
            path_matches = (
                reported.as_posix() == entry.get("path")
                or (
                    reported.is_absolute()
                    and ".." not in reported.parts
                    and len(reported.parts) >= len(expected_parts)
                    and tuple(reported.parts[-len(expected_parts) :])
                    == expected_parts
                )
            )
            if not path_matches:
                blockers.append(
                    finding(
                        "kanon_report",
                        f"crate_path={raw_crate_path!r}, expected {entry.get('path')!r}",
                    )
                )
        raw_checks = report.get("checks")
        if not isinstance(raw_checks, list):
            blockers.append(finding("kanon_report", "checks must be a list"))
        else:
            ordered_names = [
                raw.get("name") if isinstance(raw, dict) else None for raw in raw_checks
            ]
            if ordered_names != list(CHECK_ORDER):
                blockers.append(
                    finding("kanon_report", f"unexpected check order {ordered_names}")
                )
            for raw in raw_checks:
                if not isinstance(raw, dict) or not isinstance(raw.get("name"), str):
                    blockers.append(finding("kanon_report", "malformed check entry"))
                    continue
                if set(raw) != {"name", "result", "evidence"}:
                    blockers.append(
                        finding(
                            "kanon_report",
                            f"check fields {sorted(raw)} differ from "
                            "['evidence', 'name', 'result']",
                        )
                    )
                if not isinstance(raw.get("evidence"), str) or not raw["evidence"]:
                    blockers.append(
                        finding("kanon_report", f"{raw.get('name')} evidence is malformed")
                    )
                name = raw["name"]
                if name in checks:
                    blockers.append(finding("kanon_report", f"duplicate check {name}"))
                checks[name] = raw
            if set(checks) != CHECK_NAMES:
                blockers.append(
                    finding(
                        "kanon_report",
                        f"check set {sorted(checks)} differs from {sorted(CHECK_NAMES)}",
                    )
                )
        counts = {
            "PASS": report.get("pass_count"),
            "FAIL": report.get("fail_count"),
            "NEEDS_HUMAN": report.get("needs_human_count"),
        }
        if any(
            not isinstance(value, int) or isinstance(value, bool) or value < 0
            for value in counts.values()
        ):
            blockers.append(
                finding("kanon_report", "declared check counts must be nonnegative integers")
            )
        actual_counts = {key: 0 for key in counts}
        for check in checks.values():
            result = check.get("result")
            if result not in actual_counts:
                blockers.append(finding("kanon_report", f"unknown check result {result!r}"))
            else:
                actual_counts[result] += 1
        if counts != actual_counts:
            blockers.append(
                finding("kanon_report", f"declared counts {counts} differ from {actual_counts}")
            )
        for name, check in checks.items():
            if check.get("result") == "NEEDS_HUMAN":
                blockers.append(
                    finding(
                        "needs_human",
                        f"{name}: {check.get('evidence', 'no evidence')}",
                    )
                )

    outcomes, outcomes_error = read_json(args.outcomes_json)
    outcome_counts = {
        "CaughtMutant": 0,
        "MissedMutant": 0,
        "Timeout": 0,
        "Unviable": 0,
        "Success": 0,
        "Failure": 0,
    }
    if outcomes_error:
        blockers.append(finding("mutants_outcomes", outcomes_error))
    elif not isinstance(outcomes, dict):
        blockers.append(finding("mutants_outcomes", "outcomes JSON must be an object"))
    else:
        expected_lab_keys = {
            "outcomes",
            "total_mutants",
            "missed",
            "caught",
            "timeout",
            "unviable",
            "success",
            "cargo_mutants_version",
            "start_time",
            "end_time",
        }
        if set(outcomes) != expected_lab_keys:
            blockers.append(
                finding(
                    "mutants_outcomes",
                    f"lab fields {sorted(outcomes)} differ from {sorted(expected_lab_keys)}",
                )
            )
        expected_version = policy.get("tools", {}).get("cargo_mutants_version")
        if outcomes.get("cargo_mutants_version") != expected_version:
            blockers.append(
                finding(
                    "mutants_outcomes",
                    "cargo-mutants output version does not match policy",
                )
            )
        for count_key in (
            "total_mutants",
            "missed",
            "caught",
            "timeout",
            "unviable",
            "success",
        ):
            count_value = outcomes.get(count_key)
            if (
                not isinstance(count_value, int)
                or isinstance(count_value, bool)
                or count_value < 0
            ):
                blockers.append(
                    finding(
                        "mutants_outcomes",
                        f"{count_key} must be a nonnegative integer",
                    )
                )
        start_time, start_error = parse_timestamp(outcomes.get("start_time"), "start_time")
        end_time, end_error = parse_timestamp(outcomes.get("end_time"), "end_time")
        if start_error:
            blockers.append(finding("mutants_outcomes", start_error))
        if end_error:
            blockers.append(finding("mutants_outcomes", end_error))
        if start_time is not None and end_time is not None and end_time < start_time:
            blockers.append(
                finding("mutants_outcomes", "cargo-mutants end_time precedes start_time")
            )
        raw_outcomes = outcomes.get("outcomes")
        if not isinstance(raw_outcomes, list) or not raw_outcomes:
            blockers.append(finding("mutants_outcomes", "outcomes list is empty or missing"))
        else:
            for raw in raw_outcomes:
                if not isinstance(raw, dict):
                    blockers.append(finding("mutants_outcomes", "malformed outcome entry"))
                    continue
                expected_outcome_keys = {
                    "scenario",
                    "summary",
                    "log_path",
                    "diff_path",
                    "phase_results",
                }
                if set(raw) != expected_outcome_keys:
                    blockers.append(
                        finding(
                            "mutants_outcomes",
                            f"outcome fields {sorted(raw)} differ from "
                            f"{sorted(expected_outcome_keys)}",
                        )
                    )
                summary = raw.get("summary")
                if summary not in outcome_counts:
                    blockers.append(
                        finding("mutants_outcomes", f"unknown outcome summary {summary!r}")
                    )
                    continue
                scenario = raw.get("scenario")
                if scenario == "Baseline":
                    blockers.append(
                        finding(
                            "mutants_outcomes",
                            "unexpected Baseline scenario despite --baseline=skip",
                        )
                    )
                    continue
                if not isinstance(scenario, dict) or set(scenario) != {"Mutant"}:
                    blockers.append(
                        finding("mutants_outcomes", "scenario must contain exactly Mutant")
                    )
                    continue
                mutant = scenario.get("Mutant") if isinstance(scenario, dict) else None
                if not isinstance(mutant, dict):
                    blockers.append(finding("mutants_outcomes", "outcome lacks Mutant scenario"))
                    continue
                expected_mutant_keys = {
                    "name",
                    "package",
                    "file",
                    "function",
                    "span",
                    "replacement",
                    "genre",
                }
                if set(mutant) != expected_mutant_keys:
                    blockers.append(
                        finding(
                            "mutants_outcomes",
                            f"Mutant fields {sorted(mutant)} differ from "
                            f"{sorted(expected_mutant_keys)}",
                        )
                    )
                outcome_counts[summary] += 1
                raw_path = mutant.get("file")
                name = mutant.get("name")
                if not isinstance(raw_path, str) or not isinstance(name, str) or not name:
                    blockers.append(finding("mutants_outcomes", "mutant identity is malformed"))
                    continue
                if not isinstance(mutant.get("replacement"), str):
                    blockers.append(
                        finding("mutants_outcomes", f"{name} replacement is malformed")
                    )
                if mutant.get("genre") not in MUTANT_GENRES:
                    blockers.append(finding("mutants_outcomes", f"{name} genre is invalid"))
                function = mutant.get("function")
                if function is not None:
                    function_valid = (
                        isinstance(function, dict)
                        and set(function) == {"function_name", "return_type", "span"}
                        and isinstance(function.get("function_name"), str)
                        and bool(function.get("function_name"))
                        and isinstance(function.get("return_type"), str)
                        and valid_span(function.get("span"))
                    )
                    if not function_valid:
                        blockers.append(
                            finding("mutants_outcomes", f"{name} function is malformed")
                        )
                span = mutant.get("span")
                if not valid_span(span):
                    blockers.append(finding("mutants_outcomes", f"{name} span is malformed"))
                if not isinstance(raw.get("log_path"), str) or not raw.get("log_path"):
                    blockers.append(finding("mutants_outcomes", f"{name} log_path is malformed"))
                if raw.get("diff_path") is not None and not isinstance(
                    raw.get("diff_path"), str
                ):
                    blockers.append(finding("mutants_outcomes", f"{name} diff_path is malformed"))

                derived_summary, phase_errors = validate_phase_results(
                    raw.get("phase_results"),
                    entry.get("features", []),
                    args.crate,
                    crate_version or "",
                    name,
                )
                blockers.extend(
                    finding("mutants_outcomes", phase_error)
                    for phase_error in phase_errors
                )
                if derived_summary is not None and summary != derived_summary:
                    blockers.append(
                        finding(
                            "mutants_outcomes",
                            f"{name} summary {summary!r} differs from derived "
                            f"{derived_summary!r}",
                        )
                    )
                normalized = normalize_mutant_path(raw_path, entry.get("path", ""))
                if normalized is None or not (repo_root / normalized).is_file():
                    blockers.append(
                        finding("mutants_outcomes", f"unsafe or missing mutant path {raw_path!r}")
                    )
                    continue
                if mutant.get("package") != args.crate:
                    blockers.append(
                        finding(
                            "mutants_outcomes",
                            f"{name} package={mutant.get('package')!r}, expected {args.crate!r}",
                        )
                    )
                if summary in ("MissedMutant", "Timeout"):
                    item = finding(
                        "mutation",
                        f"{summary}: {name}",
                        path=normalized,
                        owner_key=f"mutation:{args.crate}",
                    )
                    if is_critical(normalized, entry.get("critical_paths", [])):
                        blockers.append(item)
                    else:
                        advisories.append(item)
                elif summary == "Failure":
                    blockers.append(
                        finding("mutants_outcomes", f"unclassified mutant failure: {name}")
                    )
                elif summary == "Success":
                    blockers.append(
                        finding("mutants_outcomes", f"unexpected untested mutant success: {name}")
                    )
            declared = {
                "total_mutants": sum(outcome_counts.values()),
                "missed": outcome_counts["MissedMutant"],
                "caught": outcome_counts["CaughtMutant"],
                "timeout": outcome_counts["Timeout"],
                "unviable": outcome_counts["Unviable"],
                "success": outcome_counts["Success"],
            }
            for key, actual in declared.items():
                if outcomes.get(key) != actual:
                    blockers.append(
                        finding(
                            "mutants_outcomes",
                            f"{key}={outcomes.get(key)!r}, counted {actual}",
                        )
                    )

    tautological, taut_errors = scan_tautological_docs(
        repo_root, entry.get("path", "")
    )
    blockers.extend(finding("tautological_doc", error) for error in taut_errors)
    for raw in tautological:
        item = finding(
            "tautological_doc",
            raw["text"],
            path=raw["path"],
            line=raw["line"],
            owner_key=f"tautological_doc:{args.crate}",
        )
        if is_critical(raw["path"], entry.get("critical_paths", [])):
            blockers.append(item)
        else:
            advisories.append(item)

    taut_result = checks.get("tautological_doc", {}).get("result")
    expected_taut_result = "FAIL" if tautological else "PASS"
    if taut_result is not None and taut_result != expected_taut_result:
        blockers.append(
            finding(
                "tautological_doc",
                f"Kanon reports {taut_result}, full scan requires {expected_taut_result}",
            )
        )

    config_check = checks.get("always_default_config", {})
    if config_check.get("result") == "FAIL":
        global_advisories.append(
            finding(
                "always_default_config",
                str(config_check.get("evidence", "performing configurability detected")),
                owner_key="always_default_config:workspace",
            )
        )

    mutation_result = checks.get("mutation", {}).get("result")
    has_mutation_problem = outcome_counts["MissedMutant"] + outcome_counts["Timeout"] > 0
    expected_mutation_result = "FAIL" if has_mutation_problem else "PASS"
    if mutation_result is not None and mutation_result != expected_mutation_result:
        blockers.append(
            finding(
                "mutation",
                f"Kanon reports {mutation_result}, raw outcomes require {expected_mutation_result}",
            )
        )

    any_check_failed = any(
        check.get("result") == "FAIL" for check in checks.values()
    )
    expected_exit = 1 if any_check_failed else 0
    if audit_exit in (0, 1) and audit_exit != expected_exit:
        blockers.append(
            finding(
                "kanon_exit",
                f"Kanon exited {audit_exit}, report requires {expected_exit}",
            )
        )

    status = "BLOCKED" if blockers else "PASS_WITH_ADVISORIES" if advisories or global_advisories else "PASS"
    return {
        "schema_version": SCHEMA_VERSION,
        "crate": args.crate,
        "crate_path": entry.get("path"),
        "repo_sha": args.repo_sha,
        "release_pr": args.release_pr,
        "source_run_id": args.source_run_id,
        "source_run_url": args.source_run_url,
        "policy_sha256": sha256_file(args.policy) if args.policy.is_file() else None,
        "policy": {
            "features": entry.get("features", []),
            "critical_paths": entry.get("critical_paths", []),
        },
        "check_results": {
            name: {
                "result": checks.get(name, {}).get("result"),
                "evidence": checks.get(name, {}).get("evidence"),
            }
            for name in CHECK_ORDER
        },
        "status": status,
        "blockers": blockers,
        "advisories": advisories,
        "global_advisories": global_advisories,
        "counts": outcome_counts,
        "evidence_sha256": hashes,
        "tools": metadata if isinstance(metadata, dict) else None,
    }


def parse_issue_map(raw: str) -> tuple[dict[str, str], list[str]]:
    try:
        value = json.loads(raw)
    except json.JSONDecodeError as error:
        return {}, [f"advisory issue map is not JSON: {error}"]
    if not isinstance(value, dict):
        return {}, ["advisory issue map must be an object"]
    result: dict[str, str] = {}
    errors: list[str] = []
    for key, url in value.items():
        if not isinstance(key, str) or not key:
            errors.append("advisory issue map contains an empty/non-string key")
        elif not isinstance(url, str) or not ISSUE_RE.fullmatch(url):
            errors.append(f"advisory owner {key!r} is not an Aletheia issue URL")
        else:
            result[key] = url
    return result, errors


def validate_finding_item(
    item: Any,
    *,
    crate: str | None,
    owner_required: bool,
    global_owner: bool = False,
    allow_crate_field: bool = False,
) -> list[str]:
    if not isinstance(item, dict):
        return ["finding is not an object"]
    allowed = {"kind", "detail", "path", "line", "owner_key"}
    if allow_crate_field:
        allowed.add("crate")
    errors: list[str] = []
    if not set(item).issubset(allowed):
        errors.append(f"finding has unsupported fields {sorted(set(item) - allowed)}")
    for key in ("kind", "detail"):
        if not isinstance(item.get(key), str) or not item[key]:
            errors.append(f"finding {key} must be a nonempty string")
    path = item.get("path")
    if path is not None:
        candidate = PurePosixPath(path) if isinstance(path, str) else None
        if (
            candidate is None
            or candidate.is_absolute()
            or ".." in candidate.parts
            or not candidate.parts
        ):
            errors.append("finding path is unsafe or malformed")
    line = item.get("line")
    if line is not None and (
        not isinstance(line, int) or isinstance(line, bool) or line < 1
    ):
        errors.append("finding line must be a positive integer")
    owner = item.get("owner_key")
    if owner_required and not isinstance(owner, str):
        errors.append("finding lacks owner_key")
    if owner is not None:
        if global_owner:
            allowed_owners = {"always_default_config:workspace"}
        elif crate is not None:
            allowed_owners = {f"mutation:{crate}", f"tautological_doc:{crate}"}
        else:
            allowed_owners = set()
        if owner not in allowed_owners:
            errors.append(f"finding has unsupported owner_key {owner!r}")
    if allow_crate_field and "crate" in item and item["crate"] not in CRATES:
        errors.append("finding crate is invalid")
    return errors


def validate_per_crate_receipt(
    receipt: dict[str, Any],
    *,
    crate: str,
    policy: dict[str, Any],
    policy_path: Path,
    policy_hash: str,
    receipt_path: Path,
    repo_root: Path,
    repo_sha: str,
    release_pr: int,
    source_run_id: str,
) -> list[str]:
    expected_keys = {
        "schema_version",
        "crate",
        "crate_path",
        "repo_sha",
        "release_pr",
        "source_run_id",
        "source_run_url",
        "policy_sha256",
        "policy",
        "check_results",
        "status",
        "blockers",
        "advisories",
        "global_advisories",
        "counts",
        "evidence_sha256",
        "tools",
    }
    errors: list[str] = []
    if set(receipt) != expected_keys:
        errors.append(
            f"fields {sorted(receipt)} differ from {sorted(expected_keys)}"
        )
    entry = policy.get("crates", {}).get(crate, {})
    expected_bindings = {
        "schema_version": SCHEMA_VERSION,
        "crate": crate,
        "crate_path": entry.get("path"),
        "repo_sha": repo_sha,
        "release_pr": release_pr,
        "source_run_id": source_run_id,
        "policy_sha256": policy_hash,
        "policy": {
            "features": entry.get("features", []),
            "critical_paths": entry.get("critical_paths", []),
        },
    }
    for key, expected in expected_bindings.items():
        if receipt.get(key) != expected:
            errors.append(f"{key}={receipt.get(key)!r}, expected {expected!r}")

    run_match = RUN_URL_RE.fullmatch(str(receipt.get("source_run_url", "")))
    if run_match is None or run_match.group(1) != source_run_id:
        errors.append("source_run_url does not match source_run_id")

    check_results = receipt.get("check_results")
    if not isinstance(check_results, dict) or set(check_results) != CHECK_NAMES:
        errors.append("check_results must contain the exact detector set")
    else:
        for name in CHECK_ORDER:
            result = check_results[name]
            if not isinstance(result, dict) or set(result) != {"result", "evidence"}:
                errors.append(f"check_results.{name} has invalid fields")
                continue
            if result.get("result") not in {"PASS", "FAIL", "NEEDS_HUMAN"}:
                errors.append(f"check_results.{name} has invalid result")
            if not isinstance(result.get("evidence"), str) or not result["evidence"]:
                errors.append(f"check_results.{name} has invalid evidence")

    lists: dict[str, list[Any]] = {}
    for label in ("blockers", "advisories", "global_advisories"):
        value = receipt.get(label)
        if not isinstance(value, list):
            errors.append(f"{label} must be a list")
            lists[label] = []
        else:
            lists[label] = value
    for label, items in lists.items():
        for item in items:
            errors.extend(
                f"{label}: {error}"
                for error in validate_finding_item(
                    item,
                    crate=crate,
                    owner_required=label != "blockers",
                    global_owner=label == "global_advisories",
                )
            )

    expected_status = (
        "BLOCKED"
        if lists["blockers"]
        else "PASS_WITH_ADVISORIES"
        if lists["advisories"] or lists["global_advisories"]
        else "PASS"
    )
    if receipt.get("status") not in RECEIPT_STATUSES:
        errors.append(f"status {receipt.get('status')!r} is invalid")
    elif receipt["status"] != expected_status:
        errors.append(
            f"status {receipt['status']!r} contradicts derived {expected_status!r}"
        )

    counts = receipt.get("counts")
    if not isinstance(counts, dict) or set(counts) != OUTCOME_COUNT_KEYS:
        errors.append("counts must contain the exact cargo-mutants outcome set")
    elif any(
        not isinstance(value, int) or isinstance(value, bool) or value < 0
        for value in counts.values()
    ):
        errors.append("counts values must be nonnegative integers")

    evidence = receipt.get("evidence_sha256")
    if not isinstance(evidence, dict) or set(evidence) != set(EVIDENCE_FILES):
        errors.append("evidence_sha256 must contain every raw evidence file")
    else:
        for label, relative in EVIDENCE_FILES.items():
            claimed = evidence[label]
            if not isinstance(claimed, str) or not re.fullmatch(r"[0-9a-f]{64}", claimed):
                errors.append(f"evidence_sha256.{label} is invalid")
                continue
            raw_path = receipt_path.parent / relative
            if not raw_path.is_file():
                errors.append(f"raw evidence is missing: {relative}")
            elif sha256_file(raw_path) != claimed:
                errors.append(f"raw evidence hash mismatch: {relative}")

    tools = receipt.get("tools")
    expected_tools = {
        "kanon_version": policy.get("tools", {}).get("kanon_version"),
        "kanon_build_sha": policy.get("tools", {}).get("kanon_commit"),
        "cargo_mutants_version": policy.get("tools", {}).get(
            "cargo_mutants_version"
        ),
        "rustc_version": policy.get("tools", {}).get("kanon_rust"),
    }
    if not isinstance(tools, dict) or set(tools) != {
        *expected_tools,
        "kanon_binary_sha256",
    }:
        errors.append("tools must contain the exact pinned tool identities")
    else:
        for key, expected in expected_tools.items():
            if tools.get(key) != expected:
                errors.append(f"tools.{key}={tools.get(key)!r}, expected {expected!r}")
        binary_hash = tools.get("kanon_binary_sha256")
        if not isinstance(binary_hash, str) or not re.fullmatch(
            r"[0-9a-f]{64}", binary_hash
        ):
            errors.append("tools.kanon_binary_sha256 is invalid")

    if not errors:
        raw_root = receipt_path.parent
        expected_receipt = classify(
            argparse.Namespace(
                repo_root=repo_root,
                policy=policy_path,
                crate=crate,
                repo_sha=repo_sha,
                release_pr=release_pr,
                source_run_id=source_run_id,
                source_run_url=(
                    "https://github.com/forkwright/aletheia/actions/runs/"
                    f"{source_run_id}"
                ),
                audit_json=raw_root / EVIDENCE_FILES["audit_json"],
                outcomes_json=raw_root / EVIDENCE_FILES["outcomes_json"],
                tool_metadata=raw_root / EVIDENCE_FILES["tool_metadata"],
                mutants_config=raw_root / EVIDENCE_FILES["mutants_config"],
                audit_exit=raw_root / EVIDENCE_FILES["audit_exit"],
                baseline_exit=raw_root / EVIDENCE_FILES["baseline_exit"],
                clean_exit=raw_root / EVIDENCE_FILES["clean_exit"],
            )
        )
        if receipt != expected_receipt:
            differing = sorted(
                key
                for key in set(receipt) | set(expected_receipt)
                if receipt.get(key) != expected_receipt.get(key)
            )
            errors.append(
                "receipt differs from raw-evidence reclassification in: "
                + ", ".join(differing)
            )
    return errors


def aggregate(args: argparse.Namespace) -> dict[str, Any]:
    receipts: dict[str, dict[str, Any]] = {}
    receipt_paths: dict[str, Path] = {}
    blockers: list[dict[str, Any]] = []
    repo_root = args.repo_root.resolve()
    policy, policy_errors = load_policy(args.policy, repo_root)
    blockers.extend(finding("policy", error) for error in policy_errors)
    if args.policy.is_file():
        policy_hash = sha256_file(args.policy)
    else:
        policy_hash = ""
        blockers.append(finding("policy", f"missing policy: {args.policy}"))
    source_run_id = str(args.source_run_id)
    source_run_url = (
        "https://github.com/forkwright/aletheia/actions/runs/" f"{source_run_id}"
    )
    if not re.fullmatch(r"[1-9][0-9]*", source_run_id):
        blockers.append(finding("receipt", "source_run_id must be a positive integer"))
    if RUN_URL_RE.fullmatch(args.run_url) is None:
        blockers.append(finding("receipt", "run_url is not a canonical Aletheia run URL"))
    for path in args.receipts:
        value, error = read_json(path)
        if error:
            blockers.append(finding("receipt", error))
            continue
        if not isinstance(value, dict):
            blockers.append(finding("receipt", f"{path} is not an object"))
            continue
        crate = value.get("crate")
        if crate not in CRATES:
            blockers.append(finding("receipt", f"{path} has unknown crate {crate!r}"))
            continue
        if crate in receipts:
            blockers.append(finding("receipt", f"duplicate receipt for {crate}"))
            continue
        receipts[crate] = value
        receipt_paths[crate] = path

    missing = sorted(set(CRATES) - set(receipts))
    if missing:
        blockers.append(finding("receipt", f"missing receipts: {', '.join(missing)}"))
    issue_map, issue_errors = parse_issue_map(args.advisory_issues_json)
    blockers.extend(finding("advisory_owner", error) for error in issue_errors)

    advisories_by_key: dict[str, list[dict[str, Any]]] = {}
    global_by_key: dict[str, dict[str, Any]] = {}
    global_observations: list[tuple[Any, Any]] = []
    valid_receipts: set[str] = set()
    for crate in CRATES:
        receipt = receipts.get(crate)
        if receipt is None:
            continue
        receipt_valid = False
        if not policy_errors:
            validation_errors = validate_per_crate_receipt(
                receipt,
                crate=crate,
                policy=policy,
                policy_path=args.policy,
                policy_hash=policy_hash,
                receipt_path=receipt_paths[crate],
                repo_root=repo_root,
                repo_sha=args.repo_sha,
                release_pr=args.release_pr,
                source_run_id=source_run_id,
            )
            blockers.extend(
                finding("receipt", f"{crate}: {error}")
                for error in validation_errors
            )
            receipt_valid = not validation_errors

        if not receipt_valid:
            continue
        valid_receipts.add(crate)

        raw_blockers = receipt.get("blockers")
        if isinstance(raw_blockers, list):
            for item in raw_blockers:
                if isinstance(item, dict):
                    blockers.append({"crate": crate, **item})
        raw_advisories = receipt.get("advisories")
        if isinstance(raw_advisories, list):
            for item in raw_advisories:
                if not isinstance(item, dict):
                    continue
                key = item.get("owner_key")
                if isinstance(key, str):
                    advisories_by_key.setdefault(key, []).append(
                        {"crate": crate, **item}
                    )
        raw_global = receipt.get("global_advisories")
        check_results = receipt.get("check_results")
        config_result = (
            check_results.get("always_default_config")
            if isinstance(check_results, dict)
            else None
        )
        global_observations.append((config_result, raw_global))
        if not isinstance(raw_global, list):
            continue
        for item in raw_global:
            if not isinstance(item, dict):
                continue
            key = item.get("owner_key")
            if isinstance(key, str):
                global_by_key.setdefault(key, item)

    if global_observations and any(
        observation != global_observations[0]
        for observation in global_observations[1:]
    ):
        blockers.append(
            finding(
                "receipt",
                "always_default_config results differ across the five crate receipts",
            )
        )

    all_advisory_keys = set(advisories_by_key) | set(global_by_key)
    for key in sorted(all_advisory_keys):
        if key not in issue_map:
            blockers.append(finding("advisory_owner", f"{key} has no verified issue owner"))
    unused = sorted(set(issue_map) - all_advisory_keys)
    if unused:
        blockers.append(
            finding("advisory_owner", f"issue map has unused keys: {', '.join(unused)}")
        )

    status = "BLOCKED" if blockers else "PASS_WITH_ADVISORIES" if all_advisory_keys else "PASS"
    advisory_receipts: dict[str, dict[str, Any]] = {}
    for key in sorted(all_advisory_keys):
        items = (
            advisories_by_key[key]
            if key in advisories_by_key
            else [global_by_key[key]]
        )
        advisory_receipts[key] = {"issue": issue_map.get(key), "findings": items}
    return {
        "schema_version": SCHEMA_VERSION,
        "repo_sha": args.repo_sha,
        "release_pr": args.release_pr,
        "source_run_id": source_run_id,
        "source_run_url": source_run_url,
        "run_url": args.run_url,
        "policy_sha256": policy_hash,
        "status": status,
        "blockers": blockers,
        "advisories": advisory_receipts,
        "receipts": {
            crate: {
                "status": receipt.get("status") if crate in valid_receipts else "BLOCKED",
                "sha256": sha256_file(receipt_paths[crate]),
            }
            for crate, receipt in sorted(receipts.items())
        },
    }


def validate_aggregate_receipt(
    receipt: dict[str, Any],
    *,
    policy_hash: str,
    repo_sha: str,
    release_pr: int,
    source_run_id: str,
    run_url: str,
    require_complete: bool,
) -> list[str]:
    expected_keys = {
        "schema_version",
        "repo_sha",
        "release_pr",
        "source_run_id",
        "source_run_url",
        "run_url",
        "policy_sha256",
        "status",
        "blockers",
        "advisories",
        "receipts",
    }
    errors: list[str] = []
    if set(receipt) != expected_keys:
        errors.append(
            f"aggregate fields {sorted(receipt)} differ from {sorted(expected_keys)}"
        )
    bindings = {
        "schema_version": SCHEMA_VERSION,
        "repo_sha": repo_sha,
        "release_pr": release_pr,
        "source_run_id": source_run_id,
        "source_run_url": (
            "https://github.com/forkwright/aletheia/actions/runs/"
            f"{source_run_id}"
        ),
        "run_url": run_url,
        "policy_sha256": policy_hash,
    }
    for key, expected in bindings.items():
        if receipt.get(key) != expected:
            errors.append(f"aggregate {key}={receipt.get(key)!r}, expected {expected!r}")
    for label in ("source_run_url", "run_url"):
        if RUN_URL_RE.fullmatch(str(receipt.get(label, ""))) is None:
            errors.append(f"aggregate {label} is not canonical")

    blockers = receipt.get("blockers")
    if not isinstance(blockers, list):
        errors.append("aggregate blockers must be a list")
        blockers = []
    else:
        for item in blockers:
            item_crate = item.get("crate") if isinstance(item, dict) else None
            errors.extend(
                f"aggregate blocker: {error}"
                for error in validate_finding_item(
                    item,
                    crate=item_crate if item_crate in CRATES else None,
                    owner_required=False,
                    allow_crate_field=True,
                )
            )

    advisories = receipt.get("advisories")
    if not isinstance(advisories, dict):
        errors.append("aggregate advisories must be an object")
        advisories = {}
    else:
        for owner_key, bundle in advisories.items():
            if not isinstance(owner_key, str) or not owner_key:
                errors.append("aggregate advisory owner key is invalid")
                continue
            if not isinstance(bundle, dict) or set(bundle) != {"issue", "findings"}:
                errors.append(f"aggregate advisory {owner_key!r} has invalid fields")
                continue
            issue = bundle.get("issue")
            if not isinstance(issue, str) or ISSUE_RE.fullmatch(issue) is None:
                errors.append(f"aggregate advisory {owner_key!r} lacks an issue")
            findings = bundle.get("findings")
            if not isinstance(findings, list) or not findings:
                errors.append(f"aggregate advisory {owner_key!r} has no findings")
                continue
            for item in findings:
                item_crate = item.get("crate") if isinstance(item, dict) else None
                is_global = owner_key == "always_default_config:workspace"
                finding_errors = validate_finding_item(
                    item,
                    crate=item_crate if item_crate in CRATES else None,
                    owner_required=True,
                    global_owner=is_global,
                    allow_crate_field=True,
                )
                if isinstance(item, dict) and item.get("owner_key") != owner_key:
                    finding_errors.append("finding owner_key differs from bundle key")
                if not is_global and item_crate not in CRATES:
                    finding_errors.append("crate advisory lacks a valid crate")
                if is_global and isinstance(item, dict) and "crate" in item:
                    finding_errors.append("global advisory must not name one crate")
                errors.extend(
                    f"aggregate advisory {owner_key!r}: {error}"
                    for error in finding_errors
                )

    summaries = receipt.get("receipts")
    if not isinstance(summaries, dict):
        errors.append("aggregate receipts must be an object")
        summaries = {}
    if require_complete and set(summaries) != set(CRATES):
        errors.append("aggregate must contain exactly five crate receipt summaries")
    elif not set(summaries).issubset(CRATES):
        errors.append("aggregate contains an unknown crate receipt summary")
    for crate, summary in summaries.items():
        if not isinstance(summary, dict) or set(summary) != {"status", "sha256"}:
            errors.append(f"aggregate receipt summary for {crate} is malformed")
            continue
        if summary.get("status") not in RECEIPT_STATUSES:
            errors.append(f"aggregate receipt status for {crate} is invalid")
        claimed_hash = summary.get("sha256")
        if not isinstance(claimed_hash, str) or re.fullmatch(
            r"[0-9a-f]{64}", claimed_hash
        ) is None:
            errors.append(f"aggregate receipt hash for {crate} is invalid")

    summary_statuses = {
        summary.get("status")
        for summary in summaries.values()
        if isinstance(summary, dict)
    }
    has_blocked_summary = "BLOCKED" in summary_statuses
    has_advisory_summary = "PASS_WITH_ADVISORIES" in summary_statuses
    if has_advisory_summary and not advisories:
        errors.append("advisory receipt summary has no aggregate advisory bundle")
    expected_status = (
        "BLOCKED"
        if blockers or has_blocked_summary
        else "PASS_WITH_ADVISORIES"
        if advisories or has_advisory_summary
        else "PASS"
    )
    if receipt.get("status") not in RECEIPT_STATUSES:
        errors.append(f"aggregate status {receipt.get('status')!r} is invalid")
    elif receipt["status"] != expected_status:
        errors.append(
            f"aggregate status {receipt['status']!r} contradicts {expected_status!r}"
        )
    if receipt.get("status") != "BLOCKED" and set(summaries) != set(CRATES):
        errors.append("non-blocked aggregate lacks the exact five receipts")
    return errors


def render_body(body: str, receipt: dict[str, Any], receipt_sha: str) -> str:
    status = receipt.get("status", "BLOCKED")
    advisories = receipt.get("advisories", {})
    lines = [
        START_MARKER,
        "### Substance audit receipt",
        "",
        f"- Verdict: **{status}**",
        f"- Commit: `{receipt.get('repo_sha')}`",
        f"- Hosted run: {receipt.get('run_url')}",
        f"- Aggregate receipt SHA-256: `{receipt_sha}`",
    ]
    if advisories:
        lines.append("- Advisory owners:")
        for key, value in sorted(advisories.items()):
            lines.append(f"  - `{key}`: {value.get('issue')}")
    else:
        lines.append("- Advisory owners: none")
    if receipt.get("blockers"):
        lines.append(f"- Blocking findings: {len(receipt['blockers'])}")
    lines.append(END_MARKER)
    block = "\n".join(lines)

    if START_MARKER in body or END_MARKER in body:
        if body.count(START_MARKER) != 1 or body.count(END_MARKER) != 1:
            raise ValueError("PR body has malformed substance-audit markers")
        start = body.index(START_MARKER)
        end = body.index(END_MARKER, start) + len(END_MARKER)
        return body[:start].rstrip() + "\n\n" + block + body[end:].rstrip() + "\n"
    return body.rstrip() + "\n\n" + block + "\n"


def cmd_validate(args: argparse.Namespace) -> int:
    policy, errors = load_policy(args.policy, args.repo_root.resolve())
    if not errors:
        errors.extend(validate_workflow_contract(policy, args.repo_root.resolve()))
    if errors:
        for error in errors:
            print(f"substance-audit: {error}", file=sys.stderr)
        return 1
    print("substance-audit: policy valid")
    return 0


def cmd_matrix(args: argparse.Namespace) -> int:
    policy, errors = load_policy(args.policy, args.repo_root.resolve())
    if errors:
        for error in errors:
            print(f"substance-audit: {error}", file=sys.stderr)
        return 1
    print(json.dumps({"include": matrix(policy)}, separators=(",", ":")))
    return 0


def cmd_write_config(args: argparse.Namespace) -> int:
    policy, errors = load_policy(args.policy, args.repo_root.resolve())
    if errors or args.crate not in CRATES:
        for error in errors:
            print(f"substance-audit: {error}", file=sys.stderr)
        if args.crate not in CRATES:
            print(f"substance-audit: unsupported crate {args.crate!r}", file=sys.stderr)
        return 1
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        render_mutants_config(policy["crates"][args.crate]["features"]),
        encoding="utf-8",
    )
    return 0


def cmd_classify(args: argparse.Namespace) -> int:
    receipt = classify(args)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"substance-audit: {args.crate} {receipt['status']}")
    return 0


def cmd_aggregate(args: argparse.Namespace) -> int:
    receipt = aggregate(args)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"substance-audit: aggregate {receipt['status']}")
    return 0


def cmd_render_body(args: argparse.Namespace) -> int:
    receipt, error = read_json(args.receipt)
    if error or not isinstance(receipt, dict):
        print(f"substance-audit: {error or 'receipt is not an object'}", file=sys.stderr)
        return 1
    if not re.fullmatch(r"[0-9a-f]{64}", args.receipt_sha256):
        print("substance-audit: receipt SHA256 is malformed", file=sys.stderr)
        return 1
    if sha256_file(args.receipt) != args.receipt_sha256:
        print("substance-audit: receipt SHA256 does not match receipt bytes", file=sys.stderr)
        return 1
    policy_hash = sha256_file(args.policy) if args.policy.is_file() else ""
    embedded_release_pr = receipt.get("release_pr")
    validation_errors = validate_aggregate_receipt(
        receipt,
        policy_hash=policy_hash,
        repo_sha=str(receipt.get("repo_sha", "")),
        release_pr=(
            embedded_release_pr
            if isinstance(embedded_release_pr, int)
            and not isinstance(embedded_release_pr, bool)
            else -1
        ),
        source_run_id=str(receipt.get("source_run_id", "")),
        run_url=str(receipt.get("run_url", "")),
        require_complete=receipt.get("status") != "BLOCKED",
    )
    if validation_errors:
        for validation_error in validation_errors:
            print(f"substance-audit: {validation_error}", file=sys.stderr)
        return 1
    try:
        result = render_body(
            args.body.read_text(encoding="utf-8"), receipt, args.receipt_sha256
        )
    except (OSError, UnicodeError, ValueError) as error_value:
        print(f"substance-audit: {error_value}", file=sys.stderr)
        return 1
    args.output.write_text(result, encoding="utf-8")
    return 0


def cmd_issue_map(args: argparse.Namespace) -> int:
    value, errors = parse_issue_map(args.json)
    if errors:
        for error in errors:
            print(f"substance-audit: {error}", file=sys.stderr)
        return 1
    print(json.dumps(value, sort_keys=True, separators=(",", ":")))
    return 0


def cmd_enforce(args: argparse.Namespace) -> int:
    receipt, error = read_json(args.receipt)
    if error or not isinstance(receipt, dict):
        print(f"substance-audit: {error or 'receipt is not an object'}", file=sys.stderr)
        return 1
    policy_hash = sha256_file(args.policy) if args.policy.is_file() else ""
    validation_errors = validate_aggregate_receipt(
        receipt,
        policy_hash=policy_hash,
        repo_sha=args.repo_sha,
        release_pr=args.release_pr,
        source_run_id=str(args.source_run_id),
        run_url=args.run_url,
        require_complete=True,
    )
    if validation_errors:
        for validation_error in validation_errors:
            print(f"substance-audit: {validation_error}", file=sys.stderr)
        return 1
    if receipt.get("status") not in ("PASS", "PASS_WITH_ADVISORIES"):
        print("substance-audit: release is BLOCKED; inspect aggregate receipt", file=sys.stderr)
        return 1
    print(f"substance-audit: release may proceed ({receipt['status']})")
    return 0


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument(
        "--policy",
        type=Path,
        default=Path(__file__).with_name("substance-audit-policy.toml"),
    )
    result.add_argument("--repo-root", type=Path, default=Path(__file__).parents[1])
    commands = result.add_subparsers(dest="command", required=True)
    commands.add_parser("validate").set_defaults(func=cmd_validate)
    commands.add_parser("matrix").set_defaults(func=cmd_matrix)

    write_config = commands.add_parser("write-config")
    write_config.add_argument("--crate", required=True)
    write_config.add_argument("--output", type=Path, required=True)
    write_config.set_defaults(func=cmd_write_config)

    classify_parser = commands.add_parser("classify")
    classify_parser.add_argument("--crate", required=True)
    classify_parser.add_argument("--repo-sha", required=True)
    classify_parser.add_argument("--release-pr", type=int, required=True)
    classify_parser.add_argument("--source-run-id", required=True)
    classify_parser.add_argument("--source-run-url", required=True)
    classify_parser.add_argument("--audit-json", type=Path, required=True)
    classify_parser.add_argument("--outcomes-json", type=Path, required=True)
    classify_parser.add_argument("--audit-exit", type=Path, required=True)
    classify_parser.add_argument("--baseline-exit", type=Path, required=True)
    classify_parser.add_argument("--clean-exit", type=Path, required=True)
    classify_parser.add_argument("--tool-metadata", type=Path, required=True)
    classify_parser.add_argument("--mutants-config", type=Path, required=True)
    classify_parser.add_argument("--output", type=Path, required=True)
    classify_parser.set_defaults(func=cmd_classify)

    aggregate_parser = commands.add_parser("aggregate")
    aggregate_parser.add_argument("--repo-sha", required=True)
    aggregate_parser.add_argument("--release-pr", type=int, required=True)
    aggregate_parser.add_argument("--source-run-id", required=True)
    aggregate_parser.add_argument("--run-url", required=True)
    aggregate_parser.add_argument("--advisory-issues-json", default="{}")
    aggregate_parser.add_argument("--receipts", type=Path, nargs="+", required=True)
    aggregate_parser.add_argument("--output", type=Path, required=True)
    aggregate_parser.set_defaults(func=cmd_aggregate)

    render_parser = commands.add_parser("render-body")
    render_parser.add_argument("--body", type=Path, required=True)
    render_parser.add_argument("--receipt", type=Path, required=True)
    render_parser.add_argument("--receipt-sha256", required=True)
    render_parser.add_argument("--output", type=Path, required=True)
    render_parser.set_defaults(func=cmd_render_body)

    issue_parser = commands.add_parser("issue-map")
    issue_parser.add_argument("--json", required=True)
    issue_parser.set_defaults(func=cmd_issue_map)

    enforce_parser = commands.add_parser("enforce")
    enforce_parser.add_argument("--receipt", type=Path, required=True)
    enforce_parser.add_argument("--repo-sha", required=True)
    enforce_parser.add_argument("--release-pr", type=int, required=True)
    enforce_parser.add_argument("--source-run-id", required=True)
    enforce_parser.add_argument("--run-url", required=True)
    enforce_parser.set_defaults(func=cmd_enforce)
    return result


def main() -> int:
    args = parser().parse_args()
    if hasattr(args, "repo_sha") and not SHA_RE.fullmatch(args.repo_sha):
        print("substance-audit: repo SHA must be lowercase 40-hex", file=sys.stderr)
        return 2
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
