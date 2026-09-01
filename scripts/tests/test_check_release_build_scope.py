from __future__ import annotations

import copy
import importlib.util
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
from pathlib import Path
from typing import Callable

import yaml

SCRIPT_PATH = Path(__file__).resolve().parents[1] / "check-release-build-scope.py"
SPEC = importlib.util.spec_from_file_location("check_release_build_scope", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {SCRIPT_PATH}")
scope = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = scope
SPEC.loader.exec_module(scope)

NATIVE = (
    "cargo auditable build --locked --release -p aletheia --bin aletheia "
    '--target "$BUILD_TARGET" --features recall,embed-candle'
)


def workflow() -> dict:
    """Use the complete current job so hash pinning is exercised, not mocked."""
    return copy.deepcopy(yaml.safe_load((scope.REPO_ROOT / scope.RELEASE_WORKFLOW).read_text()))


def build_step(candidate: dict, name: str) -> dict:
    return next(step for step in candidate["jobs"]["build"]["steps"] if step.get("name") == name)


def expected_packages(koina_features: frozenset[str] | None = None) -> dict[Path, scope.ExpectedLocalPackage]:
    return {
        Path("/workspace/crates/aletheia"): scope.ExpectedLocalPackage(
            "aletheia", frozenset({"default", "recall", "embed-candle"})
        ),
        Path("/workspace/crates/koina"): scope.ExpectedLocalPackage(
            "koina", koina_features or frozenset({"default", "rustls-provider"})
        ),
    }


class ReleaseBuildValidation(unittest.TestCase):
    def assert_rejected(self, candidate: dict) -> None:
        with mock.patch.object(scope, "validate_release_repository"):
            with self.assertRaises(scope.ScopeCheckError):
                scope.validated_release_builds(candidate)

    def test_current_job_is_accepted_and_derives_replay_args(self) -> None:
        with mock.patch.object(scope, "validate_release_repository"):
            builds = scope.validated_release_builds(workflow())
        self.assertEqual([build.package for build in builds], ["aletheia", "aletheia"])
        self.assertEqual([build.features for build in builds], ["recall,embed-candle"] * 2)
        self.assertEqual(
            [build.expected.matrix_target for build in builds],
            ["aarch64-apple-darwin", "x86_64-unknown-linux-musl"],
        )

    def test_rejects_extra_features_all_features_prefixes_and_repeats(self) -> None:
        for extra in (
            "--features recall,embed-candle,test-support",
            "--all-features",
            "-p aletheia",
            "--bin aletheia",
            "--features recall,embed-candle",
        ):
            with self.subTest(extra=extra):
                candidate = workflow()
                build_step(candidate, "Build (native)")["run"] = f"{NATIVE} {extra}"
                self.assert_rejected(candidate)
        for replacement in ("-p aletheia-malicious", "--bin aletheia-helper"):
            with self.subTest(replacement=replacement):
                candidate = workflow()
                build_step(candidate, "Build (native)")["run"] = NATIVE.replace(
                    "-p aletheia" if replacement.startswith("-p") else "--bin aletheia",
                    replacement,
                )
                self.assert_rejected(candidate)

    def test_rejects_missing_duplicate_and_unscoped_builds(self) -> None:
        for name in ("Build (native)", "Build (cross)"):
            with self.subTest(name=name):
                candidate = workflow()
                candidate["jobs"]["build"]["steps"].remove(build_step(candidate, name))
                self.assert_rejected(candidate)
        candidate = workflow()
        candidate["jobs"]["build"]["steps"].append(copy.deepcopy(build_step(candidate, "Build (native)")))
        self.assert_rejected(candidate)
        for run in (
            "cargo --locked build --release",
            "bash -c '\"cargo\" auditable build --locked --release'",
            "./release-wrapper",
            "eval 'cargo build --release'",
        ):
            with self.subTest(run=run):
                candidate = workflow()
                candidate["jobs"]["build"]["steps"].append({"name": "Decoy", "run": run})
                self.assert_rejected(candidate)

    def test_rejects_sibling_release_build_jobs_and_mutated_job_graphs(self) -> None:
        candidate = workflow()
        candidate["jobs"]["alternate-release-build"] = {
            "runs-on": "ubuntu-latest",
            "steps": [
                {
                    "uses": scope.CHECKOUT_ACTION,
                    "with": {
                        "persist-credentials": False,
                        "ref": "${{ inputs.release_sha || github.sha }}",
                    },
                },
                {"run": "cargo build --release --workspace --features test-support"},
            ],
        }
        self.assert_rejected(candidate)
        candidate = workflow()
        candidate["jobs"][scope.OUTCOME_OBSERVER_JOB_ID]["permissions"] = {"contents": "write"}
        self.assert_rejected(candidate)
        candidate = workflow()
        candidate["jobs"]["alternate-artifact-producer"] = {
            "runs-on": "ubuntu-latest",
            "steps": [
                {"run": "cargo build --release --workspace --features test-support"},
                {
                    "uses": "actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a",
                    "with": {"name": "release-aletheia-linux-x86_64", "path": "decoy/"},
                },
            ],
        }
        self.assert_rejected(candidate)
        mutations = (
            lambda candidate: candidate["jobs"]["test"]["steps"].append(
                {"run": "bash -c 'cargo build --release --workspace --features test-support'"}
            ),
            lambda candidate: candidate["jobs"]["feature-check"].update(
                {"strategy": {"matrix": {"feature": ["test-support"]}}}
            ),
            lambda candidate: candidate["jobs"]["sbom"]["steps"].append(
                {"uses": scope.RUST_TOOLCHAIN_ACTION, "with": {"toolchain": "candidate"}}
            ),
            lambda candidate: candidate["jobs"]["build"]["steps"][-1]["with"].update(
                {"name": "release-decoy"}
            ),
            lambda candidate: candidate["jobs"]["publish-release"]["steps"][1]["with"].update(
                {"pattern": "alternate-release-*"}
            ),
        )
        for mutate in mutations:
            with self.subTest(mutate=mutate):
                candidate = workflow()
                mutate(candidate)
                self.assert_rejected(candidate)

    def test_rejects_changed_existing_non_build_step_and_bad_matrix_linkage(self) -> None:
        candidate = workflow()
        build_step(candidate, "Package tarball")["run"] += "\nbash -c 'cargo build --release'"
        self.assert_rejected(candidate)
        for mutate in (
            lambda candidate: build_step(candidate, "Build (native)")["env"].update({"BUILD_TARGET": "wrong"}),
            lambda candidate: build_step(candidate, "Build (cross)").update({"if": "${{ !matrix.cross }}"}),
            lambda candidate: candidate["jobs"]["build"]["strategy"]["matrix"]["include"].__setitem__(0, {"target": "x86_64-unknown-linux-gnu", "cross": True}),
        ):
            with self.subTest(mutate=mutate):
                candidate = workflow()
                mutate(candidate)
                self.assert_rejected(candidate)

    def test_rejects_shell_working_directory_defaults_and_environment_redirects(self) -> None:
        mutations = (
            lambda candidate: build_step(candidate, "Build (native)").update({"shell": "./scripts/unscoped-wrapper {0}"}),
            lambda candidate: build_step(candidate, "Build (native)").update({"working-directory": "decoy-workspace"}),
            lambda candidate: build_step(candidate, "Build (native)")["env"].update({"PATH": "./decoy:$PATH"}),
            lambda candidate: candidate.update({"defaults": {"run": {"shell": "bash {0}"}}}),
            lambda candidate: candidate["jobs"]["build"].update({"defaults": {"run": {"working-directory": "decoy"}}}),
            lambda candidate: candidate["env"].update({"CARGO_ALIAS_BUILD": "build --workspace"}),
            lambda candidate: candidate["jobs"]["build"].update({"env": {"RUSTC_WRAPPER": "./wrapper"}}),
            lambda candidate: candidate["jobs"]["build"]["env"].update({"RUSTUP_HOME": "./rustup"}),
            lambda candidate: candidate["jobs"]["build"]["env"].update({"RUSTUP_TOOLCHAIN": "candidate"}),
            lambda candidate: build_step(candidate, "Build (cross)")["env"].update({"CARGO_HOME": "./decoy"}),
            lambda candidate: build_step(candidate, "Build (native)")["env"].update({"UNRELATED": "value"}),
            lambda candidate: candidate["jobs"]["build"].update({"container": "evil:latest"}),
        )
        for mutate in mutations:
            with self.subTest(mutate=mutate):
                candidate = workflow()
                mutate(candidate)
                self.assert_rejected(candidate)

    def test_rejects_build_step_failure_and_timeout_controls(self) -> None:
        mutations = (
            lambda step: step.update({"continue-on-error": True}),
            lambda step: step.update({"timeout-minutes": 1}),
            lambda step: step.update({"id": "cached-artifact"}),
            lambda step: step.update({"unexpected": "accepted-nowhere"}),
        )
        for name in ("Build (native)", "Build (cross)"):
            for mutate in mutations:
                with self.subTest(name=name, mutate=mutate):
                    candidate = workflow()
                    mutate(build_step(candidate, name))
                    self.assert_rejected(candidate)

    def test_rejects_untrusted_actions_and_checkout_context_mutation(self) -> None:
        candidate = workflow()
        candidate["jobs"]["build"]["steps"].append({"uses": "./.github/actions/unscoped-build"})
        self.assert_rejected(candidate)
        for mutate in (
            lambda candidate: candidate["jobs"]["build"]["steps"].append({"uses": "actions/cache@deadbeef"}),
            lambda candidate: next(step for step in candidate["jobs"]["build"]["steps"] if step.get("uses") == scope.CHECKOUT_ACTION)["with"].update({"submodules": True}),
            lambda candidate: next(step for step in candidate["jobs"]["build"]["steps"] if step.get("uses") == scope.CHECKOUT_ACTION)["with"].update({"ref": "main"}),
            lambda candidate: next(step for step in candidate["jobs"]["build"]["steps"] if step.get("uses") == scope.CHECKOUT_ACTION)["with"].update({"path": "decoy"}),
            lambda candidate: next(step for step in candidate["jobs"]["build"]["steps"] if step.get("uses") == scope.CHECKOUT_ACTION).update({"working-directory": "decoy"}),
            lambda candidate: next(step for step in candidate["jobs"]["build"]["steps"] if step.get("uses") == scope.RUST_TOOLCHAIN_ACTION)["with"].update({"toolchain": "candidate"}),
        ):
            with self.subTest(mutate=mutate):
                candidate = workflow()
                mutate(candidate)
                self.assert_rejected(candidate)


class PublicationIntake(unittest.TestCase):
    def downloads(self, candidate: dict) -> list[dict]:
        return [
            step for step in candidate["jobs"]["publish-release"]["steps"]
            if step.get("uses") == scope.DOWNLOAD_ARTIFACT_ACTION
        ]

    def assert_intake_rejected(self, candidate: dict) -> None:
        with self.assertRaises(scope.ScopeCheckError):
            scope.validate_publication_intake(candidate)

    def test_exact_canonical_artifacts_are_the_only_publication_intake(self) -> None:
        candidate = workflow()
        self.assertEqual(
            [
                (step["with"]["name"], step["with"]["path"])
                for step in self.downloads(candidate)
            ],
            list(scope.TRUSTED_PUBLICATION_DOWNLOADS),
        )
        scope.validate_publication_intake(candidate)

    def test_rejects_patterns_duplicates_omissions_extras_and_merge_inputs(self) -> None:
        mutations = (
            lambda candidate: self.downloads(candidate)[0].update(
                {"with": {"pattern": "release-*", "path": "release-assets", "merge-multiple": True}}
            ),
            lambda candidate: self.downloads(candidate)[1]["with"].update(
                {"name": self.downloads(candidate)[0]["with"]["name"]}
            ),
            lambda candidate: candidate["jobs"]["publish-release"]["steps"].remove(
                self.downloads(candidate)[-1]
            ),
            lambda candidate: candidate["jobs"]["publish-release"]["steps"].append(
                {"uses": scope.DOWNLOAD_ARTIFACT_ACTION, "with": {"name": "release-shaped-diagnostic", "path": "release-assets"}}
            ),
            lambda candidate: self.downloads(candidate)[0]["with"].update({"merge-multiple": True}),
        )
        for mutate in mutations:
            with self.subTest(mutate=mutate):
                candidate = workflow()
                mutate(candidate)
                self.assert_intake_rejected(candidate)

    def test_reusable_callee_upload_cannot_enter_explicit_publication_intake(self) -> None:
        security = yaml.safe_load((scope.REPO_ROOT / ".github/workflows/security.yml").read_text())
        security["jobs"]["cargo-deny"]["steps"].append(
            {
                "uses": "actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a",
                "with": {"name": "release-shaped-diagnostic", "path": "diagnostic.txt"},
            }
        )
        self.assertEqual(
            security["jobs"]["cargo-deny"]["steps"][-1]["with"]["name"],
            "release-shaped-diagnostic",
        )
        candidate = workflow()
        self.assertNotIn(
            "release-shaped-diagnostic",
            [step["with"]["name"] for step in self.downloads(candidate)],
        )
        scope.validate_publication_intake(candidate)


class ResolutionParsing(unittest.TestCase):
    def parse(self, output: str, expected: dict[Path, scope.ExpectedLocalPackage] | None = None) -> dict[str, set[str]]:
        return scope.resolved_member_features(output, expected or expected_packages())

    def test_clean_complete_graph_passes(self) -> None:
        output = """\\
aletheia v0.45.0 (/workspace/crates/aletheia)|default,embed-candle,recall
koina v0.45.0 (/workspace/crates/koina)|default,rustls-provider
"""
        self.assertEqual(scope.forbidden_resolutions(self.parse(output), "test"), [])

    def test_colored_duplicate_markers_are_normalized(self) -> None:
        output = (
            "aletheia v0.45.0 (/workspace/crates/aletheia)|default,embed-candle,recall\n"
            "koina v0.45.0 (/workspace/crates/koina)|default,rustls-provider "
            "\x1b[33m\x1b[2m(*)\x1b[39m\x1b[22m\n"
        )
        self.assertEqual(scope.forbidden_resolutions(self.parse(output), "test"), [])

    def test_leaks_and_unexpected_features_are_reported(self) -> None:
        output = """\\
aletheia v0.45.0 (/workspace/crates/aletheia)|default,embed-candle,recall
koina v0.45.0 (/workspace/crates/koina)|default,test-support
"""
        report = "\n".join(
            scope.forbidden_resolutions(self.parse(output, expected_packages(frozenset({"default", "test-support"}))), "test")
        )
        self.assertIn("test-support", report)
        unexpected = output.replace("test-support", "made-up")
        with self.assertRaises(scope.ScopeCheckError):
            self.parse(unexpected)

    def test_empty_feature_columns_cannot_certify_a_resolved_graph(self) -> None:
        output = """\\
aletheia v0.45.0 (/workspace/crates/aletheia)|
koina v0.45.0 (/workspace/crates/koina)|
"""
        with self.assertRaises(scope.ScopeCheckError):
            self.parse(output)

    def test_missing_truncated_or_unexpected_local_rows_fail_closed(self) -> None:
        cases = (
            "aletheia v0.45.0 (/workspace/crates/aletheia)|default,embed-candle,recall\n",
            "aletheia v0.45.0 (/workspace/crates/aletheia)|default,embed-candle,recall\nkoina v0.45.0\n",
            "aletheia v0.45.0 (/workspace/crates/aletheia)|default,embed-candle,recall\nkoina v0.45.0 (/workspace/crates/koina)\n",
            "aletheia v0.45.0 (/workspace/crates/aletheia)|default,embed-candle,recall\nother v0.45.0 (/workspace/crates/other)|default\n",
        )
        for output in cases:
            with self.subTest(output=output):
                with self.assertRaises(scope.ScopeCheckError):
                    self.parse(output)


class ProbeManifest(unittest.TestCase):
    def test_dev_dependency_sections_are_not_resolution_inputs(self) -> None:
        manifest = """\
[package]
name = "probe"

[dependencies]
kept = "1"

[dev-dependencies]
test-only = "1"

[target.'cfg(unix)'.dev-dependencies]
also-test-only = "1"

[lints]
workspace = true
"""
        rewritten = scope.without_dev_dependencies(manifest)
        self.assertIn('kept = "1"', rewritten)
        self.assertIn("[lints]", rewritten)
        self.assertNotIn("test-only", rewritten)


class CrossInputs(unittest.TestCase):
    def test_cross_configuration_and_wrapper_are_content_pinned(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for relative in scope.TRUSTED_CROSS_INPUTS:
                target = root / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes((scope.REPO_ROOT / relative).read_bytes())
            scope.validate_cross_inputs(root)
            for relative in scope.TRUSTED_CROSS_INPUTS:
                with self.subTest(relative=relative):
                    target = root / relative
                    target.write_bytes(target.read_bytes() + b"\n# unscoped cargo wrapper\n")
                    with self.assertRaises(scope.ScopeCheckError):
                        scope.validate_cross_inputs(root)
                    target.write_bytes((scope.REPO_ROOT / relative).read_bytes())


class CargoConfigurationBoundary(unittest.TestCase):
    def git(self, root: Path, *arguments: str) -> None:
        completed = subprocess.run(
            ["git", "-C", str(root), *arguments], capture_output=True, check=False
        )
        if completed.returncode:
            self.fail(completed.stderr.decode("utf-8", "replace"))

    def initialize_checkout(self, root: Path, ignored_config: bool = False) -> None:
        cargo = root / ".cargo"
        cargo.mkdir(parents=True)
        (cargo / "audit.toml").write_text("[advisories]\n", encoding="utf-8")
        if ignored_config:
            (root / ".gitignore").write_text(".cargo/config.toml\n", encoding="utf-8")
        self.git(root, "init", "-q")
        self.git(root, "config", "user.name", "release scope test")
        self.git(root, "config", "user.email", "release-scope@example.invalid")
        self.git(root, "add", "-A")
        self.git(root, "commit", "-qm", "fixture")

    def assert_tree_rejected(
        self, create: Callable[[Path], None], expected_entry: tuple[str, str]
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            create(root)
            self.git(root, "init", "-q")
            self.git(root, "config", "user.name", "release scope test")
            self.git(root, "config", "user.email", "release-scope@example.invalid")
            self.git(root, "add", "-A")
            self.git(root, "commit", "-qm", "fixture")
            self.assertIn(expected_entry, scope.head_tree_entries(root))
            with self.assertRaises(scope.ScopeCheckError):
                scope.validate_cargo_configuration_boundary(root)

    def test_clean_head_allows_only_root_audit_toml(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.initialize_checkout(root)
            self.assertEqual(scope.head_tree_entries(root), [("100644", ".cargo/audit.toml")])
            scope.validate_cargo_configuration_boundary(root)

    def test_tracked_configuration_paths_are_rejected_from_head(self) -> None:
        cases = (
            (Path(".cargo/config.toml"), '[alias]\nauditable = ["run", "--bin", "wrapper", "--"]\n'),
            (Path(".cargo/config"), '[build]\nrustc-wrapper = "./wrapper"\n'),
            (Path("decoy/.cargo/config.toml"), '[target.x86_64-unknown-linux-musl]\nrunner = "./runner"\n'),
            (Path("decoy/.cargo/config"), '[env]\nPATH = "./wrapper"\n'),
        )
        for relative, contents in cases:
            with self.subTest(relative=relative):
                def create(root: Path, relative: Path = relative, contents: str = contents) -> None:
                    config = root / relative
                    config.parent.mkdir(parents=True, exist_ok=True)
                    config.write_text(contents, encoding="utf-8")

                self.assert_tree_rejected(create, ("100644", relative.as_posix()))

    def test_case_colliding_cargo_configuration_is_rejected_for_macos(self) -> None:
        def create(root: Path) -> None:
            (root / ".cargo").mkdir()
            (root / ".cargo" / "audit.toml").write_text("[advisories]\n", encoding="utf-8")
            (root / ".Cargo").mkdir()
            (root / ".Cargo" / "config.toml").write_text(
                '[alias]\nauditable = ["run", "--bin", "wrapper", "--"]\n', encoding="utf-8"
            )

        self.assert_tree_rejected(create, ("100644", ".Cargo/config.toml"))

    def test_tracked_cargo_symlinks_are_rejected_by_git_mode(self) -> None:
        cases = ("evil-cargo", "missing-cargo", "/project/evil-cargo", "/project/missing-cargo")
        for target in cases:
            with self.subTest(target=target):
                def create(root: Path, target: str = target) -> None:
                    if target == "/project/evil-cargo":
                        (root / "evil-cargo").mkdir()
                        (root / "evil-cargo" / "config.toml").write_text(
                            '[alias]\nauditable = ["run", "--bin", "wrapper", "--"]\n', encoding="utf-8"
                        )
                    elif target == "evil-cargo":
                        (root / target).mkdir()
                    (root / ".cargo").symlink_to(target, target_is_directory=True)

                self.assert_tree_rejected(create, ("120000", ".cargo"))

    def test_untracked_and_ignored_configuration_surfaces_are_rejected(self) -> None:
        for ignored in (False, True):
            with self.subTest(ignored=ignored):
                with tempfile.TemporaryDirectory() as directory:
                    root = Path(directory)
                    self.initialize_checkout(root, ignored_config=ignored)
                    (root / ".cargo" / "config.toml").write_text(
                        '[alias]\nauditable = ["run", "--bin", "wrapper", "--"]\n', encoding="utf-8"
                    )
                    with self.assertRaises(scope.ScopeCheckError):
                        scope.validate_cargo_configuration_boundary(root)

    def test_ignored_casefolded_configuration_is_rejected_for_macos(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.initialize_checkout(root)
            (root / ".gitignore").write_text(".Cargo/CONFIG\n", encoding="utf-8")
            self.git(root, "add", ".gitignore")
            self.git(root, "commit", "-qm", "ignore casefolded config")
            (root / ".Cargo").mkdir()
            (root / ".Cargo" / "CONFIG").write_text(
                '[build]\nrustc-wrapper = "./wrapper"\n', encoding="utf-8"
            )
            with self.assertRaises(scope.ScopeCheckError):
                scope.validate_cargo_configuration_boundary(root)

    def test_dirty_tracked_audit_file_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.initialize_checkout(root)
            (root / ".cargo" / "audit.toml").write_text("[advisories]\nignore = []\n", encoding="utf-8")
            with self.assertRaises(scope.ScopeCheckError):
                scope.validate_cargo_configuration_boundary(root)


class ToolchainBoundary(CargoConfigurationBoundary):
    def initialize_toolchain_checkout(self, root: Path) -> None:
        self.initialize_checkout(root)
        (root / scope.RUST_TOOLCHAIN).write_bytes((scope.REPO_ROOT / scope.RUST_TOOLCHAIN).read_bytes())
        self.git(root, "add", scope.RUST_TOOLCHAIN.as_posix())
        self.git(root, "commit", "-qm", "canonical toolchain")

    def assert_toolchain_tree_rejected(
        self, create: Callable[[Path], None], expected_entry: tuple[str, str]
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.initialize_toolchain_checkout(root)
            create(root)
            self.git(root, "add", "-A")
            self.git(root, "commit", "-qm", "candidate selector")
            self.assertIn(expected_entry, scope.head_tree_entries(root))
            with self.assertRaises(scope.ScopeCheckError):
                scope.validate_toolchain_boundary(root)

    def test_canonical_toolchain_definition_is_accepted(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.initialize_toolchain_checkout(root)
            self.assertEqual(
                [entry for entry in scope.head_tree_entries(root) if scope.is_toolchain_tree_path(entry[1])],
                [("100644", "rust-toolchain.toml")],
            )
            scope.validate_toolchain_boundary(root)

    def test_legacy_path_toolchain_and_wrappers_are_rejected(self) -> None:
        def create(root: Path) -> None:
            (root / "candidate-toolchain" / "bin").mkdir(parents=True)
            for executable in ("cargo", "rustc"):
                path = root / "candidate-toolchain" / "bin" / executable
                path.write_text("#!/bin/sh\nexit 99\n", encoding="utf-8")
                path.chmod(0o755)
            (root / "rust-toolchain").write_text(
                '[toolchain]\npath = "candidate-toolchain"\n', encoding="utf-8"
            )

        self.assert_toolchain_tree_rejected(create, ("100644", "rust-toolchain"))

    def test_tampered_canonical_and_casefolded_selectors_are_rejected(self) -> None:
        cases = (
            (Path("rust-toolchain.toml"), '[toolchain]\nchannel = "candidate"\n'),
            (Path("Rust-toolchain"), '[toolchain]\npath = "candidate-toolchain"\n'),
            (Path(".rustup"), "candidate rustup selector\n"),
        )
        for relative, contents in cases:
            with self.subTest(relative=relative):
                def create(root: Path, relative: Path = relative, contents: str = contents) -> None:
                    selector = root / relative
                    selector.parent.mkdir(parents=True, exist_ok=True)
                    selector.write_text(contents, encoding="utf-8")

                self.assert_toolchain_tree_rejected(create, ("100644", relative.as_posix()))

    def test_canonical_toolchain_symlink_is_rejected_by_mode(self) -> None:
        def create(root: Path) -> None:
            canonical = root / scope.RUST_TOOLCHAIN
            canonical.unlink()
            target = root / "candidate-toolchain.toml"
            target.write_text('[toolchain]\npath = "candidate-toolchain"\n', encoding="utf-8")
            canonical.symlink_to(target.name)

        self.assert_toolchain_tree_rejected(create, ("120000", "rust-toolchain.toml"))

    def test_ignored_toolchain_selector_is_rejected_before_tool_invocation(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.initialize_toolchain_checkout(root)
            (root / ".gitignore").write_text("Rust-toolchain\n", encoding="utf-8")
            self.git(root, "add", ".gitignore")
            self.git(root, "commit", "-qm", "ignore selector")
            (root / "Rust-toolchain").write_text(
                '[toolchain]\npath = "candidate-toolchain"\n', encoding="utf-8"
            )
            with mock.patch.object(scope, "validate_cross_inputs") as cross_inputs:
                with self.assertRaises(scope.ScopeCheckError):
                    scope.validate_release_repository(root)
            cross_inputs.assert_not_called()


if __name__ == "__main__":
    unittest.main()
