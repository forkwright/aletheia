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
import stat
import subprocess
import sys
import tempfile
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import yaml

REPO_ROOT = Path(__file__).resolve().parents[1]
RELEASE_WORKFLOW = Path(".github/workflows/release.yml")
MANIFEST_NAME = "Cargo.toml"
LOCKFILE_NAME = "Cargo.lock"
CROSS_CONFIG = Path("Cross.toml")
CROSS_INSTALLER = Path("scripts/install-cargo-auditable-cross.sh")
CARGO_DIRECTORY = ".cargo"
CARGO_CONFIG_NAMES = ("config.toml", "config")
ALLOWED_CARGO_TREE_ENTRY = ("100644", ".cargo/audit.toml")
RUST_TOOLCHAIN = Path("rust-toolchain.toml")
RUST_TOOLCHAIN_CHANNEL = "1.97.1"
RUST_TOOLCHAIN_COMPONENTS = ("rustfmt", "clippy")
RUST_TOOLCHAIN_PROFILE = "default"
RUST_TOOLCHAIN_TARGETS = ("aarch64-apple-darwin", "x86_64-unknown-linux-musl")
RUST_TOOLCHAIN_DIGEST = "0cdc1f132157e51ec479c2d6103efb37f115495b16c64dfc2baf7d895d45db7c"
ALLOWED_TOOLCHAIN_TREE_ENTRY = ("100644", RUST_TOOLCHAIN.as_posix())
RUSTUP_SELECTOR_NAMES = frozenset(
    {"rust-toolchain", "rust-toolchain.toml", "rustup", "rustup.toml", ".rustup"}
)
CROSS_PRE_BUILD = 'pre-build = "./scripts/install-cargo-auditable-cross.sh"'
CROSS_TOOLCHAIN_CARGO = 'exec /rust/bin/cargo auditable "$@"'
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
MEMBERS_SECTION = re.compile(r"(?ms)^members = \[\n.*?^\]")
EXCLUDE_SECTION = re.compile(r"(?ms)^exclude = \[\n.*?^\]")


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
EXPECTED_BUILD_ENV = {"RUSTUP_TOOLCHAIN": RUST_TOOLCHAIN_CHANNEL}
CHECKOUT_ACTION = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1"
RUST_TOOLCHAIN_ACTION = "dtolnay/rust-toolchain@631a55b12751854ce901bb631d5902ceb48146f7"
DOWNLOAD_ARTIFACT_ACTION = "actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c"
TRUSTED_PUBLICATION_DOWNLOADS = (
    ("release-aletheia-linux-x86_64", "release-assets"),
    ("release-aletheia-macos-aarch64", "release-assets"),
    ("release-workspace-sboms", "release-assets"),
)
EXPECTED_ACTIONS = frozenset(
    {
        CHECKOUT_ACTION,
        RUST_TOOLCHAIN_ACTION,
        "taiki-e/install-action@ba47c86ac325773530516bb756137ac718732518",
        "Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6",
        "anchore/sbom-action@e22c389904149dbc22b58101806040fa8d37a610",
        "actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8",
        "actions/attest-sbom@c604332985a26aa8cf1bdc465b92731239ec6b9e",
        "actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a",
    }
)
SAFE_BUILD_JOB_DIGEST = "e2912932b31c5fc612194005c91e85dffa6741462110c16b5dbf57c9b8f84139"
TRUSTED_SIBLING_JOB_DIGESTS = {
    "release-identity": "0ecff762dd676cafa8ff2b6c4c4ae4bd673b8fbcd19c012f8528bf035336f5f5",
    "canonical-gate": "78f789ffcde12193716211dd9fd6211be8d5a7c39351206d465a3e7bfe79ad35",
    "canonical-security": "ffd7b358618de7306658f5e7a942da8e5736a32f8c8597f4c7572111c5e21070",
    "prepare-release": "33887262e769a288c88c6dfae3b69a73c6bb0eefcc6d487281d21d56adff9fc5",
    "test": "9b646ba550445385930fe1d89fa274a3b9b22091c072645edd9b7e82906d74ab",
    "feature-policy": "badb05261d8d49dec38f87d4f7c6b47999b977a236f7cc355053f15204ae25dc",
    "feature-check": "ac4acd4e28b26aafe83ba446d19e7e2a61b3478e905277cfbdb6472eb3e24b2d",
    "no-default-recipes": "0226eaf504e06b429bd759b469d965690fca80e228ad164ba48b16b3bfadef27",
    "sbom": "98e5e93cb024969a21fb84259ef050c53bb4c2c8c0e0afdc1d1a02e07ac60166",
    "publish-release": "8b83b7d5a7e03d0d6c4eca6f83a139455177aad868eaf156f2a1db6e19fb710f",
}
OUTCOME_OBSERVER_JOB_ID = "release-outcome"
TRUSTED_RELEASE_JOB_IDS = frozenset(
    (*TRUSTED_SIBLING_JOB_DIGESTS, "build", OUTCOME_OBSERVER_JOB_ID)
)
ARTIFACT_FLOW_JOB_IDS = frozenset({"build", "sbom", "publish-release"})
TRUSTED_BUILD_STEP_DIGESTS = {
    "Build (native)": "d93be08a02ad6d7580059f7cd104cb9568dfdd1e876d7c0980c735645265986d",
    "Build (cross)": "b082568fcb69bca5c417cea6bec6862b84333e1a06b948251e10dec2c8173e55",
}
TRUSTED_CROSS_INPUTS = {
    CROSS_CONFIG: "c59f137bd29a0c72e07313f4ac636e00a2a5c8e5b5a61a2204465b5977724b17",
    CROSS_INSTALLER: "3b4dee409d2372bc4f8993624f1cb99659adf6f03500090a9697f22d50e254fc",
}
# Exact fingerprints of every non-release jobs.build step, in workflow order.
# They bind run text, uses references, `with`, `if`, env, shell, and working
# directory together. None marks the only two permitted artifact builds.
SAFE_STEP_DIGESTS = (
    "d1016ce5746c54a2f8916b6899a6b3d4d8c1aa552dca182364c386d3747018f1",
    "f15858d5aa5e3a0cdf7386474998073069eb5afb72d67368aa728ab1eafc37cf",
    "522453cb2d8914f58a3e2731c036e5964b973afe8663297937af774428158912",
    "c674226898a8f57726378bafbf9668c5419da78533994126472fcbca2297928c",
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


def validate_outcome_observer(jobs: dict[str, Any]) -> None:
    """Permit one terminal read-only observer without widening artifact flow."""
    outcome = jobs[OUTCOME_OBSERVER_JOB_ID]
    if not isinstance(outcome, dict):
        raise ScopeCheckError("release outcome observer is not a mapping")
    required_needs = frozenset((*TRUSTED_SIBLING_JOB_DIGESTS, "build"))
    outcome_needs = outcome.get("needs")
    if not isinstance(outcome_needs, list) or set(outcome_needs) != required_needs:
        raise ScopeCheckError("release outcome observer does not wait for the complete graph")
    if outcome.get("if") != "${{ always() }}":
        raise ScopeCheckError("release outcome observer is not terminal")
    if outcome.get("permissions") != {"actions": "read", "contents": "read"}:
        raise ScopeCheckError("release outcome observer permissions are not read-only")
    steps = outcome.get("steps")
    if not isinstance(steps, list) or len(steps) != 2:
        raise ScopeCheckError("release outcome observer steps differ from the trusted shape")
    rendered = json.dumps(outcome, sort_keys=True)
    if "scripts/check-release-outcome.py --attempts 6 --retry-seconds 10" not in rendered:
        raise ScopeCheckError("release outcome observer does not run the outcome checker")
    prohibited = ("cargo build", "cargo auditable", "cross build", "gh release upload")
    if any(token in rendered for token in prohibited):
        raise ScopeCheckError("release outcome observer may not build or mutate release artifacts")


def validate_release_job_graph(workflow: dict[str, Any]) -> None:
    """Close artifact production and publishing to the trusted complete job graph."""
    jobs = workflow.get("jobs")
    if not isinstance(jobs, dict):
        raise ScopeCheckError("release workflow jobs is not a mapping")
    missing_artifact_jobs = sorted(ARTIFACT_FLOW_JOB_IDS - set(jobs))
    if missing_artifact_jobs:
        raise ScopeCheckError(f"release workflow is missing artifact-flow jobs: {missing_artifact_jobs}")
    if set(jobs) != TRUSTED_RELEASE_JOB_IDS:
        raise ScopeCheckError(f"release workflow job IDs differ from the trusted set: {sorted(jobs)!r}")
    validate_outcome_observer(jobs)
    for name, digest in TRUSTED_SIBLING_JOB_DIGESTS.items():
        job = jobs[name]
        steps = job.get("steps", []) if isinstance(job, dict) else None
        if not isinstance(steps, list) or not all(isinstance(step, dict) for step in steps):
            raise ScopeCheckError(f"release workflow sibling job {name!r} has invalid steps")
        if step_digest(job) != digest:
            raise ScopeCheckError(f"release workflow sibling job {name!r} differs from its trusted graph")


def validate_publication_intake(workflow: dict[str, Any]) -> None:
    """Require publish-release to intake only the three vetted staged artifacts."""
    publish = workflow["jobs"]["publish-release"]
    steps = publish.get("steps") if isinstance(publish, dict) else None
    if not isinstance(steps, list):
        raise ScopeCheckError("publish-release steps are not a list")
    downloads = [step for step in steps if step.get("uses") == DOWNLOAD_ARTIFACT_ACTION]
    actual: list[tuple[str, str]] = []
    for step in downloads:
        inputs = step.get("with")
        if not isinstance(inputs, dict) or set(inputs) != {"name", "path"}:
            raise ScopeCheckError("publish-release artifact intake must use only explicit name and path")
        name, path = inputs["name"], inputs["path"]
        if not isinstance(name, str) or not isinstance(path, str):
            raise ScopeCheckError("publish-release artifact intake names and paths must be strings")
        actual.append((name, path))
    if tuple(actual) != TRUSTED_PUBLICATION_DOWNLOADS:
        raise ScopeCheckError(f"publish-release artifact intake differs from the trusted set: {actual!r}")


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


def file_digest(path: Path) -> str:
    """Return the exact bytes digest for a local build-time input."""
    return hashlib.sha256(path.read_bytes()).hexdigest()


def validate_cross_inputs(root: Path) -> None:
    """Pin Cross's image-side Cargo wrapper and its mounted host toolchain use."""
    for relative, expected in TRUSTED_CROSS_INPUTS.items():
        candidate = root / relative
        if not candidate.is_file() or file_digest(candidate) != expected:
            raise ScopeCheckError(f"cross build input differs from its trusted content: {relative}")
    if CROSS_PRE_BUILD not in (root / CROSS_CONFIG).read_text(encoding="utf-8"):
        raise ScopeCheckError("Cross no longer runs the trusted image setup")
    if CROSS_TOOLCHAIN_CARGO not in (root / CROSS_INSTALLER).read_text(encoding="utf-8"):
        raise ScopeCheckError("Cross no longer invokes Cargo from its mounted toolchain")


def git_output(root: Path, arguments: list[str]) -> bytes:
    """Run Git plumbing without a shell and return its byte-preserving output."""
    completed = subprocess.run(
        ["git", "-C", str(root), *arguments], capture_output=True, check=False
    )
    if completed.returncode:
        detail = completed.stderr.decode("utf-8", "replace").strip()
        raise ScopeCheckError(f"git {' '.join(arguments)} failed: {detail or completed.returncode}")
    return completed.stdout


def head_tree_entries(root: Path) -> list[tuple[str, str]]:
    """Parse the exact HEAD tree in a NUL-safe, mode-preserving representation."""
    entries: list[tuple[str, str]] = []
    for record in git_output(root, ["ls-tree", "-rz", "--full-tree", "HEAD"]).split(b"\0"):
        if not record:
            continue
        metadata, separator, raw_path = record.partition(b"\t")
        fields = metadata.split()
        if not separator or len(fields) != 3:
            raise ScopeCheckError("git ls-tree emitted a malformed record")
        entries.append((fields[0].decode("ascii"), raw_path.decode("utf-8", "surrogateescape")))
    return entries


def is_cargo_tree_path(path: str) -> bool:
    """Whether a path is a Cargo directory on the case-insensitive macOS runner."""
    return any(component.casefold() == CARGO_DIRECTORY for component in Path(path).parts)


def is_cargo_config_name(name: str) -> bool:
    """Recognize both legacy Cargo config spellings on macOS's release filesystem."""
    return name.casefold() in CARGO_CONFIG_NAMES


def is_toolchain_selector_name(name: str) -> bool:
    """Recognize rustup selectors/configuration on either release runner filesystem."""
    return name.casefold() in RUSTUP_SELECTOR_NAMES


def is_toolchain_tree_path(path: str) -> bool:
    """Whether a tracked path can select or configure a repository Rust toolchain."""
    return any(is_toolchain_selector_name(component) for component in Path(path).parts)


def validate_head_cargo_tree(root: Path) -> None:
    """Allow only the audited root Cargo directory in the immutable HEAD tree."""
    cargo_entries = [entry for entry in head_tree_entries(root) if is_cargo_tree_path(entry[1])]
    if cargo_entries != [ALLOWED_CARGO_TREE_ENTRY]:
        raise ScopeCheckError(f"HEAD tree has untrusted Cargo paths or modes: {cargo_entries!r}")


def validate_head_toolchain_tree(root: Path) -> None:
    """Permit one exact canonical rustup selector in the immutable HEAD tree."""
    selectors = [entry for entry in head_tree_entries(root) if is_toolchain_tree_path(entry[1])]
    if selectors != [ALLOWED_TOOLCHAIN_TREE_ENTRY]:
        raise ScopeCheckError(f"HEAD tree has untrusted Rust toolchain selectors: {selectors!r}")


def require_clean_checkout(root: Path) -> None:
    """Require the index and tracked worktree to equal the checked-out HEAD tree."""
    git_output(root, ["diff", "--quiet", "--exit-code", "HEAD", "--"])
    status = git_output(root, ["status", "--porcelain=v1", "-z", "--untracked-files=all"])
    if status:
        raise ScopeCheckError("release scope requires a clean worktree and index")


def workspace_entries(root: Path) -> list[tuple[Path, str, Path, int]]:
    """List candidate workspace entries without following links or entering .git."""
    entries: list[tuple[Path, str, Path, int]] = []
    pending = [root]
    while pending:
        directory = pending.pop()
        try:
            with os.scandir(directory) as directory_entries:
                children = [(Path(entry.path), entry.name) for entry in directory_entries]
        except OSError as error:
            raise ScopeCheckError(f"cannot scan release workspace path {directory}: {error}") from error
        for path, name in children:
            if directory == root and name == ".git":
                continue
            try:
                mode = os.lstat(path).st_mode
            except OSError as error:
                raise ScopeCheckError(f"cannot stat release workspace path {path}: {error}") from error
            entries.append((path, name, directory, mode))
            if stat.S_ISDIR(mode):
                pending.append(path)
    return entries


def filesystem_cargo_config_paths(root: Path) -> list[Path]:
    """Supplement Git's immutable tree check with untracked/ignored config detection."""
    configurations: list[Path] = []
    for path, name, directory, mode in workspace_entries(root):
        is_link = stat.S_ISLNK(mode)
        in_cargo_directory = directory.name.casefold() == CARGO_DIRECTORY
        is_cargo_directory = name.casefold() == CARGO_DIRECTORY
        if in_cargo_directory and is_cargo_config_name(name):
            configurations.append(path)
        if is_cargo_directory and is_link:
            configurations.append(path)
    return configurations


def filesystem_toolchain_selector_paths(root: Path) -> list[Path]:
    """Find ignored or untracked rustup selectors without following symlinks."""
    selectors: list[Path] = []
    for path, name, _directory, _mode in workspace_entries(root):
        if is_toolchain_selector_name(name) and path != root / RUST_TOOLCHAIN:
            selectors.append(path)
    return selectors


def validate_cargo_configuration_boundary(root: Path) -> None:
    """Bind Cargo configuration to HEAD before any candidate code can run.

    The trusted authority is the immutable Git tree plus a clean index/worktree;
    the filesystem scan only catches untracked or ignored configuration surfaces.
    This deliberately does not claim protection from arbitrary post-check mutation.
    """
    validate_head_cargo_tree(root)
    require_clean_checkout(root)
    configurations = filesystem_cargo_config_paths(root)
    if configurations:
        paths = ", ".join(str(path.relative_to(root)) for path in configurations)
        raise ScopeCheckError(f"untracked Cargo configuration is forbidden: {paths}")


def validate_toolchain_definition(root: Path) -> None:
    """Verify the complete canonical rustup selector before Cargo can execute."""
    candidate = root / RUST_TOOLCHAIN
    if not candidate.is_file() or file_digest(candidate) != RUST_TOOLCHAIN_DIGEST:
        raise ScopeCheckError("rust-toolchain.toml differs from the trusted release pin")
    try:
        parsed = tomllib.loads(candidate.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise ScopeCheckError(f"cannot parse trusted rust-toolchain.toml: {error}") from error
    expected = {
        "toolchain": {
            "channel": RUST_TOOLCHAIN_CHANNEL,
            "components": list(RUST_TOOLCHAIN_COMPONENTS),
            "profile": RUST_TOOLCHAIN_PROFILE,
            "targets": list(RUST_TOOLCHAIN_TARGETS),
        }
    }
    if parsed != expected:
        raise ScopeCheckError("rust-toolchain.toml has an unexpected release toolchain definition")


def validate_toolchain_boundary(root: Path) -> None:
    """Bind rustup selection to the clean immutable checkout, not candidate wrappers."""
    validate_head_toolchain_tree(root)
    require_clean_checkout(root)
    validate_toolchain_definition(root)
    selectors = filesystem_toolchain_selector_paths(root)
    if selectors:
        paths = ", ".join(str(path.relative_to(root)) for path in selectors)
        raise ScopeCheckError(f"untracked Rust toolchain selectors are forbidden: {paths}")


def validate_release_repository(root: Path) -> None:
    """Validate candidate-controlled files before the release commands run."""
    validate_toolchain_boundary(root)
    validate_cargo_configuration_boundary(root)
    validate_cross_inputs(root)


def validate_environment(env: Any, scope: str, allowed: frozenset[str] = frozenset()) -> None:
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
        if key not in allowed and (
            key in forbidden or key.startswith("CARGO_ALIAS_") or key.startswith("CARGO_BUILD_")
            or key.startswith("RUSTUP_")
        )
    )
    if redirected:
        raise ScopeCheckError(f"{scope} env redirects execution: {', '.join(redirected)}")


def validate_job_context(workflow: dict[str, Any]) -> None:
    """Reject job-level shell, environment, runner, or container redirection."""
    job = workflow["jobs"]["build"]
    envelope = {key: value for key, value in job.items() if key != "steps"}
    if step_digest(envelope) != SAFE_BUILD_JOB_DIGEST:
        raise ScopeCheckError("jobs.build execution envelope differs from the trusted release job")
    if job.get("defaults") is not None or job.get("env") != EXPECTED_BUILD_ENV:
        raise ScopeCheckError("jobs.build must use only the trusted toolchain environment")
    validate_environment(job.get("env"), "jobs.build", frozenset(EXPECTED_BUILD_ENV))


def validate_workflow_context(workflow: dict[str, Any]) -> None:
    """Accept only the release workflow's known inherited environment."""
    if workflow.get("defaults") is not None:
        raise ScopeCheckError("workflow defaults are forbidden for release artifact execution")
    if workflow.get("env") != EXPECTED_WORKFLOW_ENV:
        raise ScopeCheckError("workflow env differs from the allowed release environment")
    validate_environment(workflow.get("env"), "workflow")


def validate_step_context(step: dict[str, Any]) -> None:
    """Reject shell, directory, action, and environment indirection per step."""
    name = step.get("name")
    validate_environment(step.get("env"), f"jobs.build step {name!r}")
    if "run" in step and ("shell" in step or "working-directory" in step):
        raise ScopeCheckError(f"jobs.build command step {name!r} changes shell or working directory")
    uses = step.get("uses")
    if uses is not None and (
        not isinstance(uses, str) or uses.startswith("./") or uses not in EXPECTED_ACTIONS
    ):
        raise ScopeCheckError(f"jobs.build uses untrusted action: {uses!r}")


def validate_checkout(steps: list[dict[str, Any]]) -> None:
    """Require one checkout of the supplied release SHA at the repository root."""
    checkouts = [step for step in steps if step.get("uses") == CHECKOUT_ACTION]
    expected_with = {
        "persist-credentials": False,
        "ref": "${{ inputs.release_sha || github.sha }}",
        "submodules": False,
    }
    if len(checkouts) != 1 or checkouts[0].get("with") != expected_with or "path" in checkouts[0]:
        raise ScopeCheckError("checkout must use the release ref at repository root with no submodules")


def validate_toolchain_action(steps: list[dict[str, Any]]) -> None:
    """Require the action that installs the same pinned toolchain for both legs."""
    actions = [step for step in steps if step.get("uses") == RUST_TOOLCHAIN_ACTION]
    expected_with = {
        "toolchain": RUST_TOOLCHAIN_CHANNEL,
        "targets": "${{ matrix.target }}",
    }
    if len(actions) != 1 or actions[0].get("with") != expected_with:
        raise ScopeCheckError("release builds must install the exact matrix Rust toolchain")


def validate_execution_envelope(workflow: dict[str, Any], steps: list[dict[str, Any]]) -> None:
    """Bind defaults, inherited environment, actions, and checkout semantics."""
    validate_job_context(workflow)
    validate_workflow_context(workflow)
    for step in steps:
        validate_step_context(step)
    validate_checkout(steps)
    validate_toolchain_action(steps)


def validate_build_step(step: dict[str, Any], index: int, expected: ExpectedBuild) -> ValidatedBuild:
    """Require every artifact-step key to match the trusted release step."""
    if SAFE_STEP_DIGESTS[index] is not None or step_digest(step) != TRUSTED_BUILD_STEP_DIGESTS[expected.name]:
        raise ScopeCheckError(f"{expected.kind} release build differs from the trusted step")
    run = step["run"]
    argv = direct_shell_argv(run)
    if tuple(argv) != expected.argv:
        raise ScopeCheckError(f"unrecognized {expected.kind} release command: {argv!r}")
    return ValidatedBuild(
        expected=expected,
        package=argv[argv.index("-p") + 1],
        binary=argv[argv.index("--bin") + 1],
        features=argv[argv.index("--features") + 1],
    )


def validated_release_builds(workflow: dict[str, Any]) -> list[ValidatedBuild]:
    """Validate every jobs.build run step against its intentionally narrow grammar."""
    validate_release_job_graph(workflow)
    validate_publication_intake(workflow)
    validate_matrix(workflow)
    steps = build_steps(workflow)
    validate_execution_envelope(workflow, steps)
    validate_release_repository(REPO_ROOT)
    if len(steps) != len(SAFE_STEP_DIGESTS):
        raise ScopeCheckError("jobs.build step count differs from the trusted execution graph")
    expected_by_name = {build.name: build for build in EXPECTED_BUILDS}
    seen_builds: dict[str, ValidatedBuild] = {}

    for index, step in enumerate(steps):
        name = step.get("name")
        expected = expected_by_name.get(name)
        if expected is not None:
            if expected.kind in seen_builds:
                raise ScopeCheckError(f"duplicate {expected.kind} release build command")
            seen_builds[expected.kind] = validate_build_step(step, index, expected)
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
    if not (REPO_ROOT / MANIFEST_NAME).is_file():
        raise ScopeCheckError(f"workspace root {REPO_ROOT} has no {MANIFEST_NAME}")
    return REPO_ROOT


def cargo_environment() -> dict[str, str]:
    """Make machine parsing deterministic even when CI globally requests color."""
    return {**os.environ, "CARGO_TERM_COLOR": "never"}


def without_dev_dependencies(manifest: str) -> str:
    """Remove dev-only resolution inputs from a copied probe manifest."""
    retained: list[str] = []
    skipping = False
    for line in manifest.splitlines(keepends=True):
        if line.startswith("[") and line.rstrip().endswith("]"):
            skipping = is_dev_dependency_header(line.rstrip())
        if not skipping:
            retained.append(line)
    return "".join(retained)


def is_dev_dependency_header(header: str) -> bool:
    """Recognize only Cargo's plain and target-specific dev-dependency tables."""
    return header == "[dev-dependencies]" or (
        header.startswith("[target.") and header.endswith(".dev-dependencies]")
    )


def with_probe_workspace(manifest: str, manifest_dir: Path, probe: Path) -> str:
    """Keep excluded path dependencies attached to the probe workspace settings."""
    relative_root = os.path.relpath(probe, manifest_dir).replace(os.sep, "/")
    rewritten, replacements = re.subn(
        r"(?m)^\[package\]$", f'[package]\nworkspace = "{relative_root}"', manifest, count=1
    )
    if replacements != 1:
        raise ScopeCheckError("copied local manifest has no package table")
    return rewritten


def crate_manifests() -> list[Path]:
    """List local manifests shallow-first so nested crate paths stay lexical."""
    return sorted((REPO_ROOT / "crates").rglob(MANIFEST_NAME), key=lambda path: len(path.parts))


def replace_manifest_section(pattern: re.Pattern[str], replacement: str, message: str, manifest: str) -> str:
    """Replace one required root-workspace TOML section or fail closed."""
    rewritten, replacements = pattern.subn(replacement, manifest, count=1)
    if replacements != 1:
        raise ScopeCheckError(message)
    return rewritten


def probe_root_manifest(build: ValidatedBuild, manifest_dirs: set[Path]) -> str:
    """Keep workspace settings but select and exclude exactly the probe graph."""
    source = (REPO_ROOT / MANIFEST_NAME).read_text(encoding="utf-8")
    members = f'members = [\n    "crates/{build.package}",\n]'
    rewritten = replace_manifest_section(
        MEMBERS_SECTION, members, f"cannot isolate the workspace members in {MANIFEST_NAME}", source
    )
    excluded = [
        directory.relative_to(REPO_ROOT).as_posix()
        for directory in manifest_dirs
        if directory != REPO_ROOT / "crates" / build.package
    ]
    exclude = "exclude = [\n" + "".join(f'    "{path}",\n' for path in sorted(excluded)) + "]"
    return replace_manifest_section(
        EXCLUDE_SECTION, exclude, f"cannot isolate the workspace exclusions in {MANIFEST_NAME}", rewritten
    )


def has_nested_manifest(entry: Path, manifest_dirs: set[Path]) -> bool:
    """Whether a directory needs a lexical copy rather than one source symlink."""
    return entry.is_dir() and any(entry == child or entry in child.parents for child in manifest_dirs)


def mirror_crate_manifest(source: Path, probe: Path, manifest_dirs: set[Path]) -> None:
    """Copy one manifest and link its non-manifest contents into the probe."""
    source_dir = source.parent
    destination = probe / source_dir.relative_to(REPO_ROOT)
    destination.mkdir(parents=True, exist_ok=True)
    copied = with_probe_workspace(without_dev_dependencies(source.read_text(encoding="utf-8")), destination, probe)
    (destination / MANIFEST_NAME).write_text(copied, encoding="utf-8")
    for entry in source_dir.iterdir():
        if entry.name == MANIFEST_NAME:
            continue
        target = destination / entry.name
        if has_nested_manifest(entry, manifest_dirs):
            target.mkdir(exist_ok=True)
        else:
            os.symlink(entry, target, target_is_directory=entry.is_dir())


def materialize_probe(build: ValidatedBuild, probe: Path) -> None:
    """Create the isolated manifests-only workspace used for metadata resolution."""
    manifests = crate_manifests()
    directories = {manifest.parent for manifest in manifests}
    (probe / MANIFEST_NAME).write_text(probe_root_manifest(build, directories), encoding="utf-8")
    shutil.copyfile(REPO_ROOT / LOCKFILE_NAME, probe / LOCKFILE_NAME)
    for manifest in manifests:
        mirror_crate_manifest(manifest, probe, directories)


def probe_metadata(build: ValidatedBuild, probe: Path) -> dict[str, Any]:
    """Run and decode Cargo's authoritative machine-readable feature graph."""
    command = [
        "cargo", "metadata", "--manifest-path", str(probe / MANIFEST_NAME), "--offline",
        "--format-version", "1", "--filter-platform", build.expected.matrix_target,
        "--features", build.features,
    ]
    completed = subprocess.run(
        command, capture_output=True, text=True, cwd=REPO_ROOT, env=cargo_environment(), check=False
    )
    if completed.returncode:
        detail = completed.stderr.strip().splitlines()
        message = " | ".join(detail[:6]) if detail else "(no stderr output)"
        raise ScopeCheckError(f"cargo metadata failed: {message}")
    try:
        metadata = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise ScopeCheckError(f"cargo metadata emitted invalid JSON: {error}") from error
    if not isinstance(metadata, dict):
        raise ScopeCheckError("cargo metadata root is not an object")
    return metadata


def normalize_probe_paths(metadata: dict[str, Any], probe: Path) -> list[dict[str, Any]]:
    """Map probe-local manifest paths back to their checked-out source paths."""
    packages = metadata.get("packages")
    if not isinstance(packages, list) or not all(isinstance(package, dict) for package in packages):
        raise ScopeCheckError("cargo metadata packages is not a list of objects")
    for package in packages:
        manifest_path = package.get("manifest_path")
        if not isinstance(manifest_path, str):
            raise ScopeCheckError("cargo metadata package has no manifest path")
        path = Path(manifest_path).resolve()
        if path == probe or probe in path.parents:
            package["manifest_path"] = str(REPO_ROOT / path.relative_to(probe))
    return packages


def validate_probe_members(metadata: dict[str, Any], packages: list[dict[str, Any]], build: ValidatedBuild) -> None:
    """Require metadata to select exactly the shipped workspace member."""
    members = metadata.get("workspace_members")
    if not isinstance(members, list) or not all(isinstance(member, str) for member in members):
        raise ScopeCheckError("cargo metadata workspace_members is not a string list")
    manifest = str(REPO_ROOT / "crates" / build.package / MANIFEST_NAME)
    shipped = [package.get("id") for package in packages if package.get("name") == build.package and package.get("manifest_path") == manifest]
    if len(shipped) != 1 or members != shipped:
        raise ScopeCheckError("probe metadata includes workspace members outside the shipped package")


def metadata_for(build: ValidatedBuild) -> dict[str, Any]:
    """Resolve the shipped graph in an isolated workspace with no dev inputs."""
    with tempfile.TemporaryDirectory(prefix="aletheia-release-scope-") as directory:
        probe = Path(directory).resolve()
        materialize_probe(build, probe)
        metadata = probe_metadata(build, probe)
        packages = normalize_probe_paths(metadata, probe)
    validate_probe_members(metadata, packages, build)
    return metadata


def package_index(metadata: dict[str, Any]) -> dict[str, dict[str, Any]]:
    """Index well-formed metadata packages by Cargo's stable package ID."""
    packages = metadata.get("packages")
    if not isinstance(packages, list):
        raise ScopeCheckError("cargo metadata packages is not a list")
    indexed = {
        package.get("id"): package
        for package in packages
        if isinstance(package, dict) and isinstance(package.get("id"), str)
    }
    if not indexed:
        raise ScopeCheckError("cargo metadata has no package IDs")
    return indexed


def shipped_package_id(packages: dict[str, dict[str, Any]], workspace_root: Path) -> str:
    """Find the one metadata package that represents the shipped binary crate."""
    manifest = (workspace_root / "crates" / SHIPPED_PACKAGE / MANIFEST_NAME).resolve()
    roots = [
        package_id for package_id, package in packages.items()
        if package.get("name") == SHIPPED_PACKAGE
        and Path(str(package.get("manifest_path", ""))).resolve() == manifest
    ]
    if len(roots) != 1:
        raise ScopeCheckError("cargo metadata cannot identify exactly one shipped package root")
    return roots[0]


def normal_or_build_dependency(dependency: Any) -> str | None:
    """Return one dependency ID only when it participates in a shipped build."""
    if not isinstance(dependency, dict) or not isinstance(dependency.get("pkg"), str):
        return None
    kinds = dependency.get("dep_kinds")
    if not isinstance(kinds, list):
        raise ScopeCheckError("cargo metadata resolve.dep_kinds is not a list")
    if any(isinstance(kind, dict) and kind.get("kind") in (None, "build") for kind in kinds):
        return dependency["pkg"]
    return None


def resolution_index(metadata: dict[str, Any]) -> tuple[dict[str, frozenset[str]], dict[str, list[str]]]:
    """Extract exact resolved features and non-dev edges from Cargo metadata."""
    resolve = metadata.get("resolve")
    nodes = resolve.get("nodes") if isinstance(resolve, dict) else None
    if not isinstance(nodes, list):
        raise ScopeCheckError("cargo metadata resolve.nodes is not a list")
    features: dict[str, frozenset[str]] = {}
    dependencies: dict[str, list[str]] = {}
    for node in nodes:
        if not isinstance(node, dict) or not isinstance(node.get("id"), str):
            continue
        node_features = node.get("features")
        node_deps = node.get("deps")
        if not isinstance(node_features, list) or not all(isinstance(feature, str) for feature in node_features):
            raise ScopeCheckError("cargo metadata resolve.features is not a string list")
        if not isinstance(node_deps, list):
            raise ScopeCheckError("cargo metadata resolve.deps is not a list")
        features[node["id"]] = frozenset(node_features)
        dependencies[node["id"]] = [
            package for dependency in node_deps if (package := normal_or_build_dependency(dependency))
        ]
    return features, dependencies


def reachable_packages(root: str, packages: dict[str, dict[str, Any]], dependencies: dict[str, list[str]]) -> set[str]:
    """Traverse every normal/build edge and reject an incomplete metadata graph."""
    reachable: set[str] = set()
    pending = [root]
    while pending:
        package_id = pending.pop()
        if package_id in reachable:
            continue
        if package_id not in packages or package_id not in dependencies:
            raise ScopeCheckError("cargo metadata dependency graph is incomplete")
        reachable.add(package_id)
        pending.extend(dependencies[package_id])
    return reachable


def expected_local_packages(metadata: dict[str, Any], workspace_root: Path) -> dict[Path, ExpectedLocalPackage]:
    """Bind all reachable local rows to Cargo's resolved feature arrays."""
    packages = package_index(metadata)
    root = shipped_package_id(packages, workspace_root)
    features, dependencies = resolution_index(metadata)
    reachable = reachable_packages(root, packages, dependencies)
    expected: dict[Path, ExpectedLocalPackage] = {}
    local_root = workspace_root.resolve()
    for package_id in reachable:
        package = packages[package_id]
        source = Path(str(package.get("manifest_path", ""))).resolve().parent
        if source != local_root and local_root not in source.parents:
            continue
        name = package.get("name")
        if not isinstance(name, str) or source in expected or package_id not in features:
            raise ScopeCheckError("cargo metadata local package row is malformed or incomplete")
        expected[source] = ExpectedLocalPackage(name, features[package_id])
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
