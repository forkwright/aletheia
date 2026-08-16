from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

SCRIPT_PATH = Path(__file__).resolve().parents[1] / "check-metrics-doc.py"
SPEC = importlib.util.spec_from_file_location("check_metrics_doc", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {SCRIPT_PATH}")
cmd = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = cmd
SPEC.loader.exec_module(cmd)

FRAGMENT_TEST_SNIPPET = """
    #[test]
    fn register_exposes_all_metric_families() {
        let out = encode(&r);
        for fragment in [
            "aletheia_llm_tokens_total",
            "aletheia_llm_cost_usd_total",
            "aletheia_llm_request_duration_seconds",
        ] {
            assert!(out.contains(fragment));
        }
    }
"""


class GroundTruthExtractionTestCase(unittest.TestCase):
    """WHY: a fresh REPO_ROOT/METRICS_RS per test avoids reading this repo's
    real multi-hundred-line metrics.rs and keeps the fragment-list fixture
    exact and minimal."""

    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.repo_root = Path(tmp.name)
        (self.repo_root / "crates" / "hermeneus" / "src").mkdir(parents=True)
        self.metrics_rs = self.repo_root / "crates" / "hermeneus" / "src" / "metrics.rs"

        for name, value in (
            ("REPO_ROOT", self.repo_root),
            ("METRICS_RS", self.metrics_rs),
        ):
            patcher = mock.patch.object(cmd, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)

    def write_metrics_rs(self, body: str = FRAGMENT_TEST_SNIPPET) -> None:
        self.metrics_rs.write_text(body, encoding="utf-8")

    def write_surface(self, name: str, content: str) -> Path:
        path = self.repo_root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
        return path

    def test_extracts_fragment_list_from_registered_test(self) -> None:
        self.write_metrics_rs()
        self.assertEqual(
            cmd.ground_truth_names(),
            {
                "aletheia_llm_tokens_total",
                "aletheia_llm_cost_usd_total",
                "aletheia_llm_request_duration_seconds",
            },
        )

    def test_missing_fragment_function_raises(self) -> None:
        self.write_metrics_rs("// no test here")
        with self.assertRaises(RuntimeError):
            cmd.ground_truth_names()

    def test_histogram_suffix_is_known(self) -> None:
        ground_truth = {"aletheia_llm_request_duration_seconds"}
        self.assertTrue(
            cmd.is_known("aletheia_llm_request_duration_seconds_bucket", ground_truth)
        )
        self.assertTrue(
            cmd.is_known("aletheia_llm_request_duration_seconds_count", ground_truth)
        )

    def test_unknown_token_is_not_known(self) -> None:
        ground_truth = {"aletheia_llm_tokens_total"}
        self.assertFalse(cmd.is_known("aletheia_llm_cost_total", ground_truth))


class DriftDetectionTestCase(GroundTruthExtractionTestCase):
    def test_clean_surface_produces_no_drift(self) -> None:
        self.write_metrics_rs()
        doc = self.write_surface("docs/RUNBOOK.md", "See `aletheia_llm_tokens_total`.\n")
        with mock.patch.object(cmd, "WATCHED_SURFACES", [doc]):
            problems = cmd.find_drift(cmd.ground_truth_names())
        self.assertEqual(problems, [])

    def test_stale_metric_name_is_flagged_with_file_and_line(self) -> None:
        self.write_metrics_rs()
        doc = self.write_surface(
            "docs/RUNBOOK.md",
            "line one\nSee `aletheia_llm_cost_total{provider=...}`.\n",
        )
        with mock.patch.object(cmd, "WATCHED_SURFACES", [doc]):
            problems = cmd.find_drift(cmd.ground_truth_names())
        self.assertEqual(len(problems), 1)
        self.assertIn("RUNBOOK.md:2", problems[0])
        self.assertIn("aletheia_llm_cost_total", problems[0])

    def test_missing_surface_is_flagged(self) -> None:
        self.write_metrics_rs()
        missing = self.repo_root / "docs" / "GONE.md"
        with mock.patch.object(cmd, "WATCHED_SURFACES", [missing]):
            problems = cmd.find_drift(cmd.ground_truth_names())
        self.assertEqual(len(problems), 1)
        self.assertIn("no longer exists", problems[0])


if __name__ == "__main__":
    unittest.main()
