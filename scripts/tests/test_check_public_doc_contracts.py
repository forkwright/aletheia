from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path

SCRIPT_PATH = Path(__file__).resolve().parents[1] / "check-public-doc-contracts.py"
SPEC = importlib.util.spec_from_file_location("check_public_doc_contracts", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {SCRIPT_PATH}")
dc = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = dc
SPEC.loader.exec_module(dc)


class LocalPathPattern(unittest.TestCase):
    def hits(self, line: str) -> list[str]:
        return [m.group(0) for m in dc.LOCAL_PATH.finditer(line)]

    def test_a_real_maintainer_path_is_caught(self) -> None:
        self.assertTrue(self.hits("cp /home/ck/aletheia/bin/aletheia ."))

    def test_the_fleet_data_mount_is_caught(self) -> None:
        """WHY `/data` at all: it is a real bulk-storage mount on this fleet, so an
        absolute `/data/...` in a public snippet is a maintainer path, not an example."""
        self.assertTrue(self.hits('BACKUP="/data/backups/instance/x"'))

    def test_both_spellings_of_an_expansion_agree(self) -> None:
        """WHY this is the case worth pinning: `${VAR}/data/x` and `$VAR/data/x` are the
        same path. Only the braced one was flagged, because `$VAR` ends in a word
        character and was already excluded by the lookbehind. A rule whose verdict turns
        on which spelling an author chose is arbitrary, and its failure reads as a
        maintainer path leaking rather than as the rule mis-firing."""
        self.assertEqual(self.hits('BACKUP="$ALETHEIA_ROOT/data/backups/x"'), [])
        self.assertEqual(self.hits('BACKUP="${ALETHEIA_ROOT}/data/backups/x"'), [])

    def test_a_path_relative_to_an_expansion_is_still_caught_when_absolute(self) -> None:
        """The rule must not have been loosened into uselessness: an absolute path in
        the same line is still a finding."""
        self.assertTrue(self.hits('cp "${SRC}" /data/backups/x'))

    def test_a_placeholder_user_is_allowed(self) -> None:
        allowed = next(iter(dc.PLACEHOLDER_USERS))
        self.assertTrue(
            all(
                m.group("user") is None or m.group("user").strip("{}$<>").lower() in dc.PLACEHOLDER_USERS
                for m in dc.LOCAL_PATH.finditer(f"/home/{allowed}/aletheia")
            )
        )


class ThisRepository(unittest.TestCase):
    def test_the_public_docs_are_clean(self) -> None:
        """WHY bound to the repo: the pattern tests above are generic, and a rule that
        stopped matching anything would pass every one of them."""
        self.assertEqual(dc.main(), 0)


if __name__ == "__main__":
    unittest.main()
