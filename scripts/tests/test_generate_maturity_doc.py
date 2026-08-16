from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

SCRIPT_PATH = Path(__file__).resolve().parents[1] / "generate-maturity-doc.py"
SPEC = importlib.util.spec_from_file_location("generate_maturity_doc", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {SCRIPT_PATH}")
gmd = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = gmd
SPEC.loader.exec_module(gmd)


class SyntheticWorkspaceTestCase(unittest.TestCase):
    """WHY: a fresh REPO_ROOT/CRATE_INDEX_PATH/DOC_PATH per test avoids
    reading this repo's real 48-crate workspace and keeps fixtures exact."""

    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.repo_root = Path(tmp.name)

        for name, value in (
            ("REPO_ROOT", self.repo_root),
            ("CRATE_INDEX_PATH", self.repo_root / "CRATE-INDEX.toml"),
            ("DOC_PATH", self.repo_root / "docs" / "MATURITY.md"),
        ):
            patcher = mock.patch.object(gmd, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)

    def write_crate(
        self, name: str, path: str, kanon_block: str | None = None
    ) -> None:
        crate_dir = self.repo_root / path
        crate_dir.mkdir(parents=True, exist_ok=True)
        body = f'[package]\nname = "{name}"\nversion = "0.1.0"\n'
        if kanon_block:
            body += f"\n[package.metadata.kanon]\n{kanon_block}\n"
        (crate_dir / "Cargo.toml").write_text(body, encoding="utf-8")

    def write_crate_index(self, entries: dict[str, str]) -> None:
        lines = []
        for name, path in entries.items():
            lines.append(f"[crates.{name}]")
            lines.append(f'path = "{path}"')
        (self.repo_root / "CRATE-INDEX.toml").write_text(
            "\n".join(lines) + "\n", encoding="utf-8"
        )

    def write_doc(self, extra: str = "") -> None:
        doc_path = self.repo_root / "docs" / "MATURITY.md"
        doc_path.parent.mkdir(parents=True, exist_ok=True)
        doc_path.write_text(
            "# Feature maturity matrix\n\n"
            f"{gmd.BEGIN_MARKER}\n\n{gmd.END_MARKER}\n\n"
            f"{extra}",
            encoding="utf-8",
        )

    def test_declared_maturity_normalizes_to_display_vocabulary(self) -> None:
        self.write_crate(
            "foo",
            "crates/foo",
            'maturity = "production"\nsince = "2026-01-01"\nexit-criteria = "n/a"\n',
        )
        self.write_crate_index({"foo": "crates/foo"})
        rows = gmd.crate_rows()
        self.assertEqual(rows, [("foo", "crates/foo", "Stable", "2026-01-01", "n/a")])

    def test_undeclared_crate_renders_undeclared_not_stable(self) -> None:
        self.write_crate("bar", "crates/bar")
        self.write_crate_index({"bar": "crates/bar"})
        rows = gmd.crate_rows()
        self.assertEqual(rows, [("bar", "crates/bar", "Undeclared", "—", "—")])

    def test_check_passes_when_doc_matches(self) -> None:
        self.write_crate("foo", "crates/foo", 'maturity = "alpha"\n')
        self.write_crate_index({"foo": "crates/foo"})
        self.write_doc()
        gmd.apply(check=False)
        self.assertEqual(gmd.apply(check=True), 0)

    def test_check_fails_when_doc_is_stale(self) -> None:
        self.write_crate("foo", "crates/foo", 'maturity = "alpha"\n')
        self.write_crate_index({"foo": "crates/foo"})
        self.write_doc()
        # Never regenerated -- the block is still empty, so --check must fail.
        self.assertEqual(gmd.apply(check=True), 1)

    def test_apply_preserves_prose_outside_markers(self) -> None:
        self.write_crate("foo", "crates/foo")
        self.write_crate_index({"foo": "crates/foo"})
        self.write_doc(extra="## Known gaps\n\nhand-authored prose\n")
        gmd.apply(check=False)
        doc = gmd.DOC_PATH.read_text(encoding="utf-8")
        self.assertIn("## Known gaps", doc)
        self.assertIn("hand-authored prose", doc)
        self.assertIn("`foo`", doc)

    def test_missing_markers_fails_loudly(self) -> None:
        self.write_crate("foo", "crates/foo")
        self.write_crate_index({"foo": "crates/foo"})
        doc_path = self.repo_root / "docs" / "MATURITY.md"
        doc_path.parent.mkdir(parents=True, exist_ok=True)
        doc_path.write_text("# no markers here\n", encoding="utf-8")
        self.assertEqual(gmd.apply(check=False), 1)


if __name__ == "__main__":
    unittest.main()
