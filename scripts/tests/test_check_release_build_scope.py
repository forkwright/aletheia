from __future__ import annotations

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


def workflow(native: str = NATIVE, cross: str = CROSS) -> dict:
    return {
        "jobs": {
            "build": {
                "strategy": {"matrix": {"include": [
                    {"target": "x86_64-unknown-linux-musl"},
                    {"target": "aarch64-apple-darwin"},
                ]}},
                "steps": [{"run": native}, {"run": cross}],
            }
        }
    }


class ReleaseBuildCommands(unittest.TestCase):
    def test_finds_native_and_cross_builds(self) -> None:
        self.assertEqual(scope.release_build_commands(workflow()), [NATIVE, CROSS])

    def test_finds_a_build_inside_multiline_shell(self) -> None:
        self.assertEqual(
            scope.release_build_commands(workflow(native=f"set -eu\n{NATIVE}\necho done")),
            [NATIVE, CROSS],
        )

    def test_scope_requires_package_bin_features_and_lock(self) -> None:
        for removed in ("-p aletheia", "--bin aletheia", "--features recall,embed-candle", "--locked"):
            with self.subTest(removed=removed):
                self.assertTrue(scope.command_violations([NATIVE.replace(f" {removed}", "")]))

    def test_workspace_scope_is_rejected(self) -> None:
        self.assertTrue(scope.command_violations([NATIVE + " --workspace"]))

    def test_missing_build_commands_fails_closed(self) -> None:
        self.assertTrue(scope.command_violations([]))


class ResolutionParsing(unittest.TestCase):
    CLEAN = """\\
aletheia v0.45.0 (/w/crates/aletheia) default,embed-candle,recall
koina v0.45.0 (/w/crates/koina) default,rustls-provider
mneme v0.45.0 (/w/crates/mneme) default,mneme-engine
serde v1.0.0 default,derive
"""
    LEAKY = """\\
aletheia v0.45.0 (/w/crates/aletheia) default,embed-candle,recall
koina v0.45.0 (/w/crates/koina) default,test-support
mneme v0.45.0 (/w/crates/mneme) crash-injection,default
hermeneus v0.45.0 (/w/crates/hermeneus) test-support,test-utils
integration-tests v0.45.0 (/w/crates/integration-tests) default
"""

    def test_parses_only_workspace_members(self) -> None:
        members = scope.resolved_member_features(self.CLEAN)
        self.assertEqual(members["koina"], {"default", "rustls-provider"})
        self.assertNotIn("serde", members)

    def test_clean_scoped_graph_passes(self) -> None:
        self.assertEqual(
            scope.forbidden_resolutions(scope.resolved_member_features(self.CLEAN), "test"), []
        )

    def test_leaky_graph_reports_features_and_harness(self) -> None:
        report = "\n".join(
            scope.forbidden_resolutions(scope.resolved_member_features(self.LEAKY), "test")
        )
        for expected in ("test-support", "test-utils", "crash-injection", "integration-tests"):
            with self.subTest(expected=expected):
                self.assertIn(expected, report)

    def test_empty_parse_fails_closed(self) -> None:
        self.assertTrue(scope.forbidden_resolutions({}, "test"))


class MatrixTargets(unittest.TestCase):
    def test_reads_all_release_targets(self) -> None:
        self.assertEqual(
            scope.matrix_targets(workflow()),
            ["x86_64-unknown-linux-musl", "aarch64-apple-darwin"],
        )

    def test_missing_matrix_fails_closed(self) -> None:
        with self.assertRaises(scope.ScopeCheckError):
            scope.matrix_targets({"jobs": {"build": {}}})


if __name__ == "__main__":
    unittest.main()
