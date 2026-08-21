from __future__ import annotations

import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT_PATH = Path(__file__).resolve().parents[1] / "check-domain-id-suppressions.py"
SPEC = importlib.util.spec_from_file_location("check_domain_id_suppressions", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {SCRIPT_PATH}")
ds = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = ds
SPEC.loader.exec_module(ds)

MARKER = "kanon:ignore RUST/primitive-for-domain-id"


class Counting(unittest.TestCase):
    def _repo(self, tmp: str, files: dict[str, str]) -> Path:
        root = Path(tmp)
        subprocess.run(["git", "init", "-q"], cwd=root, check=True)
        for name, body in files.items():
            path = root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(body, encoding="utf-8")
        subprocess.run(["git", "add", "-A"], cwd=root, check=True)
        return root

    def test_suppressions_are_counted_per_file(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = self._repo(tmp, {
                "a.rs": f"// {MARKER} -- one\n// {MARKER} -- two\n",
                "b.rs": f"// {MARKER} -- one\n",
                "clean.rs": "fn main() {}\n",
            })
            self.assertEqual(ds.counts(root), {"a.rs": 2, "b.rs": 1})

    def test_untracked_files_are_not_counted(self) -> None:
        """WHY tracked-only: a `target/` directory or a scratch checkout under the tree
        would otherwise move the number for reasons unrelated to the code, and the
        ratchet would fire on something nobody wrote."""
        with tempfile.TemporaryDirectory() as tmp:
            root = self._repo(tmp, {"a.rs": f"// {MARKER}\n"})
            (root / "scratch.rs").write_text(f"// {MARKER}\n", encoding="utf-8")
            self.assertEqual(ds.counts(root), {"a.rs": 1})


class Ratchet(unittest.TestCase):
    """The whole point: this number may only go DOWN, and it must be exact."""

    def test_an_increase_in_an_exempt_file_is_a_regression(self) -> None:
        regressions, stale = ds.compare({"a.rs": 4}, {"a.rs": 3})
        self.assertTrue(regressions)
        self.assertFalse(stale)
        self.assertIn("baseline 3", regressions[0])

    def test_a_first_suppression_in_a_new_file_is_a_regression(self) -> None:
        regressions, _ = ds.compare({"new.rs": 1}, {"a.rs": 3})
        self.assertTrue(regressions)
        self.assertIn("this file had none", regressions[0])

    def test_holding_steady_passes(self) -> None:
        regressions, stale = ds.compare({"a.rs": 3}, {"a.rs": 3})
        self.assertEqual((regressions, stale), ([], []))

    def test_a_DECREASE_also_fails_because_the_baseline_is_now_stale(self) -> None:
        """WHY a drop is an error and not a quiet pass: a ratchet that tolerates being
        loose stops being a ratchet. The recorded ceiling drifts above the real count,
        and the next addition lands inside that slack with nothing firing."""
        regressions, stale = ds.compare({"a.rs": 1}, {"a.rs": 3})
        self.assertFalse(regressions)
        self.assertTrue(stale)
        self.assertIn("baseline still says 3", stale[0])

    def test_a_file_emptied_entirely_is_still_stale(self) -> None:
        _, stale = ds.compare({}, {"a.rs": 3})
        self.assertTrue(stale)

    def test_one_file_cannot_absorb_another_file_s_conversions(self) -> None:
        """WHY per-file rather than a single total: with a total-only budget, deleting
        three suppressions here and adding three there nets to zero and the ratchet
        never fires -- which is exactly how the population grew from 188 to 200 while
        the issue describing it sat open."""
        regressions, stale = ds.compare({"a.rs": 0 + 6, "b.rs": 0}, {"a.rs": 3, "b.rs": 3})
        self.assertTrue(regressions, "the increase in a.rs must be reported")
        self.assertTrue(stale, "and the drop in b.rs must not silently pay for it")


class BaselineFile(unittest.TestCase):
    def test_an_empty_baseline_is_refused(self) -> None:
        """WHY loud: an empty baseline accepts any number of new suppressions while
        reporting a clean result -- a green that means nothing was checked."""
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "b.json"
            path.write_text('{"files": {}}', encoding="utf-8")
            with self.assertRaises(SystemExit):
                ds.load_baseline(path)

    def test_a_missing_baseline_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaises(SystemExit):
                ds.load_baseline(Path(tmp) / "absent.json")


class ThisRepository(unittest.TestCase):
    def test_the_live_tree_is_at_or_below_its_baseline(self) -> None:
        """WHY bound to the repo: the tests above are generic, and a reader that had
        stopped matching the token would pass every one of them while reporting zero."""
        actual = ds.counts(ds.REPO_ROOT)
        self.assertTrue(actual, "the marker must still match; zero means a broken reader")
        regressions, stale = ds.compare(actual, ds.load_baseline(ds.BASELINE))
        self.assertEqual(regressions, [])
        self.assertEqual(stale, [])


if __name__ == "__main__":
    unittest.main()
