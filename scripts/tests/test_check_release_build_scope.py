from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path

SCRIPT_PATH = Path(__file__).resolve().parents[1] / "check-release-build-scope.py"
SPEC = importlib.util.spec_from_file_location("check_release_build_scope", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {SCRIPT_PATH}")
crbs = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = crbs
SPEC.loader.exec_module(crbs)

SCOPED_NATIVE = (
    "cargo auditable build --locked --release -p aletheia --bin aletheia "
    '--target "$BUILD_TARGET" --features recall,embed-candle'
)
SCOPED_CROSS = (
    "cross build --locked --release -p aletheia --bin aletheia "
    '--target "$BUILD_TARGET" --features recall,embed-candle'
)


def workflow(native: str = SCOPED_NATIVE, cross: str = SCOPED_CROSS) -> dict:
    return {
        "jobs": {
            "build": {
                "strategy": {
                    "matrix": {
                        "include": [
                            {"target": "x86_64-unknown-linux-musl", "cross": True},
                            {"target": "aarch64-apple-darwin", "cross": False},
                        ]
                    }
                },
                "steps": [
                    {"name": "Build (native)", "run": native},
                    {"name": "Build (cross)", "run": cross},
                    {"name": "Package tarball", "run": "tar czf out.tar.gz dir\n"},
                ],
            },
            "sbom": {"steps": [{"name": "Generate", "run": "scripts/generate-sbom.sh"}]},
        }
    }


class BuildCommandExtraction(unittest.TestCase):
    def test_finds_both_release_build_commands(self) -> None:
        commands = crbs.release_build_commands(workflow())
        self.assertEqual(commands, [SCOPED_NATIVE, SCOPED_CROSS])

    def test_finds_a_build_command_inside_a_multiline_run_block(self) -> None:
        """WHY: a future edit may wrap the build in guard lines; the scan must
        keep seeing the command rather than certifying an empty set."""
        native = f"set -x\n{SCOPED_NATIVE}\necho done\n"
        commands = crbs.release_build_commands(workflow(native=native))
        self.assertIn(SCOPED_NATIVE, commands)

    def test_ignores_steps_without_a_run_block(self) -> None:
        wf = workflow()
        wf["jobs"]["build"]["steps"].append({"name": "Checkout", "uses": "actions/checkout@sha"})
        self.assertEqual(len(crbs.release_build_commands(wf)), 2)


class CommandViolations(unittest.TestCase):
    def test_scoped_commands_pass(self) -> None:
        self.assertEqual(crbs.command_violations([SCOPED_NATIVE, SCOPED_CROSS]), [])

    def test_an_empty_command_set_is_a_violation(self) -> None:
        """WHY: if the build steps are renamed out from under the scan, the
        check must fail loudly instead of passing while inspecting nothing."""
        violations = crbs.command_violations([])
        self.assertEqual(len(violations), 1)
        self.assertIn("no release build command", violations[0])

    def test_a_command_missing_the_package_scope_is_flagged(self) -> None:
        unscoped = (
            "cargo auditable build --locked --release "
            '--target "$BUILD_TARGET" --features recall,embed-candle'
        )
        violations = crbs.command_violations([unscoped])
        self.assertTrue(any("-p aletheia" in v for v in violations))

    def test_a_command_missing_the_bin_scope_is_flagged(self) -> None:
        no_bin = SCOPED_NATIVE.replace(" --bin aletheia", "")
        violations = crbs.command_violations([no_bin])
        self.assertTrue(any("--bin aletheia" in v for v in violations))

    def test_a_command_missing_locked_is_flagged(self) -> None:
        no_lock = SCOPED_NATIVE.replace(" --locked", "")
        violations = crbs.command_violations([no_lock])
        self.assertTrue(any("--locked" in v for v in violations))

    def test_a_command_missing_the_feature_set_is_flagged(self) -> None:
        no_features = SCOPED_NATIVE.replace(" --features recall,embed-candle", "")
        violations = crbs.command_violations([no_features])
        self.assertTrue(any("--features recall,embed-candle" in v for v in violations))

    def test_a_workspace_wide_command_is_flagged(self) -> None:
        wide = SCOPED_NATIVE + " --workspace"
        violations = crbs.command_violations([wide])
        self.assertTrue(any("--workspace" in v for v in violations))


class MatrixTargets(unittest.TestCase):
    def test_extracts_every_matrix_target(self) -> None:
        self.assertEqual(
            crbs.matrix_targets(workflow()),
            ["x86_64-unknown-linux-musl", "aarch64-apple-darwin"],
        )

    def test_a_missing_matrix_is_reported_not_skipped(self) -> None:
        wf = workflow()
        del wf["jobs"]["build"]["strategy"]
        with self.assertRaises(crbs.ScopeCheckError):
            crbs.matrix_targets(wf)


TREE_OUTPUT = """\
aletheia v0.44.0 (/w/crates/aletheia) cc-provider,default,embed-candle,recall,storage-fjall,tui
koina feature "default" (command-line)
koina v0.44.0 (/w/crates/koina) default,fjall-helpers,rustls-provider
koina v0.44.0 (/w/crates/koina) default,fjall-helpers,rustls-provider (*)
mneme v0.44.0 (/w/crates/mneme) default,embed-candle,mneme-engine,storage-fjall
serde v1.0.219 default,derive,std
eidos v0.44.0 (/w/crates/eidos)
"""

LEAKY_TREE_OUTPUT = """\
aletheia v0.44.0 (/w/crates/aletheia) cc-provider,default,embed-candle,recall,storage-fjall,tui
koina v0.44.0 (/w/crates/koina) default,fjall-helpers,rustls-provider,test-support
mneme v0.44.0 (/w/crates/mneme) crash-injection,default,mneme-engine,storage-fjall,test-support
hermeneus v0.44.0 (/w/crates/hermeneus) cc-provider,test-support,test-utils
integration-tests v0.44.0 (/w/crates/integration-tests) default,knowledge-store
serde v1.0.219 default,derive,std
"""


class TreeParsing(unittest.TestCase):
    def test_collects_workspace_members_with_their_features(self) -> None:
        members = crbs.resolved_member_features(TREE_OUTPUT)
        self.assertEqual(
            members["koina"], {"default", "fjall-helpers", "rustls-provider"}
        )
        self.assertEqual(members["eidos"], set())

    def test_ignores_registry_packages_and_feature_pseudo_nodes(self) -> None:
        """WHY: registry crates legitimately expose features named like test
        helpers; only workspace members feed the shipped-binary policy."""
        members = crbs.resolved_member_features(TREE_OUTPUT)
        self.assertNotIn("serde", members)
        self.assertEqual(len(members), 4)

    def test_deduplicated_repeat_lines_union_instead_of_clobbering(self) -> None:
        repeat = TREE_OUTPUT + "koina v0.44.0 (/w/crates/koina) extra\n"
        members = crbs.resolved_member_features(repeat)
        self.assertIn("extra", members["koina"])
        self.assertIn("default", members["koina"])


class ForbiddenResolutions(unittest.TestCase):
    def test_a_clean_scoped_graph_passes(self) -> None:
        members = crbs.resolved_member_features(TREE_OUTPUT)
        self.assertEqual(crbs.forbidden_resolutions(members, "x86_64-unknown-linux-musl"), [])

    def test_test_only_features_in_the_shipped_graph_are_flagged(self) -> None:
        """WHY(#6999): the pre-scoping release build resolved koina, mneme,
        eidos and organon with test-support on -- shipped binaries carried
        test-gated code the --features line never declared."""
        members = crbs.resolved_member_features(LEAKY_TREE_OUTPUT)
        violations = crbs.forbidden_resolutions(members, "x86_64-unknown-linux-musl")
        flagged = "\n".join(violations)
        self.assertIn("koina", flagged)
        self.assertIn("test-support", flagged)
        self.assertIn("crash-injection", flagged)
        self.assertIn("test-utils", flagged)

    def test_the_test_harness_crate_in_the_shipped_graph_is_flagged(self) -> None:
        members = crbs.resolved_member_features(LEAKY_TREE_OUTPUT)
        violations = crbs.forbidden_resolutions(members, "x86_64-unknown-linux-musl")
        self.assertTrue(any("integration-tests" in v for v in violations))

    def test_an_empty_member_map_is_a_violation(self) -> None:
        """WHY: an empty parse means the tree invocation or the parser broke;
        that must not read as a clean graph."""
        violations = crbs.forbidden_resolutions({}, "x86_64-unknown-linux-musl")
        self.assertEqual(len(violations), 1)
        self.assertIn("no workspace members", violations[0])


if __name__ == "__main__":
    unittest.main()
