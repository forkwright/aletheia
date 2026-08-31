#!/usr/bin/env python3
"""Certify that release artifacts resolve only the declared binary graph.

The release job once ran build commands at the virtual workspace root. Cargo
therefore built every member, including integration-tests, whose normal
dependencies enable test-only code in shared crates. This check binds the
release workflow's build commands to the shipped package and replays each
release target's normal/build dependency graph without compiling it.
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path
from typing import Any

import yaml

RELEASE_WORKFLOW = Path(".github/workflows/release.yml")
SHIPPED_PACKAGE = "aletheia"
SHIPPED_BIN = "aletheia"
RELEASE_FEATURES = "recall,embed-candle"
TEST_HARNESS_MEMBER = "integration-tests"
FORBIDDEN_FEATURES = frozenset(
    {
        "test-support",
        "test-utils",
        "test-core",
        "test-full",
        "crash-injection",
        "online-tests",
    }
)
REQUIRED_FRAGMENTS = (
    "--locked",
    f"-p {SHIPPED_PACKAGE}",
    f"--bin {SHIPPED_BIN}",
    f"--features {RELEASE_FEATURES}",
)
BUILD_COMMAND = re.compile(r"^(?:cargo auditable build|cross build)\b")
MEMBER_LINE = re.compile(
    r"^(?P<name>[A-Za-z0-9_-]+) v[^ ]+ \(/[^)]*\)(?: (?P<features>[^ (]+))?(?: \(\*\))?$"
)


class ScopeCheckError(Exception):
    """The checker cannot certify the current workflow shape."""


def release_build_commands(workflow: dict[str, Any]) -> list[str]:
    """Return release-binary build command lines from all workflow steps."""
    commands: list[str] = []
    for job in workflow.get("jobs", {}).values():
        for step in job.get("steps", []) or []:
            run = step.get("run")
            if isinstance(run, str):
                commands.extend(
                    line for raw in run.splitlines()
                    if BUILD_COMMAND.match(line := raw.strip())
                )
    return commands


def command_violations(commands: list[str]) -> list[str]:
    """Identify build commands that do not name exactly the shipped scope."""
    if not commands:
        return [
            f"no release build command found in {RELEASE_WORKFLOW}; "
            "update release_build_commands() before certifying this workflow"
        ]
    violations: list[str] = []
    for command in commands:
        for fragment in REQUIRED_FRAGMENTS:
            if fragment not in command:
                violations.append(f"missing `{fragment}`: {command}")
        if "--workspace" in command:
            violations.append(f"workspace-wide release build: {command}")
    return violations


def matrix_targets(workflow: dict[str, Any]) -> list[str]:
    """Return the build job's release targets, or fail closed on workflow drift."""
    try:
        targets = [
            entry["target"]
            for entry in workflow["jobs"]["build"]["strategy"]["matrix"]["include"]
        ]
    except (KeyError, TypeError) as error:
        raise ScopeCheckError(
            f"cannot read build matrix targets from {RELEASE_WORKFLOW}: {error!r}"
        ) from error
    if not targets or not all(isinstance(target, str) for target in targets):
        raise ScopeCheckError(f"build matrix in {RELEASE_WORKFLOW} lists no valid targets")
    return targets


def resolved_member_features(tree_stdout: str) -> dict[str, set[str]]:
    """Parse workspace-member features from `cargo tree -f '{p} {f}'` output."""
    members: dict[str, set[str]] = {}
    for raw in tree_stdout.splitlines():
        match = MEMBER_LINE.match(raw.strip())
        if match is None:
            continue
        features = match.group("features")
        members.setdefault(match.group("name"), set()).update(
            features.split(",") if features else ()
        )
    return members


def forbidden_resolutions(members: dict[str, set[str]], target: str) -> list[str]:
    """Return forbidden test-only features and harness-member appearances."""
    if not members:
        return [
            f"[{target}] no workspace members parsed from cargo tree output; "
            "the replay certifies nothing"
        ]
    violations = [
        f"[{target}] {name} resolves test-only feature(s) {','.join(leaked)} "
        "into the shipped graph"
        for name, features in sorted(members.items())
        if (leaked := sorted(features & FORBIDDEN_FEATURES))
    ]
    if TEST_HARNESS_MEMBER in members:
        violations.append(f"[{target}] {TEST_HARNESS_MEMBER} is in the shipped dependency graph")
    return violations


def scoped_tree(target: str) -> subprocess.CompletedProcess[str]:
    """Replay the release package's normal/build resolution for one target."""
    return subprocess.run(
        [
            "cargo", "tree", "--locked", "-p", SHIPPED_PACKAGE, "--features",
            RELEASE_FEATURES, "-e", "normal,build,features", "-f", "{p} {f}",
            "--prefix", "none", "--target", target,
        ],
        capture_output=True,
        text=True,
        check=False,
    )


def main() -> int:
    workflow = yaml.safe_load(RELEASE_WORKFLOW.read_text(encoding="utf-8"))
    violations = command_violations(release_build_commands(workflow))
    for target in matrix_targets(workflow):
        completed = scoped_tree(target)
        if completed.returncode:
            detail = completed.stderr.strip().splitlines()
            violations.append(
                f"[{target}] cargo tree failed: {detail[0] if detail else '(no stderr output)'}"
            )
        else:
            violations.extend(forbidden_resolutions(resolved_member_features(completed.stdout), target))
    if violations:
        for violation in violations:
            print(f"release-build-scope: {violation}", file=sys.stderr)
        return 1
    print(
        "release-build-scope: release build is scoped to "
        f"-p {SHIPPED_PACKAGE} --bin {SHIPPED_BIN} --features {RELEASE_FEATURES}; "
        "no test-only feature resolves into the shipped graph"
    )
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except ScopeCheckError as error:
        print(f"release-build-scope: {error}", file=sys.stderr)
        sys.exit(1)
