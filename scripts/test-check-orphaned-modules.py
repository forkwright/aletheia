#!/usr/bin/env python3
"""Tests for check-orphaned-modules.py.

Covers the two failure modes the checker exists to catch (a file with no
`mod` declaration reaching it; a `mod` declaration that resolves to nothing)
plus the literal-blanking regressions found while building it: a `#[path]`
attribute's own string content must survive so the override still resolves,
while a `mod x;`-shaped string in a test fixture and a brace char literal
(`'{'`) must NOT be read as real source.
"""

from __future__ import annotations

import importlib.util
import sys
import tempfile
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    "check_orphaned_modules",
    Path(__file__).resolve().parent / "check-orphaned-modules.py",
)
assert SPEC and SPEC.loader
CHECK = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = CHECK
SPEC.loader.exec_module(CHECK)

FAILURES: list[str] = []


def write_crate(root: Path, files: dict[str, str]) -> Path:
    """Build a fake crate's src/ tree under `root` from a {relpath: content} map."""
    src_dir = root / "src"
    for relpath, content in files.items():
        p = src_dir / relpath
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(content)
    return root


def reached_relpaths(result: "CHECK.CrateResult") -> set[str]:
    return {p.relative_to(result.src_dir).as_posix() for p in result.reached}


def orphans(result: "CHECK.CrateResult") -> set[str]:
    return {p.relative_to(result.src_dir).as_posix() for p in (result.all_files - result.reached)}


def expect(label: str, cond: bool, detail: str = "") -> None:
    if not cond:
        FAILURES.append(f"{label}: {detail}" if detail else label)


# --------------------------------------------------------------------------
# strip_comments: literal blanking (strings, char literals, path attrs)


def test_strip_comments_blanks_string_literal_mod_shape() -> None:
    # WHY: a test fixture that hands a synthetic ".rs" source as a plain
    # string must not have its embedded `mod foo;`-looking text read as a
    # real declaration -- this was a genuine false positive found against
    # crates/gnosis/src/index_tests.rs before literal-blanking existed.
    text = 'std::fs::write(&p, "pub mod foo;\\npub fn keep() {}").unwrap();'
    decls = CHECK.scan_mod_decls(CHECK.strip_comments(text))
    expect("string-literal mod shape", decls == [], f"got {decls!r}")


def test_strip_comments_preserves_path_attr_string() -> None:
    # WHY: blanking every string literal ALSO blanked the string inside
    # `#[path = "..."]`, silently discarding the override and reporting a
    # real, correctly-wired file as unresolved. Regression guard.
    text = '#[path = "providers_dto.rs"]\nmod providers_dto;\n'
    decls = CHECK.scan_mod_decls(CHECK.strip_comments(text))
    expect("path-attr survives strip", len(decls) == 1, f"got {decls!r}")
    if decls:
        expect("path-attr value preserved", decls[0].path_attr == "providers_dto.rs", f"got {decls[0].path_attr!r}")


def test_strip_comments_char_literal_brace_does_not_desync_nesting() -> None:
    # WHY: a bare `'}'` char literal is a single token to TOKEN_RE if not
    # blanked -- inside a NAMED inline mod, its phantom close prematurely
    # pops that mod's stack entry, so a real nested `mod inner;` right
    # after it resolves with the wrong (empty) virtual path instead of
    # ("outer",) -- i.e. `resolve_mod` would look in the wrong directory.
    # Verified present in this repo (crates/taxis/src/interpolate.rs and
    # others) before char-literal blanking existed.
    text = "mod outer {\n    fn f(c: char) { if c == '}' { } }\n    mod inner;\n}\n"
    decls = CHECK.scan_mod_decls(CHECK.strip_comments(text))
    expect("mod inner found once", len(decls) == 1, f"got {decls!r}")
    if decls:
        expect("mod inner nested under outer", decls[0].virtual_path == ("outer",), f"got {decls[0].virtual_path!r}")


def test_strip_comments_does_not_eat_lifetime() -> None:
    # A lifetime (`'a`) has no closing quote nearby and must not be treated
    # as a char literal -- the char-literal pattern requires exactly one
    # char/escape immediately followed by a closing `'`.
    text = "fn f<'a>(x: &'a str) -> &'a str { x }\nmod after;\n"
    decls = CHECK.scan_mod_decls(CHECK.strip_comments(text))
    expect("mod after lifetimes found", len(decls) == 1, f"got {decls!r}")


def test_strip_comments_raw_string_with_hashes() -> None:
    text = 'let s = r#"mod fake; "#;\nmod after;\n'
    decls = CHECK.scan_mod_decls(CHECK.strip_comments(text))
    expect("mod after raw string found", len(decls) == 1, f"got {decls!r}")
    if decls:
        expect("only 'after' found", decls[0].name == "after", f"got {decls[0].name!r}")


# --------------------------------------------------------------------------
# scan_mod_decls: nesting and nested-mod resolution


def test_nested_inline_mod_virtual_path() -> None:
    text = "mod outer {\n    mod inner;\n}\n"
    decls = CHECK.scan_mod_decls(CHECK.strip_comments(text))
    expect("one decl for nested inline mod", len(decls) == 1, f"got {decls!r}")
    if decls:
        expect("inner name", decls[0].name == "inner", f"got {decls[0].name!r}")
        expect("virtual path is (outer,)", decls[0].virtual_path == ("outer",), f"got {decls[0].virtual_path!r}")


# --------------------------------------------------------------------------
# walk_crate: end-to-end filesystem resolution


def test_walk_crate_all_reachable() -> None:
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        crate_dir = write_crate(
            root,
            {
                "lib.rs": "mod a;\nmod sub {\n    mod nested;\n}\n",
                "a.rs": "pub fn f() {}\n",
                "sub/nested.rs": "pub fn g() {}\n",
            },
        )
        result = CHECK.walk_crate("fixture", crate_dir, repo_root=root)
        expect("no orphans in fully-wired crate", orphans(result) == set(), f"got {orphans(result)!r}")
        expect("no unresolved decls", result.unresolved == [], f"got {result.unresolved!r}")


def test_walk_crate_detects_real_orphan() -> None:
    # WHY: this is the exact defect class the checker exists to catch --
    # verified against crates/pylon/src/tests/metrics.rs (created, never
    # wired) before this fix landed.
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        crate_dir = write_crate(
            root,
            {
                "lib.rs": "mod a;\n",
                "a.rs": "pub fn f() {}\n",
                "forgotten.rs": "pub fn dead() {}\n",
            },
        )
        result = CHECK.walk_crate("fixture", crate_dir, repo_root=root)
        expect("forgotten.rs is orphaned", orphans(result) == {"forgotten.rs"}, f"got {orphans(result)!r}")


def test_walk_crate_detects_unresolved_mod() -> None:
    # WHY: verified against crates/taxis/src/config_tests.rs before this
    # fix landed -- a dangling #[path] left pointing at a target that was
    # renamed/deleted out from under it.
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        crate_dir = write_crate(
            root,
            {
                "lib.rs": "mod a;\n",
                "a.rs": "mod missing;\n",
            },
        )
        result = CHECK.walk_crate("fixture", crate_dir, repo_root=root)
        expect("one unresolved decl", len(result.unresolved) == 1, f"got {result.unresolved!r}")


def test_walk_crate_path_attr_redirect_resolves() -> None:
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        crate_dir = write_crate(
            root,
            {
                "lib.rs": '#[path = "b.rs"]\nmod a;\n',
                "b.rs": "pub fn f() {}\n",
            },
        )
        result = CHECK.walk_crate("fixture", crate_dir, repo_root=root)
        expect("path-attr redirect resolves", orphans(result) == set(), f"got {orphans(result)!r}")
        expect("path-attr redirect has no unresolved", result.unresolved == [], f"got {result.unresolved!r}")


def main() -> int:
    tests = [v for k, v in sorted(globals().items()) if k.startswith("test_") and callable(v)]
    for t in tests:
        t()

    if FAILURES:
        for f in FAILURES:
            print(f"FAIL: {f}", file=sys.stderr)
        print(f"\n{len(FAILURES)} failure(s) across {len(tests)} test functions", file=sys.stderr)
        return 1

    print(f"OK: {len(tests)} test functions passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
