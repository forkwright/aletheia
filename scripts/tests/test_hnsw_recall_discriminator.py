from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path

SCRIPT_PATH = Path(__file__).resolve().parents[1] / "hnsw-recall-discriminator.py"
SPEC = importlib.util.spec_from_file_location("hnsw_recall_discriminator", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {SCRIPT_PATH}")
hrd = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = hrd
SPEC.loader.exec_module(hrd)

TARGET = hrd.TARGET_TEST

NEXTEST_PASS = f"""\
  Starting 1 tests across 1 binaries
        PASS [   0.006s] krites runtime::hnsw::close_reopen_tests::{TARGET}
hnsw-recall: phase=post-reopen avg=0.9333
hnsw-recall: phase=post-delete-reopen avg=0.8667
"""

NEXTEST_FAIL = f"""\
        FAIL [   0.006s] krites runtime::hnsw::close_reopen_tests::{TARGET}
hnsw-recall: phase=post-reopen avg=0.9100
hnsw-recall: phase=post-delete-reopen avg=0.0000
"""

CARGO_PASS = f"""\
test runtime::hnsw::close_reopen_tests::{TARGET} ... ok
hnsw-recall: phase=post-reopen avg=0.9333
hnsw-recall: phase=post-delete-reopen avg=0.4000
"""

CARGO_FAIL = f"""\
test runtime::hnsw::close_reopen_tests::{TARGET} ... FAILED
hnsw-recall: phase=post-reopen avg=0.9333
"""


class BuildCommand(unittest.TestCase):
    def test_nextest_serial_is_single_test_single_threaded(self) -> None:
        cmd = hrd.build_command("nextest", "serial", "module")
        self.assertIn("--test-threads=1", cmd)
        self.assertTrue(any(TARGET in part for part in cmd))
        self.assertIn("test-core,krites_sovereign_hnsw", cmd)

    def test_nextest_concurrent_keeps_default_parallelism(self) -> None:
        cmd = hrd.build_command("nextest", "concurrent", "module")
        self.assertNotIn("--test-threads=1", cmd)
        self.assertTrue(any("runtime::hnsw::" in part for part in cmd))

    def test_nextest_never_uses_no_capture(self) -> None:
        # WHY: nextest's --no-capture silently forces serial execution, which
        # would destroy the concurrent leg's independent variable. The whole
        # discriminator is void if that flag ever appears.
        for leg in hrd.LEGS:
            for scope in ("module", "package"):
                cmd = hrd.build_command("nextest", leg, scope)
                self.assertNotIn("--no-capture", cmd)

    def test_cargo_serial_and_concurrent(self) -> None:
        serial = hrd.build_command("cargo", "serial", "module")
        self.assertIn("--test-threads=1", serial)
        self.assertIn("--nocapture", serial)
        concurrent = hrd.build_command("cargo", "concurrent", "module")
        self.assertNotIn("--test-threads=1", concurrent)
        self.assertIn("runtime::hnsw::", concurrent)

    def test_package_scope_widens_the_concurrent_filter(self) -> None:
        self.assertIn("all()", hrd.build_command("nextest", "concurrent", "package"))
        cargo = hrd.build_command("cargo", "concurrent", "package")
        self.assertNotIn("runtime::hnsw::", cargo)
        self.assertNotIn(TARGET, cargo)


class ParseRun(unittest.TestCase):
    def test_nextest_pass_with_both_markers(self) -> None:
        outcome = hrd.parse_run(NEXTEST_PASS, "nextest")
        self.assertEqual(outcome["status"], "pass")
        self.assertEqual(outcome["post_reopen_avg"], 0.9333)
        self.assertEqual(outcome["post_delete_avg"], 0.8667)

    def test_nextest_fail_with_exact_zero(self) -> None:
        outcome = hrd.parse_run(NEXTEST_FAIL, "nextest")
        self.assertEqual(outcome["status"], "fail")
        self.assertEqual(outcome["post_delete_avg"], 0.0)

    def test_cargo_pass(self) -> None:
        outcome = hrd.parse_run(CARGO_PASS, "cargo")
        self.assertEqual(outcome["status"], "pass")
        self.assertEqual(outcome["post_delete_avg"], 0.4)

    def test_cargo_fail_before_second_phase_leaves_marker_missing(self) -> None:
        outcome = hrd.parse_run(CARGO_FAIL, "cargo")
        self.assertEqual(outcome["status"], "fail")
        self.assertIsNone(outcome["post_delete_avg"])

    def test_no_status_line_is_unknown_not_pass(self) -> None:
        outcome = hrd.parse_run("compiling...\nnothing matched\n", "nextest")
        self.assertEqual(outcome["status"], "unknown")


class PhaseStats(unittest.TestCase):
    def test_distribution_buckets(self) -> None:
        stats = hrd.phase_stats([0.9, 0.0, 0.04, None, 0.9])
        self.assertEqual(stats["samples"], 4)
        self.assertEqual(stats["missing"], 1)
        self.assertEqual(stats["exact_zero"], 1)
        self.assertEqual(stats["sub_floor_nonzero"], 1)
        self.assertEqual(stats["min"], 0.0)
        self.assertEqual(stats["max"], 0.9)

    def test_all_missing_is_empty_not_crash(self) -> None:
        stats = hrd.phase_stats([None, None])
        self.assertEqual(stats["samples"], 0)
        self.assertIsNone(stats["min"])


class Classify(unittest.TestCase):
    def _summary(self, fails: int) -> dict:
        return {"runs": 10, "target_pass": 10 - fails, "target_fail": fails, "target_unknown": 0}

    def test_no_failures(self) -> None:
        self.assertIn("no failure reproduced", hrd.classify(self._summary(0), self._summary(0)))

    def test_serial_only_is_persistence_shape(self) -> None:
        self.assertIn("persistence-defect shape", hrd.classify(self._summary(2), self._summary(0)))

    def test_concurrent_only_is_race_shape(self) -> None:
        self.assertIn("fixture-race shape", hrd.classify(self._summary(0), self._summary(3)))

    def test_both_legs_is_concurrency_independent(self) -> None:
        self.assertIn("BOTH legs", hrd.classify(self._summary(1), self._summary(4)))


class RenderMarkdown(unittest.TestCase):
    def test_summary_renders_table_and_reading(self) -> None:
        runs = [hrd.parse_run(NEXTEST_PASS, "nextest"), hrd.parse_run(NEXTEST_FAIL, "nextest")]
        report = {
            "runner": "nextest",
            "features": hrd.FEATURES,
            "target_test": TARGET,
            "runs_per_leg": 2,
            "legs": {
                "serial": {"runs": runs, "summary": hrd.summarize(runs)},
                "concurrent": {"runs": runs, "summary": hrd.summarize(runs)},
            },
        }
        report["reading"] = hrd.classify(report["legs"]["serial"]["summary"], report["legs"]["concurrent"]["summary"])
        md = hrd.render_markdown(report)
        self.assertIn("#6952", md)
        self.assertIn("post-delete-reopen", md)
        self.assertIn("exact 0.00", md)
        self.assertIn("BOTH legs", md)


class ResolveOutDir(unittest.TestCase):
    def test_accepts_repo_relative_path(self) -> None:
        out = hrd.resolve_out_dir("target/hnsw-recall-discriminator")
        self.assertEqual(out, hrd.REPO_ROOT / "target" / "hnsw-recall-discriminator")

    def test_rejects_dotdot_escape(self) -> None:
        with self.assertRaises(SystemExit):
            hrd.resolve_out_dir("../outside")

    def test_rejects_absolute_path(self) -> None:
        with self.assertRaises(SystemExit):
            hrd.resolve_out_dir("/tmp/outside")

    def test_rejects_repo_root_itself(self) -> None:
        with self.assertRaises(SystemExit):
            hrd.resolve_out_dir(".")


if __name__ == "__main__":
    unittest.main()
