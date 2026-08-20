"""Shared Rust-source lexing plus strict nextest evidence decoding for Krites.

The capability checker uses the lexical helpers here to enumerate source-owned
capability categories without letting comments, dead cfg branches, or orphaned
modules impersonate live declarations. These helpers do *not* infer runnable
tests. Rust procedural macros and Cargo target selection make that a compiler
fact, so `gate_test` existence and ignored state come only from a supplied
`cargo nextest list --message-format json` result decoded by
`load_nextest_list()`.
"""

from __future__ import annotations

import ast
import json
import re
from pathlib import Path
from typing import TextIO

# WHY a depth cap rather than a visited-file set: `#[path]` legitimately reaches
# one file from two module paths (runtime/hnsw's cfg pair does exactly this), so
# a file already seen is not proof of a cycle. The cap bounds a genuine cycle --
# two modules whose `#[path]` attributes point at each other -- without
# rejecting the legal aliasing.
MAX_MODULE_DEPTH = 32
MAX_CFG_ATOMS = 16
RUST_TOKEN_START = r"(?<![A-Za-z0-9_\u0080-\U0010ffff])"
RUST_TOKEN_END = r"(?![A-Za-z0-9_\u0080-\U0010ffff])"
SINGLETON_CFG_KEYS = frozenset(
    {
        "panic",
        "target_abi",
        "target_arch",
        "target_endian",
        "target_env",
        "target_os",
        "target_pointer_width",
        "target_vendor",
    }
)

_PATH_ATTR_RE = re.compile(r'^path\s*=\s*"([^"\\]*)"$')


def strip_noise(text: str) -> str:
    """Blank out comments and literal contents, preserving length and newlines.

    WHY length-preserving: every declaration line and attribute span computed
    on the stripped text must name the same place in the real file.

    WHY at all: krites embeds Datalog scripts as raw strings, and those scripts
    contain `fn`, `mod` and `#[...]`-shaped text. A source-inventory scanner
    that reads literal contents as code invents modules and declarations that
    do not exist, allowing a removed capability to remain falsely mapped.
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
    # Rust input preprocessing removes one initial BOM and then one shebang
    # (`#!` not followed by `[`). Blank them here as trivia while preserving
    # every later source offset used for line/citation reporting.
    prefix = 0
    if text.startswith("\ufeff"):
        blank(0, 1)
        prefix = 1
    if text[prefix : prefix + 2] == "#!" and text[prefix : prefix + 3] != "#![":
        end = text.find("\n", prefix)
        blank(prefix, n if end == -1 else end)
    return "".join(out)


def _matching_meta_delimiter(text: str, open_at: int, opener: str, closer: str) -> int:
    """Find a meta-item delimiter in length-preserving noise-stripped text."""
    clean = strip_noise(text)
    if open_at >= len(clean) or clean[open_at] != opener:
        raise ValueError(f"expected {opener!r} in cfg expression")
    depth = 0
    for offset in range(open_at, len(clean)):
        if clean[offset] == opener:
            depth += 1
        elif clean[offset] == closer:
            depth -= 1
            if depth == 0:
                return offset
    raise ValueError(f"unterminated {opener!r} in cfg expression")


def _split_meta_args(text: str) -> list[str]:
    """Split Rust meta-item arguments on top-level commas."""
    clean = strip_noise(text)
    if not clean.strip():
        return []
    stack: list[str] = []
    pairs = {")": "(", "]": "[", "}": "{"}
    start = 0
    spans: list[tuple[int, int]] = []
    for offset, ch in enumerate(clean):
        if ch in "([{":
            stack.append(ch)
        elif ch in pairs:
            if not stack or stack[-1] != pairs[ch]:
                raise ValueError("unbalanced delimiter in cfg expression")
            stack.pop()
        elif ch == "," and not stack:
            spans.append((start, offset))
            start = offset + 1
    if stack:
        raise ValueError("unterminated delimiter in cfg expression")
    spans.append((start, len(text)))
    args: list[str] = []
    for index, (span_start, span_end) in enumerate(spans):
        if not clean[span_start:span_end].strip():
            if index == len(spans) - 1 and len(spans) > 1:
                continue
            raise ValueError("empty argument in cfg expression")
        args.append(text[span_start:span_end].strip())
    return args


def _meta_call(text: str) -> tuple[str, str] | None:
    """Return an exact outer `name(...)` meta call, if present."""
    raw = text.strip()
    clean = strip_noise(raw)
    leading_trivia = len(clean) - len(clean.lstrip())
    raw = raw[leading_trivia:]
    clean = clean[leading_trivia:]
    match = re.match(r"(?P<name>[A-Za-z_][A-Za-z0-9_]*)\s*\(", clean)
    if match is None:
        return None
    open_at = match.end() - 1
    close_at = _matching_meta_delimiter(raw, open_at, "(", ")")
    if clean[close_at + 1 :].strip():
        return None
    return match.group("name"), raw[open_at + 1 : close_at]


def _normalize_cfg_atom(text: str) -> str:
    """Canonicalize one Rust cfg option without inventing literal semantics."""
    raw = text.strip()
    clean_full = strip_noise(raw)
    start = len(clean_full) - len(clean_full.lstrip())
    end = len(clean_full.rstrip())
    clean = clean_full[start:end]
    raw = raw[start:end]
    identifier = r"[A-Za-z_][A-Za-z0-9_]*"
    if re.fullmatch(identifier, clean):
        return clean
    assignment = re.fullmatch(
        rf"(?P<key>{identifier})\s*=\s*(?P<literal>.+?)\s*",
        clean,
        re.DOTALL,
    )
    if assignment is None:
        raise ValueError(f"unsupported cfg option {raw!r}")
    literal = raw[assignment.start("literal") : assignment.end("literal")].strip()
    cooked = re.fullmatch(r'"(?P<value>[^"\\]*)"', literal, re.DOTALL)
    if cooked:
        value = cooked.group("value")
    else:
        raw_string = re.fullmatch(
            r'r(?P<hashes>#{0,255})"(?P<value>.*)"(?P=hashes)',
            literal,
            re.DOTALL,
        )
        if raw_string is None:
            raise ValueError(
                f"cfg option {raw!r} uses an escaped or unsupported string literal; "
                "refusing to guess its value"
            )
        value = raw_string.group("value")
    return f"{assignment.group('key')}={value!r}"


def _cfg_expr(text: str) -> tuple:
    """Parse the Boolean portion of Rust cfg syntax into a small AST."""
    raw = text.strip()
    if not raw:
        raise ValueError("empty cfg predicate")
    call = _meta_call(raw)
    if call is not None and call[0] in {"all", "any", "not"}:
        operator, body = call
        args = [_cfg_expr(arg) for arg in _split_meta_args(body)]
        if operator == "not" and len(args) != 1:
            raise ValueError("cfg not(...) requires exactly one predicate")
        if operator == "not":
            return ("not", args[0])
        return (operator, tuple(args))
    clean = strip_noise(raw).strip()
    if clean == "true":
        return ("const", True)
    if clean == "false":
        return ("const", False)
    return ("atom", _normalize_cfg_atom(raw))


def _cfg_attr_formula(attr: str) -> tuple:
    """Return the item-presence formula contributed by one outer attribute."""
    call = _meta_call(attr)
    if call is None:
        return ("const", True)
    name, body = call
    if name == "cfg":
        return _cfg_expr(body)
    if name != "cfg_attr":
        return ("const", True)
    args = _split_meta_args(body)
    if not args:
        raise ValueError("cfg_attr(...) requires a predicate")
    condition = _cfg_expr(args[0])
    if len(args) == 1:
        return ("const", True)
    implications: list[tuple] = []
    for nested in args[1:]:
        effect = _cfg_attr_formula(nested)
        implications.append(("any", (("not", condition), effect)))
    return ("all", tuple(implications))


def _cfg_eval(formula: tuple, assignment: dict[str, bool]) -> bool | None:
    kind = formula[0]
    if kind == "const":
        return formula[1]
    if kind == "atom":
        return assignment.get(formula[1])
    if kind == "not":
        value = _cfg_eval(formula[1], assignment)
        return None if value is None else not value
    values = [_cfg_eval(child, assignment) for child in formula[1]]
    if kind == "all":
        if any(value is False for value in values):
            return False
        return True if all(value is True for value in values) else None
    if kind == "any":
        if any(value is True for value in values):
            return True
        return False if all(value is False for value in values) else None
    raise ValueError(f"unknown cfg formula node {kind!r}")


def _cfg_atoms(formula: tuple) -> set[str]:
    if formula[0] == "atom":
        return {formula[1]}
    if formula[0] == "const":
        return set()
    if formula[0] == "not":
        return _cfg_atoms(formula[1])
    return set().union(*(_cfg_atoms(child) for child in formula[1]))


def _cfg_domain_consistent(assignment: dict[str, bool]) -> bool:
    """Enforce the bounded built-in relationships this symbolic model knows."""
    selected: dict[str, str] = {}
    for atom, enabled in assignment.items():
        if not enabled or "=" not in atom:
            continue
        key, _, encoded_value = atom.partition("=")
        if key not in SINGLETON_CFG_KEYS:
            continue
        value = ast.literal_eval(encoded_value)
        previous = selected.setdefault(key, value)
        if previous != value:
            return False
    # rustc exposes the common target-family values through both a keyed cfg
    # and their historical bare aliases.  They are equivalent when both atoms
    # occur in a formula; target_family itself is deliberately not singleton.
    for family in ("unix", "windows"):
        alias = assignment.get(family)
        keyed = assignment.get(f"target_family={family!r}")
        if alias is not None and keyed is not None and alias != keyed:
            return False
    return True


def _formula_satisfiable(formula: tuple) -> bool:
    """Whether one bounded symbolic cfg assignment satisfies ``formula``."""
    decided = _cfg_eval(formula, {})
    if decided is not None:
        return decided
    atoms = sorted(_cfg_atoms(formula))
    if len(atoms) > MAX_CFG_ATOMS:
        raise ValueError(
            f"cfg presence formula has {len(atoms)} atoms, above the "
            f"bounded SAT limit {MAX_CFG_ATOMS}"
        )

    def search(assignment: dict[str, bool], remaining: list[str]) -> bool:
        if not _cfg_domain_consistent(assignment):
            return False
        value = _cfg_eval(formula, assignment)
        if value is not None:
            return value
        atom = remaining[0]
        tail = remaining[1:]
        return search({**assignment, atom: False}, tail) or search(
            {**assignment, atom: True}, tail
        )

    return search({}, atoms)


def cfg_attrs_satisfiable(attrs: list[str] | tuple[str, ...]) -> bool:
    """Whether some cfg assignment can make an attributed Rust item exist.

    Unknown feature/target predicates remain symbolic, preserving this module's
    documented possible-build superset. Boolean constants, contradictions,
    Rust's single-valued built-in options, and the unix/windows family aliases
    are enforced. This is a bounded symbolic model, not a claim that every
    satisfying assignment corresponds to a shipped rustc target triple.
    """
    formula = ("all", tuple(_cfg_attr_formula(attr) for attr in attrs))
    return _formula_satisfiable(formula)


def preceding_outer_attributes(
    raw: str,
    clean: str,
    item_offset: int,
    lower_bound: int = 0,
) -> list[str]:
    """Return contiguous outer attributes immediately preceding an item."""
    attrs: list[str] = []
    cursor = item_offset
    while True:
        while cursor > lower_bound and clean[cursor - 1].isspace():
            cursor -= 1
        if cursor <= lower_bound or clean[cursor - 1] != "]":
            break
        close_at = cursor - 1
        depth = 1
        open_at = close_at - 1
        while open_at >= lower_bound:
            if clean[open_at] == "]":
                depth += 1
            elif clean[open_at] == "[":
                depth -= 1
                if depth == 0:
                    break
            open_at -= 1
        marker = open_at
        while marker > lower_bound and clean[marker - 1].isspace():
            marker -= 1
        if marker > lower_bound and clean[marker - 1] == "!":
            break
        if marker <= lower_bound or clean[marker - 1] != "#":
            break
        attrs.append(raw[open_at + 1 : close_at].strip())
        cursor = marker - 1
    attrs.reverse()
    return attrs


def leading_inner_attributes(raw: str, clean: str | None = None) -> list[str]:
    """Return module/file inner attributes before the first source item."""
    clean = strip_noise(raw) if clean is None else clean
    attrs: list[str] = []
    cursor = 0
    while True:
        while cursor < len(clean) and clean[cursor].isspace():
            cursor += 1
        parsed = _read_attribute(clean, cursor)
        if parsed is None or not parsed[3]:
            break
        start, end, cursor, _ = parsed
        attrs.append(raw[start:end].strip())
    return attrs


def inner_attributes_after(
    raw: str,
    clean: str,
    body_open: int,
) -> list[str]:
    """Return contiguous inner attributes at the start of a `{ ... }` body."""
    attrs: list[str] = []
    cursor = body_open + 1
    while True:
        while cursor < len(clean) and clean[cursor].isspace():
            cursor += 1
        parsed = _read_attribute(clean, cursor)
        if parsed is None or not parsed[3]:
            break
        start, end, cursor, _ = parsed
        attrs.append(raw[start:end].strip())
    return attrs


def _function_body_open(clean: str, signature_start: int) -> int | None:
    """Locate a function body without mistaking const-generic braces for it."""

    def follows_macro_bang(offset: int) -> bool:
        prefix = clean[signature_start:offset]
        segment = r"(?:r#[A-Za-z_][A-Za-z0-9_]*|[A-Za-z_\u0080-\U0010ffff][A-Za-z0-9_\u0080-\U0010ffff]*)"
        path = rf"(?:\$crate|{segment})(?:\s*::\s*{segment})*"
        owner = re.search(
            rf"{RUST_TOKEN_START}(?P<path>{path})\s*!\s*$",
            prefix,
        )
        if owner is None:
            return False
        last_segment = re.split(r"\s*::\s*", owner.group("path"))[-1]
        if last_segment in {"const", "mut"}:
            return False
        before = prefix[: owner.start("path")].rstrip()
        # `'a !` and `*const !` / `*mut !` end in Rust's never type, not a
        # brace-delimited macro invocation.  In each shape the identifier-like
        # suffix that the path regex sees is owned by `'` or `*`.
        return not before.endswith(("'", "*"))

    stack: list[str] = []
    pairs = {")": "(", "]": "[", "}": "{"}
    angle_depth = 0
    for offset in range(signature_start, len(clean)):
        ch = clean[offset]
        if (
            ch in "(["
            or ch == "{"
            and (stack or angle_depth or follows_macro_bang(offset))
        ):
            stack.append(ch)
        elif ch in pairs:
            if not stack or stack[-1] != pairs[ch]:
                raise ValueError("unbalanced function signature delimiter")
            stack.pop()
        elif not stack and ch == "<":
            angle_depth += 1
        elif (
            not stack
            and ch == ">"
            and angle_depth
            and clean[offset - 1 : offset] != "-"
        ):
            angle_depth -= 1
        elif not stack and not angle_depth and ch == "{":
            return offset
        elif not stack and not angle_depth and ch == ";":
            return None
    if stack or angle_depth:
        raise ValueError("unterminated function signature delimiter")
    return None


def _read_attribute(text: str, pos: int) -> tuple[int, int, int, bool] | None:
    """Parse a bracket-matched `#[...]` / `#![...]` at `pos`.

    Returns (inner start, inner end, index just past the closing bracket,
    is_inner), or None. Bracket-matched rather than regex-terminated so a nested
    `#[cfg(all(a, b))]` or an attribute containing `]` inside a string is not
    truncated early.

    WHY offsets and not the text: the caller scans the noise-stripped source but
    must read an attribute's VALUE from the raw source. `#[path = "x/mod.rs"]`
    is the case that forces this -- stripping blanks the literal, and a blank
    `path` silently resolves the module to nothing while looking like a module
    that has no file.
    """
    if pos >= len(text) or text[pos] != "#":
        return None
    j = pos + 1
    while j < len(text) and text[j].isspace():
        j += 1
    is_inner = False
    if j < len(text) and text[j] == "!":
        is_inner = True
        j += 1
        while j < len(text) and text[j].isspace():
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
                return j + 1, k, k + 1, is_inner
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


def _path_attr(attrs: list[str]) -> str | None:
    for a in attrs:
        clean_full = strip_noise(a)
        start = len(clean_full) - len(clean_full.lstrip())
        end = len(clean_full.rstrip())
        clean = clean_full[start:end]
        raw = a[start:end]
        if re.match(r"^cfg_attr\s*\(", clean) and re.search(r"\bpath\s*=", clean):
            raise ValueError(
                "conditional #[cfg_attr(..., path = ...)] cannot be resolved without "
                "evaluating cfg; refusing the default module path"
            )
        m = _PATH_ATTR_RE.match(clean)
        if m:
            literal = raw[m.start(1) - 1 : m.end(1) + 1]
            decoded = re.fullmatch(r'"([^"\\]*)"', literal)
            if decoded is None:
                raise ValueError(
                    "#[path = ...] uses an escaped string literal; refusing to "
                    "guess rustc's decoded module path"
                )
            return decoded.group(1)
        if re.match(r"^path\s*=", clean):
            raise ValueError(
                "#[path = ...] uses an unsupported string-literal form; refusing the "
                "default module path"
            )
    return None


def load_nextest_list(source: TextIO) -> dict[str, bool]:
    """Decode filter-matching nextest tests into {test id: ignored}.

    Opening the operator-selected CLI input belongs to the caller. This helper
    only decodes text it is handed; it neither constructs nor expands a
    filesystem path. The shape is validated because an absent or malformed
    suite map must never collapse into an authoritative empty test universe.
    Listed-but-filtered testcases still count toward nextest's declared total,
    but cannot satisfy a gate pointer because the paired run will not execute
    them.
    """
    payload = json.load(source)
    if not isinstance(payload, dict):
        raise TypeError("nextest list root must be a JSON object")
    suites = payload.get("rust-suites")
    if not isinstance(suites, dict):
        raise TypeError("nextest list must contain a 'rust-suites' object")
    result: dict[str, bool] = {}
    seen_test_ids: set[str] = set()
    decoded_count = 0
    for suite_key, suite in suites.items():
        if not isinstance(suite, dict):
            raise TypeError(f"nextest suite {suite_key!r} must be an object")
        status = suite.get("status")
        if status not in {"listed", "skipped", "skipped-default-filter"}:
            raise ValueError(
                f"nextest suite {suite_key!r} has unsupported status {status!r}"
            )
        binary_id = suite.get("binary-id")
        if not isinstance(binary_id, str):
            raise TypeError(f"nextest suite {suite_key!r} has a non-string binary-id")
        if not binary_id:
            raise ValueError(f"nextest suite {suite_key!r} has no non-empty binary-id")
        testcases = suite.get("testcases")
        if not isinstance(testcases, dict):
            raise TypeError(f"nextest suite {suite_key!r} has no testcases object")
        if status != "listed" and testcases:
            raise ValueError(
                f"nextest suite {suite_key!r} is {status!r} but has testcases"
            )
        for name, meta in testcases.items():
            if not isinstance(name, str):
                raise TypeError(
                    f"nextest suite {suite_key!r} has a non-string test name"
                )
            if not name:
                raise ValueError(
                    f"nextest suite {suite_key!r} has an invalid test name"
                )
            if not isinstance(meta, dict):
                raise TypeError(
                    f"nextest test {binary_id}::{name} metadata must be an object"
                )
            if not isinstance(meta.get("ignored"), bool):
                raise TypeError(
                    f"nextest test {binary_id}::{name} has no boolean ignored field"
                )
            test_id = f"{binary_id}::{name}"
            if test_id in seen_test_ids:
                raise ValueError(f"nextest list contains duplicate test id {test_id!r}")
            seen_test_ids.add(test_id)
            filter_match = meta.get("filter-match")
            if not isinstance(filter_match, dict):
                raise TypeError(f"nextest test {test_id} has no filter-match object")
            filter_status = filter_match.get("status")
            if filter_status not in {"matches", "mismatch"}:
                raise ValueError(
                    f"nextest test {test_id} has unsupported filter-match status "
                    f"{filter_status!r}"
                )
            if filter_status == "mismatch":
                reason = filter_match.get("reason")
                if not isinstance(reason, str) or not reason:
                    raise ValueError(
                        f"nextest test {test_id} has a mismatch without a reason"
                    )
            else:
                result[test_id] = meta["ignored"]
            decoded_count += 1
    declared_count = payload.get("test-count")
    if not isinstance(declared_count, int) or isinstance(declared_count, bool):
        raise TypeError("nextest list must contain an integer test-count")
    if declared_count != decoded_count:
        raise ValueError(
            f"nextest test-count says {declared_count}, decoded {decoded_count} testcases"
        )
    return result
