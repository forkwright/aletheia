#!/usr/bin/env python3
"""Find `#[expect(clippy::LINT)]` attributes that cannot fire in this tree.

Under `-D warnings`, `unfulfilled-lint-expectations` is itself an error: an
`#[expect(lint)]` whose lint never triggers fails the build. That only
surfaces after the whole workspace compiles, at the end of a ~25-minute gate.
This is the proactive, textual complement, catching the same shape in
seconds by reading rather than compiling.

WHY narrow rather than broad: a false positive here gets the check disabled
within a week, which is worse than not having it. This covers exactly two
shapes and says no to everything else:

  - file-level `#![expect(clippy::LINT, ...)]` -- scope is the whole file
    (or, when the attribute sits just inside a `mod NAME {`, the whole mod
    body), so a textual search over that span is sound.
  - item-level `#[expect(clippy::LINT, ...)]` immediately followed by a
    `mod NAME {` header -- scope is that module's body, found by brace
    matching.

Only three lints are covered, because only these three have a trigger that
is an unambiguous source substring: `unwrap_used` -> `.unwrap(` or
`.unwrap_err(`, `expect_used` -> `.expect(` or `.expect_err(`, `panic` ->
`panic!(`. The `_err` variants are not optional extras: clippy dispatches
`.unwrap_err()`/`.expect_err()` to the SAME `unwrap_used`/`expect_used`
lints as their non-`_err` forms (rust-lang/rust-clippy#9338), so a mod whose
tests only call `.unwrap_err()` genuinely fulfills `#[expect(unwrap_used)]`.
`clippy::indexing_slicing` is deliberately NOT covered -- `[` is everywhere
in Rust and a textual search for it is not a trigger, it is noise.

A scope's search span is not limited to its own physical text: a lint-level
attribute's scope is the module it sits on PLUS every descendant item,
including child modules declared via `mod NAME;` (or `#[path = "..."] mod
NAME;`) that live in an entirely different file -- the same way
`#[allow(...)]` reaches into split-out submodules. Both the file-level and
mod-level cases resolve every such declaration inside their span and fold
the declared file's (recursively-resolved) content into the search,
uncapped by that child's own cfg-gating -- finding the trigger there is
always sound evidence of fulfillment, never a reason to flag. See
`_gather_child_content`. Without this, a test suite split for
`RUST/file-too-long` reads as if it had no test code at all: the umbrella
`mod.rs` this repo's convention produces (e.g.
`crates/daemon/src/runner_tests/mod.rs`, which declares four child modules
and contains no test code of its own) would otherwise report every child's
real `.unwrap()`/`.expect()` calls as absent.

Deliberately NOT covered (false-positive risk too high to textually resolve):

  - `#[expect(...)]` on a function, struct, or any non-`mod` item. Whether
    the lint's trigger is "inside its scope" depends on parsing the item's
    body, which a `mod`'s brace-matched span gives for free but a function
    signature does not (the trigger could be in a sibling fn with the same
    name, a doc example, anywhere).
  - `#[expect(...)]` whose effective content is produced by macro expansion
    -- there is no macro expander here, only source text.
  - any lint besides the three above.

cfg-gating: an `#[expect]` on a module (or a file reachable only through a
feature-gated `mod`/`#[path]` declaration) can be unfulfilled in one
build configuration and fulfilled in another. Getting this wrong in the
"it's fine" direction is a false negative (acceptable -- it just means this
check does not catch that instance, the same as any lint it does not
attempt); getting it wrong in the "flag it" direction is exactly the false
positive this script exists to avoid. So: any cfg predicate that is not
provably true whenever `cfg(test)` holds (bare `test`, or an `any(...)`
that reduces to true through `test`) makes the item's home un-checkable,
and it is excluded rather than evaluated. See `cfg_is_test_safe`.

File-level reachability is resolved ONE hop: the direct `mod NAME;` /
`#[path = "..."] mod NAME;` statement that brings the file into its crate's
module tree is found and checked for a non-test-safe cfg. A file reachable
only through a MULTI-hop chain (a clean direct declaration, but an ancestor
module further up that is itself feature-gated) is a known gap -- every
instance of gated inclusion found in this tree gates at the immediate hop
(`crates/energeia/src/store/mod.rs`, `crates/graphe/.../fjall_store_tests.rs`),
so this is believed to cover the real cases without the cost of full
transitive resolution. If a file's declaring statement cannot be found at
all (most commonly: `tests/*.rs` integration-test binaries, which cargo
discovers directly and which are never declared via `mod`), it is treated
as unconditionally reachable -- the common case, and the one this script
exists to check.

Relationship to `utilities/drop-unfulfilled-expects.py` (metis-ops): that
script is REACTIVE -- it reads a gate log naming lines the compiler already
found unfulfilled, and strips just the lint (or the whole attribute when
none remains). This script is the PROACTIVE complement: it finds the same
defect shape ahead of a gate run, by reading source instead of a log, so
the fix lands before the 25-minute round trip rather than after it.
"""

from __future__ import annotations

import logging
import re
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path

LOGGER = logging.getLogger("check-unfulfilled-expects")

REPO_ROOT = Path(__file__).resolve().parent.parent

# Lint name (as written after `clippy::`) -> its unambiguous source
# trigger(s), any ONE of which fulfills it. Keep this short. A lint belongs
# here only if a plain substring search for every trigger cannot be fooled
# by anything short of a string/comment (which `strip_noncode` already
# removes).
#
# WHY unwrap_used/expect_used each carry two triggers: clippy's
# `unwrap_expect_used` check dispatches `.unwrap()` and `.unwrap_err()` to
# the SAME `unwrap_used` lint, and `.expect()` and `.expect_err()` to the
# SAME `expect_used` lint (rust-clippy clippy_lints/src/methods/mod.rs,
# `unwrap_expect_used::check(..., Variant::Unwrap | Variant::Expect)`,
# merged in rust-lang/rust-clippy#9338). A mod whose tests call only
# `.unwrap_err()` genuinely fulfills `#[expect(clippy::unwrap_used)]` --
# covering only `.unwrap(` would flag it as a false positive (found live at
# `crates/koina/src/error.rs`, whose `mod tests` uses `.unwrap_err()`
# throughout and zero bare `.unwrap()`).
TRIGGERS: dict[str, tuple[str, ...]] = {
    "unwrap_used": (".unwrap(", ".unwrap_err("),
    "expect_used": (".expect(", ".expect_err("),
    "panic": ("panic!(",),
}

MOD_HEADER_RE = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*\{\s*$"
)
ATTR_START_RE = re.compile(r"^(\s*)#(!)?\[")
CFG_ATTR_RE = re.compile(r"^\s*#!?\[cfg\((.*)\)\]\s*$")
EXPECT_ATTR_START_RE = re.compile(r"^\s*#(!)?\[expect\(")
MOD_DECL_RE = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;\s*$")
PATH_ATTR_RE = re.compile(r'^\s*#\[path\s*=\s*"([^"]+)"\]\s*$')
LINT_NAME_RE = re.compile(r"clippy::([a-z_]+)")


@dataclass
class Violation:
    path: str
    line: int
    lint: str
    scope: str  # "file" | "mod NAME"


# --------------------------------------------------------------------------
# Non-code stripping: blank comments and literal contents so brace-matching
# and trigger search cannot be fooled by a brace or a `.unwrap(` that is
# only text. Preserves line structure (newlines untouched) so line numbers
# in the stripped text still line up 1:1 with the source.
# --------------------------------------------------------------------------


_SPECIAL_RE = re.compile(r'["\'/]')
# WHY searched backward from the opening quote rather than forward from a
# leading `r`/`b`: scanning forward over every `r` and `b` in the file (both
# very common identifier letters) is exactly the per-character cost this
# jump-search exists to avoid. Only `"`, `'`, `/` are jump targets; a raw or
# byte-string prefix is recovered by looking a short, bounded distance
# backward from the quote instead.
_RAW_PREFIX_RE = re.compile(r"(?:b?r)(#*)$")


def strip_noncode(text: str) -> str:
    """Blank line/block comments and string/char literal contents to spaces.

    WARNING: a heuristic scanner, not a full Rust lexer. The one genuinely
    ambiguous case -- `'` starting either a char literal (`'x'`, `'\\n'`) or
    a lifetime (`'a`) -- is resolved by lookahead: a `'` is only treated as
    a literal opener when a closing `'` is found within a short escape or a
    single following character, otherwise it is left as ordinary code. That
    is exactly the shape Rust source actually uses; a mis-scan only affects
    brace-counting fidelity for the one construct it misreads.

    PERF: jumps between occurrences of `"`, `'`, `/` via a compiled regex
    (C-speed) instead of examining every character in Python -- the earlier
    per-character version was the dominant cost of a repo-wide run (~20s of
    a ~27s total). Only the content actually inside a found span (a
    comment's body, a string's body) still gets a per-character scan, which
    is unavoidable there and comparatively small.
    """
    out = list(text)
    n = len(text)

    def blank(a: int, b: int) -> None:
        for k in range(a, b):
            if out[k] != "\n":
                out[k] = " "

    i = 0
    while i < n:
        m = _SPECIAL_RE.search(text, i)
        if m is None:
            break
        pos = m.start()
        c = text[pos]

        if c == "/":
            if pos + 1 < n and text[pos + 1] == "/":
                j = text.find("\n", pos)
                j = n if j == -1 else j
                blank(pos, j)
                i = j
                continue
            if pos + 1 < n and text[pos + 1] == "*":
                depth = 1
                j = pos + 2
                while j < n and depth > 0:
                    if text[j : j + 2] == "/*":
                        depth += 1
                        j += 2
                    elif text[j : j + 2] == "*/":
                        depth -= 1
                        j += 2
                    else:
                        j += 1
                blank(pos, j)
                i = j
                continue
            i = pos + 1
            continue

        if c == '"':
            rm = _RAW_PREFIX_RE.search(text[max(0, pos - 12) : pos])
            if rm:
                hashes = len(rm.group(1))
                start = pos - len(rm.group(0))
                closer = '"' + ("#" * hashes)
                end = text.find(closer, pos + 1)
                end = n if end == -1 else end + len(closer)
                blank(start, end)
                i = end
                continue
            # plain or byte string: (b)?"..."
            start = pos - 1 if pos > 0 and text[pos - 1] == "b" else pos
            j = pos + 1
            while j < n:
                if text[j] == "\\":
                    j += 2
                    continue
                if text[j] == '"':
                    j += 1
                    break
                j += 1
            blank(start, j)
            i = j
            continue

        # c == "'"
        if pos + 2 < n and text[pos + 1] == "\\":
            k = pos + 2
            while k < n and text[k] != "'" and k - pos < 8:
                k += 1
            if k < n and text[k] == "'":
                blank(pos, k + 1)
                i = k + 1
                continue
        elif pos + 2 < n and text[pos + 2] == "'":
            blank(pos, pos + 3)
            i = pos + 3
            continue
        # else: lifetime / generic tick -- not a literal, leave as code
        i = pos + 1

    return "".join(out)


def find_matching(lines: list[str], li: int, ci: int, open_ch: str, close_ch: str) -> tuple[int, int] | None:
    """Depth-match `open_ch` at (li, ci) forward to its `close_ch`, over `lines`."""
    depth = 0
    for row in range(li, len(lines)):
        line = lines[row]
        col0 = ci if row == li else 0
        for col in range(col0, len(line)):
            ch = line[col]
            if ch == open_ch:
                depth += 1
            elif ch == close_ch:
                depth -= 1
                if depth == 0:
                    return (row, col)
    return None


def find_enclosing_open(lines: list[str], before_li: int) -> tuple[int, int] | None:
    """Walk backward from the start of `before_li` to the nearest unclosed `{`."""
    extra_close = 0
    for row in range(before_li - 1, -1, -1):
        line = lines[row]
        for col in range(len(line) - 1, -1, -1):
            ch = line[col]
            if ch == "}":
                extra_close += 1
            elif ch == "{":
                if extra_close > 0:
                    extra_close -= 1
                else:
                    return (row, col)
    return None


# --------------------------------------------------------------------------
# cfg predicate evaluation: 3-valued (True / False / Unknown), with `test`
# bound True and every other atom Unknown. Only a provably-True predicate is
# "test safe" -- Unknown and False both mean "cannot tell it fires under the
# checked configuration", which is excluded rather than flagged.
# --------------------------------------------------------------------------

_TRUE, _FALSE, _UNKNOWN = "true", "false", "unknown"

_CFG_TOKEN_RE = re.compile(r'"[^"]*"|[A-Za-z_][A-Za-z0-9_]*|[(),=]')


def _eval_cfg_tokens(tokens: list[str], pos: list[int]) -> str:
    name = tokens[pos[0]]
    pos[0] += 1
    if name in ("all", "any", "not") and pos[0] < len(tokens) and tokens[pos[0]] == "(":
        pos[0] += 1
        parts = [_eval_cfg_tokens(tokens, pos)]
        while pos[0] < len(tokens) and tokens[pos[0]] == ",":
            pos[0] += 1
            if pos[0] < len(tokens) and tokens[pos[0]] == ")":
                break
            parts.append(_eval_cfg_tokens(tokens, pos))
        if pos[0] < len(tokens) and tokens[pos[0]] == ")":
            pos[0] += 1
        if name == "all":
            if _FALSE in parts:
                return _FALSE
            return _TRUE if all(p == _TRUE for p in parts) else _UNKNOWN
        if name == "any":
            if _TRUE in parts:
                return _TRUE
            return _FALSE if all(p == _FALSE for p in parts) else _UNKNOWN
        p = parts[0]
        return {_TRUE: _FALSE, _FALSE: _TRUE, _UNKNOWN: _UNKNOWN}[p]

    if pos[0] < len(tokens) and tokens[pos[0]] == "=":
        pos[0] += 1
        # INVARIANT: the value is a string literal, and strip_noncode blanks
        # string contents (quotes included) to spaces, so on stripped text
        # there is never an actual value token here to discard -- only ever
        # the `,` or `)` that closes this atom's enclosing list. Consuming
        # it unconditionally would eat that delimiter and desynchronize the
        # parser for every atom after this one. Only consume a token that is
        # demonstrably not a delimiter (keeps this correct if ever fed
        # unstripped text, where the literal is still present).
        if pos[0] < len(tokens) and tokens[pos[0]] not in (",", ")"):
            pos[0] += 1
    return _TRUE if name == "test" else _UNKNOWN


def cfg_is_test_safe(predicate_text: str) -> bool:
    """True only if `predicate_text` is guaranteed true whenever `test` is.

    Anything that also needs a feature flag, a target, or any other atom
    this evaluator cannot resolve to True is NOT test-safe -- the item may
    legitimately be absent from the configuration this check reads, so it
    is excluded rather than risking a false positive.
    """
    tokens = _CFG_TOKEN_RE.findall(predicate_text)
    if not tokens:
        return False
    try:
        pos = [0]
        result = _eval_cfg_tokens(tokens, pos)
    except (IndexError, KeyError):
        return False
    return result == _TRUE


def combined_cfg_test_safe(predicates: list[str]) -> bool:
    """Stacked `#[cfg(...)]` attributes on one item AND together."""
    return all(cfg_is_test_safe(p) for p in predicates)


# --------------------------------------------------------------------------
# In-file scan: outer-attribute runs (-> mod-level candidates) and inner
# attributes (-> file-level or mod-level candidates), in one forward pass.
# --------------------------------------------------------------------------


@dataclass
class _Candidate:
    lints: dict[str, int]  # lint name -> attribute's own line (1-based)
    scope: str  # "file" | f"mod {name}"
    body_start: int  # 0-based line index, inclusive
    body_end: int  # 0-based line index, inclusive
    cfg_predicates: list[str] = field(default_factory=list)


def _parse_attr_extent(stripped: list[str], li: int) -> tuple[int, int] | None:
    """(end_li, end_col) of the `]` matching the `[` on line `li`."""
    ci = stripped[li].index("[")
    return find_matching(stripped, li, ci, "[", "]")


def _lint_names(stripped: list[str], start_li: int, end_li: int) -> set[str]:
    span = "\n".join(stripped[start_li : end_li + 1])
    return set(LINT_NAME_RE.findall(span)) & set(TRIGGERS)


def _cfg_predicate(stripped: list[str], start_li: int, end_li: int) -> str | None:
    m = CFG_ATTR_RE.match(stripped[start_li]) if start_li == end_li else None
    if m:
        return m.group(1)
    return None  # multi-line cfg: unparsed -> caller must treat conservatively


def scan_file(stripped: list[str]) -> list[_Candidate]:
    n = len(stripped)
    candidates: list[_Candidate] = []
    # terminal mod-header line -> cfg predicates gating it from outside,
    # populated as outer-attribute runs are consumed, consulted later when
    # an inner `#![expect(...)]` sits just inside that same mod.
    mod_outer_cfg: dict[int, list[str]] = {}
    unparsed_cfg: dict[int, bool] = {}

    li = 0
    while li < n:
        line = stripped[li]
        if line.strip() == "":
            li += 1
            continue

        m = ATTR_START_RE.match(line)
        if not m:
            li += 1
            continue
        is_inner = m.group(2) == "!"

        if is_inner:
            extent = _parse_attr_extent(stripped, li)
            if extent is None:
                li += 1
                continue
            end_li, _end_col = extent
            # INVARIANT: only `expect(...)` participates in unfulfilled-lint-
            # expectations. `_lint_names` matches any `clippy::LINT` text in
            # the bracket span, so without this gate a `#![deny(clippy::foo,
            # clippy::unwrap_used)]` policy line reads its lint names as if
            # they were expectations, which is never what rustc reports on.
            lints = _lint_names(stripped, li, end_li) if EXPECT_ATTR_START_RE.match(line) else set()
            if lints:
                enclosing = find_enclosing_open(stripped, li)
                if enclosing is None:
                    # true file scope
                    body_start, body_end = 0, n - 1
                    cfg_preds: list[str] = []
                    scope = "file"
                    ok = True
                else:
                    open_li, open_col = enclosing
                    header = stripped[open_li]
                    hm = MOD_HEADER_RE.match(header)
                    if hm is None:
                        ok = False
                        scope = ""
                        body_start = body_end = 0
                        cfg_preds = []
                    else:
                        close = find_matching(stripped, open_li, open_col, "{", "}")
                        if close is None:
                            ok = False
                            scope = ""
                            body_start = body_end = 0
                            cfg_preds = []
                        else:
                            ok = True
                            scope = f"mod {hm.group(1)}"
                            body_start, body_end = open_li + 1, close[0]
                            cfg_preds = mod_outer_cfg.get(open_li, [])
                            if unparsed_cfg.get(open_li):
                                ok = False
                if ok:
                    lint_lines = {name: li + 1 for name in lints}
                    candidates.append(
                        _Candidate(lint_lines, scope, body_start, body_end, cfg_preds)
                    )
            li = end_li + 1
            continue

        # Outer attribute: collect the contiguous run up to its terminal item.
        run_start = li
        run_lints: dict[str, int] = {}
        run_cfg: list[str] = []
        run_cfg_unparsed = False
        cursor = li
        while cursor < n:
            cur_line = stripped[cursor]
            if cur_line.strip() == "":
                cursor += 1
                continue
            am = ATTR_START_RE.match(cur_line)
            if not am or am.group(2) == "!":
                break  # non-attribute (or an inner attr, which starts a new unit) ends the run
            extent = _parse_attr_extent(stripped, cursor)
            if extent is None:
                cursor += 1
                break
            end_li, _end_col = extent
            if EXPECT_ATTR_START_RE.match(cur_line):
                for name in _lint_names(stripped, cursor, end_li):
                    run_lints.setdefault(name, cursor + 1)
            else:
                pred = _cfg_predicate(stripped, cursor, end_li)
                if re.match(r"^\s*#!?\[cfg\(", cur_line):
                    if pred is not None:
                        run_cfg.append(pred)
                    else:
                        run_cfg_unparsed = True
            cursor = end_li + 1
        terminal_li = cursor
        while terminal_li < n and stripped[terminal_li].strip() == "":
            terminal_li += 1

        if terminal_li < n:
            hm = MOD_HEADER_RE.match(stripped[terminal_li])
            if hm is not None:
                mod_outer_cfg[terminal_li] = run_cfg
                unparsed_cfg[terminal_li] = run_cfg_unparsed
                if run_lints and not run_cfg_unparsed:
                    close = find_matching(
                        stripped, terminal_li, stripped[terminal_li].index("{"), "{", "}"
                    )
                    if close is not None:
                        candidates.append(
                            _Candidate(
                                dict(run_lints),
                                f"mod {hm.group(1)}",
                                terminal_li + 1,
                                close[0],
                                run_cfg,
                            )
                        )
                # else: run_lints on a non-mod item (fn/struct/impl/...), or
                # an unparsed cfg on a mod -- both out of the reliable subset.
        li = run_start + 1 if cursor == run_start else cursor

    return candidates


def _find_declaring_statement(target: Path) -> tuple[list[str]] | None:
    """One-hop: cfg predicates gating the `mod`/`#[path]` statement that
    declares `target`, or None if no such statement was found (default:
    unconditionally reachable -- see module docstring)."""
    directory = target.parent
    if target.name == "mod.rs":
        search_dir = directory.parent
        decl_stem = directory.name
    else:
        search_dir = directory
        decl_stem = target.stem

    if not search_dir.is_dir():
        return None

    for f in sorted(search_dir.glob("*.rs")):
        try:
            text = f.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        # PERF: strip_noncode is the expensive step (a per-character scan).
        # A file that mentions neither the module name nor the exact
        # filename anywhere cannot possibly declare `target` -- skip the
        # scan entirely rather than stripping every unrelated peer in a
        # directory once per file-level candidate that lives in it.
        if decl_stem not in text and target.name not in text:
            continue
        stripped = strip_noncode(text).splitlines()
        pending_path: str | None = None
        cfg_run: list[str] = []
        for line in stripped:
            s = line.strip()
            if s == "":
                continue
            pm = PATH_ATTR_RE.match(line)
            if pm:
                pending_path = pm.group(1)
                continue
            cm = CFG_ATTR_RE.match(line)
            if cm:
                cfg_run.append(cm.group(1))
                continue
            dm = MOD_DECL_RE.match(line)
            if dm:
                matches = (
                    pending_path == target.name
                    if pending_path is not None
                    else dm.group(1) == decl_stem
                )
                if matches:
                    return (list(cfg_run),)
                pending_path = None
                cfg_run = []
                continue
            pending_path = None
            cfg_run = []
    return None


def file_is_test_safe(path: Path) -> bool:
    found = _find_declaring_statement(path)
    if found is None:
        return True  # not found -> default reachable (see docstring)
    (cfg_predicates,) = found
    return combined_cfg_test_safe(cfg_predicates)


def _resolve_child_file(declaring_file: Path, mod_name: str, path_attr: str | None) -> Path | None:
    """The file a `mod NAME;` / `#[path = "..."] mod NAME;` declaration
    inside `declaring_file` brings into the module tree, or None if no such
    file exists on disk."""
    if path_attr is not None:
        candidate = declaring_file.parent / path_attr
        return candidate if candidate.is_file() else None
    base = (
        declaring_file.parent
        if declaring_file.name == "mod.rs"
        else declaring_file.parent / declaring_file.stem
    )
    for candidate in (base / f"{mod_name}.rs", base / mod_name / "mod.rs"):
        if candidate.is_file():
            return candidate
    return None


def _gather_child_content(declaring_file: Path, span_lines: list[str], visited: set[Path]) -> str:
    """Stripped content of every file a `mod`/`#[path]` declaration inside
    `span_lines` brings into the module tree, gathered recursively (test
    files here are commonly split more than one level deep).

    WHY: a lint-level attribute's scope is the module it sits on PLUS every
    descendant item, regardless of which physical file that descendant's
    text lives in -- exactly like `#[allow(...)]`. An umbrella `mod.rs` that
    only declares child test modules (`crates/daemon/src/runner_tests/mod.rs`
    declares `mod cron_and_output;` etc. and contains no test code itself)
    would otherwise search an almost-empty file and report every child's
    real `.unwrap()`/`.expect()` calls as absent. Gating on the CHILD's own
    cfg is deliberately skipped: finding the trigger there is always safe
    evidence of fulfillment (the risk this script exists to avoid is only
    ever "flag something that is actually fine", never "stay quiet on
    something that is actually fine").
    """
    extra: list[str] = []
    pending_path: str | None = None
    for line in span_lines:
        s = line.strip()
        if s == "":
            continue
        pm = PATH_ATTR_RE.match(line)
        if pm:
            pending_path = pm.group(1)
            continue
        dm = MOD_DECL_RE.match(line)
        if dm:
            child = _resolve_child_file(declaring_file, dm.group(1), pending_path)
            pending_path = None
            if child is None or child in visited:
                continue
            visited.add(child)
            try:
                child_text = child.read_text(encoding="utf-8", errors="replace")
            except OSError:
                continue
            child_stripped = strip_noncode(child_text).splitlines()
            extra.append("\n".join(child_stripped))
            extra.append(_gather_child_content(child, child_stripped, visited))
            continue
        pending_path = None
    return "\n".join(e for e in extra if e)


# --------------------------------------------------------------------------
# Driver
# --------------------------------------------------------------------------


def tracked_rs_files() -> list[str]:
    out = subprocess.run(
        ["git", "ls-files", "-z", "--", "*.rs"],
        cwd=REPO_ROOT,
        capture_output=True,
        check=True,
    )
    return [p for p in out.stdout.decode("utf-8", "replace").split("\0") if p]


def check_text(path: Path, text: str, *, skip_file_cfg_check: bool = False) -> list[Violation]:
    """Violations in one file's already-read `text`. `path` is used only for
    reporting and (for file-level candidates) cross-file cfg resolution;
    pass `skip_file_cfg_check=True` in tests that do not set up a real tree.
    """
    if "expect(clippy::" not in text:
        return []

    stripped_lines = strip_noncode(text).splitlines()
    violations: list[Violation] = []

    for cand in scan_file(stripped_lines):
        if not combined_cfg_test_safe(cand.cfg_predicates):
            continue
        if cand.scope == "file" and not skip_file_cfg_check and not file_is_test_safe(path):
            continue

        scope_lines = stripped_lines[cand.body_start : cand.body_end + 1]
        span = "\n".join(scope_lines)
        extra = _gather_child_content(path, scope_lines, {path})
        if extra:
            span = span + "\n" + extra
        for lint, attr_line in sorted(cand.lints.items(), key=lambda kv: kv[1]):
            if not any(trigger in span for trigger in TRIGGERS[lint]):
                violations.append(Violation(str(path), attr_line, lint, cand.scope))

    return violations


def main() -> int:
    violations: list[Violation] = []
    for rel in tracked_rs_files():
        full = REPO_ROOT / rel
        try:
            text = full.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        if "expect(clippy::" not in text:
            continue
        violations.extend(check_text(full, text))

    if not violations:
        LOGGER.info("unfulfilled-expect check: clean")
        return 0

    LOGGER.error("unfulfilled-expect check FAILED: %d attribute(s) cannot fire", len(violations))
    for v in sorted(violations, key=lambda v: (v.path, v.line)):
        rel = Path(v.path).relative_to(REPO_ROOT) if Path(v.path).is_absolute() else v.path
        LOGGER.error(
            "  %s:%d: clippy::%s never triggers in its %s scope -- unfulfilled",
            rel,
            v.line,
            v.lint,
            v.scope,
        )
    LOGGER.error("")
    LOGGER.error(
        "Under -D warnings this is a compile error (unfulfilled-lint-expectations)."
    )
    LOGGER.error(
        "Drop the lint from #[expect(...)] (or the whole attribute if none remain)."
    )
    return 1


if __name__ == "__main__":
    logging.basicConfig(format="%(message)s", level=logging.INFO, stream=sys.stderr)
    raise SystemExit(main())
