from __future__ import annotations

import copy
import importlib.util
import sys
import unittest
from pathlib import Path

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


def expected_packages() -> dict[Path, scope.ExpectedLocalPackage]:
    return {
        Path("/workspace/crates/aletheia"): scope.ExpectedLocalPackage(
            "aletheia", frozenset({"default", "recall", "embed-candle"})
        ),
        Path("/workspace/crates/koina"): scope.ExpectedLocalPackage(
            "koina", frozenset({"default", "rustls-provider", "test-support"})
        ),
    }


class ReleaseBuildValidation(unittest.TestCase):
    def assert_rejected(self, candidate: dict) -> None:
        with self.assertRaises(scope.ScopeCheckError):
            scope.validated_release_builds(candidate)

    def test_current_job_is_accepted_and_derives_replay_args(self) -> None:
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


class ResolutionParsing(unittest.TestCase):
    def parse(self, output: str) -> dict[str, set[str]]:
        return scope.resolved_member_features(output, expected_packages())

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
        report = "\n".join(scope.forbidden_resolutions(self.parse(output), "test"))
        self.assertIn("test-support", report)
        unexpected = output.replace("test-support", "made-up")
        with self.assertRaises(scope.ScopeCheckError):
            self.parse(unexpected)

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


if __name__ == "__main__":
    unittest.main()
