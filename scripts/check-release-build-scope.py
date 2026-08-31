#!/usr/bin/env python3
"""Certify that release artifacts resolve only the declared binary graph.

The release job once built at the virtual workspace root, which built every
member and unified integration-tests' test-only features into release artifacts.
This check fail-closes on workflow drift, then replays the *validated* release
package/features/targets without compiling them.
"""

from __future__ import annotations

import re
import shlex
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import yaml

REPO_ROOT = Path(__file__).resolve().parents[1]
RELEASE_WORKFLOW = Path(".github/workflows/release.yml")
SHIPPED_PACKAGE = "aletheia"
SHIPPED_BIN = "aletheia"
RELEASE_FEATURES = "recall,embed-candle"
BUILD_TARGET_ENV = "$BUILD_TARGET"
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

NATIVE_TARGET = "aarch64-apple-darwin"
CROSS_TARGET = "x86_64-unknown-linux-musl"
BUILD_TEXT = re.compile(r"\b(?:cargo\s+(?:auditable\s+)?build|cross\s+build)\b")
LOCAL_PACKAGE_LINE = re.compile(
    r"^(?P<name>[A-Za-z0-9_-]+) v[^ ]+ \((?P<source>/[^)]*)\)"
    r"(?: (?P<features>[^ (]+))?(?: \(\*\))?$"
)


class ScopeCheckError(Exception):
    """The checker cannot certify the workflow or its dependency graph."""


@dataclass(frozen=True)
class ExpectedBuild:
    kind: str
    name: str
    condition: str
    matrix_target: str
    matrix_cross: bool
    argv: tuple[str, ...]


@dataclass(frozen=True)
class ValidatedBuild:
    expected: ExpectedBuild
    package: str
    binary: str
    features: str


EXPECTED_BUILDS = (
    ExpectedBuild(
        kind="native",
        name="Build (native)",
        condition="${{ !matrix.cross }}",
        matrix_target=NATIVE_TARGET,
        matrix_cross=False,
        argv=(
            "cargo", "auditable", "build", "--locked", "--release", "-p",
            SHIPPED_PACKAGE, "--bin", SHIPPED_BIN, "--target", BUILD_TARGET_ENV,
            "--features", RELEASE_FEATURES,
        ),
    ),
    ExpectedBuild(
        kind="cross",
        name="Build (cross)",
        condition="${{ matrix.cross }}",
        matrix_target=CROSS_TARGET,
        matrix_cross=True,
        argv=(
            "cross", "build", "--locked", "--release", "-p", SHIPPED_PACKAGE,
            "--bin", SHIPPED_BIN, "--target", BUILD_TARGET_ENV, "--features",
            RELEASE_FEATURES,
        ),
    ),
)


def direct_shell_argv(run: str) -> list[str]:
    """Parse one direct shell command, rejecting compound or multiline runs."""
    lines = [line.strip() for line in run.splitlines() if line.strip()]
    if len(lines) != 1:
        raise ScopeCheckError("release build command must be one direct shell command")
    try:
        argv = shlex.split(lines[0], posix=True)
    except ValueError as error:
        raise ScopeCheckError(f"cannot parse release build shell command: {error}") from error
    if not argv or any(token in {";", "&&", "||", "|", "&"} for token in argv):
        raise ScopeCheckError("release build command must not use shell composition")
    return argv


def is_release_build_argv(argv: list[str]) -> bool:
    return argv[:2] in (["cargo", "build"], ["cross", "build"]) or argv[:3] == [
        "cargo", "auditable", "build"
    ]


def build_steps(workflow: dict[str, Any]) -> list[dict[str, Any]]:
    """Inspect only jobs.build, the job that produces release artifacts."""
    try:
        steps = workflow["jobs"]["build"]["steps"]
    except (KeyError, TypeError) as error:
        raise ScopeCheckError(f"cannot read jobs.build steps: {error!r}") from error
    if not isinstance(steps, list):
        raise ScopeCheckError("jobs.build.steps is not a list")
    return [step for step in steps if isinstance(step, dict)]


def validate_matrix(workflow: dict[str, Any]) -> None:
    """Require the two known targets and their cross/native linkage exactly."""
    try:
        include = workflow["jobs"]["build"]["strategy"]["matrix"]["include"]
    except (KeyError, TypeError) as error:
        raise ScopeCheckError(f"cannot read jobs.build matrix: {error!r}") from error
    actual = {
        (entry.get("target"), entry.get("cross"))
        for entry in include
        if isinstance(entry, dict)
    } if isinstance(include, list) else set()
    expected = {(build.matrix_target, build.matrix_cross) for build in EXPECTED_BUILDS}
    if actual != expected:
        raise ScopeCheckError(
            f"jobs.build matrix must be exactly {sorted(expected)!r}, got {sorted(actual)!r}"
        )


def validated_release_builds(workflow: dict[str, Any]) -> list[ValidatedBuild]:
    """Validate every release build invocation and derive its replay inputs."""
    validate_matrix(workflow)
    expected_by_argv = {build.argv: build for build in EXPECTED_BUILDS}
    seen: dict[str, ValidatedBuild] = {}

    for step in build_steps(workflow):
        run = step.get("run")
        if not isinstance(run, str) or not BUILD_TEXT.search(run):
            continue
        argv = direct_shell_argv(run)
        if not is_release_build_argv(argv):
            raise ScopeCheckError(
                f"jobs.build contains shell text resembling a release build but no direct build argv: {run!r}"
            )
        expected = expected_by_argv.get(tuple(argv))
        if expected is None:
            raise ScopeCheckError(f"unrecognized jobs.build release command: {argv!r}")
        if step.get("name") != expected.name:
            raise ScopeCheckError(f"{expected.kind} release build has unexpected step name")
        if step.get("if") != expected.condition:
            raise ScopeCheckError(f"{expected.kind} release build has incorrect matrix condition")
        env = step.get("env")
        if not isinstance(env, dict) or env.get("BUILD_TARGET") != "${{ matrix.target }}":
            raise ScopeCheckError(f"{expected.kind} release build lacks BUILD_TARGET matrix linkage")
        if expected.kind in seen:
            raise ScopeCheckError(f"duplicate {expected.kind} release build command")
        seen[expected.kind] = ValidatedBuild(
            expected=expected,
            package=argv[argv.index("-p") + 1],
            binary=argv[argv.index("--bin") + 1],
            features=argv[argv.index("--features") + 1],
        )

    missing = sorted({build.kind for build in EXPECTED_BUILDS} - set(seen))
    if missing:
        raise ScopeCheckError(f"missing required release build step(s): {', '.join(missing)}")
    return [seen[build.kind] for build in EXPECTED_BUILDS]


def require_workspace_root() -> Path:
    """Do not accept a dependency graph generated from a different checkout."""
    if Path.cwd().resolve() != REPO_ROOT:
        raise ScopeCheckError(f"must run from workspace root {REPO_ROOT}")
    if not (REPO_ROOT / "Cargo.toml").is_file():
        raise ScopeCheckError(f"workspace root {REPO_ROOT} has no Cargo.toml")
    return REPO_ROOT


def resolved_member_features(tree_stdout: str, workspace_root: Path) -> dict[str, set[str]]:
    """Completely parse every workspace-local package line in cargo tree output."""
    members: dict[str, set[str]] = {}
    root = workspace_root.resolve()
    for raw in tree_stdout.splitlines():
        line = raw.strip()
        if " (/" not in line:
            continue
        match = LOCAL_PACKAGE_LINE.match(line)
        if match is None:
            raise ScopeCheckError(f"unparsed local package line: {line}")
        source = Path(match.group("source")).resolve()
        if source != root and root not in source.parents:
            raise ScopeCheckError(f"local package source is outside workspace root: {source}")
        features = match.group("features")
        members.setdefault(match.group("name"), set()).update(
            features.split(",") if features else ()
        )
    if not members:
        raise ScopeCheckError("no workspace-local package lines parsed from cargo tree output")
    return members


def forbidden_resolutions(members: dict[str, set[str]], target: str) -> list[str]:
    """Return forbidden test-only features and harness-member appearances."""
    violations = [
        f"[{target}] {name} resolves test-only feature(s) {','.join(leaked)} into the shipped graph"
        for name, features in sorted(members.items())
        if (leaked := sorted(features & FORBIDDEN_FEATURES))
    ]
    if TEST_HARNESS_MEMBER in members:
        violations.append(f"[{target}] {TEST_HARNESS_MEMBER} is in the shipped dependency graph")
    return violations


def scoped_tree(build: ValidatedBuild) -> subprocess.CompletedProcess[str]:
    """Replay args derived from the exact workflow argv for its matrix target."""
    return subprocess.run(
        [
            "cargo", "tree", "--locked", "-p", build.package, "--features",
            build.features, "-e", "normal,build,features", "-f", "{p} {f}",
            "--prefix", "none", "--target", build.expected.matrix_target,
        ],
        capture_output=True,
        text=True,
        cwd=REPO_ROOT,
        check=False,
    )


def main() -> int:
    root = require_workspace_root()
    workflow = yaml.safe_load(RELEASE_WORKFLOW.read_text(encoding="utf-8"))
    if not isinstance(workflow, dict):
        raise ScopeCheckError(f"{RELEASE_WORKFLOW} is not a mapping")
    violations: list[str] = []
    for build in validated_release_builds(workflow):
        completed = scoped_tree(build)
        target = build.expected.matrix_target
        if completed.returncode:
            detail = completed.stderr.strip().splitlines()
            violations.append(
                f"[{target}] cargo tree failed: {detail[0] if detail else '(no stderr output)'}"
            )
            continue
        try:
            members = resolved_member_features(completed.stdout, root)
        except ScopeCheckError as error:
            violations.append(f"[{target}] {error}")
            continue
        violations.extend(forbidden_resolutions(members, target))
    if violations:
        for violation in violations:
            print(f"release-build-scope: {violation}", file=sys.stderr)
        return 1
    print(
        "release-build-scope: exact release argv and both target graphs certify "
        f"-p {SHIPPED_PACKAGE} --bin {SHIPPED_BIN} --features {RELEASE_FEATURES}"
    )
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except ScopeCheckError as error:
        print(f"release-build-scope: {error}", file=sys.stderr)
        sys.exit(1)
