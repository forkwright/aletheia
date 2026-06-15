#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = [
#   "tree-sitter>=0.25",
#   "tree-sitter-rust>=0.24",
# ]
# ///
"""Fixture-based tests for scripts/llm-extract-l3.py.

Creates a tiny synthetic crate on disk, runs the extractor against it, and
asserts the extracted L3 index contains the expected public items and
omits the hidden ones (pub(crate), cfg(test)-gated modules, private items).

The extractor is covered by these invariants:

    - bare `pub` items are captured (fn, struct, enum, trait, type, const)
    - `pub(crate)` items are excluded from the cross-crate API surface
    - private (no visibility modifier) items are excluded
    - items inside `#[cfg(test)] mod tests { ... }` blocks are excluded
    - doc comments immediately above a pub item are attached to it
    - fn bodies are stripped (only the signature is rendered)
    - running the extractor twice produces identical output (determinism)

Run with:
    uv run scripts/test_llm_extract_l3.py

Exits 0 on success, 1 on first failure. Designed to be invoked by CI.
"""

from __future__ import annotations

import hashlib
import importlib.util
import sys
import tempfile
from pathlib import Path


# ---------------------------------------------------------------------------
# Import the extractor module from its on-disk filename (hyphen makes it
# non-importable via `import scripts.llm_extract_l3`).
# ---------------------------------------------------------------------------

_SCRIPT_PATH = Path(__file__).parent / "llm-extract-l3.py"


def _load_extractor() -> object:
    """Load the extractor as a module so its functions are callable."""
    spec = importlib.util.spec_from_file_location("llm_extract_l3", _SCRIPT_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"failed to load {_SCRIPT_PATH}")
    module = importlib.util.module_from_spec(spec)
    # WHY: register in sys.modules BEFORE exec so @dataclass can resolve the
    # module in cls.__module__ lookup during class construction.
    sys.modules["llm_extract_l3"] = module
    spec.loader.exec_module(module)
    return module


EXTRACTOR = _load_extractor()


# ---------------------------------------------------------------------------
# Fixture
# ---------------------------------------------------------------------------

FIXTURE_LIB_RS = '''\
//! Synthetic fixture crate for llm-extract-l3 tests.

/// A public struct with one public field.
pub struct Widget {
    pub id: u64,
}

/// Private struct that must not appear in the L3 output.
struct HiddenWidget {
    pub id: u64,
}

/// A crate-internal struct that must not appear in the L3 output.
pub(crate) struct InternalWidget;

/// A public enum with two variants.
pub enum Signal {
    On,
    Off,
}

/// A public trait with two method signatures.
pub trait Handler {
    /// Handle a signal.
    fn handle(&self, s: Signal);
    fn noop(&self) {}
}

/// A public free function with a doc comment.
pub fn make_widget(id: u64) -> Widget {
    let body_should_not_appear = id + 1;
    Widget { id: body_should_not_appear }
}

/// Public type alias.
pub type WidgetId = u64;

/// Public constant.
pub const MAX_WIDGETS: u32 = 42;

/// Private helper that must not appear in the L3 output.
fn private_helper() -> u32 {
    0
}

impl Widget {
    pub fn new(id: u64) -> Self {
        Self { id }
    }

    pub(crate) fn internal_id(&self) -> u64 {
        self.id
    }

    fn private_id(&self) -> u64 {
        self.id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// This pub fn is inside a cfg(test) module and must not leak to L3.
    pub fn should_not_appear_in_l3() -> u32 {
        123
    }

    pub struct TestOnlyWidget;
}
'''

FIXTURE_CARGO_TOML = """\
[package]
name = "fixture-crate"
version = "0.0.1"
edition = "2024"
"""


# ---------------------------------------------------------------------------
# Test harness
# ---------------------------------------------------------------------------

_FAILURES: list[str] = []


def expect(condition: bool, msg: str) -> None:
    """Record a test-level assertion failure without aborting."""
    if not condition:
        _FAILURES.append(msg)


def run_extractor_on_fixture(tmp: Path) -> tuple[object, str]:
    """Scaffold the fixture crate, run the extractor, return (index, markdown)."""
    crate_dir = tmp / "fixture-crate"
    (crate_dir / "src").mkdir(parents=True)
    (crate_dir / "Cargo.toml").write_text(FIXTURE_CARGO_TOML)
    (crate_dir / "src" / "lib.rs").write_text(FIXTURE_LIB_RS)

    # WHY: extract_crate computes crate_dir.relative_to(REPO_ROOT). For tmpdir
    # fixtures that path won't be under the real repo root, so we rebind
    # REPO_ROOT on the module for the duration of this call.
    original_root = EXTRACTOR.REPO_ROOT  # type: ignore[attr-defined]
    EXTRACTOR.REPO_ROOT = tmp  # type: ignore[attr-defined]
    try:
        idx = EXTRACTOR.extract_crate("fixture-crate", crate_dir)  # type: ignore[attr-defined]
    finally:
        EXTRACTOR.REPO_ROOT = original_root  # type: ignore[attr-defined]
    md = EXTRACTOR.render_crate_markdown(idx)  # type: ignore[attr-defined]
    idx.token_estimate = EXTRACTOR.estimate_tokens(md)  # type: ignore[attr-defined]
    return idx, md


def test_pub_items_extracted(md: str) -> None:
    """Bare pub items must appear in the rendered markdown."""
    expect("pub struct Widget" in md, "pub struct Widget missing from L3 output")
    expect("pub enum Signal" in md, "pub enum Signal missing from L3 output")
    expect("pub trait Handler" in md, "pub trait Handler missing from L3 output")
    expect("pub fn make_widget" in md, "pub fn make_widget missing from L3 output")
    expect("pub type WidgetId" in md, "pub type WidgetId missing from L3 output")
    expect(
        "pub const MAX_WIDGETS" in md,
        "pub const MAX_WIDGETS missing from L3 output",
    )


def test_non_pub_items_excluded(md: str) -> None:
    """Private and pub(crate) items must not leak into the L3 output."""
    expect(
        "HiddenWidget" not in md,
        "private struct HiddenWidget leaked into L3 output",
    )
    expect(
        "InternalWidget" not in md,
        "pub(crate) struct InternalWidget leaked into L3 output",
    )
    expect(
        "private_helper" not in md,
        "private fn private_helper leaked into L3 output",
    )
    expect(
        "private_id" not in md,
        "private method private_id leaked into L3 output",
    )
    expect(
        "internal_id" not in md,
        "pub(crate) method internal_id leaked into L3 output",
    )


def test_cfg_test_items_excluded(md: str) -> None:
    """Items inside cfg(test) modules must not leak into the L3 output."""
    expect(
        "should_not_appear_in_l3" not in md,
        "cfg(test) pub fn leaked into L3 output",
    )
    expect(
        "TestOnlyWidget" not in md,
        "cfg(test) pub struct leaked into L3 output",
    )


def test_doc_comments_attached(md: str) -> None:
    """Doc comments above pub items should appear in the rendered output."""
    expect(
        "A public free function with a doc comment." in md,
        "doc comment not attached to pub fn make_widget",
    )
    expect(
        "A public trait with two method signatures." in md,
        "doc comment not attached to pub trait Handler",
    )


def test_fn_body_stripped(md: str) -> None:
    """Function bodies must be stripped; only signatures are rendered."""
    expect(
        "body_should_not_appear" not in md,
        "pub fn body leaked into L3 output (should be signature-only)",
    )


def test_deterministic(tmp: Path) -> None:
    """Running the extractor twice on unchanged source produces identical output."""
    _, md_a = run_extractor_on_fixture(tmp / "a")
    _, md_b = run_extractor_on_fixture(tmp / "b")
    expect(
        md_a == md_b,
        "extractor output is non-deterministic across runs on identical source",
    )


def test_source_hash_stable(tmp: Path) -> None:
    """Source hash is stable across repeated runs on the same crate."""
    crate_dir = tmp / "hash-stable" / "fixture-crate"
    (crate_dir / "src").mkdir(parents=True)
    (crate_dir / "Cargo.toml").write_text(FIXTURE_CARGO_TOML)
    (crate_dir / "src" / "lib.rs").write_text(FIXTURE_LIB_RS)

    h1 = EXTRACTOR.hash_crate_sources(crate_dir)  # type: ignore[attr-defined]
    h2 = EXTRACTOR.hash_crate_sources(crate_dir)  # type: ignore[attr-defined]
    expect(h1 == h2, f"source hash drifted across runs: {h1} vs {h2}")


def test_source_hash_changes_with_content(tmp: Path) -> None:
    """Source hash changes when content changes."""
    crate_dir = tmp / "hash-changes" / "fixture-crate"
    (crate_dir / "src").mkdir(parents=True)
    (crate_dir / "Cargo.toml").write_text(FIXTURE_CARGO_TOML)
    (crate_dir / "src" / "lib.rs").write_text("pub fn a() {}\n")
    h1 = EXTRACTOR.hash_crate_sources(crate_dir)  # type: ignore[attr-defined]

    (crate_dir / "src" / "lib.rs").write_text("pub fn b() {}\n")
    h2 = EXTRACTOR.hash_crate_sources(crate_dir)  # type: ignore[attr-defined]

    expect(h1 != h2, "source hash did not change when content changed")


def test_source_hash_multi_file_matches_contract(tmp: Path) -> None:
    """Multi-file crate hash matches the path+bytes contract exactly."""
    crate_dir = tmp / "hash-multi" / "fixture-crate"
    (crate_dir / "src").mkdir(parents=True)
    (crate_dir / "Cargo.toml").write_text(FIXTURE_CARGO_TOML)
    (crate_dir / "src" / "lib.rs").write_text("pub fn a() {}\n")
    (crate_dir / "src" / "helper.rs").write_text("pub fn b() {}\n")

    actual = EXTRACTOR.hash_crate_sources(crate_dir)  # type: ignore[attr-defined]

    expected = hashlib.sha256(
        b"src/helper.rs" + b"pub fn b() {}\n" +
        b"src/lib.rs" + b"pub fn a() {}\n"
    ).hexdigest()
    expect(
        actual == expected,
        f"multi-file source hash mismatch: {actual} vs {expected}",
    )


def test_source_hash_excludes_target_dir(tmp: Path) -> None:
    """Files under a target/ directory do not contribute to the source hash."""
    crate_dir = tmp / "hash-target" / "fixture-crate"
    (crate_dir / "src").mkdir(parents=True)
    (crate_dir / "target" / "debug").mkdir(parents=True)
    (crate_dir / "Cargo.toml").write_text(FIXTURE_CARGO_TOML)
    (crate_dir / "src" / "lib.rs").write_text("pub fn a() {}\n")
    (crate_dir / "target" / "debug" / "build.rs").write_text("pub fn ignored() {}\n")

    actual = EXTRACTOR.hash_crate_sources(crate_dir)  # type: ignore[attr-defined]
    expected = hashlib.sha256(b"src/lib.rs" + b"pub fn a() {}\n").hexdigest()
    expect(
        actual == expected,
        f"source hash should ignore target/ files: {actual} vs {expected}",
    )


def test_empty_crate_produces_no_modules(tmp: Path) -> None:
    """A crate with no pub items produces no module sections."""
    crate_root = tmp / "empty"
    crate_dir = crate_root / "no-pubs"
    (crate_dir / "src").mkdir(parents=True)
    (crate_dir / "Cargo.toml").write_text(FIXTURE_CARGO_TOML)
    (crate_dir / "src" / "lib.rs").write_text("fn private() {}\n")

    original_root = EXTRACTOR.REPO_ROOT  # type: ignore[attr-defined]
    EXTRACTOR.REPO_ROOT = crate_root  # type: ignore[attr-defined]
    try:
        idx = EXTRACTOR.extract_crate("no-pubs", crate_dir)  # type: ignore[attr-defined]
    finally:
        EXTRACTOR.REPO_ROOT = original_root  # type: ignore[attr-defined]
    expect(
        idx.modules == [],
        f"expected empty modules list for crate with no pub items, got {len(idx.modules)} module(s)",
    )


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------


def main() -> int:
    """Run all fixture tests; return 0 on success, 1 on any failure."""
    with tempfile.TemporaryDirectory() as tmp_str:
        tmp = Path(tmp_str)
        _, md = run_extractor_on_fixture(tmp / "primary")

        test_pub_items_extracted(md)
        test_non_pub_items_excluded(md)
        test_cfg_test_items_excluded(md)
        test_doc_comments_attached(md)
        test_fn_body_stripped(md)
        test_deterministic(tmp / "det")
        test_source_hash_stable(tmp)
        test_source_hash_changes_with_content(tmp)
        test_source_hash_multi_file_matches_contract(tmp)
        test_source_hash_excludes_target_dir(tmp)
        test_empty_crate_produces_no_modules(tmp)

    if _FAILURES:
        print(f"FAIL: {len(_FAILURES)} assertion(s) failed", file=sys.stderr)
        for failure in _FAILURES:
            print(f"  - {failure}", file=sys.stderr)
        return 1

    print("OK: all L3 extractor fixture tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
