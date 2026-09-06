#!/usr/bin/env python3
"""Ratchet: `koilon` and `proskenion` may only shrink their direct-transport surface.

Ruling B (aletheia#7187, tracked further at #7198) is that skene is the sole
protocol boundary between a first-party client (proskenion, koilon) and the
server (pylon): a client talks to `skene::api::client`/`skene::api::streaming`,
never to `reqwest` or a hand-rolled route string on its own. #7187 already
moved proskenion's health check and per-turn streaming behind skene; #7198
observed that nothing *mechanically* stops the next call site from drifting
back out from under that boundary the way it drifted in the first place.

This script is that mechanical check. It scans `crates/theatron/koilon/src`
and `crates/theatron/proskenion/src` (tracked `*.rs` files only) for three
independent signals that a client is talking to the server surface directly
instead of through skene:

  (a) any `reqwest` use (`reqwest::Client`, `use reqwest::...`, ...),
  (b) any string literal containing the substring `/api/v1/` (a hand-built
      route, as opposed to one of skene's `ClientRouteContract` builders), and
  (c) a hand-built SSE decoder shaped like the one #7187 deleted from
      proskenion -- `fn parse_stream_event` and the helpers it was built
      from (`str_field`, `str_any_field`, `opt_str_any_field`,
      `stream_error_message`). This is the exact name set #7187's own proof
      grepped for to confirm the deletion; widening it to "anything that
      looks like a decoder" would need the same kind of judgement call
      check-stub-accountability.py's KEYWORDS comment warns against for its
      own five words -- so it stays a closed, named list.

## What is scanned, and what is deliberately not

Comments (`//`, `/* */`) are blanked before any of the three signals are
matched -- a WHY-comment or doc-comment that *mentions* `reqwest` or an
`/api/v1/` path in prose (there are several) is not a call site. String
content is checked only for signal (b); signals (a) and (c) are matched
against code text with strings ALSO blanked, so a `reqwest` occurring inside
a printed message would not match (it never does in either crate today, but
the intent is "used", not "mentioned").

Test-scoped code is out of scope for all three signals: a `#[cfg(test)]`
module or a `#[test]`/`#[tokio::test]` function, and any whole file matching
`*_test.rs`/`*_tests.rs` or living under a `tests/` path component. Both
crates' unit tests stand up a mock HTTP server and register routes like
`/api/v1/nous` to *simulate* pylon for that one test -- that is a test
harness speaking for the server side, not a client bypassing skene, and
excluding it is what lets koilon baseline at zero (the #7198 audit found no
production direct-HTTP site in koilon; if this script ever finds one, that
is a real regression worth reporting, not a bug in the exclusion).

## Baseline is a ceiling, not a ledger

Unlike scripts/domain-id-suppressions.json (which also demands the baseline
be lowered the moment the tree does, "stale in both directions"),
scripts/client-boundary-baseline.toml is a plain per-file ceiling: growth
past it, or a new file appearing with a nonzero count, fails; a file that
shrinks below its recorded ceiling passes outright (reported as a ratchet
delta, not an error) -- the desktop migration this guard exists ahead of
(#7198) will retire proskenion's remaining call sites one PR at a time, and
a check that also failed on every partial improvement would make each of
those PRs carry an unrelated `--write-baseline` diff. The ceiling only ever
being *raised* by a human running --write-baseline (and it showing up in the
PR diff for review) is what stops it drifting loose the other way.

Usage:
    python3 scripts/check-client-boundary.py
    python3 scripts/check-client-boundary.py --write-baseline
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
BASELINE_PATH = Path(__file__).resolve().parent / "client-boundary-baseline.toml"

SCANNED_ROOTS = (
    "crates/theatron/koilon/src",
    "crates/theatron/proskenion/src",
)

# --------------------------------------------------------------------------
# Comment / string literal tokenizing
#
# A two-pass "blank literals, then find comments against the blanked copy"
# approach (check-orphaned-modules.py's strategy) does not work for this
# script: this codebase's doc comments routinely quote plain English in
# double quotes (`/// a 413 is "split the note", a 409 is "reload before
# saving"`, `crates/theatron/proskenion/src/api/client.rs` alone has such a
# comment). A literal-first pass run against text that STILL has comments in
# it reads those prose quotes as string delimiters, and one such
# misdetection desyncs every string/comment boundary for the remainder of
# the file -- silently swallowing real string literals into what it thinks
# is one giant "string". 198 lines across these two crates carry a `"`
# inside a comment, so this is not a rare edge case here.
#
# The fix is a single left-to-right pass with one piece of state (code /
# line-comment / block-comment / string), so a `"` encountered while already
# inside a comment is correctly just comment text, never mistaken for a
# string boundary, and vice versa -- no separate pass can get the two
# confused because only one is ever "in progress" at a time.

RAW_STRING_PREFIX_RE = re.compile(r'b?r(#{0,8})"')
NORMAL_STRING_PREFIX_RE = re.compile(r'b?"')
CHAR_LITERAL_RE = re.compile(r"'(?:\\.|[^'\\\n])'")

REQWEST_RE = re.compile(r"\breqwest\b")
API_LITERAL = "/api/v1/"
SSE_FN_RE = re.compile(
    r"\bfn\s+(parse_stream_event|str_field|str_any_field|opt_str_any_field|stream_error_message)\b"
)

CFG_TEST_RE = re.compile(r"#!?\[\s*cfg\s*\(\s*test\s*\)\s*\]")
TEST_ATTR_RE = re.compile(r"#\[\s*(?:tokio::)?test(?:\s*\([^)]*\))?\s*\]")
ATTR_RE = re.compile(r"#!?\[[^\]]*\]")
MOD_OR_FN_RE = re.compile(r"(?:pub(?:\([^)]*\))?\s+)?(?:async\s+|unsafe\s+|const\s+)*(?:mod|fn)\b")
TERMINATOR_RE = re.compile(r"[;{]")

TEST_PATH_RE = re.compile(r"(^|/)tests?/")
TEST_FILE_RE = re.compile(r"(?:_test|_tests|test|tests)\.rs$")


def is_test_file(rel: str) -> bool:
    if TEST_PATH_RE.search(rel):
        return True
    return bool(TEST_FILE_RE.search(rel.rsplit("/", 1)[-1]))


def blank_spans(text: str, spans: list[tuple[int, int]]) -> str:
    """Replace every character in each [start, end) span with a space,
    except newlines, which are left alone -- length- and line-preserving."""
    if not spans:
        return text
    chars = list(text)
    for start, end in spans:
        for i in range(start, end):
            if chars[i] != "\n":
                chars[i] = " "
    return "".join(chars)


def tokenize(text: str) -> list[tuple[int, int, str]]:
    """One left-to-right pass over `text`. Returns (start, end, kind) spans
    covering the whole text with no gaps and no overlaps, kind one of
    "code", "comment", "string", "char". Nothing downstream reads a char
    literal's content -- it is its own kind (not folded into "code") only
    so that scan_file's non-code blanking also blanks it, since a `"` or
    `{`/`}` used AS a char literal's payload (`'"'`, `'{'`) must not desync
    the string/brace scans that run over the blanked code text. A lifetime
    (`'a`, no closing quote nearby) is never mistaken for a string/comment
    delimiter either way, so it is left as plain "code".

    Block comments track nesting depth (Rust allows `/* /* */ */`); line
    comments and strings do not need to. A raw string's hash count is read
    once at its opening delimiter and used verbatim as its closing
    delimiter (`r##"..."##` closes only on `"##`, not a bare `"`).
    """
    n = len(text)
    i = 0
    segments: list[tuple[int, int, str]] = []
    seg_start = 0
    seg_kind = "code"

    def flush(end: int, next_kind: str) -> None:
        nonlocal seg_start, seg_kind
        if end > seg_start:
            segments.append((seg_start, end, seg_kind))
        seg_start = end
        seg_kind = next_kind

    while i < n:
        if text[i : i + 2] == "//":
            flush(i, "comment")
            i += 2
            while i < n and text[i] != "\n":
                i += 1
            flush(i, "code")
            continue
        if text[i : i + 2] == "/*":
            flush(i, "comment")
            i += 2
            depth = 1
            while i < n and depth > 0:
                if text[i : i + 2] == "/*":
                    depth += 1
                    i += 2
                elif text[i : i + 2] == "*/":
                    depth -= 1
                    i += 2
                else:
                    i += 1
            flush(i, "code")
            continue
        raw = RAW_STRING_PREFIX_RE.match(text, i)
        if raw:
            flush(i, "string")
            hashes = raw.group(1)
            terminator = '"' + hashes
            end_idx = text.find(terminator, raw.end())
            i = end_idx + len(terminator) if end_idx != -1 else n
            flush(i, "code")
            continue
        norm = NORMAL_STRING_PREFIX_RE.match(text, i)
        if norm:
            flush(i, "string")
            i = norm.end()
            while i < n:
                c = text[i]
                if c == "\\":
                    i += 2
                    continue
                i += 1
                if c == '"':
                    break
            flush(i, "code")
            continue
        if text[i] == "'":
            m = CHAR_LITERAL_RE.match(text, i)
            if m:
                flush(i, "char")
                i = m.end()
                flush(i, "code")
                continue
            i += 1
            continue
        i += 1

    flush(n, seg_kind)
    return segments


def find_test_scope_spans(code_text: str) -> list[tuple[int, int]]:
    """Byte ranges of `code_text` governed by `#[cfg(test)]` or
    `#[test]`/`#[tokio::test]`: the attribute through the matching close
    brace of the `mod { ... }`/`fn { ... }` it decorates, brace-depth
    matched against `code_text` (comments and strings already blanked out
    of it, so every remaining `{`/`}` is real code structure)."""
    spans: list[tuple[int, int]] = []
    n = len(code_text)
    for attr_re in (CFG_TEST_RE, TEST_ATTR_RE):
        for m in attr_re.finditer(code_text):
            pos = m.end()
            while True:
                ws = re.match(r"\s*", code_text[pos:])
                pos += ws.end()
                attr = ATTR_RE.match(code_text[pos:])
                if attr:
                    pos += attr.end()
                    continue
                break
            kw = MOD_OR_FN_RE.match(code_text[pos:])
            if not kw:
                continue
            pos += kw.end()
            # `mod name;` (an external test file, e.g. `#[cfg(test)] mod
            # app_tests;`) has no brace body -- whichever of `;`/`{` comes
            # first decides the shape. Searching for `{` alone would walk
            # straight past the semicolon to the next unrelated item's
            # brace and exclude everything in between as "test scope".
            term = TERMINATOR_RE.search(code_text, pos)
            if term is None:
                continue
            if term.group(0) == ";":
                spans.append((m.start(), term.end()))
                continue
            depth = 0
            i = term.start()
            end = n
            while i < n:
                c = code_text[i]
                if c == "{":
                    depth += 1
                elif c == "}":
                    depth -= 1
                    if depth == 0:
                        end = i + 1
                        break
                i += 1
            spans.append((m.start(), end))
    return spans


def is_excluded(pos: int, spans: list[tuple[int, int]]) -> bool:
    return any(start <= pos < end for start, end in spans)


@dataclass(frozen=True)
class Occurrence:
    file: str
    line: int
    kind: str  # "reqwest" | "api-literal" | "sse-decode"
    detail: str


def scan_file(rel: str, text: str) -> list[Occurrence]:
    if is_test_file(rel):
        return []

    segments = tokenize(text)
    non_code_spans = [(s, e) for s, e, k in segments if k != "code"]
    code_text = blank_spans(text, non_code_spans)
    excluded = find_test_scope_spans(code_text)

    occurrences: list[Occurrence] = []

    for m in REQWEST_RE.finditer(code_text):
        if is_excluded(m.start(), excluded):
            continue
        line = text.count("\n", 0, m.start()) + 1
        occurrences.append(Occurrence(rel, line, "reqwest", "reqwest"))

    for m in SSE_FN_RE.finditer(code_text):
        if is_excluded(m.start(), excluded):
            continue
        line = text.count("\n", 0, m.start()) + 1
        occurrences.append(Occurrence(rel, line, "sse-decode", m.group(0)))

    for start, end, kind in segments:
        if kind != "string":
            continue
        content = text[start:end]
        if API_LITERAL not in content:
            continue
        if is_excluded(start, excluded):
            continue
        line = text.count("\n", 0, start) + 1
        snippet = content if len(content) <= 80 else content[:77] + "...\""
        occurrences.append(Occurrence(rel, line, "api-literal", snippet))

    occurrences.sort(key=lambda o: o.line)
    return occurrences


def tracked_files() -> list[str]:
    result = subprocess.run(
        ["git", "ls-files"] + [f"{root}/*.rs" for root in SCANNED_ROOTS],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        check=True,
    )
    return sorted(line for line in result.stdout.splitlines() if line)


def scan_all() -> list[Occurrence]:
    occurrences: list[Occurrence] = []
    for rel in tracked_files():
        path = REPO_ROOT / rel
        try:
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeError):
            continue
        occurrences.extend(scan_file(rel, text))
    return occurrences


def group_by_file(occurrences: list[Occurrence]) -> dict[str, list[Occurrence]]:
    by_file: dict[str, list[Occurrence]] = {}
    for occ in occurrences:
        by_file.setdefault(occ.file, []).append(occ)
    for lst in by_file.values():
        lst.sort(key=lambda o: o.line)
    return by_file


# --------------------------------------------------------------------------
# Baseline: a per-file ceiling, load/write/compare


def load_baseline() -> dict[str, int]:
    if not BASELINE_PATH.exists():
        raise SystemExit(
            f"missing {BASELINE_PATH.name}; run "
            "scripts/check-client-boundary.py --write-baseline"
        )
    try:
        with BASELINE_PATH.open("rb") as fh:
            data = tomllib.load(fh)
    except (OSError, ValueError) as error:
        raise SystemExit(f"cannot read {BASELINE_PATH.name}: {error}") from error
    files = data.get("files", {})
    if not isinstance(files, dict):
        raise SystemExit(f"{BASELINE_PATH.name} has no [files] table")
    return {str(k): int(v) for k, v in files.items()}


def write_baseline(counts: dict[str, int]) -> None:
    lines = [
        "# Per-file occurrence CEILING for the client -> skene boundary ratchet",
        "# (aletheia#7198, enforcing ruling B / aletheia#7187: skene is the sole",
        "# protocol boundary for koilon and proskenion). Counts every `reqwest`",
        '# use, string literal containing "/api/v1/", and parse_stream_event-',
        "# shaped SSE decoder in crates/theatron/{koilon,proskenion}/src outside",
        "# test-scoped code. This number may only go DOWN as call sites move",
        "# behind skene::api::client / skene::api::streaming.",
        "#",
        "# Regenerate with: scripts/check-client-boundary.py --write-baseline",
        "",
        "[files]",
    ]
    for path, n in sorted(counts.items()):
        escaped = path.replace("\\", "\\\\").replace('"', '\\"')
        lines.append(f'"{escaped}" = {n}')
    BASELINE_PATH.write_text("\n".join(lines) + "\n", encoding="utf-8")


def compare(
    actual: dict[str, int], baseline: dict[str, int]
) -> tuple[list[tuple[str, int, int]], list[tuple[str, int, int]]]:
    """Return (regressions, improvements) as (path, actual, baseline) triples.

    A file absent from the baseline is treated as having an allowance of 0
    -- any occurrence in it is growth (a new file appearing with a nonzero
    count fails, per #7198's spec). A file present in the baseline but now
    at a lower count (including 0, i.e. gone from the tree) is an
    improvement, reported but never a failure -- the one-directional
    ratchet this script implements, as distinct from scripts/domain-id-
    suppressions.py's stricter both-directions-must-match design.
    """
    regressions: list[tuple[str, int, int]] = []
    improvements: list[tuple[str, int, int]] = []
    for path, n in sorted(actual.items()):
        allowed = baseline.get(path, 0)
        if n > allowed:
            regressions.append((path, n, allowed))
        elif n < allowed:
            improvements.append((path, n, allowed))
    for path, allowed in sorted(baseline.items()):
        if path not in actual and allowed > 0:
            improvements.append((path, 0, allowed))
    return regressions, improvements


def format_report(
    actual: dict[str, int],
    baseline: dict[str, int],
    by_file: dict[str, list[Occurrence]],
) -> tuple[int, list[str]]:
    """Pure decision + message layer, split out from scan_all()'s filesystem
    I/O so the four required behaviors (unchanged/growth/shrink/new-file)
    are directly unit-testable against synthetic (actual, baseline, by_file)
    triples -- see scripts/test-check-client-boundary.py."""
    regressions, improvements = compare(actual, baseline)
    lines: list[str] = []

    if regressions:
        lines.append(
            "check-client-boundary: a client crate is bypassing skene "
            "(ruling B, aletheia#7187):"
        )
        for path, n, allowed in regressions:
            if path in baseline:
                lines.append(f"  {path}: {n}, baseline {allowed}")
            else:
                lines.append(f"  {path}: {n} new (this file had none)")
            for occ in by_file.get(path, [])[allowed:]:
                lines.append(f"    {occ.file}:{occ.line}: {occ.kind} -- {occ.detail}")
        lines.append("")
        lines.append(
            "Route through skene::api::client / skene::api::streaming instead -- "
            "see aletheia#7187 for the worked migration (health check and "
            "per-turn streaming already moved). If this is deliberate, "
            "reviewed new debt, regenerate the baseline with --write-baseline "
            "and say why in the PR description."
        )
        return 1, lines

    total = sum(actual.values())
    allowed_total = sum(baseline.values())
    lines.append(
        f"check-client-boundary: {total} occurrence(s) across {len(actual)} "
        f"file(s), at or below the baseline of {allowed_total}"
    )
    if improvements:
        lines.append("")
        lines.append(
            "Ratchet delta (fewer occurrences than the baseline claims -- a "
            "call site moved behind skene):"
        )
        for path, n, allowed in sorted(improvements):
            lines.append(f"  {path}: {n} now, baseline {allowed} ({allowed - n} paid down)")
        lines.append(
            "Run scripts/check-client-boundary.py --write-baseline to lock in "
            "the improvement."
        )
    return 0, lines


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--write-baseline",
        action="store_true",
        help="regenerate scripts/client-boundary-baseline.toml from the current tree and exit",
    )
    args = parser.parse_args()

    occurrences = scan_all()
    by_file = group_by_file(occurrences)
    actual = {f: len(v) for f, v in by_file.items()}

    if args.write_baseline:
        write_baseline(actual)
        print(
            f"wrote {BASELINE_PATH.relative_to(REPO_ROOT)}: "
            f"{sum(actual.values())} occurrence(s) across {len(actual)} file(s)"
        )
        return 0

    baseline = load_baseline()
    code, lines = format_report(actual, baseline, by_file)
    stream = sys.stderr if code else sys.stdout
    for line in lines:
        print(line, file=stream)
    return code


if __name__ == "__main__":
    sys.exit(main())
