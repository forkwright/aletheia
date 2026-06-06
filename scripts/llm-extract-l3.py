#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = [
#   "tree-sitter>=0.25",
#   "tree-sitter-rust>=0.24",
# ]
# ///
"""Extract L3 API index from all workspace crates.

Parses every .rs source file in each workspace crate using tree-sitter,
extracts all public item signatures (fn, struct, enum, trait, impl) along
with their leading doc comments, and writes one markdown file per crate to
_llm/L3-api-index/<crate_name>.md.

Also writes _llm/manifest.toml recording generation metadata, source hashes,
and token estimates per crate.

Usage:
    uv run scripts/llm-extract-l3.py
"""

from __future__ import annotations

import argparse
import hashlib
import sys
import tomllib
from dataclasses import dataclass, field
from datetime import UTC, datetime
from pathlib import Path

import tree_sitter_rust
from tree_sitter import Language, Node, Parser

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

REPO_ROOT = Path(__file__).parent.parent.resolve()
CRATES_DIR = REPO_ROOT / "crates"
LLM_DIR = REPO_ROOT / "_llm"
L3_DIR = LLM_DIR / "L3-api-index"
MANIFEST_PATH = LLM_DIR / "manifest.toml"
WORKSPACE_CARGO = REPO_ROOT / "Cargo.toml"

# Item node types that represent public API surface.
# NOTE: tree-sitter-rust calls the `pub type X = Y;` form a `type_item`, and
# `static` declarations a `static_item`. Both must appear in the cross-crate
# API surface or consumers won't see re-exported type aliases (common in
# facade crates like mneme) or exported statics.
PUB_ITEM_TYPES = {
    "function_item",
    "struct_item",
    "enum_item",
    "trait_item",
    "impl_item",
    "type_item",
    "const_item",
    "static_item",
}

# Node types that carry doc comments
DOC_COMMENT_TYPES = {"line_comment"}


# Source rustdocs carry mixed unicode punctuation; the `_llm/L3-api-index`
# corpus must stay ASCII-clean (kanon basanos `WRITING/em-dash` +
# `WRITING/ellipsis` + smart-quote rules). Normalize at render so the
# output is independent of source-rustdoc hygiene.
_UNICODE_TO_ASCII: dict[str, str] = {
    "\u2014": " - ",   # em dash
    "\u2013": "-",      # en dash
    "\u2026": "...",    # ellipsis
    "\u2018": "'",      # left single quote
    "\u2019": "'",      # right single quote / apostrophe
    "\u201c": '"',      # left double quote
    "\u201d": '"',      # right double quote
    "\u2212": "-",      # minus sign
    "\u00a0": " ",      # non-breaking space
}


def _ascii_normalize(text: str) -> str:
    for src, dst in _UNICODE_TO_ASCII.items():
        text = text.replace(src, dst)
    return text


# ---------------------------------------------------------------------------
# Data structures
# ---------------------------------------------------------------------------


@dataclass
class PublicItem:
    """A single extracted public item with its signature and doc comments."""

    signature: str
    doc_comments: list[str] = field(default_factory=list)

    def render(self) -> str:
        """Render as markdown with optional doc comments above fenced block."""
        parts: list[str] = []
        for comment in self.doc_comments:
            # Strip leading `///` and normalise whitespace
            stripped = comment.strip()
            if stripped.startswith("///"):
                stripped = stripped[3:]
            stripped = _ascii_normalize(stripped)
            parts.append(f">{stripped}" if stripped.startswith(" ") else f"> {stripped}")
        doc_block = "\n".join(parts)
        sig_block = f"```rust\n{self.signature}\n```"
        if doc_block:
            return doc_block + "\n" + sig_block
        return sig_block


@dataclass
class ModuleSection:
    """Items extracted from one .rs file."""

    module_path: str  # e.g. "src/id.rs" relative to crate root
    items: list[PublicItem] = field(default_factory=list)


@dataclass
class CrateIndex:
    """Aggregated L3 index for one crate."""

    name: str
    path: str  # relative to repo root, e.g. "crates/koina"
    modules: list[ModuleSection] = field(default_factory=list)
    source_hash: str = ""
    token_estimate: int = 0


# ---------------------------------------------------------------------------
# Tree-sitter setup
# ---------------------------------------------------------------------------

_LANG = Language(tree_sitter_rust.language())
_PARSER = Parser(_LANG)


# ---------------------------------------------------------------------------
# Workspace member discovery
# ---------------------------------------------------------------------------


def workspace_members() -> list[tuple[str, Path]]:
    """Return (crate_name, crate_path) pairs from [workspace.members].

    Reads Cargo.toml at repo root. Resolves each member path and reads its
    own Cargo.toml to obtain the canonical crate name (which may differ from
    the directory name, e.g. poiesis/core -> poiesis-core).
    """
    with open(WORKSPACE_CARGO, "rb") as fh:
        ws = tomllib.load(fh)

    members = ws.get("workspace", {}).get("members", [])
    result: list[tuple[str, Path]] = []

    for member in members:
        crate_dir = REPO_ROOT / member
        if not crate_dir.is_dir():
            continue
        member_cargo = crate_dir / "Cargo.toml"
        if not member_cargo.exists():
            continue
        with open(member_cargo, "rb") as fh:
            pkg = tomllib.load(fh)
        crate_name = pkg.get("package", {}).get("name", crate_dir.name)
        result.append((crate_name, crate_dir))

    return result


# ---------------------------------------------------------------------------
# Source hashing
# ---------------------------------------------------------------------------


def hash_crate_sources(crate_dir: Path) -> str:
    """SHA-256 of all .rs file contents in a crate, sorted by relative path."""
    h = hashlib.sha256()
    rs_files = sorted(
        f for f in crate_dir.rglob("*.rs") if "target" not in f.parts
    )
    for rs_file in rs_files:
        try:
            content = rs_file.read_bytes()
        except OSError:
            continue
        h.update(rs_file.relative_to(crate_dir).as_posix().encode())
        h.update(content)
    return h.hexdigest()


# ---------------------------------------------------------------------------
# Tree-sitter extraction helpers
# ---------------------------------------------------------------------------


def is_doc_comment(node: Node, source: bytes) -> bool:
    """Return True if node is a `///` outer doc comment line."""
    if node.type not in DOC_COMMENT_TYPES:
        return False
    text = source[node.start_byte : node.end_byte].decode("utf-8", errors="replace")
    return text.strip().startswith("///")


def extract_doc_comments(node: Node, source: bytes, siblings: list[Node]) -> list[str]:
    """Collect consecutive `///` doc comment lines immediately before node.

    Walks backward through siblings to find the run of doc comments just
    before the given node, then returns them in forward order.
    """
    idx = next((i for i, s in enumerate(siblings) if s == node), None)
    if idx is None:
        return []

    doc_lines: list[str] = []
    i = idx - 1
    while i >= 0:
        sib = siblings[i]
        if is_doc_comment(sib, source):
            text = source[sib.start_byte : sib.end_byte].decode("utf-8", errors="replace")
            doc_lines.append(text.strip())
            i -= 1
        else:
            break

    return list(reversed(doc_lines))


def node_is_pub(node: Node, source: bytes) -> bool:
    """Return True if the item has bare `pub` visibility (not pub(crate), pub(super), etc.).

    Only bare `pub` items are part of the cross-crate public API surface.
    pub(crate) items are internal and excluded from the L3 index.
    """
    for child in node.children:
        if child.type == "visibility_modifier":
            text = source[child.start_byte : child.end_byte].decode("utf-8", errors="replace").strip()
            # bare `pub` has exactly one child token
            return text == "pub"
    return False


def extract_fn_signature(node: Node, source: bytes) -> str:
    """Extract a function_item or function_signature_item header without body.

    For function_signature_item the trailing `;` is part of the node and is
    included. For function_item the `;` is absent and must be added by callers.
    """
    parts: list[str] = []
    for child in node.children:
        if child.type == "block":
            break
        if child.type == ";":
            # Don't include the semicolon here — callers decide the suffix
            break
        parts.append(source[child.start_byte : child.end_byte].decode("utf-8", errors="replace"))
    return " ".join(p.strip() for p in parts if p.strip())


def extract_impl_signature(node: Node, source: bytes) -> str:
    """Render an impl block as: header line + pub method signatures only.

    Example output:
        impl Foo {
            pub fn new() -> Self;
            pub fn get(&self) -> i32;
        }
    """
    # Build header: impl [Trait for] TypeName
    header_parts: list[str] = []
    for child in node.children:
        if child.type == "declaration_list":
            break
        header_parts.append(
            source[child.start_byte : child.end_byte].decode("utf-8", errors="replace").strip()
        )
    header = " ".join(p for p in header_parts if p)

    # Collect pub method signatures from the declaration_list
    decl_list = next((c for c in node.children if c.type == "declaration_list"), None)
    method_sigs: list[str] = []
    if decl_list is not None:
        decl_siblings = decl_list.children
        for item in decl_siblings:
            if item.type in ("function_item", "function_signature_item") and node_is_pub(item, source):
                sig = extract_fn_signature(item, source)
                method_sigs.append(f"    {sig};")

    if method_sigs:
        inner = "\n".join(method_sigs)
        return f"{header} {{\n{inner}\n}}"
    return header


def extract_trait_signature(node: Node, source: bytes) -> str:
    """Render a trait as: header + method signatures only (no default implementations).

    Example output:
        pub trait MyTrait: Send {
            fn name(&self) -> &str;
            fn process(&mut self, input: &[u8]) -> Vec<u8>;
        }
    """
    # Build header: pub trait Name [: Bounds]
    header_parts: list[str] = []
    for child in node.children:
        if child.type == "declaration_list":
            break
        header_parts.append(
            source[child.start_byte : child.end_byte].decode("utf-8", errors="replace").strip()
        )
    header = " ".join(p for p in header_parts if p)

    # Collect method signatures from the declaration_list
    decl_list = next((c for c in node.children if c.type == "declaration_list"), None)
    method_sigs: list[str] = []
    if decl_list is not None:
        decl_siblings = decl_list.children
        for item in decl_siblings:
            if item.type == "function_signature_item":
                sig = extract_fn_signature(item, source)
                method_sigs.append(f"    {sig};")
            elif item.type == "function_item":
                # Default methods: show signature only
                sig = extract_fn_signature(item, source)
                method_sigs.append(f"    {sig}; // default impl")

    if method_sigs:
        inner = "\n".join(method_sigs)
        return f"{header} {{\n{inner}\n}}"
    return header + " { }"


def extract_signature(node: Node, source: bytes) -> str:
    """Extract the item signature, omitting body implementations.

    - struct/enum: full declaration (these are type definitions, not code)
    - fn: header only, no body block
    - trait: header + method signatures only, no default bodies
    - impl: header + pub method signatures only, no bodies
    - type_item (pub type X = Y;), const_item, static_item: full text
    """
    if node.type in ("struct_item", "enum_item"):
        return source[node.start_byte : node.end_byte].decode("utf-8", errors="replace").strip()

    if node.type == "function_item":
        return extract_fn_signature(node, source)

    if node.type == "trait_item":
        return extract_trait_signature(node, source)

    if node.type == "impl_item":
        return extract_impl_signature(node, source)

    # type_item, const_item, static_item, and anything else: full text
    return source[node.start_byte : node.end_byte].decode("utf-8", errors="replace").strip()


def is_in_cfg_test(node: Node, siblings: list[Node]) -> bool:
    """Return True if the node is a mod_item preceded by #[cfg(test)].

    This function checks whether the immediately preceding non-whitespace
    sibling is an attribute_item containing `cfg(test)`.
    """
    if node.type != "mod_item":
        return False

    idx = next((i for i, s in enumerate(siblings) if s == node), None)
    if idx is None:
        return False

    # Walk backward for the nearest attribute_item
    i = idx - 1
    while i >= 0:
        sib = siblings[i]
        if sib.type == "attribute_item":
            text = sib.text.decode("utf-8", errors="replace") if isinstance(sib.text, bytes) else (sib.text or "")
            return "cfg(test)" in text or "cfg(any(test" in text
        # Doc comments between attribute and item are allowed; anything else breaks the chain
        if sib.type not in DOC_COMMENT_TYPES:
            break
        i -= 1
    return False


def walk_items(
    node: Node,
    source: bytes,
) -> list[PublicItem]:
    """Recursively walk a node tree and collect public items.

    Skips items inside `#[cfg(test)] mod { ... }` blocks entirely.

    Args:
        node: Current tree-sitter node to inspect.
        source: Full file source bytes.
    """
    items: list[PublicItem] = []
    current_siblings = node.children

    for child in node.children:
        # Detect a mod_item gated by #[cfg(test)] and skip with all contents
        if is_in_cfg_test(child, current_siblings):
            continue

        # For all mod items (pub or not), recurse into the body — pub items
        # inside non-pub mods are still part of the crate's public API.
        if child.type == "mod_item":
            decl_list = next((c for c in child.children if c.type == "declaration_list"), None)
            if decl_list is not None:
                items.extend(walk_items(decl_list, source))
            continue

        if child.type in PUB_ITEM_TYPES:
            if not node_is_pub(child, source):
                # Non-pub impl blocks may still contain pub methods.
                if child.type == "impl_item":
                    decl_list = next((c for c in child.children if c.type == "declaration_list"), None)
                    if decl_list is not None:
                        has_pub = any(
                            c.type in PUB_ITEM_TYPES and node_is_pub(c, source)
                            for c in decl_list.children
                        )
                        if has_pub:
                            doc_comments = extract_doc_comments(child, source, current_siblings)
                            sig = extract_signature(child, source)
                            items.append(PublicItem(signature=sig, doc_comments=doc_comments))
                continue

            doc_comments = extract_doc_comments(child, source, current_siblings)
            sig = extract_signature(child, source)
            items.append(PublicItem(signature=sig, doc_comments=doc_comments))

    return items


# ---------------------------------------------------------------------------
# Per-file extraction
# ---------------------------------------------------------------------------


def extract_from_file(rs_path: Path, crate_dir: Path) -> ModuleSection | None:
    """Parse one .rs file and return a ModuleSection (or None if nothing public)."""
    try:
        source_bytes = rs_path.read_bytes()
    except OSError:
        return None

    tree = _PARSER.parse(source_bytes)
    items = walk_items(tree.root_node, source_bytes)

    if not items:
        return None

    module_path = rs_path.relative_to(crate_dir).as_posix()
    return ModuleSection(module_path=module_path, items=items)


# ---------------------------------------------------------------------------
# Crate-level extraction
# ---------------------------------------------------------------------------


def extract_crate(crate_name: str, crate_dir: Path) -> CrateIndex:
    """Extract all public items from a crate's .rs files."""
    idx = CrateIndex(
        name=crate_name,
        path=crate_dir.relative_to(REPO_ROOT).as_posix(),
    )

    rs_files = sorted(
        f for f in crate_dir.rglob("*.rs") if "target" not in f.parts
    )
    for rs_file in rs_files:
        section = extract_from_file(rs_file, crate_dir)
        if section:
            idx.modules.append(section)

    idx.source_hash = hash_crate_sources(crate_dir)
    return idx


# ---------------------------------------------------------------------------
# Rendering
# ---------------------------------------------------------------------------


def render_crate_markdown(idx: CrateIndex) -> str:
    """Render the L3 index for one crate as a markdown document."""
    lines: list[str] = [
        f"# L3 API Index: {idx.name}",
        "",
        f"Crate path: `{idx.path}`",
        "",
        "Public API signatures extracted from source. Each signature is preceded by its doc comment.",
        "For implementation context, read the source directly (`L4`).",
        "",
    ]

    for module in idx.modules:
        lines.append(f"## `{module.module_path}`")
        lines.append("")
        for item in module.items:
            lines.append(item.render())
            lines.append("")

    return "\n".join(lines)


def estimate_tokens(text: str) -> int:
    """Rough token estimate: len(text) / 4."""
    return len(text) // 4


# ---------------------------------------------------------------------------
# Manifest writing
# ---------------------------------------------------------------------------


def read_existing_manifest() -> dict[str, dict[str, object]]:
    """Read existing manifest and return a dict mapping crate name to its entry data."""
    if not MANIFEST_PATH.exists():
        return {}

    try:
        with open(MANIFEST_PATH, "rb") as fh:
            data = tomllib.load(fh)
    except tomllib.TOMLDecodeError:
        return {}

    result: dict[str, dict[str, object]] = {}
    for crate in data.get("crates", []):
        name = crate.get("name")
        if isinstance(name, str):
            result[name] = crate
    return result


def _read_existing_toml() -> dict[str, object]:
    """Read the existing manifest as a raw TOML dict, or `{}` when absent/unparseable."""
    if not MANIFEST_PATH.exists():
        return {}
    try:
        with open(MANIFEST_PATH, "rb") as fh:
            return tomllib.load(fh)
    except tomllib.TOMLDecodeError:
        return {}


def _emit_l1_block(data: dict[str, object]) -> list[str]:
    """Re-emit the `[l1]` section verbatim when present in the existing manifest.

    L1 and L2 sections are hand-authored (Phase 2 of the multi-resolution
    architecture, #3670); the L3 extractor must preserve them across
    regeneration or those tiers silently disappear from the manifest.
    """
    l1 = data.get("l1")
    if not isinstance(l1, dict):
        return []

    lines = ["[l1]"]
    for key in ("file", "source_hash"):
        value = l1.get(key)
        if isinstance(value, str):
            lines.append(f'{key} = "{value}"')
    token_estimate = l1.get("token_estimate")
    if isinstance(token_estimate, int):
        lines.append(f"token_estimate = {token_estimate}")
    lines.append("")
    return lines


def _emit_l2_blocks(data: dict[str, object]) -> list[str]:
    """Re-emit `[[l2]]` sections verbatim when present in the existing manifest."""
    l2 = data.get("l2")
    if not isinstance(l2, list):
        return []

    lines: list[str] = []
    for entry in l2:
        if not isinstance(entry, dict):
            continue
        lines.append("[[l2]]")
        for key in ("name", "file", "source_hash"):
            value = entry.get(key)
            if isinstance(value, str):
                lines.append(f'{key} = "{value}"')
        token_estimate = entry.get("token_estimate")
        if isinstance(token_estimate, int):
            lines.append(f"token_estimate = {token_estimate}")
        lines.append("")
    return lines


def write_manifest(
    crate_indices: list[CrateIndex],
    generated_at: str,
    merge: bool = False,
) -> None:
    """Write _llm/manifest.toml.

    If *merge* is True, preserve unmodified crate entries from the existing
    manifest and only overwrite the crates present in *crate_indices*.

    Hand-authored `[levels.L1]`, `[levels.L2]`, `[l1]`, and `[[l2]]` sections
    from the existing manifest (Phase 2 of the multi-resolution plan) are
    preserved on every regeneration so the script does not silently erase
    them.
    """
    existing_toml = _read_existing_toml()

    # Finalize the crate list before building the header so we can compare
    # source hashes and avoid bumping generated_at when nothing changed.
    # WHY: the _llm freshness CI check runs `git status --porcelain _llm/`
    # after re-running the script; a timestamp-only diff would always fail it.
    if merge:
        existing = read_existing_manifest()
        updated_names = {idx.name for idx in crate_indices}
        merged: list[CrateIndex] = []
        merged.extend(crate_indices)
        for name, entry in existing.items():
            if name not in updated_names:
                merged.append(CrateIndex(
                    name=name,
                    path=entry.get("path", ""),
                    source_hash=entry.get("source_hash", ""),
                    token_estimate=entry.get("l3_token_estimate", 0),
                ))
        crate_indices = sorted(merged, key=lambda c: c.name)
    else:
        crate_indices.sort(key=lambda c: c.name)

    # WHY: always preserve the existing generated_at (not only when source
    # hashes are unchanged) so concurrent dispatch branches never conflict on a
    # wall-clock bump of this line in _llm/manifest.toml. The field is write-only
    # metadata; freshness is determined by per-crate source_hash, not this stamp.
    existing_at = existing_toml.get("generated_at")
    if isinstance(existing_at, str):
        generated_at = existing_at

    lines: list[str] = [
        "# _llm manifest — generated by scripts/llm-extract-l3.py",
        "# Do not edit by hand. Regenerate with: uv run scripts/llm-extract-l3.py",
        "#",
        "# L1 and L2 metadata blocks are hand-authored; the L3 extractor",
        "# preserves them verbatim across regeneration. See _llm/README.md.",
        "",
        "version = 1",
        'generated_at = "' + generated_at + '"',
        "",
    ]

    # WHY: preserve [levels.L1] and [levels.L2] when the existing manifest
    # already declares them; otherwise the L3-only header hides the other
    # tiers from downstream consumers.
    levels = existing_toml.get("levels")
    if isinstance(levels, dict):
        l1_level = levels.get("L1")
        if isinstance(l1_level, dict):
            lines.append("[levels.L1]")
            for key in ("path", "description"):
                value = l1_level.get(key)
                if isinstance(value, str):
                    lines.append(f'{key} = "{value}"')
            lines.append("")
        l2_level = levels.get("L2")
        if isinstance(l2_level, dict):
            lines.append("[levels.L2]")
            for key in ("path", "description"):
                value = l2_level.get(key)
                if isinstance(value, str):
                    lines.append(f'{key} = "{value}"')
            lines.append("")

    lines.extend([
        "[levels.L3]",
        'path = "L3-api-index"',
        'generator = "scripts/llm-extract-l3.py"',
        "",
    ])

    # L1 and L2 blocks are hand-authored; passing them through keeps the
    # manifest a single source of truth for all tiers.
    lines.extend(_emit_l1_block(existing_toml))
    lines.extend(_emit_l2_blocks(existing_toml))

    for idx in crate_indices:
        lines.extend([
            "[[crates]]",
            'name = "' + idx.name + '"',
            'path = "' + idx.path + '"',
            'source_hash = "' + idx.source_hash + '"',
            f"l3_token_estimate = {idx.token_estimate}",
            "",
        ])

    MANIFEST_PATH.write_text("\n".join(lines), encoding="utf-8")


# ---------------------------------------------------------------------------
# Summary printing
# ---------------------------------------------------------------------------


def print_summary(crate_indices: list[CrateIndex]) -> None:
    """Print per-crate extraction summary and total token estimate."""
    high_token_threshold = 800

    print(f"\n{'Crate':<30} {'Items':>6} {'Tokens':>7} {'Files':>6}")
    print("-" * 54)

    total_items = 0
    total_tokens = 0
    high_token_crates: list[tuple[str, int]] = []

    for idx in crate_indices:
        item_count = sum(len(m.items) for m in idx.modules)
        file_count = len(idx.modules)
        total_items += item_count
        total_tokens += idx.token_estimate

        marker = " !" if idx.token_estimate > high_token_threshold else ""
        print(f"  {idx.name:<28} {item_count:>6} {idx.token_estimate:>7} {file_count:>6}{marker}")

        if idx.token_estimate > high_token_threshold:
            high_token_crates.append((idx.name, idx.token_estimate))

    print("-" * 54)
    print(f"  {'TOTAL':<28} {total_items:>6} {total_tokens:>7}")
    print()

    if high_token_crates:
        print("Crates exceeding 800 token estimate (signal, not truncated):")
        for name, tokens in high_token_crates:
            print(f"  {name}: ~{tokens} tokens")
        print()


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


def main() -> int:
    """Run the L3 extraction pipeline."""
    parser = argparse.ArgumentParser(
        description="Extract L3 API index from workspace crates.",
    )
    parser.add_argument(
        "--crates",
        type=str,
        default="",
        help='Comma-separated list of crate names to regenerate. If omitted, all crates are processed.',
    )
    args = parser.parse_args()

    L3_DIR.mkdir(parents=True, exist_ok=True)

    members = workspace_members()
    if not members:
        print("ERROR: no workspace members found", file=sys.stderr)
        return 1

    # Filter to requested crates if --crates was given
    selected_crates: set[str] = set()
    if args.crates:
        selected_crates = {c.strip() for c in args.crates.split(",") if c.strip()}
        unknown = selected_crates - {name for name, _ in members}
        if unknown:
            print(
                f"ERROR: unknown crate(s): {', '.join(sorted(unknown))}",
                file=sys.stderr,
            )
            return 1

    generated_at = datetime.now(UTC).strftime("%Y-%m-%dT%H:%M:%SZ")
    crate_indices: list[CrateIndex] = []

    if selected_crates:
        members = [(n, p) for n, p in members if n in selected_crates]
        print(f"Extracting L3 API index for {len(members)} selected crate(s)...")
    else:
        print(f"Extracting L3 API index for {len(members)} workspace crates...")

    for crate_name, crate_dir in members:
        idx = extract_crate(crate_name, crate_dir)
        md = render_crate_markdown(idx)
        idx.token_estimate = estimate_tokens(md)
        crate_indices.append(idx)

        out_path = L3_DIR / f"{crate_name}.md"
        out_path.write_text(md, encoding="utf-8")

    write_manifest(crate_indices, generated_at, merge=bool(selected_crates))

    print_summary(crate_indices)

    print(f"Written: {len(crate_indices)} files in {L3_DIR.relative_to(REPO_ROOT)}/")
    print(f"Written: {MANIFEST_PATH.relative_to(REPO_ROOT)}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
