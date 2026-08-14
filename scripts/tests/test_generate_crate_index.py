from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

SCRIPT_PATH = Path(__file__).resolve().parents[1] / "generate-crate-index.py"
SPEC = importlib.util.spec_from_file_location("generate_crate_index", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {SCRIPT_PATH}")
gci = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = gci
SPEC.loader.exec_module(gci)


class SyntheticWorkspaceTestCase(unittest.TestCase):
    """Builds a small synthetic Cargo workspace under a fresh tmp REPO_ROOT so
    each test exercises `derive_graph()` against manifests it fully controls,
    rather than the real multi-crate aletheia tree this repo runs inside."""

    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.repo_root = Path(tmp.name)

        for name, value in (
            ("REPO_ROOT", self.repo_root),
            ("ROOT_MANIFEST", self.repo_root / "Cargo.toml"),
            ("INDEX_PATH", self.repo_root / "CRATE-INDEX.toml"),
        ):
            patcher = mock.patch.object(gci, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)

    def write_root_manifest(self, member_dirs: list[str]) -> None:
        members = ", ".join(f'"{m}"' for m in member_dirs)
        (self.repo_root / "Cargo.toml").write_text(
            f"[workspace]\nmembers = [{members}]\n", encoding="utf-8"
        )

    def write_crate(
        self,
        dir_name: str,
        package_name: str,
        deps: list[str] | None = None,
        dev_deps: list[str] | None = None,
    ) -> None:
        crate_dir = self.repo_root / "crates" / dir_name
        crate_dir.mkdir(parents=True)
        lines = [f'[package]\nname = "{package_name}"\nversion = "0.1.0"\n']
        if deps:
            lines.append("[dependencies]")
            for dep in deps:
                lines.append(f'{dep} = {{ path = "../{dep}" }}')
            lines.append("")
        if dev_deps:
            lines.append("[dev-dependencies]")
            for dep in dev_deps:
                # WHY a distinct feature per entry: mirrors the real manifests
                # this generator reads (a dev-only pin usually carries extra
                # test-only features), and proves the deriver reads `path`,
                # not the whole table shape.
                lines.append(f'{dep} = {{ path = "../{dep}", features = ["test-support"] }}')
            lines.append("")
        (crate_dir / "Cargo.toml").write_text("\n".join(lines), encoding="utf-8")


# ── bug class 1: directory name != package name ────────────────────────────


class DirectoryNameMismatchTests(SyntheticWorkspaceTestCase):
    """crates/daemon builds as `oikonomos` in the real workspace -- a deriver
    that trusts the directory segment instead of `[package].name` would
    resolve `path = "../daemon"` to a phantom crate `daemon` that never
    appears in the index, and silently drop the real edge."""

    def test_dependency_resolves_by_package_name_not_directory(self) -> None:
        self.write_crate("daemon", "oikonomos")
        self.write_crate("consumer", "consumer", deps=["daemon"])
        self.write_root_manifest(["crates/daemon", "crates/consumer"])

        graph = gci.derive_graph()

        self.assertEqual(graph["consumer"]["depends_on"], ["oikonomos"])
        self.assertEqual(graph["oikonomos"]["used_by"], ["consumer"])


# ── bug class 2: dev-dependency and dependency confusion ───────────────────


class ProdDevOverlapTests(SyntheticWorkspaceTestCase):
    """A crate reachable through BOTH [dependencies] and [dev-dependencies]
    (a normal dep that also carries extra test-only features in the dev
    table) is a real prod edge -- dev_depends_on must name only the deps
    that are reachable ONLY in a dev/test build, or an agent computing
    blast radius double-counts a real consumer as a phantom dev-only one."""

    def test_dep_in_both_tables_is_prod_only_not_double_listed(self) -> None:
        self.write_crate("base", "base")
        self.write_crate("mid", "mid", deps=["base"], dev_deps=["base"])
        self.write_root_manifest(["crates/base", "crates/mid"])

        graph = gci.derive_graph()

        self.assertEqual(graph["mid"]["depends_on"], ["base"])
        self.assertEqual(graph["mid"]["dev_depends_on"], [])

    def test_dev_only_dep_does_not_inflate_used_by(self) -> None:
        """The exact failure mode aletheia#5574 named: a dep declared ONLY
        under [dev-dependencies] must not make the target's `used_by` claim
        a prod consumer that does not exist -- that is what makes an agent
        computing change blast-radius chase a phantom dependent."""
        self.write_crate("base", "base")
        self.write_crate("mid", "mid", dev_deps=["base"])
        self.write_root_manifest(["crates/base", "crates/mid"])

        graph = gci.derive_graph()

        self.assertEqual(graph["mid"]["dev_depends_on"], ["base"])
        self.assertEqual(graph["base"]["used_by"], [])


# ── bug class 3: self-referential dev-dependency ────────────────────────────


class SelfDevDependencyTests(SyntheticWorkspaceTestCase):
    """`path = "."` under [dev-dependencies] is a real, common Cargo idiom
    (crates/taxis/Cargo.toml does this to reach its own `test-support`
    feature from its own test binaries) -- it must be preserved, while a
    self-edge in [dependencies] (which Cargo itself would refuse to
    resolve as a real build graph) must never be fabricated."""

    def test_self_dev_dependency_is_preserved(self) -> None:
        self.write_crate("solo", "solo", dev_deps=["solo"])
        self.write_root_manifest(["crates/solo"])

        graph = gci.derive_graph()

        self.assertEqual(graph["solo"]["dev_depends_on"], ["solo"])
        self.assertEqual(graph["solo"]["depends_on"], [])


# ── rewrite(): surgical, byte-exact outside the three structural fields ────


class RewriteTests(SyntheticWorkspaceTestCase):
    def test_rewrite_preserves_hand_authored_fields(self) -> None:
        self.write_crate("base", "base")
        self.write_crate("mid", "mid", deps=["base"])
        self.write_root_manifest(["crates/base", "crates/mid"])
        graph = gci.derive_graph()

        index_text = (
            "[crates.mid]\n"
            'path = "crates/mid"\n'
            'layer = "tool"\n'
            'purpose = "hand-authored prose that must survive verbatim"\n'
            "depends_on = []\n"
            "used_by = []\n"
            "dev_depends_on = []\n"
            "\n"
            "[crates.mid.features]\n"
            'default = "not a dependency-graph field"\n'
            "\n"
            "[crates.base]\n"
            'path = "crates/base"\n'
            'layer = "types"\n'
            'purpose = "leaf crate"\n'
            "depends_on = []\n"
            "used_by = []\n"
            "dev_depends_on = []\n"
        )

        new_text, problems = gci.rewrite(index_text, graph)

        self.assertEqual(problems, [])
        self.assertIn('purpose = "hand-authored prose that must survive verbatim"', new_text)
        self.assertIn('default = "not a dependency-graph field"', new_text)
        self.assertIn('depends_on = ["base"]', new_text)  # mid's corrected field
        self.assertIn('used_by = ["mid"]', new_text)  # base's corrected field

    def test_rewrite_is_idempotent(self) -> None:
        self.write_crate("base", "base")
        self.write_crate("mid", "mid", deps=["base"])
        self.write_root_manifest(["crates/base", "crates/mid"])
        graph = gci.derive_graph()

        stale_text = (
            "[crates.mid]\n"
            'path = "crates/mid"\n'
            "depends_on = []\n"
            "used_by = []\n"
            "dev_depends_on = []\n"
            "\n"
            "[crates.base]\n"
            'path = "crates/base"\n'
            "depends_on = []\n"
            "used_by = []\n"
            "dev_depends_on = []\n"
        )

        once, _ = gci.rewrite(stale_text, graph)
        twice, _ = gci.rewrite(once, graph)

        self.assertEqual(once, twice)

    def test_rewrite_flags_workspace_member_missing_from_index(self) -> None:
        self.write_crate("base", "base")
        self.write_root_manifest(["crates/base"])
        graph = gci.derive_graph()

        new_text, problems = gci.rewrite("", graph)

        self.assertTrue(any("absent from CRATE-INDEX.toml" in p for p in problems))


if __name__ == "__main__":
    unittest.main()
