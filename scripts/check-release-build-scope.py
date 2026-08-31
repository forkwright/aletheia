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
import shutil
import subprocess
import sys
import tempfile
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
DEV_DEPENDENCY_SECTION = re.compile(
    r"(?ms)^\[(?:dev-dependencies|target\..*\.dev-dependencies)\]\n.*?(?=^\[|\Z)"
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
    resolved_features: frozenset[str]


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

EXPECTED_WORKFLOW_ENV = {
    "CARGO_TERM_COLOR": "always",
    "GH_REPO": "${{ github.repository }}",
    "RELEASE_SHA": "${{ inputs.release_sha || github.sha }}",
    "RELEASE_TAG": "${{ inputs.tag_name || github.ref_name }}",
}
CHECKOUT_ACTION = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1"
EXPECTED_ACTIONS = frozenset(
    {
        CHECKOUT_ACTION,
        "dtolnay/rust-toolchain@631a55b12751854ce901bb631d5902ceb48146f7",
        "taiki-e/install-action@ba47c86ac325773530516bb756137ac718732518",
        "Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6",
        "anchore/sbom-action@e22c389904149dbc22b58101806040fa8d37a610",
        "actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8",
        "actions/attest-sbom@c604332985a26aa8cf1bdc465b92731239ec6b9e",
        "actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a",
    }
)
SAFE_BUILD_JOB_DIGEST = "8d7af53b17f2bfcfc2c540fa596c4b7b808584337dda63d861b595b782881ff5"
# Exact fingerprints of every non-release jobs.build step, in workflow order.
# They bind run text, uses references, `with`, `if`, env, shell, and working
# directory together. None marks the only two permitted artifact builds.
SAFE_STEP_DIGESTS = (
    "d1016ce5746c54a2f8916b6899a6b3d4d8c1aa552dca182364c386d3747018f1",
    "f15858d5aa5e3a0cdf7386474998073069eb5afb72d67368aa728ab1eafc37cf",
    "522453cb2d8914f58a3e2731c036e5964b973afe8663297937af774428158912",
    "7e5adfd0bc284c926908190c2564adef683afde5fc0fbbbbf4c731e5d8e63f01",
    "6d53fd65a3ee6d39a8b3fe0fc2401136bb58817015a0933ecceff56908312275",
    "51e58b431773670272c7c4786699b87a95129ef96fa0c6c2f2badeaaf729227c",
    "7737c0fa3076e51a8cb4e6ca1d79d4cb1cc7214aa457862a5fedac5590fedcee",
    "cc0884b02ed7e3212e5ed1f79e524a5e1ce280126d0e7b4af54c8e9dec9a9286",
    "69f497840a9cd30fc23a3c0ae7b09dcdaf4b17003033f8a82c17273f47fe5426",
    None,
    None,
    "5607e468ff4514fcf140d884db9fdcc42b58871295c011c86e125222debbb1a1",
    "c42d24aeef4b2d84c8f69ae4d8049a64b9758405e0839f08506222b680d93794",
    "0b56c93b63fcb891d122031530ef9751b9efdc42a21aafd5177141987fe9490a",
    "23fc0a3e1a4dcc48f200568a82a405b1cac43562e0dd333bb9275f9ac4dc30c2",
    "254a9b665907991c0bb9c10b063964b7eef36cc908816269e1382cb9f3137d5e",
    "6935723b4226f6064fd83401fdc0fdd182b412e1ca07a0f4e6f3095e67e70f74",
    "a09cdca7c33c4925993074ba05a72c5907de1750358f96d6806cfa34311ff970",
    "1a5c42cebbd9a0e9c0b3022a50ed4abd4276ab17f2b79bc2207456cb67a7d383",
    "1c414e9ac174b740376ad341da317f0b62aef77eafef005d2808712c3a7a13ca",
    "b18313c0e9b56d421dae73c41dffa1e8f486b230db71fa41952323f609a3b6ee",
    "7f58cc4a1cdea155f33a87f4b664c315803e8d1b5bf067155738ab9f192d0407",
    "89a42cf7cded6d105c03ad3967054b0dc74c22c3f1df051ac9ac0024f148ff38",
    "249762ab27a23311400abf0a2627b59d45e5756c53e9c888ee75210d70ef604a",
    "21c3e23ed5c875c4f7ddd410bd80fb67b4de96e4849a8bc919c5c434a633dc9a",
    "ff9841cfd5b23704c7f095606195dfefe45caf19b33b37c549a1a83879b51a94",
)


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


def step_digest(step: dict[str, Any]) -> str:
    canonical = json.dumps(step, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def validate_environment(env: Any, scope: str) -> None:
    """Reject environment variables that redirect a Cargo/Rust invocation."""
    if env is None:
        return
    if not isinstance(env, dict) or not all(isinstance(key, str) for key in env):
        raise ScopeCheckError(f"{scope} env is not a string-keyed mapping")
    forbidden = {
        "PATH", "RUSTC", "RUSTDOC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER",
        "RUSTFLAGS", "RUSTDOCFLAGS", "CARGO_HOME", "CARGO_CONFIG", "CARGO_TARGET_DIR",
        "CARGO_BUILD_TARGET", "CARGO_BUILD_RUSTC", "CARGO_BUILD_RUSTDOC",
        "CARGO_ENCODED_RUSTFLAGS",
    }
    redirected = sorted(
        key for key in env
        if key in forbidden or key.startswith("CARGO_ALIAS_") or key.startswith("CARGO_BUILD_")
    )
    if redirected:
        raise ScopeCheckError(f"{scope} env redirects execution: {', '.join(redirected)}")


def validate_execution_envelope(workflow: dict[str, Any], steps: list[dict[str, Any]]) -> None:
    """Bind defaults, inherited environment, actions, and checkout semantics."""
    job = workflow["jobs"]["build"]
    job_envelope = {key: value for key, value in job.items() if key != "steps"}
    if step_digest(job_envelope) != SAFE_BUILD_JOB_DIGEST:
        raise ScopeCheckError("jobs.build execution envelope differs from the trusted release job")
    for scope, mapping in (("workflow", workflow), ("jobs.build", workflow["jobs"]["build"])):
        defaults = mapping.get("defaults")
        if defaults is not None:
            raise ScopeCheckError(f"{scope} defaults are forbidden for release artifact execution")
    if workflow.get("env") != EXPECTED_WORKFLOW_ENV:
        raise ScopeCheckError("workflow env differs from the allowed release environment")
    validate_environment(workflow.get("env"), "workflow")
    if job.get("env") is not None:
        raise ScopeCheckError("jobs.build env is forbidden; command steps declare their own inputs")

    checkout_steps: list[dict[str, Any]] = []
    for step in steps:
        validate_environment(step.get("env"), f"jobs.build step {step.get('name')!r}")
        if "run" in step:
            if "shell" in step or "working-directory" in step:
                raise ScopeCheckError(
                    f"jobs.build command step {step.get('name')!r} changes shell or working directory"
                )
        uses = step.get("uses")
        if uses is not None:
            if not isinstance(uses, str) or uses.startswith("./") or uses not in EXPECTED_ACTIONS:
                raise ScopeCheckError(f"jobs.build uses untrusted action: {uses!r}")
            if uses == CHECKOUT_ACTION:
                checkout_steps.append(step)
    if len(checkout_steps) != 1:
        raise ScopeCheckError("jobs.build must contain exactly one trusted checkout step")
    checkout = checkout_steps[0]
    expected_with = {
        "persist-credentials": False,
        "ref": "${{ inputs.release_sha || github.sha }}",
        "submodules": False,
    }
    if checkout.get("with") != expected_with or "path" in checkout:
        raise ScopeCheckError("checkout must use the release ref at repository root with no submodules")


def validated_release_builds(workflow: dict[str, Any]) -> list[ValidatedBuild]:
    """Validate every jobs.build run step against its intentionally narrow grammar."""
    validate_matrix(workflow)
    steps = build_steps(workflow)
    validate_execution_envelope(workflow, steps)
    if len(steps) != len(SAFE_STEP_DIGESTS):
        raise ScopeCheckError("jobs.build step count differs from the trusted execution graph")
    expected_by_name = {build.name: build for build in EXPECTED_BUILDS}
    seen_builds: dict[str, ValidatedBuild] = {}

    for index, step in enumerate(steps):
        run = step.get("run")
        name = step.get("name")
        expected = expected_by_name.get(name)
        if expected is not None:
            if SAFE_STEP_DIGESTS[index] is not None or not isinstance(run, str):
                raise ScopeCheckError(f"{expected.kind} release build appears outside its trusted step slot")
            argv = direct_shell_argv(run)
            if tuple(argv) != expected.argv:
                raise ScopeCheckError(f"unrecognized {expected.kind} release command: {argv!r}")
            if step.get("if") != expected.condition:
                raise ScopeCheckError(f"{expected.kind} release build has incorrect matrix condition")
            env = step.get("env")
            if env != {"BUILD_TARGET": "${{ matrix.target }}"}:
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

        expected_digest = SAFE_STEP_DIGESTS[index]
        if expected_digest is None or step_digest(step) != expected_digest:
            raise ScopeCheckError(f"jobs.build step {index} differs from the trusted execution graph")

    missing_builds = sorted({build.kind for build in EXPECTED_BUILDS} - set(seen_builds))
    if missing_builds:
        raise ScopeCheckError(f"missing required release build step(s): {', '.join(missing_builds)}")
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


def without_dev_dependencies(manifest: str) -> str:
    """Remove dev-only resolution inputs from a copied probe manifest."""
    return DEV_DEPENDENCY_SECTION.sub("", manifest)


def with_probe_workspace(manifest: str, manifest_dir: Path, probe: Path) -> str:
    """Keep excluded path dependencies attached to the probe workspace settings."""
    relative_root = os.path.relpath(probe, manifest_dir).replace(os.sep, "/")
    rewritten, replacements = re.subn(
        r"(?m)^\[package\]$", f'[package]\nworkspace = "{relative_root}"', manifest, count=1
    )
    if replacements != 1:
        raise ScopeCheckError("copied local manifest has no package table")
    return rewritten


def metadata_for(build: ValidatedBuild) -> dict[str, Any]:
    """Resolve the shipped package in an isolated, lock-pinned workspace.

    `cargo metadata` at the virtual root resolves every member and therefore
    unifies integration-tests' features into unrelated local packages.  Cargo
    has no package-selection flag for metadata, so materialize a temporary
    workspace which retains the real manifests, lockfile, and workspace
    settings but declares only the shipped package as a member.  Its
    ``resolve.nodes[].features`` fields are the authoritative, machine-readable
    resolved features used to certify the tree replay below.
    """
    with tempfile.TemporaryDirectory(prefix="aletheia-release-scope-") as temp:
        probe = Path(temp).resolve()
        manifests = sorted(
            (REPO_ROOT / "crates").rglob("Cargo.toml"), key=lambda path: len(path.parts)
        )
        manifest_dirs = {manifest.parent for manifest in manifests}
        source_manifest = (REPO_ROOT / "Cargo.toml").read_text(encoding="utf-8")
        members = re.compile(r"(?ms)^members = \[\n.*?^\]")
        root_manifest, replacements = members.subn(
            f'members = [\n    "crates/{build.package}",\n]', source_manifest, count=1
        )
        if replacements != 1:
            raise ScopeCheckError("cannot isolate the workspace members in Cargo.toml")
        excluded = [
            source_dir.relative_to(REPO_ROOT).as_posix()
            for source_dir in manifest_dirs
            if source_dir != REPO_ROOT / "crates" / build.package
        ]
        exclude = re.compile(r"(?ms)^exclude = \[\n.*?^\]")
        root_manifest, replacements = exclude.subn(
            "exclude = [\n" + "".join(f'    "{path}",\n' for path in sorted(excluded)) + "]",
            root_manifest,
            count=1,
        )
        if replacements != 1:
            raise ScopeCheckError("cannot isolate the workspace exclusions in Cargo.toml")
        (probe / "Cargo.toml").write_text(root_manifest, encoding="utf-8")
        shutil.copyfile(REPO_ROOT / "Cargo.lock", probe / "Cargo.lock")

        # Keep manifests lexical to the temporary root so Cargo discovers the
        # isolated workspace; every other crate entry is a symlink, making the
        # probe cheap and read-only with respect to the checkout.
        for manifest in manifests:
            source_dir = manifest.parent
            destination_dir = probe / source_dir.relative_to(REPO_ROOT)
            destination_dir.mkdir(parents=True, exist_ok=True)
            (destination_dir / "Cargo.toml").write_text(
                with_probe_workspace(
                    without_dev_dependencies(manifest.read_text(encoding="utf-8")),
                    destination_dir,
                    probe,
                ),
                encoding="utf-8",
            )
            for entry in source_dir.iterdir():
                if entry.name == "Cargo.toml":
                    continue
                if entry.is_dir() and any(entry == child or entry in child.parents for child in manifest_dirs):
                    (destination_dir / entry.name).mkdir(exist_ok=True)
                    continue
                os.symlink(entry, destination_dir / entry.name, target_is_directory=entry.is_dir())
        completed = subprocess.run(
            [
                "cargo", "metadata", "--manifest-path", str(probe / "Cargo.toml"),
                "--offline", "--format-version", "1", "--filter-platform",
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
        raise ScopeCheckError(
            f"cargo metadata failed: {' | '.join(detail[:6]) if detail else '(no stderr output)'}"
        )
    try:
        metadata = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise ScopeCheckError(f"cargo metadata emitted invalid JSON: {error}") from error
    if not isinstance(metadata, dict):
        raise ScopeCheckError("cargo metadata root is not an object")
    packages = metadata.get("packages")
    if not isinstance(packages, list):
        raise ScopeCheckError("cargo metadata packages is not a list")
    for package in packages:
        if not isinstance(package, dict):
            raise ScopeCheckError("cargo metadata package is not an object")
        manifest_path = package.get("manifest_path")
        if not isinstance(manifest_path, str):
            raise ScopeCheckError("cargo metadata package has no manifest path")
        path = Path(manifest_path).resolve()
        if path != probe and probe not in path.parents:
            continue
        package["manifest_path"] = str(REPO_ROOT / path.relative_to(probe))
    workspace_members = metadata.get("workspace_members")
    if not isinstance(workspace_members, list) or not all(isinstance(member, str) for member in workspace_members):
        raise ScopeCheckError("cargo metadata workspace_members is not a string list")
    shipped = [
        package.get("id") for package in packages
        if package.get("name") == build.package
        and package.get("manifest_path") == str(REPO_ROOT / "crates" / build.package / "Cargo.toml")
    ]
    if len(shipped) != 1 or workspace_members != shipped:
        raise ScopeCheckError("probe metadata includes workspace members outside the shipped package")
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
        candidates = [
            str(package.get("manifest_path"))
            for package in by_id.values() if package.get("name") == SHIPPED_PACKAGE
        ]
        raise ScopeCheckError(
            f"cargo metadata cannot identify exactly one shipped package root: {candidates!r}"
        )
    nodes = resolve.get("nodes")
    if not isinstance(nodes, list):
        raise ScopeCheckError("cargo metadata resolve.nodes is not a list")
    node_features: dict[str, frozenset[str]] = {}
    dependencies: dict[str, list[str]] = {}
    for node in nodes:
        if not isinstance(node, dict) or not isinstance(node.get("id"), str):
            continue
        deps = node.get("deps")
        if not isinstance(deps, list):
            raise ScopeCheckError("cargo metadata resolve.deps is not a list")
        features = node.get("features")
        if not isinstance(features, list) or not all(isinstance(feature, str) for feature in features):
            raise ScopeCheckError("cargo metadata resolve.features is not a string list")
        node_features[node["id"]] = frozenset(features)
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
        if not isinstance(name, str) or source in expected:
            raise ScopeCheckError("cargo metadata local package row is malformed or duplicated")
        if package_id not in node_features:
            raise ScopeCheckError(f"cargo metadata resolve omits feature data for {name}")
        expected[source] = ExpectedLocalPackage(name, node_features[package_id])
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
    seen: dict[Path, set[str]] = {}
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
        members.setdefault(package.name, set()).update(features)
        seen.setdefault(source, set()).update(features)
    missing = sorted(str(source) for source in set(expected) - set(seen))
    if missing:
        raise ScopeCheckError(f"cargo tree is missing metadata-reachable local package(s): {', '.join(missing)}")
    for source, package in expected.items():
        if seen[source] != package.resolved_features:
            raise ScopeCheckError(
                f"cargo tree feature column disagrees with authoritative resolution for {package.name}"
            )
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
