#!/usr/bin/env python3
"""Reject a workspace crate's `src/` tree containing a `.rs` file unreachable
from that crate's own entry points (`src/lib.rs`, `src/main.rs`, `src/bin/*.rs`)
by its `mod` chain.

WHY this check exists: basanos' substrate-orphan rule (kanon crates/basanos/
src/rules/architecture/_substrate.rs) checks a different, coarser thing -- a
shared workspace crate declared but never imported by another crate. Nothing
checks the finer-grained case: a file sitting under a crate's own `src/` tree
that no `mod` declaration, anywhere in that crate, ever reaches. Such a file
does not compile into the crate at all -- `cargo` silently ignores it -- so it
rots invisibly: stale code, an abandoned refactor, a file `git mv`'d without
its `mod` declaration following.

Resolution model (Rust 2018+ path rules, no `cargo`/`syn` dependency --
stdlib only):

- A file named `lib.rs`, `main.rs`, or `mod.rs` governs its OWN directory: a
  `mod child;` inside it resolves to `<dir>/child.rs` or `<dir>/child/mod.rs`.
- Any other file `name.rs` governs a SIBLING directory named after its own
  stem: `mod child;` inside `foo/bar.rs` resolves to `foo/bar/child.rs` or
  `foo/bar/child/mod.rs`.
- `#[path = "P"] mod child;` overrides resolution: the target is `P` taken
  relative to the directory containing the file with the attribute (not the
  governed directory above -- this is the one place those two differ).
- `mod child { ... }` (brace body, no semicolon) is self-contained: no file
  resolution needed, but a `mod grandchild;` genuinely nested inside that
  brace body resolves under a further subdirectory named after `child` --
  handled here via brace-depth-aware nesting, not by ignoring the case.

String and char literals are blanked before comments are stripped, then
comments are stripped (block, then line) -- the same "good enough, not a
full lexer" tradeoff `scripts/check-conflict-markers.py` and `scripts/
krites-module-dag.py` make. Literal-blanking matters here specifically: a
`{`/`}` inside a char literal (`'{'`) or a `mod x;`-shaped string in a test
fixture would otherwise desync the brace-depth stack this script relies on
to resolve nested `mod` declarations, or masquerade as a real declaration.

Usage:
    python3 scripts/check-orphaned-modules.py [--crate NAME ...]
"""

from __future__ import annotations

import argparse
import re
import sys
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
WORKSPACE_CARGO = REPO_ROOT / "Cargo.toml"

ENTRY_BASENAMES = {"lib.rs", "main.rs", "mod.rs"}

BLOCK_COMMENT_RE = re.compile(r"/\*.*?\*/", re.DOTALL)
LINE_COMMENT_RE = re.compile(r"//[^\n]*")

# `#[path = "..."]` first and preserved verbatim -- its string content is
# read below by PATH_ATTR_RE and must survive this pass. `include!("...")`
# next, also preserved verbatim -- its content is a same-crate file spliced
# in bodily (e.g. koina's `models.rs` splicing `model_seed_schema.rs` so the
# same types validate at build time and run time), so that file's `mod`
# declarations, once found, must be resolved from the INCLUDING file's own
# position, not treated as unreachable just because no `mod` names it. Then
# raw/byte-raw strings (`r"..."`, `br#"..."#`, hash count 0-8 covers every
# occurrence in this repo and anything higher is not idiomatic Rust), then
# normal/byte strings with escape-aware quote matching, then char literals --
# `'(?:\\.|[^'\\\n])'` requires a closing quote immediately after one
# char/escape, which is what tells a char literal (`'{'`) apart from a
# lifetime (`'a`, no closing quote nearby).
LITERAL_RE = re.compile(
    r'(?P<pathattr>#\[path[ \t]*=[ \t]*"[^"]*"\])'
    r'|(?P<includelit>include!\([ \t]*"[^"]*"[ \t]*\))'
    r'|b?r(?P<hashes>#{0,8})".*?"(?P=hashes)'
    r"|b?\"(?:[^\"\\]|\\.)*\""
    r"|'(?:\\.|[^'\\\n])'",
    re.DOTALL,
)


def _blank_literal(m: re.Match[str]) -> str:
    pathattr = m.group("pathattr")
    if pathattr is not None:
        return pathattr
    includelit = m.group("includelit")
    if includelit is not None:
        return includelit
    return ""


def strip_comments(text: str) -> str:
    text = LITERAL_RE.sub(_blank_literal, text)
    text = BLOCK_COMMENT_RE.sub("", text)
    return LINE_COMMENT_RE.sub("", text)


# Every construct the scan needs to notice, as one alternation so `finditer`
# jumps directly between matches (C-speed) instead of a Python-level
# per-character loop -- this repo's src/ trees run to ~700k lines combined.
#
# - lbrace/rbrace: generic nesting, for virtual_path bookkeeping.
# - attr: a `#[...]` attribute. Non-nested (`[^\]]*` stops at the first `]`)
#   -- sufficient for every attribute this repo places before a `mod`
#   (`cfg`, `path`, `expect`, ...); none nest a further `[`.
# - moddecl: `mod name;` or `mod name {`. The leading negative lookbehind
#   stops an identifier merely ending in "mod" (e.g. `custom_mod xyz;`) from
#   false-positiving as a declaration.
# - include: `include!("relative/path.rs")` -- a same-crate file spliced in
#   bodily rather than declared as a submodule (koina's build-time/run-time
#   schema share is the one instance in this repo). Resolved relative to the
#   INCLUDING file's own directory, per `include!`'s actual path semantics --
#   deliberately NOT the sibling-directory `mod` resolution `module_dir`
#   implements, since an included file is not a submodule.
TOKEN_RE = re.compile(
    r"(?P<lbrace>\{)"
    r"|(?P<rbrace>\})"
    r"|(?P<attr>#\[[^\]]*\])"
    r"|(?<![A-Za-z0-9_])(?:pub(?:\([^)]*\))?[ \t]+)?mod[ \t]+(?P<modname>[A-Za-z_][A-Za-z0-9_]*)[ \t\n]*(?P<modterm>[;{])"
    r'|include!\([ \t]*"(?P<incpath>[^"]*)"[ \t]*\)'
)
PATH_ATTR_RE = re.compile(r'^#\[path[ \t]*=[ \t]*"([^"]+)"\]$')


@dataclass
class ModDecl:
    name: str
    virtual_path: tuple[str, ...]  # ancestor inline-mod names, outermost first
    path_attr: str | None
    include_path: str | None = None  # set instead of the above for an `include!(...)` hit


def scan_mod_decls(text: str) -> list[ModDecl]:
    """Walk comment-stripped `text`, brace-depth-aware, collecting every
    file-backed (`;`) `mod` declaration at any nesting depth, tagged with
    the chain of enclosing inline (`{`) mod names.

    A run of `#[...]` attributes immediately preceding a `mod` -- only
    whitespace between each, in any order -- is tracked so a `#[path]`
    anywhere in that run (`#[cfg(test)] #[path = "..."] #[expect(...)]`) is
    still found; any intervening non-whitespace text, or a brace, resets it
    so it never leaks onto an unrelated declaration.
    """
    decls: list[ModDecl] = []
    stack: list[str | None] = []  # None = non-mod brace; str = inline mod name
    pending_path_attr: str | None = None
    prev_end = 0

    for m in TOKEN_RE.finditer(text):
        if text[prev_end : m.start()].strip():
            pending_path_attr = None  # real code intervened -- attribute chain broken
        prev_end = m.end()

        if m.group("lbrace"):
            stack.append(None)
            pending_path_attr = None
        elif m.group("rbrace"):
            if stack:
                stack.pop()
            pending_path_attr = None
        elif m.group("attr"):
            attr_m = PATH_ATTR_RE.match(m.group("attr"))
            if attr_m is not None:
                pending_path_attr = attr_m.group(1)
        elif m.group("incpath") is not None:
            decls.append(ModDecl(name="", virtual_path=(), path_attr=None, include_path=m.group("incpath")))
            pending_path_attr = None
        else:
            name, terminator = m.group("modname"), m.group("modterm")
            virtual_path = tuple(s for s in stack if s is not None)
            if terminator == ";":
                decls.append(ModDecl(name=name, virtual_path=virtual_path, path_attr=pending_path_attr))
            else:  # "{": inline body -- push and keep scanning inside it
                stack.append(name)
            pending_path_attr = None

    return decls


def module_dir(rs_file: Path) -> Path:
    """The directory a `mod child;` inside `rs_file` resolves relative to
    (before any virtual-path nesting from enclosing inline mods)."""
    if rs_file.name in ENTRY_BASENAMES:
        return rs_file.parent
    return rs_file.parent / rs_file.stem


def resolve_mod(decl: ModDecl, from_file: Path) -> Path | None:
    if decl.include_path is not None:
        target = (from_file.parent / decl.include_path).resolve()
        return target if target.is_file() else None

    if decl.path_attr is not None:
        target = (from_file.parent / decl.path_attr).resolve()
        return target if target.is_file() else None

    base = module_dir(from_file)
    for seg in decl.virtual_path:
        base = base / seg

    candidate_flat = base / f"{decl.name}.rs"
    candidate_dir = base / decl.name / "mod.rs"
    if candidate_flat.is_file():
        return candidate_flat
    if candidate_dir.is_file():
        return candidate_dir
    return None


@dataclass
class CrateResult:
    name: str
    src_dir: Path
    reached: set[Path] = field(default_factory=set)
    all_files: set[Path] = field(default_factory=set)
    unresolved: list[str] = field(default_factory=list)  # "file.rs: mod x; -> no target"


def entry_points(src_dir: Path) -> list[Path]:
    entries = []
    for name in ("lib.rs", "main.rs"):
        p = src_dir / name
        if p.is_file():
            entries.append(p)
    bin_dir = src_dir / "bin"
    if bin_dir.is_dir():
        entries.extend(sorted(bin_dir.glob("*.rs")))
    return entries


def walk_crate(name: str, crate_dir: Path, repo_root: Path = REPO_ROOT) -> CrateResult:
    """`repo_root` is a parameter (not a bare reach into the module-level
    REPO_ROOT) purely so this function stays testable against a fixture
    tree that isn't a subpath of the real repo -- it only affects display
    paths in `result.unresolved`, never resolution behavior."""
    src_dir = crate_dir / "src"
    result = CrateResult(name=name, src_dir=src_dir)
    if not src_dir.is_dir():
        return result

    result.all_files = {p.resolve() for p in src_dir.rglob("*.rs")}

    stack = [p.resolve() for p in entry_points(src_dir)]
    result.reached.update(stack)
    seen: set[Path] = set(stack)
    queue = list(stack)

    while queue:
        current = queue.pop()
        try:
            raw = current.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        text = strip_comments(raw)
        for decl in scan_mod_decls(text):
            target = resolve_mod(decl, current)
            if target is None:
                if decl.include_path is not None:
                    result.unresolved.append(
                        f'{current.relative_to(repo_root)}: include!("{decl.include_path}") -> no target file'
                    )
                else:
                    loc = "/".join((*decl.virtual_path, decl.name))
                    result.unresolved.append(f"{current.relative_to(repo_root)}: mod {loc}; -> no target file")
                continue
            target = target.resolve()
            if target not in seen:
                seen.add(target)
                result.reached.add(target)
                queue.append(target)

    return result


def workspace_members() -> list[tuple[str, Path]]:
    with open(WORKSPACE_CARGO, "rb") as fh:
        ws = tomllib.load(fh)
    members = ws.get("workspace", {}).get("members", [])
    result: list[tuple[str, Path]] = []
    for member in members:
        crate_dir = REPO_ROOT / member
        member_cargo = crate_dir / "Cargo.toml"
        if not member_cargo.is_file():
            continue
        with open(member_cargo, "rb") as fh:
            pkg = tomllib.load(fh)
        crate_name = pkg.get("package", {}).get("name", crate_dir.name)
        result.append((crate_name, crate_dir))
    return result


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--crate", action="append", default=None, help="Limit to these crate names (repeatable)")
    args = parser.parse_args()

    members = workspace_members()
    if args.crate:
        wanted = set(args.crate)
        members = [(n, d) for n, d in members if n in wanted]
        missing = wanted - {n for n, _ in members}
        if missing:
            print(f"unknown crate name(s): {', '.join(sorted(missing))}", file=sys.stderr)
            return 2

    orphan_failures: list[str] = []
    unresolved_failures: list[str] = []
    total_files = 0

    for crate_name, crate_dir in members:
        result = walk_crate(crate_name, crate_dir)
        total_files += len(result.all_files)
        orphaned = sorted(result.all_files - result.reached)
        for f in orphaned:
            orphan_failures.append(f"{f.relative_to(REPO_ROOT)} (crate {crate_name}): unreachable from any mod chain")
        for u in result.unresolved:
            unresolved_failures.append(f"crate {crate_name}: {u}")

    failures = orphan_failures + unresolved_failures
    if failures:
        print("orphaned-module check FAILED:", file=sys.stderr)
        for f in failures:
            print(f"  - {f}", file=sys.stderr)
        print(
            "\nEvery .rs file under a crate's src/ must be reachable from src/lib.rs, src/main.rs, or\n"
            "src/bin/*.rs through a mod chain. Wire the file in with a `mod` declaration, delete it if\n"
            "it is dead, or fix the `mod` statement that fails to resolve.",
            file=sys.stderr,
        )
        return 1

    print(f"orphaned-module check passed: {len(members)} crates, {total_files} source files, all reachable")
    return 0


if __name__ == "__main__":
    sys.exit(main())
