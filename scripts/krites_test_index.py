#!/usr/bin/env python3
"""Enumerate a Rust crate's test cases as nextest-addressable ids, from source.

CAPABILITY_MATRIX.toml's `gate_test` field names the test that would fail if a
capability disappeared. A pointer nobody resolves is a second kind of prose, so
the checker has to answer "does this test exist, and is it runnable?" -- and it
has to answer in the pure-python `gate-coverage-scripts` CI job, which has no
cargo and no compiled test binaries.

This module reconstructs what `cargo nextest list` would print, by walking the
crate's module tree the way rustc does:

  binary id   the lib target is the package name (`krites`); each `tests/*.rs`
              or `tests/*/main.rs` target is `<package>::<stem>`
  test path   the `::`-joined module path from the target root to the `fn`,
              following `mod x;` to `x.rs`/`x/mod.rs`, entering inline
              `mod x { ... }`, and honouring `#[path = "..."]`
  test        a `fn` carrying an attribute whose final path segment is `test`
              (`#[test]`, `#[tokio::test]`, `#[test_log::test]`, ...)
  ignored     that same `fn` also carrying `#[ignore]` / `#[ignore = "..."]`

The full id is `<binary id>::<test path>`, which is what `gate_test` records.

WARNING(superset over cfg): rustc compiles ONE arm of a `#[cfg]` pair; this
walker enters both, because resolving cfg without cargo means guessing a
feature set. The index is therefore a superset of any single build's test list.
A `gate_test` naming a test that exists only under a feature the CI job does
not build still resolves here. `cross_validate()` measures that gap against a
real `cargo nextest list --message-format json` dump, and
check-krites-capability-matrix.py exposes it as `--nextest-list` so the gap is
reported rather than assumed to be zero.

Usage:
    python3 scripts/krites_test_index.py <crate-dir> [--json]
    python3 scripts/krites_test_index.py <crate-dir> --cross-validate <list.json>
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path

# WHY a depth cap rather than a visited-file set: `#[path]` legitimately reaches
# one file from two module paths (runtime/hnsw's cfg pair does exactly this), so
# a file already seen is not proof of a cycle. The cap bounds a genuine cycle --
# two modules whose `#[path]` attributes point at each other -- without
# rejecting the legal aliasing.
MAX_MODULE_DEPTH = 32

_ITEM_MOD_RE = re.compile(r"(?:pub\s*(?:\([^)]*\)\s*)?)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*(?=[;{])")
_ITEM_FN_RE = re.compile(
    r"(?:pub\s*(?:\([^)]*\)\s*)?)?"
    r"(?:default\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?"
    r'(?:extern\s+(?:"[^"]*"\s+)?)?'
    r"fn\s+([A-Za-z_][A-Za-z0-9_]*)"
)
_PATH_ATTR_RE = re.compile(r'^path\s*=\s*"([^"]*)"$')
_IGNORE_ATTR_RE = re.compile(r"^ignore\b")
_TEST_ATTR_RE = re.compile(r"^([A-Za-z_][A-Za-z0-9_]*::)*test\b")


@dataclass(frozen=True)
class TestCase:
    """One `#[test]`-shaped function, addressed the way nextest addresses it."""

    binary_id: str
    test_path: str
    ignored: bool
    file: str
    line: int
    cfg_guards: tuple[str, ...]

    @property
    def test_id(self) -> str:
        return f"{self.binary_id}::{self.test_path}"


def strip_noise(text: str) -> str:
    """Blank out comments and literal contents, preserving length and newlines.

    WHY length-preserving: every offset this module reports (a test's line
    number, an attribute's span) is computed on the stripped text and must name
    the same place in the real file.

    WHY at all: krites embeds Datalog scripts as raw strings, and those scripts
    contain `fn`, `mod` and `#[...]`-shaped text. A scanner that reads literal
    contents as code invents modules and tests that do not exist -- and an index
    that over-reports is exactly what lets a `gate_test` pointer resolve to
    nothing.
    """
    out = list(text)
    i = 0
    n = len(text)

    def blank(start: int, end: int) -> None:
        for k in range(start, min(end, n)):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        ch = text[i]
        if ch == "/" and text[i : i + 2] == "//":
            j = text.find("\n", i)
            j = n if j == -1 else j
            blank(i, j)
            i = j
        elif ch == "/" and text[i : i + 2] == "/*":
            depth = 0
            j = i
            while j < n:
                if text[j : j + 2] == "/*":
                    depth += 1
                    j += 2
                elif text[j : j + 2] == "*/":
                    depth -= 1
                    j += 2
                    if depth == 0:
                        break
                else:
                    j += 1
            blank(i, j)
            i = j
        elif ch == "r" and (m := re.match(r'r(#*)"', text[i:])):
            hashes = m.group(1)
            terminator = '"' + hashes
            j = text.find(terminator, i + m.end())
            j = n if j == -1 else j + len(terminator)
            blank(i + m.end(), j - len(terminator))
            i = j
        elif ch == '"':
            j = i + 1
            while j < n:
                if text[j] == "\\":
                    j += 2
                    continue
                if text[j] == '"':
                    j += 1
                    break
                j += 1
            blank(i + 1, j - 1)
            i = j
        elif ch == "'":
            # WHY the lifetime guard: `'a` is not a char literal, and treating
            # it as an unterminated one blanks the rest of the file.
            m = re.match(r"'(?:\\.|[^\\'])'", text[i:])
            if m:
                blank(i + 1, i + m.end() - 1)
                i += m.end()
            else:
                i += 1
        else:
            i += 1
    return "".join(out)


def _read_attribute(text: str, pos: int) -> tuple[int, int, int] | None:
    """Parse a bracket-matched `#[...]` / `#![...]` at `pos`.

    Returns (inner start, inner end, index just past the closing bracket), or
    None. Bracket-matched rather than regex-terminated so a nested
    `#[cfg(all(a, b))]` or an attribute containing `]` inside a string is not
    truncated early.

    WHY offsets and not the text: the caller scans the noise-stripped source but
    must read an attribute's VALUE from the raw source. `#[path = "x/mod.rs"]`
    is the case that forces this -- stripping blanks the literal, and a blank
    `path` silently resolves the module to nothing while looking like a module
    that has no file.
    """
    if text[pos] != "#":
        return None
    j = pos + 1
    if j < len(text) and text[j] == "!":
        j += 1
    if j >= len(text) or text[j] != "[":
        return None
    depth = 0
    k = j
    while k < len(text):
        if text[k] == "[":
            depth += 1
        elif text[k] == "]":
            depth -= 1
            if depth == 0:
                return j + 1, k, k + 1
        k += 1
    return None


def _module_dir(file_path: Path, is_target_root: bool) -> Path:
    """The directory `mod x;` inside `file_path` resolves against."""
    if is_target_root or file_path.name == "mod.rs":
        return file_path.parent
    return file_path.parent / file_path.stem


def _resolve_mod_file(base_dir: Path, name: str, path_attr: str | None) -> Path | None:
    if path_attr is not None:
        candidate = (base_dir / path_attr).resolve()
        return candidate if candidate.is_file() else None
    flat = base_dir / f"{name}.rs"
    if flat.is_file():
        return flat
    nested = base_dir / name / "mod.rs"
    if nested.is_file():
        return nested
    return None


def _cfg_of(attrs: list[str]) -> tuple[str, ...]:
    return tuple(a for a in attrs if a.startswith("cfg("))


def _is_test_attr(attrs: list[str]) -> bool:
    return any(_TEST_ATTR_RE.match(a) for a in attrs)


def _is_ignored(attrs: list[str]) -> bool:
    return any(_IGNORE_ATTR_RE.match(a) for a in attrs)


def _path_attr(attrs: list[str]) -> str | None:
    for a in attrs:
        m = _PATH_ATTR_RE.match(a)
        if m:
            return m.group(1)
    return None


def _walk_file(  # noqa: C901
    file_path: Path,
    repo_root: Path,
    binary_id: str,
    mod_prefix: tuple[str, ...],
    inherited_cfg: tuple[str, ...],
    is_target_root: bool,
    depth: int,
    out: dict[str, TestCase],
    unresolved: list[str],
) -> None:
    if depth > MAX_MODULE_DEPTH:
        unresolved.append(f"{file_path}: module nesting exceeded {MAX_MODULE_DEPTH}")
        return
    raw = file_path.read_text(encoding="utf-8", errors="replace")
    text = strip_noise(raw)
    line_starts = [0]
    for idx, ch in enumerate(raw):
        if ch == "\n":
            line_starts.append(idx + 1)

    def line_of(offset: int) -> int:
        lo, hi = 0, len(line_starts) - 1
        while lo < hi:
            mid = (lo + hi + 1) // 2
            if line_starts[mid] <= offset:
                lo = mid
            else:
                hi = mid - 1
        return lo + 1

    base_dir = _module_dir(file_path, is_target_root)
    pending: list[str] = []
    file_cfg: list[str] = []
    brace_depth = 0
    # Stack of (module name, brace depth of the module's interior).
    mod_stack: list[tuple[str, int]] = []
    cfg_stack: list[tuple[tuple[str, ...], int]] = []
    i = 0
    n = len(text)
    while i < n:
        ch = text[i]
        if ch in " \t\r\n":
            i += 1
            continue
        if ch == "#":
            parsed = _read_attribute(text, i)
            if parsed is not None:
                start, end, nxt = parsed
                inner = raw[start:end].strip()
                if text[i : i + 2] == "#!":
                    file_cfg.extend(_cfg_of([inner]))
                else:
                    pending.append(inner)
                i = nxt
                continue
        m = _ITEM_MOD_RE.match(text, i)
        if m:
            name = m.group(1)
            attrs = pending
            pending = []
            local_cfg = _cfg_of(attrs)
            j = m.end()
            while j < n and text[j] in " \t\r\n":
                j += 1
            if text[j] == "{":
                mod_stack.append((name, brace_depth + 1))
                cfg_stack.append((local_cfg, brace_depth + 1))
                i = m.end()
                continue
            cfg = (
                inherited_cfg
                + tuple(file_cfg)
                + tuple(c for cfgs, _ in cfg_stack for c in cfgs)
                + local_cfg
            )
            target = _resolve_mod_file(base_dir, name, _path_attr(attrs))
            if target is None:
                unresolved.append(
                    f"{file_path.relative_to(repo_root)}:{line_of(m.start())}: "
                    f"`mod {name};` resolves to no file under {base_dir}"
                )
            else:
                _walk_file(
                    target,
                    repo_root,
                    binary_id,
                    mod_prefix + tuple(s[0] for s in mod_stack) + (name,),
                    cfg,
                    False,
                    depth + 1,
                    out,
                    unresolved,
                )
            i = j + 1
            continue
        m = _ITEM_FN_RE.match(text, i)
        if m:
            attrs = pending
            pending = []
            if _is_test_attr(attrs):
                path_parts = mod_prefix + tuple(s[0] for s in mod_stack) + (m.group(1),)
                case = TestCase(
                    binary_id=binary_id,
                    test_path="::".join(path_parts),
                    ignored=_is_ignored(attrs),
                    file=str(file_path.relative_to(repo_root)),
                    line=line_of(m.start()),
                    cfg_guards=inherited_cfg
                    + tuple(file_cfg)
                    + tuple(c for cfgs, _ in cfg_stack for c in cfgs)
                    + _cfg_of(attrs),
                )
                # WHY setdefault and not assignment: the cfg superset can reach
                # one test through two module arms. Both produce the same id;
                # keeping the first is stable and never inflates the count.
                out.setdefault(case.test_id, case)
            i = m.end()
            continue
        if ch == "{":
            brace_depth += 1
        elif ch == "}":
            brace_depth -= 1
            while mod_stack and mod_stack[-1][1] > brace_depth:
                mod_stack.pop()
            while cfg_stack and cfg_stack[-1][1] > brace_depth:
                cfg_stack.pop()
        pending = []
        i += 1


def _package_name(crate_dir: Path) -> str:
    with (crate_dir / "Cargo.toml").open("rb") as fh:
        return tomllib.load(fh)["package"]["name"]


def _test_targets(crate_dir: Path) -> list[tuple[str, Path]]:
    """Cargo's auto-discovered integration-test targets: `tests/*.rs` and
    `tests/*/main.rs`."""
    tests_dir = crate_dir / "tests"
    if not tests_dir.is_dir():
        return []
    targets: list[tuple[str, Path]] = []
    for entry in sorted(tests_dir.iterdir()):
        if entry.is_file() and entry.suffix == ".rs":
            targets.append((entry.stem, entry))
        elif entry.is_dir() and (entry / "main.rs").is_file():
            targets.append((entry.name, entry / "main.rs"))
    return targets


def build_index(crate_dir: Path, repo_root: Path) -> tuple[dict[str, TestCase], list[str]]:
    """Return ({test id: TestCase}, [unresolved-module diagnostics])."""
    crate_dir = crate_dir.resolve()
    package = _package_name(crate_dir)
    out: dict[str, TestCase] = {}
    unresolved: list[str] = []

    lib_root = crate_dir / "src" / "lib.rs"
    if lib_root.is_file():
        _walk_file(lib_root, repo_root, package, (), (), True, 0, out, unresolved)

    for stem, path in _test_targets(crate_dir):
        _walk_file(path, repo_root, f"{package}::{stem}", (), (), True, 0, out, unresolved)

    return out, unresolved


def load_nextest_list(path: Path) -> dict[str, bool]:
    """Read `cargo nextest list --message-format json` into {test id: ignored}."""
    payload = json.loads(path.read_text(encoding="utf-8"))
    result: dict[str, bool] = {}
    for suite in payload.get("rust-suites", {}).values():
        binary_id = suite.get("binary-id")
        for name, meta in (suite.get("testcases") or {}).items():
            result[f"{binary_id}::{name}"] = bool(meta.get("ignored"))
    return result


def cross_validate(index: dict[str, TestCase], truth: dict[str, bool]) -> dict[str, list[str]]:
    """Compare the source-derived index against a real nextest listing.

    `only_in_index` is the cfg superset this module documents (plus any walker
    defect). `only_in_nextest` is the dangerous direction: a test rustc compiles
    that this walker never found, so a correct `gate_test` would be reported
    missing. `ignored_disagrees` catches an `#[ignore]` the walker misread.
    """
    return {
        "only_in_index": sorted(set(index) - set(truth)),
        "only_in_nextest": sorted(set(truth) - set(index)),
        "ignored_disagrees": sorted(
            tid for tid, ign in truth.items() if tid in index and index[tid].ignored != ign
        ),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("crate_dir", type=Path)
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--cross-validate", type=Path, default=None)
    args = parser.parse_args()

    repo_root = Path(__file__).resolve().parents[1]
    index, unresolved = build_index(args.crate_dir, repo_root)

    for problem in unresolved:
        print(f"unresolved: {problem}", file=sys.stderr)

    if args.cross_validate is not None:
        delta = cross_validate(index, load_nextest_list(args.cross_validate))
        print(json.dumps(delta, indent=2) if args.json else "")
        if not args.json:
            for key, values in delta.items():
                print(f"{key}: {len(values)}")
                for value in values[:20]:
                    print(f"    {value}")
        return 1 if delta["only_in_nextest"] or delta["ignored_disagrees"] else 0

    if args.json:
        print(
            json.dumps(
                {
                    tid: {"ignored": c.ignored, "file": c.file, "line": c.line}
                    for tid, c in sorted(index.items())
                },
                indent=2,
            )
        )
    else:
        for tid in sorted(index):
            print(f"{tid}{'  IGNORED' if index[tid].ignored else ''}")
        print(f"\n{len(index)} test cases", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
