from __future__ import annotations

import stat
import subprocess
import tempfile
import textwrap
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
SMOKE_TEST = REPO_ROOT / "scripts" / "smoke-test.sh"

FAKE_CLI = r"""#!/usr/bin/env bash
set -euo pipefail

case "$*" in
    "--version")
        echo "aletheia 0.0.0"
        ;;
    "--help")
        echo "Usage: aletheia <COMMAND>"
        echo "health backup maintenance tls status credential eval export tui"
        echo "migrate-memory init import seed-skills export-skills review-skills completions"
        ;;
    "health --help") echo "server" ;;
    "backup --help") echo "backup" ;;
    "maintenance --help") echo "maintenance" ;;
    "maintenance status --help"|"maintenance run --help") echo "ok" ;;
    "tls --help") echo "certificate" ;;
    "tls generate --help") echo "ok" ;;
    "status --help") echo "status" ;;
    "credential --help") echo "Credential" ;;
    "credential status --help"|"credential refresh --help") echo "ok" ;;
    "eval --help") echo "scenario" ;;
    "export --help") echo "agent" ;;
    "tui --help") echo "dashboard" ;;
    "migrate-memory --help") echo "Qdrant" ;;
    "init --help") echo "instance" ;;
    "import --help") echo "agent" ;;
    "seed-skills --help") echo "skill" ;;
    "export-skills --help") echo "export" ;;
    "review-skills --help") echo "review" ;;
    "completions --help") echo "shell" ;;
    "completions bash")
        echo "_aletheia() {"
        echo "    aletheia"
        for ((line = 0; line < 20000; line++)); do
            printf '    completion filler %05d\n' "$line"
        done
        echo "}"
        ;;
    "completions zsh"|"completions fish") echo "completion" ;;
    "health --url "*)
        echo "connection refused"
        exit 1
        ;;
    "status --url "*)
        echo "status unavailable"
        exit 1
        ;;
    "init --instance-root "*) echo "instance created" ;;
    "import "*)
        echo "file not found" >&2
        exit 1
        ;;
    "seed-skills "*) exit 1 ;;
    *) exit 2 ;;
esac
"""


class SmokeTestHarness(unittest.TestCase):
    """Exercise the release harness without building or running Aletheia."""

    def test_full_harness_handles_ere_alternatives_and_pipefail(self) -> None:
        """WHY: release run 32992452554 exposed two shell-specific false-negative
        shapes. Escaping `|` made 13 ERE alternations literal, while `grep -q`
        closed its pipe after an early match and made the large-output producer fail
        under `pipefail`. An expected exit 1 piped into grep had the same effect.
        The fake CLI chooses later alternatives, emits a large completion, and fails
        missing imports so any observed regression makes the complete harness fail."""
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / "aletheia"
            binary.write_text(textwrap.dedent(FAKE_CLI), encoding="utf-8")
            binary.chmod(binary.stat().st_mode | stat.S_IXUSR)

            result = subprocess.run(
                ["bash", str(SMOKE_TEST), "--binary", str(binary)],
                cwd=REPO_ROOT,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                check=False,
            )

        self.assertEqual(result.returncode, 0, result.stdout)
        self.assertIn("Results", result.stdout)
        self.assertIn("51 passed", result.stdout)
        self.assertNotIn("Failed tests:", result.stdout)


if __name__ == "__main__":
    unittest.main()
