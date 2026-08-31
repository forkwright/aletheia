#!/usr/bin/env python3
"""Certify that release artifacts resolve only the declared binary graph.

The release job once built at the virtual workspace root, which unified
integration-tests' test-only features into release artifacts. This checker
allows exactly the two known release commands. It uses cargo metadata's
machine-readable dependency graph to require complete local-package coverage
from the human-readable feature tree, whose feature column cargo exposes there.
"""

from __future__ import annotations

import hashlib
import json
import os
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
ANSI_ESCAPE = re.compile(r"\x1B\[[0-?]*[ -/]*[@-~]")
LOCAL_PACKAGE_LINE = re.compile(
    r"^(?P<name>[A-Za-z0-9_-]+) v[^ ]+ \((?P<source>/[^)]*)\)\|"
    r"(?P<features>[^ ]*)(?: \(\*\))?$"
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


@dataclass(frozen=True)
class ExpectedLocalPackage:
    name: str
    declared_features: frozenset[str]


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

# Every jobs.build shell step that is not one of EXPECTED_BUILDS is pinned by
# name and content digest. This is intentionally narrow: a new alias, wrapper,
# command substitution, eval/bash/sh invocation, or chained build cannot become
# a second artifact build without changing this checker in the same review.
SAFE_NON_BUILD_RUNS = {
    "Extract version from tag": "8b26fdecc04dbcd82a8be3a62e0c88e843358c4a6897f9322e8f18b89c570296",
    "Record resolved rustc version": "80013f1a0e96e50e3c1f29e0f4038529e9fa1824e65859c2bbcb078e5016c218",
    "Install cross": "ca568ebd721af86fd84c9450a890c363be2eac6acac662ec2771f66837bedfa7",
    "Install cargo-auditable": "f2ae126ec67ba24d3c7f182ead98e9889f0ee6e00663236483a63397b1b73ffd",
    "Verify embedded auditable dependency graph": "c16c6a12df4d85f939f143d715f72046187051e1f2e887e2f50f331c6159b494",
    "Package tarball": "d6010bf5fc2dca3836da6605978345f3c577e7e84bca18edb663cb65e4ff10d9",
    "Inspect tarball contents": "3276763c30a49d487493c8df9df1e543d63cc669650378145dd2cce4aeab2732",
    "Smoke test the release binary": "e84450be83cf137138f9411f7ab66c006d27019b04d4a86c02e7a08a760796a1",
    "Verify binary version matches tag and manifests": "bc11e2c3af8ad0a036cea4ddd7e462c5816b2fe34ceb006750bada4d89b94454",
    "Generate checksums (Linux)": "c3ae2c2bb85a6d98542ba41a9173edc18ae515d30b789b7f90d6b606a59fc7c8",
    "Generate checksums (macOS)": "2430522b6669b914b89e19af5272b2a7dc951bab6f4655734c4895bb756ef429",
    "Bind SBOM package inventories to the binary": "77af1b4953484b9e1e89db5c4794fa25b4aeda1eb85c9e8489c3347e17dc2af4",
    "Stage validated platform assets": "fe022effd8009b607c4f1507afe5c402b43779ef6f6d577432c827c61b4b0de7",
}


def direct_shell_argv(run: str) -> list[str]:
    """Parse the one direct shell command permitted for a release build."""
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


def build_steps(workflow: dict[str, Any]) -> list[dict[str, Any]]:
    """Inspect only jobs.build, the job that produces release artifacts."""
    try:
        steps = workflow["jobs"]["build"]["steps"]
    except (KeyError, TypeError) as error:
        raise ScopeCheckError(f"cannot read jobs.build steps: {error!r}") from error
    if not isinstance(steps, list) or not all(isinstance(step, dict) for step in steps):
        raise ScopeCheckError("jobs.build.steps must be a list of mappings")
    return steps


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


def run_digest(run: str) -> str:
    return hashlib.sha256(run.encode("utf-8")).hexdigest()


def validated_release_builds(workflow: dict[str, Any]) -> list[ValidatedBuild]:
    """Validate every jobs.build run step against its intentionally narrow grammar."""
    validate_matrix(workflow)
    expected_by_name = {build.name: build for build in EXPECTED_BUILDS}
    seen_builds: dict[str, ValidatedBuild] = {}
    seen_safe: set[str] = set()

    for step in build_steps(workflow):
        run = step.get("run")
        if run is None:
            continue
        if not isinstance(run, str):
            raise ScopeCheckError(f"jobs.build run step {step.get('name')!r} is not a string")
        name = step.get("name")
        expected = expected_by_name.get(name)
        if expected is not None:
            argv = direct_shell_argv(run)
            if tuple(argv) != expected.argv:
                raise ScopeCheckError(f"unrecognized {expected.kind} release command: {argv!r}")
            if step.get("if") != expected.condition:
                raise ScopeCheckError(f"{expected.kind} release build has incorrect matrix condition")
            env = step.get("env")
            if not isinstance(env, dict) or env.get("BUILD_TARGET") != "${{ matrix.target }}":
                raise ScopeCheckError(f"{expected.kind} release build lacks BUILD_TARGET matrix linkage")
            if expected.kind in seen_builds:
                raise ScopeCheckError(f"duplicate {expected.kind} release build command")
            seen_builds[expected.kind] = ValidatedBuild(
                expected=expected,
                package=argv[argv.index("-p") + 1],
                binary=argv[argv.index("--bin") + 1],
                features=argv[argv.index("--features") + 1],
            )
            continue

        expected_digest = SAFE_NON_BUILD_RUNS.get(name)
        if expected_digest is None or name in seen_safe:
            raise ScopeCheckError(f"unrecognized jobs.build run step: {name!r}")
        if run_digest(run) != expected_digest:
            raise ScopeCheckError(f"jobs.build non-release step changed or could wrap a build: {name!r}")
        seen_safe.add(name)

    missing_builds = sorted({build.kind for build in EXPECTED_BUILDS} - set(seen_builds))
    if missing_builds:
        raise ScopeCheckError(f"missing required release build step(s): {', '.join(missing_builds)}")
    missing_safe = sorted(set(SAFE_NON_BUILD_RUNS) - seen_safe)
    if missing_safe:
        raise ScopeCheckError(f"missing expected jobs.build run step(s): {', '.join(missing_safe)}")
    return [seen_builds[build.kind] for build in EXPECTED_BUILDS]


def require_workspace_root() -> Path:
    """Do not accept a graph generated from a different checkout."""
    if Path.cwd().resolve() != REPO_ROOT:
        raise ScopeCheckError(f"must run from workspace root {REPO_ROOT}")
    if not (REPO_ROOT / "Cargo.toml").is_file():
        raise ScopeCheckError(f"workspace root {REPO_ROOT} has no Cargo.toml")
    return REPO_ROOT


def cargo_environment() -> dict[str, str]:
    """Make machine parsing deterministic even when CI globally requests color."""
    return {**os.environ, "CARGO_TERM_COLOR": "never"}


def metadata_for(build: ValidatedBuild) -> dict[str, Any]:
    """Resolve the target graph in Cargo's machine-readable metadata format."""
    completed = subprocess.run(
        [
            "cargo", "metadata", "--manifest-path", "crates/aletheia/Cargo.toml",
            "--locked", "--format-version", "1", "--filter-platform",
            build.expected.matrix_target, "--features", build.features,
        ],
        capture_output=True,
        text=True,
        cwd=REPO_ROOT,
        env=cargo_environment(),
        check=False,
    )
    if completed.returncode:
        detail = completed.stderr.strip().splitlines()
        raise ScopeCheckError(f"cargo metadata failed: {detail[0] if detail else '(no stderr output)'}")
    try:
        metadata = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise ScopeCheckError(f"cargo metadata emitted invalid JSON: {error}") from error
    if not isinstance(metadata, dict):
        raise ScopeCheckError("cargo metadata root is not an object")
    return metadata


def expected_local_packages(metadata: dict[str, Any], workspace_root: Path) -> dict[Path, ExpectedLocalPackage]:
    """Traverse metadata from aletheia to establish complete local coverage."""
    packages = metadata.get("packages")
    resolve = metadata.get("resolve")
    if not isinstance(packages, list) or not isinstance(resolve, dict):
        raise ScopeCheckError("cargo metadata lacks packages or resolve")
    by_id = {package.get("id"): package for package in packages if isinstance(package, dict)}
    root_manifest = (workspace_root / "crates" / SHIPPED_PACKAGE / "Cargo.toml").resolve()
    roots = [
        package_id for package_id, package in by_id.items()
        if isinstance(package_id, str)
        and package.get("name") == SHIPPED_PACKAGE
        and Path(str(package.get("manifest_path", ""))).resolve() == root_manifest
    ]
    if len(roots) != 1:
        raise ScopeCheckError("cargo metadata cannot identify exactly one shipped package root")
    nodes = resolve.get("nodes")
    if not isinstance(nodes, list):
        raise ScopeCheckError("cargo metadata resolve.nodes is not a list")
    dependencies: dict[str, list[str]] = {}
    for node in nodes:
        if not isinstance(node, dict) or not isinstance(node.get("id"), str):
            continue
        deps = node.get("deps")
        if not isinstance(deps, list):
            raise ScopeCheckError("cargo metadata resolve.deps is not a list")
        dependencies[node["id"]] = [
            dependency["pkg"]
            for dependency in deps
            if isinstance(dependency, dict)
            and isinstance(dependency.get("pkg"), str)
            and isinstance(dependency.get("dep_kinds"), list)
            and any(
                isinstance(kind, dict) and kind.get("kind") in (None, "build")
                for kind in dependency["dep_kinds"]
            )
        ]
    if roots[0] not in dependencies:
        raise ScopeCheckError("cargo metadata resolve omits the shipped package root")
    reachable: set[str] = set()
    pending = [roots[0]]
    while pending:
        package_id = pending.pop()
        if package_id in reachable:
            continue
        if package_id not in by_id or not isinstance(dependencies.get(package_id), list):
            raise ScopeCheckError("cargo metadata dependency graph is incomplete")
        reachable.add(package_id)
        pending.extend(dependency for dependency in dependencies[package_id] if isinstance(dependency, str))

    expected: dict[Path, ExpectedLocalPackage] = {}
    root = workspace_root.resolve()
    for package_id in reachable:
        package = by_id[package_id]
        manifest = Path(str(package.get("manifest_path", ""))).resolve()
        source = manifest.parent
        if source != root and root not in source.parents:
            continue
        name = package.get("name")
        features = package.get("features")
        if not isinstance(name, str) or not isinstance(features, dict) or source in expected:
            raise ScopeCheckError("cargo metadata local package row is malformed or duplicated")
        if not all(isinstance(feature, str) for feature in features):
            raise ScopeCheckError(f"cargo metadata features are malformed for {name}")
        expected[source] = ExpectedLocalPackage(name, frozenset(features))
    if not expected:
        raise ScopeCheckError("cargo metadata found no reachable local packages")
    return expected


def scoped_tree(build: ValidatedBuild) -> subprocess.CompletedProcess[str]:
    """Replay features from the exact workflow argv for one validated target."""
    return subprocess.run(
        [
            "cargo", "tree", "--locked", "-p", build.package, "--features",
            build.features, "-e", "normal,build,features", "-f", "{p}|{f}",
            "--prefix", "none", "--target", build.expected.matrix_target,
        ],
        capture_output=True,
        text=True,
        cwd=REPO_ROOT,
        env=cargo_environment(),
        check=False,
    )


def resolved_member_features(
    tree_stdout: str, expected: dict[Path, ExpectedLocalPackage]
) -> dict[str, set[str]]:
    """Parse every local tree row and require exact metadata-backed coverage."""
    members: dict[str, set[str]] = {}
    seen: set[Path] = set()
    for raw in ANSI_ESCAPE.sub("", tree_stdout).splitlines():
        line = raw.strip()
        if " (/" not in line:
            continue
        match = LOCAL_PACKAGE_LINE.match(line)
        if match is None:
            raise ScopeCheckError(f"unparsed local package line: {line}")
        source = Path(match.group("source")).resolve()
        package = expected.get(source)
        if package is None:
            raise ScopeCheckError(f"unexpected local package source in cargo tree: {source}")
        if match.group("name") != package.name:
            raise ScopeCheckError(f"cargo tree package name disagrees with metadata at {source}")
        features = set(filter(None, match.group("features").split(",")))
        unknown = features - package.declared_features
        if unknown:
            raise ScopeCheckError(
                f"cargo tree has undeclared feature(s) {','.join(sorted(unknown))} for {package.name}"
            )
        members.setdefault(package.name, set()).update(features)
        seen.add(source)
    missing = sorted(str(source) for source in set(expected) - seen)
    if missing:
        raise ScopeCheckError(f"cargo tree is missing metadata-reachable local package(s): {', '.join(missing)}")
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


def main() -> int:
    root = require_workspace_root()
    workflow = yaml.safe_load(RELEASE_WORKFLOW.read_text(encoding="utf-8"))
    if not isinstance(workflow, dict):
        raise ScopeCheckError(f"{RELEASE_WORKFLOW} is not a mapping")
    violations: list[str] = []
    for build in validated_release_builds(workflow):
        target = build.expected.matrix_target
        try:
            expected = expected_local_packages(metadata_for(build), root)
        except ScopeCheckError as error:
            violations.append(f"[{target}] {error}")
            continue
        completed = scoped_tree(build)
        if completed.returncode:
            detail = completed.stderr.strip().splitlines()
            violations.append(
                f"[{target}] cargo tree failed: {detail[0] if detail else '(no stderr output)'}"
            )
            continue
        try:
            members = resolved_member_features(completed.stdout, expected)
        except ScopeCheckError as error:
            violations.append(f"[{target}] {error}")
            continue
        violations.extend(forbidden_resolutions(members, target))
    if violations:
        for violation in violations:
            print(f"release-build-scope: {violation}", file=sys.stderr)
        return 1
    print(
        "release-build-scope: exact release argv and metadata-complete target graphs certify "
        f"-p {SHIPPED_PACKAGE} --bin {SHIPPED_BIN} --features {RELEASE_FEATURES}"
    )
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except ScopeCheckError as error:
        print(f"release-build-scope: {error}", file=sys.stderr)
        sys.exit(1)
