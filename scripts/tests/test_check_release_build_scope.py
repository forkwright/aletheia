from __future__ import annotations

import copy
import importlib.util
import sys
import unittest
from pathlib import Path

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
CROSS = (
    "cross build --locked --release -p aletheia --bin aletheia "
    '--target "$BUILD_TARGET" --features recall,embed-candle'
)


def step(name: str, condition: str, run: str) -> dict:
    return {
        "name": name,
        "if": condition,
        "run": run,
        "env": {"BUILD_TARGET": "${{ matrix.target }}"},
    }


def workflow(native: str = NATIVE, cross: str = CROSS) -> dict:
    return {
        "jobs": {
            "build": {
                "strategy": {"matrix": {"include": [
                    {"target": "x86_64-unknown-linux-musl", "cross": True},
                    {"target": "aarch64-apple-darwin", "cross": False},
                ]}},
                "steps": [
                    step("Build (native)", "${{ !matrix.cross }}", native),
                    step("Build (cross)", "${{ matrix.cross }}", cross),
                ],
            }
        }
    }


class ReleaseBuildValidation(unittest.TestCase):
    def assert_rejected(self, candidate: dict) -> None:
        with self.assertRaises(scope.ScopeCheckError):
            scope.validated_release_builds(candidate)

    def test_current_grammar_is_accepted_and_derives_replay_args(self) -> None:
        builds = scope.validated_release_builds(workflow())
        self.assertEqual([build.package for build in builds], ["aletheia", "aletheia"])
        self.assertEqual([build.features for build in builds], ["recall,embed-candle"] * 2)
        self.assertEqual(
            [build.expected.matrix_target for build in builds],
            ["aarch64-apple-darwin", "x86_64-unknown-linux-musl"],
        )

    def test_rejects_test_support_and_all_features(self) -> None:
        for extra in ("--features recall,embed-candle,test-support", "--all-features"):
            with self.subTest(extra=extra):
                self.assert_rejected(workflow(native=f"{NATIVE} {extra}"))

    def test_rejects_package_and_bin_prefixes(self) -> None:
        self.assert_rejected(workflow(native=NATIVE.replace("-p aletheia", "-p aletheia-malicious")))
        self.assert_rejected(workflow(native=NATIVE.replace("--bin aletheia", "--bin aletheia-helper")))

    def test_rejects_missing_native_or_cross_step(self) -> None:
        for index in (0, 1):
            with self.subTest(index=index):
                candidate = workflow()
                del candidate["jobs"]["build"]["steps"][index]
                self.assert_rejected(candidate)

    def test_rejects_duplicate_and_decoy_build(self) -> None:
        candidate = workflow()
        candidate["jobs"]["build"]["steps"].append(step("Build (native)", "${{ !matrix.cross }}", NATIVE))
        self.assert_rejected(candidate)

    def test_rejects_an_extra_unscoped_cargo_build(self) -> None:
        for run in (
            "cargo build --locked --release",
            "bash -c 'cargo auditable build --locked --release'",
        ):
            with self.subTest(run=run):
                candidate = workflow()
                candidate["jobs"]["build"]["steps"].append(
                    step("Decoy", "${{ matrix.cross }}", run)
                )
                self.assert_rejected(candidate)

    def test_rejects_repeated_scope_or_feature_flags(self) -> None:
        for extra in ("-p aletheia", "--bin aletheia", "--features recall,embed-candle"):
            with self.subTest(extra=extra):
                self.assert_rejected(workflow(native=f"{NATIVE} {extra}"))

    def test_rejects_wrong_target_matrix_linkage(self) -> None:
        mutations = (
            lambda candidate: candidate["jobs"]["build"]["steps"][0]["env"].update({"BUILD_TARGET": "x86_64-unknown-linux-musl"}),
            lambda candidate: candidate["jobs"]["build"]["steps"][1].update({"if": "${{ !matrix.cross }}"}),
            lambda candidate: candidate["jobs"]["build"]["strategy"]["matrix"]["include"].__setitem__(0, {"target": "x86_64-unknown-linux-gnu", "cross": True}),
        )
        for mutate in mutations:
            with self.subTest(mutate=mutate):
                candidate = copy.deepcopy(workflow())
                mutate(candidate)
                self.assert_rejected(candidate)

    def test_rejects_compound_or_multiline_release_builds(self) -> None:
        for run in (f"set -eu\n{NATIVE}", f"{NATIVE}; cargo build --release"):
            with self.subTest(run=run):
                self.assert_rejected(workflow(native=run))


class ResolutionParsing(unittest.TestCase):
    CLEAN = """\\
aletheia v0.45.0 (/workspace/crates/aletheia) default,embed-candle,recall
koina v0.45.0 (/workspace/crates/koina) default,rustls-provider
mneme v0.45.0 (/workspace/crates/mneme) default,mneme-engine
serde v1.0.0 default,derive
"""
    LEAKY = """\\
aletheia v0.45.0 (/workspace/crates/aletheia) default,embed-candle,recall
koina v0.45.0 (/workspace/crates/koina) default,test-support
mneme v0.45.0 (/workspace/crates/mneme) crash-injection,default
hermeneus v0.45.0 (/workspace/crates/hermeneus) test-support,test-utils
integration-tests v0.45.0 (/workspace/crates/integration-tests) default
"""

    def members(self, output: str) -> dict[str, set[str]]:
        return scope.resolved_member_features(output, Path("/workspace"))

    def test_clean_scoped_graph_passes(self) -> None:
        self.assertEqual(scope.forbidden_resolutions(self.members(self.CLEAN), "test"), [])

    def test_leaky_graph_reports_features_and_harness(self) -> None:
        report = "\n".join(scope.forbidden_resolutions(self.members(self.LEAKY), "test"))
        for expected in ("test-support", "test-utils", "crash-injection", "integration-tests"):
            with self.subTest(expected=expected):
                self.assertIn(expected, report)

    def test_partial_local_package_output_fails_closed(self) -> None:
        partial = "koina v0.45.0 (/workspace/crates/koina) default trailing-field\n"
        with self.assertRaises(scope.ScopeCheckError):
            self.members(partial)

    def test_outside_workspace_local_source_fails_closed(self) -> None:
        outside = "koina v0.45.0 (/other/crates/koina) default\n"
        with self.assertRaises(scope.ScopeCheckError):
            self.members(outside)


if __name__ == "__main__":
    unittest.main()
