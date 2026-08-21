#!/usr/bin/env python3
"""Every outbound HTTP request must route through the egress checkpoint, or say why not.

WHY(#6916): `send_with_safe_redirects` puts each hop of a request through the same
egress policy and internal-address revalidation. It is correct, it is tested, and
nothing verified that a given call site actually reached it. Three sites did not:

  #6910  web_search sent the Brave subscription token on the redirect-following
         `general` client. reqwest strips only Authorization, Cookie,
         Proxy-Authorization and WWW-Authenticate across a cross-host redirect, so a
         custom token header rode along to whatever host a response named.
  #6921  AcademicSource did the same with the Semantic Scholar `x-api-key`, and is
         now routed -- which is why it is absent from every dict below rather than
         sanctioned. A routed site simply has no `.send()` left to account for.
  #6916  triage interpolates an LLM-supplied `repo` argument into a URL and sends it
         on the same client with no egress check at all.

Each was found by reading, one at a time, each search wider than the last. That is the
signature of an unbounded population rather than three unlucky call sites -- so the
population is bounded here instead.

The rule refuses a SHAPE: an outbound `.send()` in these crates is either the safe path
itself, or carries a stated reason, or names an open issue tracking its correction. A
new one that is none of those fails the build. Deciding whether a given request is
"actually safe" is judgment this script does not have and does not attempt.
"""

from __future__ import annotations

import logging
import re
import subprocess
import sys
from pathlib import Path

LOGGER = logging.getLogger("check-egress-send-sites")

# Crates whose outbound requests are reachable from model input or carry fleet
# credentials. Provider SDK clients (hermeneus, agora, episteme embeddings) talk to
# fixed configured endpoints and are out of scope by construction, not by oversight.
SCANNED_ROOTS = (
    "crates/organon/src",
    "crates/aletheia/src",
    "crates/koina/src",
)

SEND_CALL = re.compile(r"\.send\(\)")


def is_comment(line: str) -> bool:
    """True when the line is entirely a comment and so cannot carry a send call.

    WHY whole-line only, rather than truncating at the first `//`: a line like
    `client.get("http://host").send()` carries `//` inside a URL literal, and cutting
    there would hide a real send site. This check's errors must fall on the side of
    reporting too much. A line whose first non-space characters are `//` has no code
    on it, so dropping it cannot hide anything.

    WHY it is needed at all: the doc comment on a routed call site explains what the
    unrouted shape was, and naming `.send()` in that prose made the site report itself
    as unaccounted-for -- so documenting the rule tripped the rule.

    Block comments are not handled: the scanned crates contain none, and the standards
    prescribe `//`-style tags. A `/* */` block would produce a false positive here, in
    the safe direction.
    """
    return line.lstrip().startswith("//")

# The checkpoint itself. These ARE the safe path; they cannot route through it.
SANCTIONED = {
    "crates/organon/src/builtins/http_client.rs": (
        "this is send_with_safe_redirects -- the checkpoint every other site routes through"
    ),
    "crates/organon/src/builtins/research.rs": (
        "this is get_with_safe_redirects -- research's copy of the same loop"
    ),
}

# Sites that do not route through the checkpoint and do not need to. A reason here is
# a claim someone can check, not a way to make the script quiet.
EXEMPT = {
    "crates/aletheia/src/commands/ingest.rs": (
        "operator CLI: the destination is an argument the operator typed, not model "
        "output, and its credential is an Authorization header -- which reqwest DOES "
        "strip across a cross-host redirect, unlike the custom token headers in #6910 "
        "and #6921"
    ),
    "crates/aletheia/src/commands/memory/mod.rs": (
        "operator CLI, same shape as ingest.rs: operator-supplied URL, Authorization "
        "header that reqwest strips cross-host"
    ),
}

# Known-unprotected, tracked, not yet corrected. An entry must name an open issue --
# see the note in main() for why that is checked rather than trusted.
TRACKED = {
    "crates/organon/src/builtins/triage/mod.rs": 6916,
}


def send_sites(repo_root: Path) -> dict[str, list[int]]:
    """Return {path: [line, ...]} for every outbound send call under the scanned roots."""
    found: dict[str, list[int]] = {}
    for root in SCANNED_ROOTS:
        base = repo_root / root
        if not base.is_dir():
            raise SystemExit(f"scanned root is missing: {root}")
        for path in sorted(base.rglob("*.rs")):
            try:
                text = path.read_text(encoding="utf-8")
            except (OSError, UnicodeError) as error:
                raise SystemExit(f"cannot read {path}: {error}") from error
            lines = [
                number
                for number, line in enumerate(text.splitlines(), start=1)
                if SEND_CALL.search(line) and not is_comment(line)
            ]
            if lines:
                found[path.relative_to(repo_root).as_posix()] = lines
    return found


def open_issue(number: int) -> bool:
    """True when the issue is open on GitHub, or when GitHub cannot be reached.

    WHY it fails OPEN on an unreachable API: this check's job is to refuse a NEW
    unrouted send site. Turning a network hiccup into a red build would make the
    check the thing people route around, and the tracked entries are already a
    known, recorded state rather than a new one.
    """
    probe = subprocess.run(
        ["gh", "issue", "view", str(number), "--repo", "forkwright/aletheia", "--json", "state"],
        capture_output=True,
        check=False,
    )
    if probe.returncode != 0:
        LOGGER.warning(
            "check-egress-send-sites: cannot reach GitHub to confirm #%d is open; "
            "treating it as open",
            number,
        )
        return True
    return b'"OPEN"' in probe.stdout


def main() -> int:
    repo_root = Path(__file__).resolve().parents[1]
    found = send_sites(repo_root)

    known = set(SANCTIONED) | set(EXEMPT) | set(TRACKED)
    unlisted = sorted(set(found) - known)
    stale = sorted(known - set(found))

    failures = False

    if unlisted:
        failures = True
        LOGGER.error(
            "check-egress-send-sites: an outbound request is not routed through the "
            "egress checkpoint and is not accounted for."
        )
        for path in unlisted:
            for line in found[path]:
                LOGGER.error("  %s:%d", path, line)
        LOGGER.error("")
        LOGGER.error("Route it through send_with_safe_redirects, so every hop -- the")
        LOGGER.error("first request and any redirect -- passes the same egress policy")
        LOGGER.error("and internal-address check http_request uses.")
        LOGGER.error("")
        LOGGER.error("If it genuinely does not need that, add it to EXEMPT in this")
        LOGGER.error("script with a reason a reviewer can check. If it needs the fix")
        LOGGER.error("but not today, add it to TRACKED with an open issue number.")

    if stale:
        failures = True
        LOGGER.error("")
        LOGGER.error(
            "check-egress-send-sites: these are listed here but have no send call "
            "any more. A stale entry silently exempts whatever takes its path next:"
        )
        for path in stale:
            LOGGER.error("  %s", path)

    for path, number in sorted(TRACKED.items()):
        if path in found and not open_issue(number):
            failures = True
            LOGGER.error("")
            LOGGER.error(
                "check-egress-send-sites: %s is tracked against #%d, which is closed "
                "while the site is still unrouted.",
                path,
                number,
            )
            LOGGER.error("Either the fix landed and this entry should go, or the issue")
            LOGGER.error("was closed without it and should be reopened.")

    if failures:
        return 1

    LOGGER.info(
        "check-egress-send-sites: %d send site(s), all sanctioned, exempted or tracked",
        sum(len(v) for v in found.values()),
    )
    return 0


if __name__ == "__main__":
    logging.basicConfig(format="%(message)s", level=logging.INFO, stream=sys.stderr)
    raise SystemExit(main())
