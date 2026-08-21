#!/usr/bin/env python3
"""`RUST/primitive-for-domain-id` suppressions may only ever decrease.

WHY(#6755): the lint stops a domain identifier being carried as a bare `String`, where
nothing validates it and any string is representable. It is currently suppressed 200
times across 88 files -- and the count is the point, not the size.

A suppression is a claim that the lint is wrong *here*. **Nothing re-tests that claim**,
so its half-life is however long the surrounding code stays still, and when it goes
stale it does not fail loudly: it keeps a real defect looking deliberate. #4638 is the
proof. `NousDefinition.id` was exempted as "not a runtime domain identifier". It *was*
the runtime agent id, and because it was exempt, config load was the one surface of five
where the shared validator was never wired -- so an id with uppercase, a leading hyphen
or a path separator, written straight into `aletheia.toml`, bypassed validation and
spawned an actor.

WHY a ratchet rather than a sweep, and why this before converting anything: #6755 was
filed against **188** suppressions. There are now **200**. The population grew by twelve
while the issue sat, so converting twelve sites would have been treading water. Stopping
the growth is what makes the conversion work stick, and it is the half that needs no
judgement about any individual site.

WHY per-file and exact, in both directions:

  * A total-only budget lets one file absorb another's conversions invisibly.
  * A drop is a FAILURE too -- "the baseline is stale, lower it". A ratchet that
    tolerates being loose stops being a ratchet: the recorded number drifts above the
    real one, and the next addition lands inside the slack without anyone noticing.
"""

from __future__ import annotations

import json
import logging
import re
import subprocess
import sys
from pathlib import Path

LOGGER = logging.getLogger("check-domain-id-suppressions")

REPO_ROOT = Path(__file__).resolve().parents[1]
BASELINE = Path(__file__).resolve().parent / "domain-id-suppressions.json"

# `kanon:ignore RUST/primitive-for-domain-id` and `#[expect(...)]`-style spellings alike;
# the token is what identifies the suppression, wherever it is written.
SUPPRESSION = re.compile(r"primitive-for-domain-id")


def tracked_rust_files(repo_root: Path) -> list[Path]:
    """Rust sources git knows about.

    WHY tracked rather than a filesystem walk: `target/` and any scratch checkout under
    the tree would otherwise be counted, and the number would move for reasons that have
    nothing to do with the code.
    """
    result = subprocess.run(
        ["git", "ls-files", "*.rs"],
        capture_output=True, text=True, check=False, cwd=repo_root,
    )
    if result.returncode != 0:
        raise SystemExit(f"git ls-files failed: {result.stderr.strip()}")
    return [repo_root / line for line in result.stdout.splitlines() if line]


def counts(repo_root: Path) -> dict[str, int]:
    """{path: number of suppressions} for every file carrying at least one."""
    found: dict[str, int] = {}
    for path in tracked_rust_files(repo_root):
        try:
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeError):
            continue
        n = len(SUPPRESSION.findall(text))
        if n:
            found[path.relative_to(repo_root).as_posix()] = n
    return found


def load_baseline(path: Path) -> dict[str, int]:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, ValueError) as error:
        raise SystemExit(f"cannot read the baseline {path.name}: {error}") from error
    files = data.get("files")
    if not isinstance(files, dict) or not files:
        raise SystemExit(
            f"{path.name} declares no files; an empty baseline would accept any "
            "number of new suppressions"
        )
    return {str(k): int(v) for k, v in files.items()}


def compare(actual: dict[str, int], baseline: dict[str, int]) -> tuple[list[str], list[str]]:
    """Return (regressions, stale entries)."""
    regressions = []
    stale = []
    for path, n in sorted(actual.items()):
        allowed = baseline.get(path)
        if allowed is None:
            regressions.append(f"  {path}: {n} new (this file had none)")
        elif n > allowed:
            regressions.append(f"  {path}: {n}, baseline {allowed}")
    for path, allowed in sorted(baseline.items()):
        n = actual.get(path, 0)
        if n < allowed:
            stale.append(f"  {path}: {n} now, baseline still says {allowed}")
    return regressions, stale


def main() -> int:
    actual = counts(REPO_ROOT)
    baseline = load_baseline(BASELINE)
    regressions, stale = compare(actual, baseline)

    total, allowed_total = sum(actual.values()), sum(baseline.values())

    if regressions:
        LOGGER.error(
            "check-domain-id-suppressions: a domain identifier is being carried as a "
            "bare String in a place that was not exempt before."
        )
        for line in regressions:
            LOGGER.error("%s", line)
        LOGGER.error("")
        LOGGER.error("Use a validated newtype -- `koina::id::NousId` is the worked")
        LOGGER.error("example, and it carries `#[serde(try_from, into)]` so it")
        LOGGER.error("round-trips through JSON and TOML byte-identically to a String.")
        LOGGER.error("")
        LOGGER.error("If the field genuinely is not a domain identifier, say which")
        LOGGER.error("PROPERTY makes that true, and raise the baseline deliberately.")
        LOGGER.error("A reason phrased as a migration cost is one nobody can falsify,")
        LOGGER.error("and #4638 is what a stale exemption costs: an unvalidated agent")
        LOGGER.error("id reached actor spawn for a full release cycle.")
        return 1

    if stale:
        LOGGER.error(
            "check-domain-id-suppressions: the baseline is looser than the tree. "
            "Lower it, or the next addition lands inside the slack unnoticed."
        )
        for line in stale:
            LOGGER.error("%s", line)
        LOGGER.error("")
        LOGGER.error("Run: scripts/check-domain-id-suppressions.py --write-baseline")
        return 1

    LOGGER.info(
        "check-domain-id-suppressions: %d suppression(s) across %d file(s), at or "
        "below the baseline of %d",
        total, len(actual), allowed_total,
    )
    return 0


def write_baseline() -> int:
    actual = counts(REPO_ROOT)
    BASELINE.write_text(
        json.dumps(
            {
                "_why": (
                    "Per-file ceiling for RUST/primitive-for-domain-id suppressions "
                    "(#6755). This number may only go DOWN. Regenerate with "
                    "scripts/check-domain-id-suppressions.py --write-baseline after "
                    "converting sites to a validated newtype."
                ),
                "files": dict(sorted(actual.items())),
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    LOGGER.info(
        "wrote %s: %d suppression(s) across %d file(s)",
        BASELINE.name, sum(actual.values()), len(actual),
    )
    return 0


if __name__ == "__main__":
    logging.basicConfig(format="%(message)s", level=logging.INFO, stream=sys.stderr)
    if "--write-baseline" in sys.argv[1:]:
        raise SystemExit(write_baseline())
    raise SystemExit(main())
