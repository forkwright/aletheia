#!/usr/bin/env python3
"""Fail if the release build is not scoped to exactly the shipped package/features.

aletheia#6999: release.yml's build job used to run `cargo auditable build`
from the virtual workspace root with no `-p`. Cargo then built every
workspace member and unified features across all of them into the shipped
binary: `integration-tests`' NORMAL dependencies switched on
`koina/test-support`, `eidos/test-support`, `organon/test-support`,
`mneme/test-support` + `mneme/crash-injection` and
`hermeneus/test-support` + `test-utils` in every released artifact — code
the `--features recall,embed-candle` line never declared. Only the
tag-triggered build ever exercised that resolution, so nothing pre-merge
could redden.

This gate binds the release build to its declared scope from both ends,
with no compile:

1. The build commands in .github/workflows/release.yml must carry
   `--locked -p aletheia --bin aletheia --features recall,embed-candle`
   and must not be workspace-wide.
2. `cargo tree` replays the scoped resolution per release target
   (`-e normal,build,features` — dev-dependencies excluded, exactly what
   `cargo build` compiles) and fails if any workspace member resolves a
   test-only feature into the shipped dependency graph, or if the test
   harness crate appears in it at all.
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
# Feature names that gate test-only code in workspace members. None of them
# may resolve on in the shipped binary's dependency graph.
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
BUILD_COMMAND = re.compile(r"^(cargo auditable build|cross build)\b")
# `{p} {f}` tree lines for workspace members carry an absolute path source:
#   koina v0.44.0 (/abs/crates/koina) default,fjall-helpers [(*)]
MEMBER_LINE = re.compile(
    r"^(?P<name>[A-Za-z0-9_-]+) v[^ ]+ \(/[^)]*\)(?: (?P<features>[^ (]+))?( \(\*\))?$"
)


class ScopeCheckError(Exception):
    """A structural failure that makes the check unable to certify anything."""


def release_build_commands(workflow: dict[str, Any]) -> list[str]:
    """Every release-binary build command line across the workflow's run blocks."""
    commands: list[str] = []
    for job in workflow.get("jobs", {}).values():
        for step in job.get("steps", []) or []:
            run = step.get("run")
            if not isinstance(run, str):
                continue
            for raw in run.splitlines():
                line = raw.strip()
                if BUILD_COMMAND.match(line):
                    commands.append(line)
    return commands


def command_violations(commands: list[str]) -> list[str]:
    if not commands:
        return [
            "no release build command found in "
            f"{RELEASE_WORKFLOW} — the scan certifies nothing; update "
            "release_build_commands() to match the current build steps"
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
    try:
        include = workflow["jobs"]["build"]["strategy"]["matrix"]["include"]
        targets = [entry["target"] for entry in include]
    except (KeyError, TypeError) as exc:
        raise ScopeCheckError(
            f"cannot read build matrix targets from {RELEASE_WORKFLOW}: {exc!r}"
        ) from exc
    if not targets:
        raise ScopeCheckError(f"build matrix in {RELEASE_WORKFLOW} lists no targets")
    return targets


def resolved_member_features(tree_stdout: str) -> dict[str, set[str]]:
    """Workspace-member package name -> resolved features, from cargo tree output.

    Registry packages (no path source) and `pkg feature "name"` pseudo-nodes
    are out of scope; repeated (deduplicated) member lines union.
    """
    members: dict[str, set[str]] = {}
    for line in tree_stdout.splitlines():
        match = MEMBER_LINE.match(line.strip())
        if match is None:
            continue
        features = match.group("features")
        resolved = set(features.split(",")) if features else set()
        members.setdefault(match.group("name"), set()).update(resolved)
    return members


def forbidden_resolutions(members: dict[str, set[str]], target: str) -> list[str]:
    if not members:
        return [
            f"[{target}] no workspace members parsed from cargo tree output — "
            "the replay certifies nothing; check the tree invocation and parser"
        ]
    violations: list[str] = []
    for name in sorted(members):
        leaked = sorted(members[name] & FORBIDDEN_FEATURES)
        if leaked:
            violations.append(
                f"[{target}] {name} resolves test-only feature(s) "
                f"{','.join(leaked)} into the shipped graph"
            )
    if TEST_HARNESS_MEMBER in members:
        violations.append(
            f"[{target}] {TEST_HARNESS_MEMBER} is in the shipped dependency graph"
        )
    return violations


def scoped_tree(target: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [
            "cargo",
            "tree",
            "--locked",
            "-p",
            SHIPPED_PACKAGE,
            "--features",
            RELEASE_FEATURES,
            "-e",
            "normal,build,features",
            "-f",
            "{p} {f}",
            "--prefix",
            "none",
            "--target",
            target,
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
        if completed.returncode != 0:
            stderr = completed.stderr.strip().splitlines()
            detail = stderr[0] if stderr else "(no stderr output)"
            violations.append(f"[{target}] cargo tree failed: {detail}")
            continue
        violations.extend(
            forbidden_resolutions(resolved_member_features(completed.stdout), target)
        )
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
    except ScopeCheckError as exc:
        print(f"release-build-scope: {exc}", file=sys.stderr)
        sys.exit(1)
