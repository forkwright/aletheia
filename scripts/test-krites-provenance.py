#!/usr/bin/env python3
"""Behavioral tests for scripts/check-krites-provenance.py + krites_provenance_lib.py.

Covers the wave-0 review's anti-backslide findings (P1, P2, P4, P6): the
exact reviewer bypass (flip a high-verbatim_pct row to sovereign) must now
be rejected, both directly (verbatim_pct left as evidence) and the sneakier
variant (verbatim_pct zeroed too, only the status-sequence check catches
it); an unresolvable --base-ref must fail closed, not silently pass as a
bootstrap commit; soak expiry must fire; offline recompute must fire when a
snapshot is present and FAIL when it is not, since the snapshot is tracked and
its absence disables the crate's only self-verification.

Also covers aletheia#6656: a 'sovereign' row must carry a real measurement
against what it replaced, not an unmeasured 0.0/none. replaced_upstream_path
may be nonzero-backed now, but only when it is honest — check_verbatim_recompute
must independently recompute it, krites-provenance-transition.py must retain
it instead of erasing it at the dual -> sovereign flip, and check_status_sequence
must reject a transition that carries forward a DIFFERENT path than the row
actually soaked against.
"""

from __future__ import annotations

import importlib.util
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import krites_provenance_lib as LIB

_CHECK_SCRIPT_PATH = Path(__file__).parent / "check-krites-provenance.py"
_TRANSITION_SCRIPT_PATH = Path(__file__).parent / "krites-provenance-transition.py"
_MEASURE_SCRIPT_PATH = Path(__file__).parent / "measure-krites-provenance.py"


def _load_module(name: str, path: Path) -> object:
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"failed to load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def _load_checker() -> object:
    return _load_module("check_krites_provenance", _CHECK_SCRIPT_PATH)


def _load_transition() -> object:
    return _load_module("krites_provenance_transition", _TRANSITION_SCRIPT_PATH)


def _load_measure() -> object:
    return _load_module("measure_krites_provenance", _MEASURE_SCRIPT_PATH)


CHECKER = _load_checker()
TRANSITION = _load_transition()
MEASURE = _load_measure()
_FAILURES: list[str] = []


def expect(condition: bool, msg: str) -> None:
    if not condition:
        _FAILURES.append(msg)


def expect_raises(exc_type: type, fn, msg: str) -> None:
    try:
        fn()
    except exc_type:
        return
    except Exception as exc:  # noqa: BLE001
        _FAILURES.append(f"{msg} (raised {type(exc).__name__} instead of {exc_type.__name__})")
        return
    _FAILURES.append(f"{msg} (raised nothing)")


def row(
    path: str,
    upstream_path: str,
    verbatim_pct: float,
    status: str,
    soak: int = 0,
    replaced_upstream_path: str = "none",
    method: str | None = None,
    method_evidence: str | None = None,
    consulted: list[str] | None = None,
) -> dict:
    # WHY method/method_evidence default from status, not a fixed literal: every
    # pre-#6797-followup test fixture in this file constructs both sovereign and
    # non-sovereign rows through this one helper, and the field's own validation
    # rule (krites_provenance_lib.validate_rows) is itself status-conditioned —
    # 'none' off sovereign, a real METHODS value (default 'unknown') on it. Mirroring
    # that here means every existing call site keeps passing without being touched.
    if method is None:
        method = "unknown" if status == "sovereign" else "none"
    if method_evidence is None:
        method_evidence = "none"
    return {
        "path": path,
        "upstream_path": upstream_path,
        "replaced_upstream_path": replaced_upstream_path,
        "verbatim_pct": verbatim_pct,
        "status": status,
        "soak_expires_at_commit_count": soak,
        "method": method,
        "method_evidence": method_evidence,
        "consulted": [] if consulted is None else consulted,
    }


# --- P1: sovereign/verbatim_pct cross-check (the reviewer's exact bypass) ---


def test_sovereign_high_verbatim_rejected() -> None:
    # WHY: this is the wave-0-review P1 reproduction verbatim — datalog.pest
    # flipped to sovereign/upstream_path=none with its 99.6 verbatim_pct left
    # untouched, the exact edit the reviewer made that the old gate missed.
    rows = [row("datalog.pest", "none", 99.6, "sovereign")]
    expect_raises(
        LIB.LedgerError,
        lambda: LIB.validate_rows(rows),
        "sovereign row with nonzero verbatim_pct (the reviewer's exact bypass) must be rejected",
    )


def test_sovereign_zero_verbatim_accepted() -> None:
    rows = [row("async_surface.rs", "none", 0.0, "sovereign")]
    try:
        LIB.validate_rows(rows)
    except LIB.LedgerError as exc:
        _FAILURES.append(f"legitimate sovereign row (verbatim_pct=0.0) must be accepted: {exc}")


# --- #6656: replaced_upstream_path — a sovereign row's number becomes real evidence ---


def test_sovereign_with_replaced_path_and_nonzero_verbatim_accepted() -> None:
    # WHY: this is the entire point of the fix — a sovereign row that retains a
    # measurement against what it replaced is no longer a bare 0.0/none claim, and
    # a nonzero verbatim_pct here is not, by itself, evidence of a bypass.
    rows = [row("fixed_rule/algos/dfs_native.rs", "none", 41.4, "sovereign",
                replaced_upstream_path="fixed_rule/algos/dfs.rs")]
    try:
        LIB.validate_rows(rows)
    except LIB.LedgerError as exc:
        _FAILURES.append(
            f"a sovereign row with a real replaced_upstream_path must accept a nonzero "
            f"verbatim_pct: {exc}"
        )


def test_sovereign_no_replaced_path_nonzero_verbatim_still_rejected() -> None:
    # WHY: the narrowed P1 protection — a sovereign row with NOTHING retained to
    # measure against (replaced_upstream_path == 'none') still cannot carry a
    # nonzero verbatim_pct; that would be an unmeasured claim with no evidence at all.
    rows = [row("kcore.rs", "none", 12.0, "sovereign")]
    expect_raises(
        LIB.LedgerError,
        lambda: LIB.validate_rows(rows),
        "a sovereign row with replaced_upstream_path='none' and nonzero verbatim_pct "
        "must still be rejected (no retained evidence backs the number)",
    )


def test_replaced_path_rejected_on_non_sovereign_row() -> None:
    # WHY: replaced_upstream_path is a sovereign-only concept — a derived/dual row
    # already carries a live lineage claim in upstream_path; a stray
    # replaced_upstream_path on such a row is structurally meaningless.
    rows = [row("x.rs", "x.rs", 80.0, "dual", soak=30, replaced_upstream_path="y.rs")]
    expect_raises(
        LIB.LedgerError,
        lambda: LIB.validate_rows(rows),
        "replaced_upstream_path on a non-sovereign row must be rejected",
    )


def test_missing_replaced_path_key_defaults_to_none() -> None:
    # WHY: a pre-#6656 ledger (e.g. the base-ref commit check-krites-provenance.py
    # diffs every PR against) has no replaced_upstream_path key at all. Absence must
    # parse as 'none', not raise — otherwise every --base-ref comparison against
    # pre-migration history hard-fails.
    legacy_row = {
        "path": "z.rs",
        "upstream_path": "z.rs",
        "verbatim_pct": 40.0,
        "status": "derived",
        "soak_expires_at_commit_count": 0,
    }
    try:
        LIB.validate_rows([legacy_row])
    except LIB.LedgerError as exc:
        _FAILURES.append(f"a row missing replaced_upstream_path entirely must default to 'none': {exc}")
    expect(
        legacy_row.get("replaced_upstream_path") == "none",
        f"validate_rows must backfill the missing key to 'none'; got {legacy_row.get('replaced_upstream_path')!r}",
    )


# --- #6656: check_verbatim_recompute now also holds sovereign rows accountable ---


def test_verbatim_recompute_catches_unmeasured_sovereign_claim() -> None:
    # WHY: this is the literal aletheia#6656 reproduction — a file that is really
    # ~41% similar to what it replaced, entered in the ledger at verbatim_pct=0.0,
    # with a real replaced_upstream_path recorded. Before this fix,
    # check_verbatim_recompute skipped every 'sovereign' row unconditionally and
    # this would have passed clean; it must now fail.
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        snapshot_dir = root / "snapshot"
        snapshot_dir.mkdir()
        (snapshot_dir / "up.rs").write_text(
            "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\n"
            "fn e() {}\nfn f() {}\nfn g() {}\nfn h() {}\n"
        )
        src_dir = root / "src"
        src_dir.mkdir()
        # shares 4 of 8 lines with up.rs -- above MIN_MATCH_BLOCK_LINES, so measurably 50.0.
        (src_dir / "local.rs").write_text(
            # WHY 4 contiguous shared lines: MIN_MATCH_BLOCK_LINES floors shorter runs,
            # so a 2-line overlap measures 0.0 and would not exercise this path at all.
            "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\n"
            "fn zzz() {}\nfn www() {}\nfn yyy() {}\nfn xxx() {}\n"
        )

        orig_snapshot = CHECKER.UPSTREAM_SNAPSHOT_DIR
        orig_src = CHECKER.KRITES_SRC
        CHECKER.UPSTREAM_SNAPSHOT_DIR = snapshot_dir
        CHECKER.KRITES_SRC = src_dir
        try:
            unmeasured = row("local.rs", "none", 0.0, "sovereign", replaced_upstream_path="up.rs")
            errors = CHECKER.check_verbatim_recompute([unmeasured])
            expect(
                any("does not match offline recomputation" in e for e in errors),
                f"a sovereign row certified at 0.0 with a real replaced_upstream_path must be "
                f"caught by offline recompute; got {errors}",
            )

            honest = row("local.rs", "none", 50.0, "sovereign", replaced_upstream_path="up.rs")
            errors2 = CHECKER.check_verbatim_recompute([honest])
            expect(errors2 == [], f"a correctly-measured sovereign row must pass; got {errors2}")
        finally:
            CHECKER.UPSTREAM_SNAPSHOT_DIR = orig_snapshot
            CHECKER.KRITES_SRC = orig_src


def test_verbatim_recompute_skips_sovereign_with_no_replaced_path() -> None:
    # WHY: a genuinely fresh file with nothing to compare against
    # (replaced_upstream_path == 'none', e.g. kcore.rs) must not be flagged — there
    # is no predecessor to recompute against, and 0.0 is the honest answer.
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        snapshot_dir = root / "snapshot"
        snapshot_dir.mkdir()
        src_dir = root / "src"
        src_dir.mkdir()
        (src_dir / "fresh.rs").write_text("fn only_here() {}\n")

        orig_snapshot = CHECKER.UPSTREAM_SNAPSHOT_DIR
        orig_src = CHECKER.KRITES_SRC
        CHECKER.UPSTREAM_SNAPSHOT_DIR = snapshot_dir
        CHECKER.KRITES_SRC = src_dir
        try:
            fresh = row("fresh.rs", "none", 0.0, "sovereign")
            errors = CHECKER.check_verbatim_recompute([fresh])
            expect(errors == [], f"a sovereign row with no retained predecessor must be skipped; got {errors}")
        finally:
            CHECKER.UPSTREAM_SNAPSHOT_DIR = orig_snapshot
            CHECKER.KRITES_SRC = orig_src


# --- #6656: check_status_sequence's dual -> sovereign replaced_upstream_path cross-check ---


def test_status_sequence_rejects_dual_to_sovereign_with_dropped_replaced_path() -> None:
    # WHY: the exact aletheia#6656 bypass at the ledger-schema level — flip status
    # to sovereign but leave replaced_upstream_path at its default 'none' instead of
    # carrying the dual-era upstream_path forward. verbatim_pct=0.0 alone would have
    # satisfied the OTHER checks (validate_rows' narrowed P1 requires exactly this
    # when replaced_upstream_path == 'none'), so this cross-check is the only thing
    # that catches a transition that discarded its own evidence.
    base_rows = [row("w.rs", "w.rs", 57.8, "dual", soak=100)]
    current_rows = [row("w.rs", "none", 0.0, "sovereign")]  # replaced_upstream_path defaults to 'none'
    errors = CHECKER.check_status_sequence(current_rows, base_rows)
    expect(
        any("must carry its dual-era upstream_path forward" in e for e in errors),
        f"dropping replaced_upstream_path across a dual -> sovereign transition must be "
        f"rejected; got {errors}",
    )


def test_status_sequence_rejects_dual_to_sovereign_with_wrong_replaced_path() -> None:
    # WHY: a replaced_upstream_path that names a DIFFERENT file than the row actually
    # soaked against is also wrong — the ledger is now proving a claim about the wrong
    # comparison target.
    base_rows = [row("w.rs", "w.rs", 57.8, "dual", soak=100)]
    current_rows = [row("w.rs", "none", 57.8, "sovereign", replaced_upstream_path="other.rs")]
    errors = CHECKER.check_status_sequence(current_rows, base_rows)
    expect(
        any("must carry its dual-era upstream_path forward" in e for e in errors),
        f"a replaced_upstream_path that does not match the row's own dual-era "
        f"upstream_path must be rejected; got {errors}",
    )


def test_status_sequence_accepts_dual_to_sovereign_with_correct_replaced_path() -> None:
    base_rows = [row("w.rs", "w.rs", 57.8, "dual", soak=100)]
    current_rows = [row("w.rs", "none", 57.8, "sovereign", replaced_upstream_path="w.rs")]
    errors = CHECKER.check_status_sequence(current_rows, base_rows)
    expect(errors == [], f"a correctly-retained dual -> sovereign transition must pass; got {errors}")


# --- #6656: krites-provenance-transition.py must retain, not erase, the measurement ---


def test_apply_to_sovereign_retains_and_recomputes() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        snapshot_dir = root / "snapshot"
        snapshot_dir.mkdir()
        (snapshot_dir / "up.rs").write_text(
            "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\n"
            "fn e() {}\nfn f() {}\nfn g() {}\nfn h() {}\n"
        )
        src_dir = root / "src"
        src_dir.mkdir()
        (src_dir / "local.rs").write_text(
            # WHY 4 contiguous shared lines: MIN_MATCH_BLOCK_LINES floors shorter runs,
            # so a 2-line overlap measures 0.0 and would not exercise this path at all.
            "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\n"
            "fn zzz() {}\nfn www() {}\nfn yyy() {}\nfn xxx() {}\n"
        )

        orig_snapshot = TRANSITION.UPSTREAM_SNAPSHOT_DIR
        orig_src = TRANSITION.KRITES_SRC
        TRANSITION.UPSTREAM_SNAPSHOT_DIR = snapshot_dir
        TRANSITION.KRITES_SRC = src_dir
        try:
            r = row("local.rs", "up.rs", 12.3, "dual", soak=100)  # stale/placeholder verbatim_pct
            TRANSITION.apply_to_sovereign(r)
            expect(r["upstream_path"] == "none", f"upstream_path must become 'none'; got {r['upstream_path']!r}")
            expect(
                r["replaced_upstream_path"] == "up.rs",
                f"replaced_upstream_path must retain the dual-era upstream_path; got {r['replaced_upstream_path']!r}",
            )
            expect(
                r["verbatim_pct"] == 50.0,
                f"verbatim_pct must be recomputed fresh against the snapshot (2 of 4 lines match "
                f"= 50.0), not left at its stale dual-era value; got {r['verbatim_pct']}",
            )
            expect(
                r["soak_expires_at_commit_count"] == 0,
                f"soak_expires_at_commit_count must be zeroed; got {r['soak_expires_at_commit_count']}",
            )
            expect(
                r["method"] == "unknown" and r["method_evidence"] == "none",
                "a status transition is not an authorship record -- a row entering sovereign "
                f"for the first time must start at method='unknown'/'none'; got "
                f"method={r['method']!r}, method_evidence={r['method_evidence']!r}",
            )
        finally:
            TRANSITION.UPSTREAM_SNAPSHOT_DIR = orig_snapshot
            TRANSITION.KRITES_SRC = orig_src


def test_apply_to_sovereign_falls_back_when_snapshot_missing() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        snapshot_dir = root / "no-such-snapshot"
        src_dir = root / "src"
        src_dir.mkdir()
        (src_dir / "local.rs").write_text("fn a() {}\n")

        orig_snapshot = TRANSITION.UPSTREAM_SNAPSHOT_DIR
        orig_src = TRANSITION.KRITES_SRC
        TRANSITION.UPSTREAM_SNAPSHOT_DIR = snapshot_dir
        TRANSITION.KRITES_SRC = src_dir
        try:
            r = row("local.rs", "up.rs", 33.3, "dual", soak=100)
            TRANSITION.apply_to_sovereign(r)
            expect(r["upstream_path"] == "none", f"upstream_path must become 'none'; got {r['upstream_path']!r}")
            expect(
                r["replaced_upstream_path"] == "up.rs",
                f"replaced_upstream_path must still be retained even without a snapshot; got {r['replaced_upstream_path']!r}",
            )
            expect(
                r["verbatim_pct"] == 33.3,
                f"verbatim_pct must fall back to the dual-era value when the snapshot is absent "
                f"(check_verbatim_recompute catches drift once it lands); got {r['verbatim_pct']}",
            )
        finally:
            TRANSITION.UPSTREAM_SNAPSHOT_DIR = orig_snapshot
            TRANSITION.KRITES_SRC = orig_src
# --- aletheia#6656: verbatim_pct metric defects (leading whitespace, punctuation floor) ---


def test_verbatim_pct_ignores_reindentation() -> None:
    # WHY: the exact aletheia#6656 reproduction — wrapping an unmodified,
    # preserved file in an extra `mod wrapper { }` nesting (a pure
    # re-indentation, no content change) must not zero out its similarity
    # score. Pre-fix, storage/mem.rs dropped from ~69% to 4.5% verbatim_pct
    # from exactly this shape of edit; nonblank_lines() only stripped the
    # trailing newline splitlines() already removes, never the leading
    # whitespace the re-indent shifted every line by.
    upstream = "fn a() {\n    let x = 1;\n    let y = 2;\n    x + y\n}\n"
    reindented = (
        "mod wrapper {\n"
        "    fn a() {\n"
        "        let x = 1;\n"
        "        let y = 2;\n"
        "        x + y\n"
        "    }\n"
        "}\n"
    )
    pct = LIB.verbatim_pct(reindented, upstream)
    expect(
        pct >= 70.0,
        f"a pure re-indentation of an otherwise-identical file must still score high "
        f"(nonblank_lines must strip leading whitespace, not just the trailing newline); got {pct}",
    )


def test_verbatim_pct_floors_out_scattered_punctuation_matches() -> None:
    # WHY: the audit's reproduction — `runtime/hnsw_sovereign/types.rs`, with
    # no authored relationship to `runtime/hnsw.rs`, still scored 12.4%
    # against it from scattered single/double-line collisions on language
    # boilerplate (`}`, `#[cfg(test)]`, `mod tests {`). Two files below share
    # only such scattered fragments — no block reaches MIN_MATCH_BLOCK_LINES
    # — and must floor to 0, even though an unfloored (block-size >= 1)
    # comparison of the same pair is nonzero (41.2%), proving the floor is
    # what suppresses the false signal, not an accident of the fixture.
    local = (
        "//! Vector cache eviction policy for the sovereign index.\n"
        "use std::num::NonZeroUsize;\n"
        "\n"
        "pub(crate) struct EvictionCache {\n"
        "    capacity: NonZeroUsize,\n"
        "}\n"
        "\n"
        "impl EvictionCache {\n"
        "    pub(crate) fn touch(&mut self, key: u64) {\n"
        "        self.recent.push(key);\n"
        "    }\n"
        "}\n"
        "\n"
        "#[cfg(test)]\n"
        "mod eviction_tests {\n"
        "    #[test]\n"
        "    fn touch_updates_recency() {\n"
        "        assert!(true);\n"
        "    }\n"
        "}\n"
    )
    upstream = (
        "//! HNSW graph traversal and neighbour search.\n"
        "use std::collections::BinaryHeap;\n"
        "\n"
        "pub(crate) struct SearchState {\n"
        "    frontier: BinaryHeap<Candidate>,\n"
        "}\n"
        "\n"
        "impl SearchState {\n"
        "    pub(crate) fn push(&mut self, candidate: Candidate) {\n"
        "        self.frontier.push(candidate);\n"
        "    }\n"
        "}\n"
        "\n"
        "#[cfg(test)]\n"
        "mod search_tests {\n"
        "    #[test]\n"
        "    fn push_orders_by_distance() {\n"
        "        assert!(false);\n"
        "    }\n"
        "}\n"
    )
    pct = LIB.verbatim_pct(local, upstream)
    expect(
        pct == 0.0,
        f"scattered sub-floor punctuation/boilerplate matches with no real shared block "
        f"must not read as evidence; got {pct}",
    )
    unfloored = sum(
        block.size
        for block in __import__("difflib")
        .SequenceMatcher(None, LIB.nonblank_lines(local), LIB.nonblank_lines(upstream), autojunk=False)
        .get_matching_blocks()
        if block.size > 0
    )
    expect(
        unfloored > 0,
        "fixture must contain real (if scattered) matches pre-floor, or the test proves nothing",
    )


def test_verbatim_pct_full_match_on_file_shorter_than_floor() -> None:
    # WHY: MIN_MATCH_BLOCK_LINES must not floor a genuinely complete verbatim
    # copy of a file shorter than the floor itself to 0 — the floor exists to
    # suppress a small match INSIDE a larger, otherwise-unrelated file, not to
    # blind the metric to short files entirely.
    identical = "fn a() {}\nfn b() {}\n"
    pct = LIB.verbatim_pct(identical, identical)
    expect(pct == 100.0, f"a file identical to upstream must score 100% regardless of length; got {pct}")
# --- P1: status-sequence enforcement (the sneakier variant: verbatim_pct zeroed too) ---


def test_status_sequence_rejects_direct_derived_to_sovereign() -> None:
    base_rows = [row("datalog.pest", "cozoscript.pest", 99.6, "derived")]
    current_rows = [row("datalog.pest", "none", 0.0, "sovereign")]
    errors = CHECKER.check_status_sequence(current_rows, base_rows)
    expect(
        any("illegal status transition" in e and "'derived' -> 'sovereign'" in e for e in errors),
        f"direct derived->sovereign with verbatim_pct zeroed must still be rejected by the "
        f"sequence check (the second half of the P1 fix); got {errors}",
    )


def test_status_sequence_accepts_derived_to_dual_and_dual_to_sovereign() -> None:
    base_rows = [row("x.rs", "x.rs", 80.0, "derived")]
    current_rows = [row("x.rs", "x.rs", 80.0, "dual", soak=30)]
    errors = CHECKER.check_status_sequence(current_rows, base_rows)
    expect(errors == [], f"derived -> dual must be legal; got {errors}")

    base_rows2 = [row("x.rs", "x.rs", 80.0, "dual", soak=30)]
    current_rows2 = [row("x.rs", "none", 80.0, "sovereign", replaced_upstream_path="x.rs")]
    errors2 = CHECKER.check_status_sequence(current_rows2, base_rows2)
    expect(errors2 == [], f"dual -> sovereign must be legal; got {errors2}")


def test_status_sequence_ignores_path_absent_from_base() -> None:
    # WHY(P3): a path with no base-ref row at all (a completeness fix, e.g.
    # fts/README.md before this PR) has no prior status to regress from —
    # must not be treated as an illegal transition.
    current_rows = [row("fts/README.md", "fts/README.md", 100.0, "derived")]
    errors = CHECKER.check_status_sequence(current_rows, base_rows=[])
    expect(errors == [], f"a brand-new ledger row must never trip the sequence check; got {errors}")


# --- P4: growth check — true regression vs. completeness-fix false positive ---


def test_no_derived_growth_rejects_true_regression() -> None:
    base_rows = [row("y.rs", "none", 0.0, "sovereign")]
    current_rows = [row("y.rs", "y.rs", 40.0, "derived")]
    errors = CHECKER.check_no_derived_growth(current_rows, base_rows)
    expect(
        any("regressed TO 'derived'" in e for e in errors),
        f"a known row regressing sovereign -> derived must fail; got {errors}",
    )


def test_no_derived_growth_ignores_path_absent_from_base() -> None:
    # WHY(P3): fts/README.md and gen_stopwords.py enter the ledger as
    # 'derived' for the first time in this PR — that is completeness, not a
    # backslide, and must not trip the growth check.
    current_rows = [row("fts/README.md", "fts/README.md", 100.0, "derived")]
    errors = CHECKER.check_no_derived_growth(current_rows, base_rows=[])
    expect(errors == [], f"a brand-new derived row must never trip the growth check; got {errors}")


def test_no_derived_growth_skips_on_bootstrap() -> None:
    errors = CHECKER.check_no_derived_growth([row("z.rs", "z.rs", 10.0, "derived")], base_rows=None)
    expect(errors == [], "base_rows=None (bootstrap) must skip the growth check, not fail")


# --- P4: fail-closed base-ref resolution, against the real repo's git history ---


def test_ref_exists_true_for_head() -> None:
    expect(CHECKER.ref_exists("HEAD"), "HEAD must resolve in a real git checkout")


def test_ref_exists_false_for_bogus_ref() -> None:
    expect(
        not CHECKER.ref_exists("this-ref-should-never-exist-zzzqqq"),
        "a nonexistent ref must not resolve",
    )


def test_load_base_rows_fails_closed_on_unresolvable_ref() -> None:
    # WHY(P4): this is the exact reviewer reproduction —
    # `--base-ref origin/does-not-exist` used to be silently treated as a
    # bootstrap commit (fail open, exit 0). Must now raise instead.
    expect_raises(
        CHECKER.BaseRefError,
        lambda: CHECKER.load_base_rows("this-ref-should-never-exist-zzzqqq"),
        "an unresolvable base ref must raise BaseRefError (fail closed), not return None",
    )


def test_git_commit_count_returns_positive_int_for_head() -> None:
    count = CHECKER.git_commit_count("HEAD")
    expect(
        isinstance(count, int) and count > 0,
        f"git_commit_count('HEAD') must return a positive int in a real checkout; got {count}",
    )


def test_git_commit_count_none_for_bogus_ref() -> None:
    count = CHECKER.git_commit_count("this-ref-should-never-exist-zzzqqq")
    expect(count is None, f"git_commit_count on a bogus ref must return None; got {count}")


# --- P2: soak expiry ---


def test_soak_expiry_flags_expired_dual_row() -> None:
    rows_ = [row("w.rs", "w.rs", 50.0, "dual", soak=100)]
    errors = CHECKER.check_soak_expiry(rows_, commit_count=100)
    expect(any("soak window expired" in e for e in errors), f"commit_count == expiry must fire; got {errors}")

    errors2 = CHECKER.check_soak_expiry(rows_, commit_count=150)
    expect(any("soak window expired" in e for e in errors2), f"commit_count > expiry must fire; got {errors2}")


def test_soak_expiry_accepts_not_yet_expired() -> None:
    rows_ = [row("w.rs", "w.rs", 50.0, "dual", soak=100)]
    errors = CHECKER.check_soak_expiry(rows_, commit_count=99)
    expect(errors == [], f"commit_count < expiry must not fire; got {errors}")


def test_soak_expiry_rejects_nonpositive_expiry_on_dual_row() -> None:
    rows_ = [row("w.rs", "w.rs", 50.0, "dual", soak=0)]
    errors = CHECKER.check_soak_expiry(rows_, commit_count=1)
    expect(
        any("requires a positive soak_expires_at_commit_count" in e for e in errors),
        f"a dual row with soak=0 (undefined window) must fail; got {errors}",
    )


def test_soak_expiry_skips_when_no_dual_rows() -> None:
    rows_ = [row("w.rs", "w.rs", 50.0, "derived")]
    errors = CHECKER.check_soak_expiry(rows_, commit_count=None)
    expect(errors == [], "no dual rows must skip soak-expiry evaluation entirely, even with commit_count=None")


def test_soak_expiry_fails_closed_when_commit_count_unavailable() -> None:
    rows_ = [row("w.rs", "w.rs", 50.0, "dual", soak=100)]
    errors = CHECKER.check_soak_expiry(rows_, commit_count=None)
    expect(
        any("could not determine the current commit count" in e for e in errors),
        f"an unavailable commit count with a live dual row must fail closed; got {errors}",
    )


# --- #6988: a land-dark module whose retiring copies carry no fuse ---


def test_land_dark_unfused_flags_shadowed_derived_rows() -> None:
    # WHY: the literal #6988 reproduction — hnsw_sovereign/* landed beside
    # hnsw/* while every derived row kept soak_expires_at_commit_count=0, and
    # no check saw it. Every derived row in the shadowed module must be named.
    rows = [
        row("runtime/hnsw/mod.rs", "runtime/hnsw.rs", 46.0, "derived"),
        row("runtime/hnsw/graph.rs", "runtime/hnsw.rs", 40.0, "derived"),
        row("runtime/hnsw_sovereign/mod.rs", "none", 0.0, "sovereign", replaced_upstream_path="runtime/hnsw.rs"),
        row("runtime/hnsw_sovereign/graph.rs", "none", 13.6, "sovereign", replaced_upstream_path="runtime/hnsw.rs"),
    ]
    errors = CHECKER.check_land_dark_unfused(rows)
    expect(
        len(errors) == 2
        and all("land-dark with no soak fuse" in e for e in errors)
        and any("runtime/hnsw/mod.rs" in e for e in errors)
        and any("runtime/hnsw/graph.rs" in e for e in errors),
        f"both shadowed derived rows must be flagged; got {errors}",
    )


def test_land_dark_unfused_accepts_dual_rows() -> None:
    # WHY: a dual row already carries the fuse this check exists to force, and
    # check_soak_expiry owns bounding it — flagging it here too would report
    # one defect twice.
    rows = [
        row("runtime/hnsw/mod.rs", "runtime/hnsw.rs", 46.0, "dual", soak=100),
        row("runtime/hnsw_sovereign/mod.rs", "none", 0.0, "sovereign", replaced_upstream_path="runtime/hnsw.rs"),
    ]
    errors = CHECKER.check_land_dark_unfused(rows)
    expect(errors == [], f"a shadowed dual row (fuse scheduled) must pass; got {errors}")


def test_land_dark_unfused_ignores_partial_retirement() -> None:
    # WHY: wave 2a's actual shape — only ascii_folding_filter's fold table was
    # retired, so its sovereign rows normalize into the fold_table/ subtree
    # while the still-live derived filter and tests sit in the module root.
    # An upstream_path-granularity check would flag them; the directory-shadow
    # check must not, or every partial retirement becomes unlandable.
    rows = [
        row("fts/tokenizer/ascii_folding_filter/mod.rs", "fts/tokenizer/ascii_folding_filter.rs", 30.0, "derived"),
        row("fts/tokenizer/ascii_folding_filter/tests/mod.rs", "fts/tokenizer/ascii_folding_filter.rs", 25.0, "derived"),
        row("fts/tokenizer/ascii_folding_filter/fold_table.rs", "none", 0.0, "sovereign", replaced_upstream_path="fts/tokenizer/ascii_folding_filter.rs"),
        row("fts/tokenizer/ascii_folding_filter/fold_table/fold_table_sovereign/mod.rs", "none", 15.5, "sovereign", replaced_upstream_path="fts/tokenizer/ascii_folding_filter.rs"),
    ]
    errors = CHECKER.check_land_dark_unfused(rows)
    expect(errors == [], f"a partial retirement (fold-table-only) must not flag the live derived files; got {errors}")


def test_land_dark_unfused_flags_paired_derived_dir_layout() -> None:
    # WHY: the stop_word_filter layout pairs derived/ and sovereign/ dirs under
    # one module dir; a derived row in that layout is just as land-dark as the
    # hnsw shape and must be caught too.
    rows = [
        row("fts/tokenizer/stop_word_filter/derived/mod.rs", "fts/tokenizer/stop_word_filter/mod.rs", 90.0, "derived"),
        row("fts/tokenizer/stop_word_filter/sovereign/mod.rs", "none", 15.5, "sovereign", replaced_upstream_path="fts/tokenizer/stop_word_filter/mod.rs"),
    ]
    errors = CHECKER.check_land_dark_unfused(rows)
    expect(
        len(errors) == 1 and "stop_word_filter/derived/mod.rs" in errors[0],
        f"a derived/ row shadowed by its paired sovereign/ dir must be flagged; got {errors}",
    )


def test_land_dark_unfused_quiet_without_any_shadow() -> None:
    # WHY: the overwhelming common case — derived rows with no sovereign
    # replacement landed yet are the program's normal state, not a defect.
    rows = [
        row("data/value.rs", "data/value.rs", 60.3, "derived"),
        row("async_surface.rs", "none", 0.0, "sovereign"),
    ]
    errors = CHECKER.check_land_dark_unfused(rows)
    expect(errors == [], f"derived rows with no sovereign shadow must pass; got {errors}")


# --- P6: offline verbatim recompute ---


def test_verbatim_recompute_fails_closed_without_snapshot() -> None:
    # WHY inverted: the old assertion pinned a SKIP, which was correct only
    # while the snapshot had not yet been vendored. It is now 108 tracked files,
    # so the skip had become an unconditional fail-open — measured, deleting the
    # whole snapshot made the checker report "clean (207 rows)" and exit 0.
    with tempfile.TemporaryDirectory() as tmp:
        fake_snapshot = Path(tmp) / "no-such-snapshot"
        orig = CHECKER.UPSTREAM_SNAPSHOT_DIR
        CHECKER.UPSTREAM_SNAPSHOT_DIR = fake_snapshot
        try:
            errors = CHECKER.check_verbatim_recompute([row("q.rs", "q.rs", 50.0, "derived")])
            expect(
                len(errors) == 1 and "upstream-snapshot/ is absent" in errors[0],
                f"absent snapshot dir must FAIL, not skip; got {errors}",
            )
        finally:
            CHECKER.UPSTREAM_SNAPSHOT_DIR = orig


def test_verbatim_recompute_detects_drift() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        snapshot_dir = root / "snapshot"
        snapshot_dir.mkdir()
        (snapshot_dir / "up.rs").write_text("fn a() {}\nfn b() {}\n")
        src_dir = root / "src"
        src_dir.mkdir()
        (src_dir / "local.rs").write_text("fn a() {}\nfn b() {}\n")  # identical -> 100.0

        orig_snapshot = CHECKER.UPSTREAM_SNAPSHOT_DIR
        orig_src = CHECKER.KRITES_SRC
        CHECKER.UPSTREAM_SNAPSHOT_DIR = snapshot_dir
        CHECKER.KRITES_SRC = src_dir
        try:
            stale_row = row("local.rs", "up.rs", 40.0, "derived")  # stale: real value is 100.0
            errors = CHECKER.check_verbatim_recompute([stale_row])
            expect(
                any("does not match offline recomputation" in e for e in errors),
                f"a stale stored verbatim_pct must be caught by offline recompute; got {errors}",
            )

            fresh_row = row("local.rs", "up.rs", 100.0, "derived")
            errors2 = CHECKER.check_verbatim_recompute([fresh_row])
            expect(errors2 == [], f"a correct stored verbatim_pct must pass; got {errors2}")
        finally:
            CHECKER.UPSTREAM_SNAPSHOT_DIR = orig_snapshot
            CHECKER.KRITES_SRC = orig_src


# --- #6797: a sovereign/'none' row must be an explicit, reasoned declaration ---


def test_no_unjustified_exemption_rejects_bare_none() -> None:
    # WHY: this is the literal aletheia#6797 reproduction — check_verbatim_recompute
    # SKIPS every replaced_upstream_path == 'none' row unconditionally, and before
    # this check existed, nothing distinguished a genuinely fresh file from one
    # nobody had ever mapped. A row not in NO_PREDECESSOR_REASONS must be rejected.
    orig = CHECKER.NO_PREDECESSOR_REASONS
    CHECKER.NO_PREDECESSOR_REASONS = {"justified.rs": "genuinely fresh, no predecessor"}
    try:
        rows = [row("unjustified.rs", "none", 0.0, "sovereign")]
        errors = CHECKER.check_no_unjustified_exemption(rows)
        expect(
            any("has no entry in" in e and "unjustified.rs" in e for e in errors),
            f"a sovereign/'none' row absent from NO_PREDECESSOR_REASONS must be rejected; got {errors}",
        )
    finally:
        CHECKER.NO_PREDECESSOR_REASONS = orig


def test_no_unjustified_exemption_accepts_justified_none() -> None:
    orig = CHECKER.NO_PREDECESSOR_REASONS
    CHECKER.NO_PREDECESSOR_REASONS = {"justified.rs": "genuinely fresh, no predecessor"}
    try:
        rows = [row("justified.rs", "none", 0.0, "sovereign")]
        errors = CHECKER.check_no_unjustified_exemption(rows)
        expect(errors == [], f"a sovereign/'none' row present in NO_PREDECESSOR_REASONS must pass; got {errors}")
    finally:
        CHECKER.NO_PREDECESSOR_REASONS = orig


def test_no_unjustified_exemption_ignores_rows_with_a_real_predecessor() -> None:
    # A sovereign row that DOES carry a real replaced_upstream_path has something to
    # measure against (check_verbatim_recompute handles it) and has no business in
    # NO_PREDECESSOR_REASONS at all.
    orig = CHECKER.NO_PREDECESSOR_REASONS
    CHECKER.NO_PREDECESSOR_REASONS = {}
    try:
        rows = [row("measured.rs", "none", 15.5, "sovereign", replaced_upstream_path="upstream.rs")]
        errors = CHECKER.check_no_unjustified_exemption(rows)
        expect(errors == [], f"a sovereign row with a real replaced_upstream_path must not be flagged; got {errors}")
    finally:
        CHECKER.NO_PREDECESSOR_REASONS = orig


def test_no_unjustified_exemption_flags_stale_reason() -> None:
    # A NO_PREDECESSOR_REASONS entry for a path that is no longer a sovereign/'none'
    # row (deleted, or graduated into SOVEREIGN_VERIFY_MAP with a real predecessor)
    # is an unread reason nobody is checking any more — the same shape of default
    # this check exists to close, facing the other direction.
    orig = CHECKER.NO_PREDECESSOR_REASONS
    CHECKER.NO_PREDECESSOR_REASONS = {"gone.rs": "stale reason for a row that no longer qualifies"}
    try:
        rows = [row("measured.rs", "none", 15.5, "sovereign", replaced_upstream_path="upstream.rs")]
        errors = CHECKER.check_no_unjustified_exemption(rows)
        expect(
            any("stale entry" in e and "gone.rs" in e for e in errors),
            f"a stale NO_PREDECESSOR_REASONS entry must be flagged; got {errors}",
        )
    finally:
        CHECKER.NO_PREDECESSOR_REASONS = orig


# --- #6797-followup: method records HOW a sovereign row was written ---


def test_method_missing_key_passes_validate_rows() -> None:
    # WHY tolerated at the validate_rows layer: a ledger serialized before this
    # field existed has no such key at all, and check-krites-provenance.py's
    # --base-ref comparison reads exactly such a ledger on every PR until enough
    # history passes it by (the same reasoning replaced_upstream_path's own
    # setdefault documents). Presence is gated at check-krites-provenance.py's
    # check_method_recorded instead, on the CURRENT ledger only.
    rows = [{"path": "old.rs", "upstream_path": "up.rs", "replaced_upstream_path": "none", "verbatim_pct": 40.0, "status": "derived", "soak_expires_at_commit_count": 0}]
    try:
        LIB.validate_rows(rows)
    except LIB.LedgerError as exc:
        _FAILURES.append(f"a row with no 'method' key at all must still pass validate_rows (pre-migration ledger read); got {exc}")


def test_method_recorded_rejects_missing_key() -> None:
    rows = [row("a.rs", "up.rs", 40.0, "derived")]
    del rows[0]["method"]
    errors = CHECKER.check_method_recorded(rows)
    expect(
        any("missing 'method'" in e and "a.rs" in e for e in errors),
        f"a row with no 'method' key must be rejected by check_method_recorded; got {errors}",
    )


def test_method_recorded_rejects_sovereign_transliterated() -> None:
    rows = [row("copy.rs", "none", 15.5, "sovereign", replaced_upstream_path="up.rs", method="transliterated", method_evidence="#6656")]
    errors = CHECKER.check_method_recorded(rows)
    expect(
        any("transliterated" in e and "copy.rs" in e for e in errors),
        f"a sovereign row carrying method='transliterated' must be rejected; got {errors}",
    )


def test_method_recorded_accepts_legitimate_value() -> None:
    rows = [
        row("oracle.rs", "none", 0.0, "sovereign", method="from_behavioral_oracle", method_evidence="3d0c035eedda8c476bb6d9b71dbdd1f5c336377c"),
        row("derived.rs", "up.rs", 40.0, "derived"),
    ]
    errors = CHECKER.check_method_recorded(rows)
    expect(errors == [], f"rows with a legitimate method must pass check_method_recorded; got {errors}")


def test_method_only_meaningful_on_sovereign() -> None:
    rows = [row("d.rs", "up.rs", 40.0, "derived", method="from_spec")]
    expect_raises(
        LIB.LedgerError,
        lambda: LIB.validate_rows(rows),
        "a non-sovereign row carrying a real method value must be rejected",
    )


def test_sovereign_method_must_be_a_known_value() -> None:
    rows = [row("s.rs", "none", 0.0, "sovereign", method="hand_waved")]
    expect_raises(
        LIB.LedgerError,
        lambda: LIB.validate_rows(rows),
        "a sovereign row's method must be one of METHODS",
    )


def test_sovereign_method_rejects_none() -> None:
    rows = [row("s.rs", "none", 0.0, "sovereign", method="none")]
    expect_raises(
        LIB.LedgerError,
        lambda: LIB.validate_rows(rows),
        "a sovereign row must never carry method='none' -- it always has an authorship claim, "
        "even if that claim is 'unknown'",
    )


def test_method_evidence_required_when_resolved() -> None:
    rows = [row("s.rs", "none", 0.0, "sovereign", method="attested_original", method_evidence="none")]
    expect_raises(
        LIB.LedgerError,
        lambda: LIB.validate_rows(rows),
        "a resolved (non-unknown) sovereign method must carry real method_evidence, not 'none'",
    )


def test_method_evidence_forbidden_when_unknown() -> None:
    rows = [row("s.rs", "none", 0.0, "sovereign", method="unknown", method_evidence="#1234")]
    expect_raises(
        LIB.LedgerError,
        lambda: LIB.validate_rows(rows),
        "method='unknown' must carry method_evidence='none' -- unknown has nothing to point at",
    )


def test_method_evidence_forbidden_off_sovereign() -> None:
    rows = [row("d.rs", "up.rs", 40.0, "derived", method_evidence="#1234")]
    expect_raises(
        LIB.LedgerError,
        lambda: LIB.validate_rows(rows),
        "a non-sovereign row must never carry a real method_evidence",
    )


def test_method_evidence_accepts_pr_ref_commit_sha_and_spec_path() -> None:
    for evidence in ("#6640", "3d0c035eedda8c476bb6d9b71dbdd1f5c336377c", "3d0c035", "spec:docs/algo.md"):
        rows = [row("s.rs", "none", 0.0, "sovereign", method="from_spec", method_evidence=evidence)]
        try:
            LIB.validate_rows(rows)
        except LIB.LedgerError as exc:
            _FAILURES.append(f"method_evidence={evidence!r} is a legitimate shape and must be accepted; got {exc}")


def test_method_evidence_rejects_prose() -> None:
    rows = [row("s.rs", "none", 0.0, "sovereign", method="from_spec", method_evidence="trust me")]
    expect_raises(
        LIB.LedgerError,
        lambda: LIB.validate_rows(rows),
        "a prose justification is not an independently-checkable evidence pointer and must be rejected",
    )


def test_dump_ledger_refuses_row_missing_method() -> None:
    rows = [row("a.rs", "up.rs", 40.0, "derived")]
    del rows[0]["method"]
    del rows[0]["method_evidence"]
    meta = {"upstream_repo": "https://example/x", "upstream_ref": "deadbeef"}
    expect_raises(
        LIB.LedgerError,
        lambda: LIB.dump_ledger(meta, rows),
        "dump_ledger must refuse to write a row with no method/method_evidence rather than crash "
        "with a bare KeyError",
    )


def test_transition_set_method_unknown_forbids_evidence() -> None:
    # WHY no ledger fixture needed: every branch below raises via parser.error()
    # (argparse -> SystemExit(2)) before main() ever touches LEDGER_PATH.
    orig_argv = sys.argv
    sys.argv = ["krites-provenance-transition.py", "--set-method", "unknown", "--evidence", "#1", "some.rs"]
    try:
        expect_raises(SystemExit, TRANSITION.main, "--set-method unknown must reject --evidence")
    finally:
        sys.argv = orig_argv


def test_transition_set_method_resolved_requires_evidence() -> None:
    orig_argv = sys.argv
    sys.argv = ["krites-provenance-transition.py", "--set-method", "from_spec", "some.rs"]
    try:
        expect_raises(SystemExit, TRANSITION.main, "--set-method from_spec with no --evidence must be rejected")
    finally:
        sys.argv = orig_argv


def test_transition_to_and_set_method_are_mutually_exclusive() -> None:
    orig_argv = sys.argv
    sys.argv = ["krites-provenance-transition.py", "--to", "sovereign", "--set-method", "unknown", "some.rs"]
    try:
        expect_raises(SystemExit, TRANSITION.main, "--to and --set-method together must be rejected")
    finally:
        sys.argv = orig_argv


def test_transition_neither_to_nor_set_method_is_rejected() -> None:
    orig_argv = sys.argv
    sys.argv = ["krites-provenance-transition.py", "some.rs"]
    try:
        expect_raises(SystemExit, TRANSITION.main, "neither --to nor --set-method must be rejected")
    finally:
        sys.argv = orig_argv


def test_transition_set_method_end_to_end_updates_ledger() -> None:
    # WHY a real tempfile round-trip, not just apply_set_method(): this is the ONLY
    # sanctioned way to clear 'unknown' with evidence (item 4's requirement), so its
    # full path -- CLI parsing, ledger read, mutation, dump_ledger's own re-validation,
    # NOTICE.md re-render -- needs to be proven end to end, not just the pure mutator.
    with tempfile.TemporaryDirectory() as tmp:
        ledger_path = Path(tmp) / "PROVENANCE.toml"
        notice_path = Path(tmp) / "NOTICE.md"
        ledger_path.write_text(
            '[meta]\n'
            'upstream_repo = "https://example/x"\n'
            'upstream_ref = "deadbeef"\n\n'
            '[[file]]\n'
            'path = "s.rs"\n'
            'upstream_path = "none"\n'
            'replaced_upstream_path = "none"\n'
            'verbatim_pct = 0.0\n'
            'status = "sovereign"\n'
            'soak_expires_at_commit_count = 0\n'
            'method = "unknown"\n'
            'method_evidence = "none"\n'
        )
        orig_ledger, orig_notice = TRANSITION.LEDGER_PATH, TRANSITION.NOTICE_PATH
        TRANSITION.LEDGER_PATH, TRANSITION.NOTICE_PATH = ledger_path, notice_path
        orig_argv = sys.argv
        sys.argv = [
            "krites-provenance-transition.py",
            "--set-method",
            "from_behavioral_oracle",
            "--evidence",
            "3d0c035eedda8c476bb6d9b71dbdd1f5c336377c",
            "s.rs",
        ]
        try:
            exit_code = TRANSITION.main()
            expect(exit_code == 0, f"a legitimate --set-method call must exit 0; got {exit_code}")
            _, written_rows = LIB.parse_ledger(ledger_path.read_text())
            written = written_rows[0]
            expect(
                written["method"] == "from_behavioral_oracle"
                and written["method_evidence"] == "3d0c035eedda8c476bb6d9b71dbdd1f5c336377c",
                f"the written ledger must carry the new method/evidence; got {written}",
            )
            expect(notice_path.exists(), "NOTICE.md must be re-rendered")
        finally:
            TRANSITION.LEDGER_PATH, TRANSITION.NOTICE_PATH = orig_ledger, orig_notice
            sys.argv = orig_argv


def test_apply_set_method_writes_both_fields() -> None:
    r = row("s.rs", "none", 0.0, "sovereign")
    TRANSITION.apply_set_method(r, "from_behavioral_oracle", "3d0c035eedda8c476bb6d9b71dbdd1f5c336377c")
    expect(
        r["method"] == "from_behavioral_oracle" and r["method_evidence"] == "3d0c035eedda8c476bb6d9b71dbdd1f5c336377c",
        f"apply_set_method must set both fields; got method={r['method']!r}, method_evidence={r['method_evidence']!r}",
    )


def test_resolve_method_preserves_across_regeneration() -> None:
    preserved = {"s.rs": ("from_spec", "#6656", ["sib.rs"])}
    got = MEASURE.resolve_method("s.rs", "sovereign", preserved)
    expect(
        got == ("from_spec", "#6656", ["sib.rs"]),
        f"a preserved resolved method must survive regeneration unchanged, consulted list with it; got {got}",
    )


def test_resolve_method_defaults_new_sovereign_row_to_unknown() -> None:
    got = MEASURE.resolve_method("new.rs", "sovereign", {})
    expect(got == ("unknown", "none", []), f"a brand-new sovereign row with no prior record must default to unknown/none/[]; got {got}")


def test_resolve_method_defaults_non_sovereign_to_none() -> None:
    got = MEASURE.resolve_method("d.rs", "derived", {})
    expect(got == ("none", "none", []), f"a non-sovereign row must default to method='none'; got {got}")


# --- #6879: the sibling rule — which siblings a clean-room rewrite may read ---


def _sibling_rows(target_method: str, consulted: list[str]) -> list[dict]:
    """A ledger with one sovereign rewrite plus one sibling of each status.

    WHY a shared fixture: every rule below is about the STATUS of a consulted
    path, so each case differs only in which siblings the rewrite names — the
    surrounding ledger must stay identical or the tests stop being comparable.
    """
    return [
        row("rewrite.rs", "none", 0.0, "sovereign", replaced_upstream_path="up.rs",
            method=target_method, method_evidence="#6879", consulted=consulted),
        row("derived_sibling.rs", "up_sibling.rs", 42.1, "derived"),
        row("sovereign_sibling.rs", "none", 0.0, "sovereign", method="attested_original", method_evidence="#6879"),
    ]


def test_consulted_from_spec_rejects_derived_sibling() -> None:
    # WHY this is the whole issue: the first clean-room rewrite under `method` read
    # fts/tokenizer/remove_long.rs — derived, jaccard 0.4215 against upstream, and
    # structurally the same artifact it was writing. Nothing saw it; the rewriter
    # volunteered it. A rewriter who said nothing would carry from_spec today.
    errors = LIB.consulted_errors(_sibling_rows("from_spec", ["derived_sibling.rs"]))
    expect(
        any("rewrite.rs" in e and "derived_sibling.rs" in e and "from_spec_derived_siblings" in e for e in errors),
        f"from_spec consulting a derived sibling must fail and name the offending path; got {errors}",
    )


def test_consulted_from_spec_accepts_sovereign_siblings() -> None:
    errors = LIB.consulted_errors(_sibling_rows("from_spec", ["sovereign_sibling.rs"]))
    expect(errors == [], f"from_spec consulting only sovereign siblings must pass; got {errors}")


def test_consulted_from_spec_accepts_empty_list() -> None:
    errors = LIB.consulted_errors(_sibling_rows("from_spec", []))
    expect(errors == [], f"from_spec that consulted nothing must pass; got {errors}")


def test_consulted_derived_siblings_rejects_all_sovereign_list() -> None:
    # WHY this direction is checked too: silently accepting the weaker value on a row
    # that earned the stronger one makes from_spec_derived_siblings the lazy default,
    # and the pair stops distinguishing anything.
    errors = LIB.consulted_errors(_sibling_rows("from_spec_derived_siblings", ["sovereign_sibling.rs"]))
    expect(
        any("rewrite.rs" in e and "from_spec" in e for e in errors),
        f"from_spec_derived_siblings whose consulted list is entirely sovereign must fail; got {errors}",
    )


def test_consulted_derived_siblings_rejects_empty_list() -> None:
    errors = LIB.consulted_errors(_sibling_rows("from_spec_derived_siblings", []))
    expect(
        any("rewrite.rs" in e and "empty" in e for e in errors),
        f"from_spec_derived_siblings with an empty consulted list must fail; got {errors}",
    )


def test_consulted_derived_siblings_accepts_mixed_list() -> None:
    errors = LIB.consulted_errors(
        _sibling_rows("from_spec_derived_siblings", ["derived_sibling.rs", "sovereign_sibling.rs"])
    )
    expect(errors == [], f"from_spec_derived_siblings naming a real derived sibling must pass; got {errors}")


def test_consulted_rejects_path_not_in_ledger() -> None:
    # WHY: a consulted path is checked by its ledger status, so a typo resolves to no
    # status at all — and an unchecked path must never read as a clean one.
    errors = LIB.consulted_errors(_sibling_rows("from_spec", ["derived_sibbling.rs"]))
    expect(
        any("rewrite.rs" in e and "derived_sibbling.rs" in e and "no PROVENANCE.toml row" in e for e in errors),
        f"a consulted path with no ledger row must fail; got {errors}",
    )


def test_consulted_unconstrained_for_rewritten_with_source_open() -> None:
    errors = LIB.consulted_errors(_sibling_rows("rewritten_with_source_open", ["derived_sibling.rs"]))
    expect(
        errors == [],
        f"rewritten_with_source_open already records reading the replaced file, so its consulted "
        f"list carries no constraint; got {errors}",
    )


def test_consulted_missing_key_passes_validate_rows() -> None:
    # WHY tolerated here: a --base-ref ledger predating this field carries no such key,
    # exactly as with 'method'. Presence is gated by consulted_errors on the CURRENT
    # ledger, and by dump_ledger on every write.
    r = row("old.rs", "up.rs", 40.0, "derived")
    del r["consulted"]
    try:
        LIB.validate_rows([r])
    except LIB.LedgerError as exc:
        _FAILURES.append(f"a row with no 'consulted' key must still pass validate_rows (pre-migration ledger read); got {exc}")


def test_consulted_missing_key_rejected_by_checker() -> None:
    r = row("a.rs", "up.rs", 40.0, "derived")
    del r["consulted"]
    errors = CHECKER.check_consulted_siblings([r])
    expect(
        any("missing 'consulted'" in e and "a.rs" in e for e in errors),
        f"the current ledger must carry 'consulted' on every row; got {errors}",
    )


def test_consulted_forbidden_off_sovereign() -> None:
    rows = [row("d.rs", "up.rs", 40.0, "derived", consulted=["x.rs"])]
    expect_raises(
        LIB.LedgerError,
        lambda: LIB.validate_rows(rows),
        "a derived/dual row makes no authorship claim, so it must never carry a consulted list",
    )


def test_consulted_must_be_a_list_of_paths() -> None:
    for bad in ("none", ["ok.rs", 7], [""]):
        rows = [row("s.rs", "none", 0.0, "sovereign", consulted=bad)]
        expect_raises(
            LIB.LedgerError,
            lambda rows=rows: LIB.validate_rows(rows),
            f"consulted={bad!r} is not a list of ledger paths and must be rejected",
        )


def test_dump_ledger_refuses_row_missing_consulted() -> None:
    r = row("a.rs", "up.rs", 40.0, "derived")
    del r["consulted"]
    meta = {"upstream_repo": "https://example/x", "upstream_ref": "deadbeef"}
    expect_raises(
        LIB.LedgerError,
        lambda: LIB.dump_ledger(meta, [r]),
        "dump_ledger must refuse to write a row with no consulted list",
    )


def test_dump_ledger_refuses_contradicting_consulted() -> None:
    # WHY the write path enforces it too: a row that only fails later in CI is the
    # failure mode this whole scheme keeps repeating — a value written by fiat, caught
    # a wave later, if at all.
    meta = {"upstream_repo": "https://example/x", "upstream_ref": "deadbeef"}
    rows = _sibling_rows("from_spec", ["derived_sibling.rs"])
    expect_raises(
        LIB.LedgerError,
        lambda: LIB.dump_ledger(meta, rows),
        "dump_ledger must refuse to write a from_spec row that consulted a derived sibling",
    )


def test_transition_spec_class_requires_consulted() -> None:
    for method in LIB.SPEC_CLASS_METHODS:
        orig_argv = sys.argv
        sys.argv = ["krites-provenance-transition.py", "--set-method", method, "--evidence", "#6879", "some.rs"]
        try:
            expect_raises(SystemExit, TRANSITION.main, f"--set-method {method} with no --consulted must be rejected")
        finally:
            sys.argv = orig_argv


def test_transition_set_method_unknown_forbids_consulted() -> None:
    orig_argv = sys.argv
    sys.argv = ["krites-provenance-transition.py", "--set-method", "unknown", "--consulted", "x.rs", "some.rs"]
    try:
        expect_raises(SystemExit, TRANSITION.main, "--set-method unknown must reject --consulted")
    finally:
        sys.argv = orig_argv


def test_transition_consulted_rejected_with_status_transition() -> None:
    orig_argv = sys.argv
    sys.argv = ["krites-provenance-transition.py", "--to", "sovereign", "--consulted", "x.rs", "some.rs"]
    try:
        expect_raises(SystemExit, TRANSITION.main, "--consulted with --to must be rejected")
    finally:
        sys.argv = orig_argv


def test_transition_set_method_consulted_end_to_end() -> None:
    # WHY a real round-trip: --consulted is the only sanctioned way to record a reading
    # list, so its full path — CLI parsing, ledger read, mutation, dump_ledger's own
    # re-validation of the sibling rule, NOTICE.md re-render — needs proving end to end.
    with tempfile.TemporaryDirectory() as tmp:
        ledger_path = Path(tmp) / "PROVENANCE.toml"
        notice_path = Path(tmp) / "NOTICE.md"
        ledger_path.write_text(
            '[meta]\n'
            'upstream_repo = "https://example/x"\n'
            'upstream_ref = "deadbeef"\n\n'
            '[[file]]\n'
            'path = "rewrite.rs"\n'
            'upstream_path = "none"\n'
            'replaced_upstream_path = "none"\n'
            'verbatim_pct = 0.0\n'
            'status = "sovereign"\n'
            'soak_expires_at_commit_count = 0\n'
            'method = "unknown"\n'
            'method_evidence = "none"\n'
            'consulted = []\n\n'
            '[[file]]\n'
            'path = "derived_sibling.rs"\n'
            'upstream_path = "up_sibling.rs"\n'
            'replaced_upstream_path = "none"\n'
            'verbatim_pct = 42.1\n'
            'status = "derived"\n'
            'soak_expires_at_commit_count = 0\n'
            'method = "none"\n'
            'method_evidence = "none"\n'
            'consulted = []\n'
        )
        orig_ledger, orig_notice = TRANSITION.LEDGER_PATH, TRANSITION.NOTICE_PATH
        orig_reasons = LIB.NO_PREDECESSOR_REASONS
        TRANSITION.LEDGER_PATH, TRANSITION.NOTICE_PATH = ledger_path, notice_path
        orig_argv = sys.argv
        sys.argv = [
            "krites-provenance-transition.py",
            "--set-method",
            "from_spec_derived_siblings",
            "--evidence",
            "#6879",
            "--consulted",
            "derived_sibling.rs",
            "rewrite.rs",
        ]
        try:
            exit_code = TRANSITION.main()
            expect(exit_code == 0, f"a legitimate --set-method + --consulted call must exit 0; got {exit_code}")
            _, written_rows = LIB.parse_ledger(ledger_path.read_text())
            written = next(r for r in written_rows if r["path"] == "rewrite.rs")
            expect(
                written["method"] == "from_spec_derived_siblings" and written["consulted"] == ["derived_sibling.rs"],
                f"the written ledger must carry the new method and its consulted list; got {written}",
            )
            expect(
                "derived_sibling.rs" in notice_path.read_text(),
                "NOTICE.md must surface what a rewrite consulted, not only its method",
            )
        finally:
            TRANSITION.LEDGER_PATH, TRANSITION.NOTICE_PATH = orig_ledger, orig_notice
            LIB.NO_PREDECESSOR_REASONS = orig_reasons
            sys.argv = orig_argv


def test_transition_refuses_to_write_a_contradicting_consulted_list() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        ledger_path = Path(tmp) / "PROVENANCE.toml"
        notice_path = Path(tmp) / "NOTICE.md"
        ledger_path.write_text(
            '[meta]\n'
            'upstream_repo = "https://example/x"\n'
            'upstream_ref = "deadbeef"\n\n'
            '[[file]]\n'
            'path = "rewrite.rs"\n'
            'upstream_path = "none"\n'
            'replaced_upstream_path = "none"\n'
            'verbatim_pct = 0.0\n'
            'status = "sovereign"\n'
            'soak_expires_at_commit_count = 0\n'
            'method = "unknown"\n'
            'method_evidence = "none"\n'
            'consulted = []\n\n'
            '[[file]]\n'
            'path = "derived_sibling.rs"\n'
            'upstream_path = "up_sibling.rs"\n'
            'replaced_upstream_path = "none"\n'
            'verbatim_pct = 42.1\n'
            'status = "derived"\n'
            'soak_expires_at_commit_count = 0\n'
            'method = "none"\n'
            'method_evidence = "none"\n'
            'consulted = []\n'
        )
        before = ledger_path.read_text()
        orig_ledger, orig_notice = TRANSITION.LEDGER_PATH, TRANSITION.NOTICE_PATH
        TRANSITION.LEDGER_PATH, TRANSITION.NOTICE_PATH = ledger_path, notice_path
        orig_argv = sys.argv
        sys.argv = [
            "krites-provenance-transition.py",
            "--set-method",
            "from_spec",
            "--evidence",
            "#6879",
            "--consulted",
            "derived_sibling.rs",
            "rewrite.rs",
        ]
        try:
            exit_code = TRANSITION.main()
            expect(exit_code == 1, f"a from_spec row consulting a derived sibling must not be written; got exit {exit_code}")
            expect(ledger_path.read_text() == before, "a refused --set-method must leave the ledger untouched")
        finally:
            TRANSITION.LEDGER_PATH, TRANSITION.NOTICE_PATH = orig_ledger, orig_notice
            sys.argv = orig_argv


def test_apply_set_method_preserves_consulted_when_omitted() -> None:
    r = row("s.rs", "none", 0.0, "sovereign", consulted=["sib.rs"])
    TRANSITION.apply_set_method(r, "attested_original", "#6879")
    expect(
        r["consulted"] == ["sib.rs"],
        f"re-recording a method must not silently clear what its author read; got {r['consulted']}",
    )


def test_apply_to_sovereign_enters_with_no_consulted_record() -> None:
    r = row("d.rs", "up_missing.rs", 40.0, "dual", soak=99)
    TRANSITION.apply_to_sovereign(r)
    expect(
        r["consulted"] == [] and r["method"] == "unknown",
        f"a row entering sovereign has no recorded reading list yet; got method={r['method']!r}, "
        f"consulted={r['consulted']!r}",
    )


# --- a moved `dual` file must not lose its soak fuse ---


def test_dual_move_blocked() -> None:
    # WHY: status preservation keys on the ledger's recorded path and looks it
    # up by the file's CURRENT path, so a moved dual file matches nothing and
    # is rewritten as `derived` with soak 0 — the fuse erased, every check
    # green. Measured on the real tree: a row at soak 3108 came back
    # `derived`/0 after a `git mv` plus the UPSTREAM_MAP rekey a move requires.
    graduated = {"data/aggr/boolean.rs": ("dual", 3108)}
    prior = {"data/aggr/boolean.rs", "data/value.rs"}
    rows = [row("data/aggr/moved.rs", "data/aggr.rs", 71.7, "derived"), row("data/value.rs", "data/value.rs", 40.0, "derived")]
    expect_raises(
        SystemExit,
        lambda: MEASURE.check_dual_survives_move(graduated, prior, rows),
        "a dual row vanishing while a new row appears must be refused (moved file loses its soak fuse)",
    )


def test_dual_retirement_allowed() -> None:
    # WHY: retirement legitimately deletes a dual file — that IS
    # land-dark -> soak -> delete completing, and it must stay possible. A
    # deletion removes a row and adds none.
    graduated = {"data/aggr/boolean.rs": ("dual", 3108)}
    prior = {"data/aggr/boolean.rs", "data/value.rs"}
    rows = [row("data/value.rs", "data/value.rs", 40.0, "derived")]
    try:
        MEASURE.check_dual_survives_move(graduated, prior, rows)
    except SystemExit as exc:
        expect(False, f"retiring a dual row must stay possible, got: {exc}")


def test_dual_move_guard_ignores_sovereign() -> None:
    # WHY: sovereign status is driven by SOVEREIGN_VERIFY_MAP, which a move
    # rekeys too, so it survives a rename on its own. Only `dual` carries a
    # fuse, so only `dual` is guarded — verified against the real tree.
    graduated = {"data/error.rs": ("sovereign", 0)}
    prior = {"data/error.rs", "data/value.rs"}
    rows = [row("data/renamed.rs", "none", 0.0, "sovereign"), row("data/value.rs", "data/value.rs", 40.0, "derived")]
    try:
        MEASURE.check_dual_survives_move(graduated, prior, rows)
    except SystemExit as exc:
        expect(False, f"a moved sovereign row must not trip the dual-fuse guard, got: {exc}")


def test_dual_move_guard_quiet_when_nothing_moved() -> None:
    graduated = {"data/aggr/boolean.rs": ("dual", 3108)}
    prior = {"data/aggr/boolean.rs", "data/value.rs"}
    rows = [row("data/aggr/boolean.rs", "data/aggr.rs", 71.7, "dual", soak=3108), row("data/value.rs", "data/value.rs", 40.0, "derived")]
    try:
        MEASURE.check_dual_survives_move(graduated, prior, rows)
    except SystemExit as exc:
        expect(False, f"an ordinary regeneration must not trip the guard, got: {exc}")


# --- an unparsable prior ledger must not silently un-graduate every row ---


def test_unparsable_ledger_fails_closed() -> None:
    # WHY: a merge conflict in PROVENANCE.toml leaves markers in the file. Both
    # readers used to treat a parse error as "nothing to preserve" -- the shape
    # that is correct for a MISSING file -- and regenerating in that state
    # demoted 5 sovereign rows and 1 dual row in one measured run, reporting a
    # normal write. Only check_status_sequence caught it, afterwards.
    with tempfile.TemporaryDirectory() as tmp:
        bad = Path(tmp) / "PROVENANCE.toml"
        bad.write_text("<<<<<<< HEAD\n[[file]]\npath = \"a.rs\"\n=======\n>>>>>>> origin/main\n")
        expect_raises(
            SystemExit,
            lambda: MEASURE.load_graduated_status(bad),
            "an unparsable prior ledger must fail, not silently preserve nothing",
        )
        expect_raises(
            SystemExit,
            lambda: MEASURE.load_prior_paths(bad),
            "load_prior_paths must fail on an unparsable ledger too",
        )


def test_missing_ledger_still_bootstraps() -> None:
    # WHY kept distinct: absence is the ledger's first-ever run and must stay
    # non-fatal. Only unreadable-but-present is the fault.
    with tempfile.TemporaryDirectory() as tmp:
        absent = Path(tmp) / "nope.toml"
        try:
            expect(MEASURE.load_graduated_status(absent) == {}, "missing ledger must yield {}")
            expect(MEASURE.load_prior_paths(absent) == set(), "missing ledger must yield an empty path set")
        except SystemExit as exc:
            expect(False, f"a MISSING ledger must remain the bootstrap case, not a failure: {exc}")


# --- #5956: per-file MPL Exhibit A notices, and the measurement they must not move ---


_UPSTREAM_STYLE_HEADER = (
    "/*\n"
    " * Copyright 2022, The Cozo Project Authors.\n"
    " *\n"
    " * This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.\n"
    " * If a copy of the MPL was not distributed with this file,\n"
    " * You can obtain one at https://mozilla.org/MPL/2.0/.\n"
    " */\n"
)


def test_verbatim_pct_is_unmoved_by_the_generated_notice() -> None:
    # WHY this is the load-bearing test in this section: verbatim_pct is
    # matched-lines / local-non-blank-lines, so a 5-line header added to 142 derived
    # files moves the de-derivation program's central metric on every one of them
    # while nothing about any file's derivation changed. A number that moves without
    # the underlying work is the exact failure the ledger exists to end, so the notice
    # must be outside what is measured -- proven here, not assumed.
    upstream = "".join(f"fn f{i}() {{}}\n" for i in range(20))
    plain = "".join(f"fn f{i}() {{}}\n" for i in range(10)) + "fn zzz() {}\n"
    stamped = LIB.add_generated_notice(plain, LIB.render_exhibit_a(".rs"))

    expect(stamped != plain, "fixture bug: the stamped text must actually carry the notice")
    expect(
        LIB.nonblank_lines(stamped) == LIB.nonblank_lines(plain),
        "the notice must not survive line extraction",
    )
    expect(
        LIB.verbatim_pct(plain, upstream) == LIB.verbatim_pct(stamped, upstream),
        f"verbatim_pct moved when the notice was added: {LIB.verbatim_pct(plain, upstream)} "
        f"-> {LIB.verbatim_pct(stamped, upstream)}",
    )

    # The negative control: without the exclusion the figure DOES move, so the
    # assertion above is measuring the exclusion rather than passing vacuously.
    naive = len([line for line in stamped.splitlines() if line.strip()])
    expect(
        naive > len(LIB.nonblank_lines(stamped)),
        "fixture bug: a naive line count must see the notice, or this test proves nothing",
    )


def test_generated_notice_roundtrip_is_exact() -> None:
    for suffix in LIB.COMMENT_SYNTAX:
        block = LIB.render_exhibit_a(suffix)
        expect(LIB.has_exhibit_a(block), f"{suffix}: the rendered block must satisfy has_exhibit_a")
        for base in ("body line one\nbody line two\n", "#!/usr/bin/env python3\nbody\n"):
            stamped = LIB.add_generated_notice(base, block)
            expect(
                LIB.remove_generated_notice(stamped, block) == base,
                f"{suffix}: add then remove must return the original bytes",
            )
            expect(
                LIB.strip_generated_notice(stamped) == base,
                f"{suffix}: strip_generated_notice must remove exactly the block",
            )
        shebang_stamped = LIB.add_generated_notice("#!/usr/bin/env python3\nbody\n", block)
        expect(
            shebang_stamped.startswith("#!/usr/bin/env python3\n"),
            f"{suffix}: the notice must go BELOW a shebang, not displace it",
        )


def test_render_exhibit_a_refuses_an_unregistered_suffix() -> None:
    expect_raises(
        LIB.LedgerError,
        lambda: LIB.render_exhibit_a(".toml"),
        "a suffix with no registered comment syntax must raise rather than guess one",
    )


def test_has_exhibit_a_accepts_a_retained_upstream_header() -> None:
    # WHY: 122 of the upstream files carry the notice in their own `/* * */` header, and
    # MPL 3.1 forbids removing it. The gate therefore has to recognise the notice in the
    # wrapping it arrives in, or it would demand a second copy of the same sentence.
    expect(LIB.has_exhibit_a(_UPSTREAM_STYLE_HEADER), "upstream's own C-style header must count")
    expect(not LIB.has_exhibit_a("fn a() {}\n"), "a file with no notice must not count")
    expect(
        not LIB.has_generated_notice_marker(_UPSTREAM_STYLE_HEADER),
        "upstream's header is not this tooling's generated block",
    )


def _exhibit_a_errors(files: dict[str, str], rows: list[dict]) -> list[str]:
    with tempfile.TemporaryDirectory() as tmp:
        src_dir = Path(tmp) / "src"
        src_dir.mkdir()
        for name, text in files.items():
            (src_dir / name).write_text(text)
        orig = CHECKER.KRITES_SRC
        CHECKER.KRITES_SRC = src_dir
        try:
            return CHECKER.check_exhibit_a_notices(rows)
        finally:
            CHECKER.KRITES_SRC = orig


def test_exhibit_a_gate_rejects_a_derived_file_with_no_notice() -> None:
    plain = "fn a() {}\n"
    errors = _exhibit_a_errors({"local.rs": plain}, [row("local.rs", "up.rs", 50.0, "derived")])
    expect(
        any("carries no MPL Exhibit A notice" in e for e in errors),
        f"a derived file with no notice must fail; got {errors}",
    )
    stamped = LIB.add_generated_notice(plain, LIB.render_exhibit_a(".rs"))
    errors2 = _exhibit_a_errors({"local.rs": stamped}, [row("local.rs", "up.rs", 50.0, "derived")])
    expect(errors2 == [], f"the same file must pass once the notice is rendered; got {errors2}")


def test_exhibit_a_gate_requires_the_notice_on_dual_too() -> None:
    # A dual row is the retiring CozoDB-lineage copy soaking before deletion, not a
    # rewrite -- it carries upstream expression for the whole soak window.
    errors = _exhibit_a_errors(
        {"local.rs": "fn a() {}\n"}, [row("local.rs", "up.rs", 50.0, "dual", soak=99)]
    )
    expect(
        any("carries no MPL Exhibit A notice" in e for e in errors),
        f"a dual file with no notice must fail; got {errors}",
    )


def test_exhibit_a_gate_accepts_a_retained_upstream_notice() -> None:
    errors = _exhibit_a_errors(
        {"local.rs": _UPSTREAM_STYLE_HEADER + "fn a() {}\n"},
        [row("local.rs", "up.rs", 50.0, "derived")],
    )
    expect(errors == [], f"a file retaining upstream's own MPL header must pass; got {errors}")


def test_exhibit_a_gate_rejects_a_notice_on_a_sovereign_row() -> None:
    # The opposite error, and the worse one: a sovereign row claims no CozoDB lineage,
    # so a notice there asserts an MPL obligation over aletheia's own work.
    stamped = LIB.add_generated_notice("fn a() {}\n", LIB.render_exhibit_a(".rs"))
    errors = _exhibit_a_errors(
        {"local.rs": stamped},
        [row("local.rs", "none", 0.0, "sovereign")],
    )
    expect(
        any("status=sovereign but the file carries an MPL notice" in e for e in errors),
        f"a sovereign file carrying the notice must fail; got {errors}",
    )
    errors2 = _exhibit_a_errors(
        {"local.rs": "fn a() {}\n"}, [row("local.rs", "none", 0.0, "sovereign")]
    )
    expect(errors2 == [], f"a sovereign file with no notice must pass; got {errors2}")


def test_exhibit_a_gate_rejects_a_hand_edited_block() -> None:
    # A block that no longer matches what render_exhibit_a emits stops being excluded
    # from the measurement, so it starts counting licence boilerplate as the file's own
    # expression. The notice sentence is still there, so only the marker reveals it.
    mangled = LIB.render_exhibit_a(".rs").replace("// v. 2.0.", "//    v. 2.0.")
    errors = _exhibit_a_errors(
        {"local.rs": mangled + "\nfn a() {}\n"}, [row("local.rs", "up.rs", 50.0, "derived")]
    )
    expect(
        any("hand-edited" in e for e in errors),
        f"a drifted generated block must fail even though the sentence survives; got {errors}",
    )


def test_sync_exhibit_a_is_status_directed_and_idempotent() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "local.rs"
        path.write_text("fn a() {}\n")

        expect(LIB.sync_exhibit_a(path, "derived") == "added", "a derived file must gain the notice")
        stamped = path.read_text()
        expect(LIB.has_exhibit_a(stamped), "the notice must actually be written")
        expect(
            LIB.sync_exhibit_a(path, "derived") is None and path.read_text() == stamped,
            "a second run must be a no-op, never a second copy of the notice",
        )
        expect(LIB.sync_exhibit_a(path, "dual") is None, "a dual row keeps the notice it had")
        expect(LIB.sync_exhibit_a(path, "sovereign") == "removed", "sovereign must lose the notice")
        expect(path.read_text() == "fn a() {}\n", "removal must restore the original bytes exactly")
        expect(LIB.sync_exhibit_a(path, "sovereign") is None, "removal must be idempotent too")

        # A notice this tooling did not write is not this tooling's to delete: upstream's
        # own copyright header goes with it, and deleting that silently is the one
        # direction that must never be automatic. The gate reports it instead.
        path.write_text(_UPSTREAM_STYLE_HEADER + "fn a() {}\n")
        expect(
            LIB.sync_exhibit_a(path, "sovereign") is None,
            "an inherited upstream header must not be auto-deleted",
        )
        expect(LIB.has_exhibit_a(path.read_text()), "the inherited header must still be there")


def test_sync_exhibit_a_skips_a_row_whose_file_is_gone() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        expect(
            LIB.sync_exhibit_a(Path(tmp) / "no-such-file.rs", "derived") is None,
            "a ledger row naming a missing file is check_completeness's finding, not a crash",
        )


def test_transition_to_sovereign_removes_the_notice() -> None:
    # #5956: a rewritten file that keeps the notice keeps asserting an MPL obligation it
    # no longer carries. The verbatim_pct assertion is the other half: the figure must be
    # the same before and after the removal, or the transition itself would move the metric.
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        snapshot_dir = root / "snapshot"
        snapshot_dir.mkdir()
        (snapshot_dir / "up.rs").write_text(
            "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\n"
            "fn e() {}\nfn f() {}\nfn g() {}\nfn h() {}\n"
        )
        src_dir = root / "src"
        src_dir.mkdir()
        body = (
            "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\n"
            "fn zzz() {}\nfn www() {}\nfn yyy() {}\nfn xxx() {}\n"
        )
        local = src_dir / "local.rs"
        local.write_text(LIB.add_generated_notice(body, LIB.render_exhibit_a(".rs")))

        orig_snapshot = TRANSITION.UPSTREAM_SNAPSHOT_DIR
        orig_src = TRANSITION.KRITES_SRC
        TRANSITION.UPSTREAM_SNAPSHOT_DIR = snapshot_dir
        TRANSITION.KRITES_SRC = src_dir
        try:
            r = row("local.rs", "up.rs", 50.0, "dual", soak=100)
            TRANSITION.apply_to_sovereign(r)
            expect(
                not LIB.has_exhibit_a(local.read_text()),
                "a dual -> sovereign transition must take the MPL notice back out",
            )
            expect(
                local.read_text() == body,
                "removal must restore the file's own bytes, nothing else",
            )
            expect(
                # WHY a tolerance on a value the generator rounds to one decimal: exact
                # float equality holds only while that rounding does, and a later change
                # to the stored precision would turn this into a flake rather than a
                # finding.
                abs(r["verbatim_pct"] - 50.0) < 1e-9,
                f"the transition must not move verbatim_pct by removing the notice; got "
                f"{r['verbatim_pct']}",
            )
        finally:
            TRANSITION.UPSTREAM_SNAPSHOT_DIR = orig_snapshot
            TRANSITION.KRITES_SRC = orig_src


def main() -> int:
    for test_fn in (
        test_sovereign_high_verbatim_rejected,
        test_sovereign_zero_verbatim_accepted,
        test_sovereign_with_replaced_path_and_nonzero_verbatim_accepted,
        test_sovereign_no_replaced_path_nonzero_verbatim_still_rejected,
        test_replaced_path_rejected_on_non_sovereign_row,
        test_missing_replaced_path_key_defaults_to_none,
        test_verbatim_recompute_catches_unmeasured_sovereign_claim,
        test_verbatim_recompute_skips_sovereign_with_no_replaced_path,
        test_status_sequence_rejects_dual_to_sovereign_with_dropped_replaced_path,
        test_status_sequence_rejects_dual_to_sovereign_with_wrong_replaced_path,
        test_status_sequence_accepts_dual_to_sovereign_with_correct_replaced_path,
        test_apply_to_sovereign_retains_and_recomputes,
        test_apply_to_sovereign_falls_back_when_snapshot_missing,
        test_verbatim_pct_ignores_reindentation,
        test_verbatim_pct_floors_out_scattered_punctuation_matches,
        test_verbatim_pct_full_match_on_file_shorter_than_floor,
        test_status_sequence_rejects_direct_derived_to_sovereign,
        test_status_sequence_accepts_derived_to_dual_and_dual_to_sovereign,
        test_status_sequence_ignores_path_absent_from_base,
        test_no_derived_growth_rejects_true_regression,
        test_no_derived_growth_ignores_path_absent_from_base,
        test_no_derived_growth_skips_on_bootstrap,
        test_ref_exists_true_for_head,
        test_ref_exists_false_for_bogus_ref,
        test_load_base_rows_fails_closed_on_unresolvable_ref,
        test_git_commit_count_returns_positive_int_for_head,
        test_git_commit_count_none_for_bogus_ref,
        test_soak_expiry_flags_expired_dual_row,
        test_soak_expiry_accepts_not_yet_expired,
        test_soak_expiry_rejects_nonpositive_expiry_on_dual_row,
        test_soak_expiry_skips_when_no_dual_rows,
        test_soak_expiry_fails_closed_when_commit_count_unavailable,
        test_land_dark_unfused_flags_shadowed_derived_rows,
        test_land_dark_unfused_accepts_dual_rows,
        test_land_dark_unfused_ignores_partial_retirement,
        test_land_dark_unfused_flags_paired_derived_dir_layout,
        test_land_dark_unfused_quiet_without_any_shadow,
        test_verbatim_recompute_fails_closed_without_snapshot,
        test_verbatim_recompute_detects_drift,
        test_no_unjustified_exemption_rejects_bare_none,
        test_no_unjustified_exemption_accepts_justified_none,
        test_no_unjustified_exemption_ignores_rows_with_a_real_predecessor,
        test_no_unjustified_exemption_flags_stale_reason,
        test_dual_move_blocked,
        test_dual_retirement_allowed,
        test_dual_move_guard_ignores_sovereign,
        test_dual_move_guard_quiet_when_nothing_moved,
        test_unparsable_ledger_fails_closed,
        test_missing_ledger_still_bootstraps,
        test_method_missing_key_passes_validate_rows,
        test_method_recorded_rejects_missing_key,
        test_method_recorded_rejects_sovereign_transliterated,
        test_method_recorded_accepts_legitimate_value,
        test_method_only_meaningful_on_sovereign,
        test_sovereign_method_must_be_a_known_value,
        test_sovereign_method_rejects_none,
        test_method_evidence_required_when_resolved,
        test_method_evidence_forbidden_when_unknown,
        test_method_evidence_forbidden_off_sovereign,
        test_method_evidence_accepts_pr_ref_commit_sha_and_spec_path,
        test_method_evidence_rejects_prose,
        test_dump_ledger_refuses_row_missing_method,
        test_transition_set_method_unknown_forbids_evidence,
        test_transition_set_method_resolved_requires_evidence,
        test_transition_to_and_set_method_are_mutually_exclusive,
        test_transition_neither_to_nor_set_method_is_rejected,
        test_transition_set_method_end_to_end_updates_ledger,
        test_apply_set_method_writes_both_fields,
        test_resolve_method_preserves_across_regeneration,
        test_resolve_method_defaults_new_sovereign_row_to_unknown,
        test_resolve_method_defaults_non_sovereign_to_none,
        test_consulted_from_spec_rejects_derived_sibling,
        test_consulted_from_spec_accepts_sovereign_siblings,
        test_consulted_from_spec_accepts_empty_list,
        test_consulted_derived_siblings_rejects_all_sovereign_list,
        test_consulted_derived_siblings_rejects_empty_list,
        test_consulted_derived_siblings_accepts_mixed_list,
        test_consulted_rejects_path_not_in_ledger,
        test_consulted_unconstrained_for_rewritten_with_source_open,
        test_consulted_missing_key_passes_validate_rows,
        test_consulted_missing_key_rejected_by_checker,
        test_consulted_forbidden_off_sovereign,
        test_consulted_must_be_a_list_of_paths,
        test_dump_ledger_refuses_row_missing_consulted,
        test_dump_ledger_refuses_contradicting_consulted,
        test_transition_spec_class_requires_consulted,
        test_transition_set_method_unknown_forbids_consulted,
        test_transition_consulted_rejected_with_status_transition,
        test_transition_set_method_consulted_end_to_end,
        test_transition_refuses_to_write_a_contradicting_consulted_list,
        test_apply_set_method_preserves_consulted_when_omitted,
        test_apply_to_sovereign_enters_with_no_consulted_record,
        test_verbatim_pct_is_unmoved_by_the_generated_notice,
        test_generated_notice_roundtrip_is_exact,
        test_render_exhibit_a_refuses_an_unregistered_suffix,
        test_has_exhibit_a_accepts_a_retained_upstream_header,
        test_exhibit_a_gate_rejects_a_derived_file_with_no_notice,
        test_exhibit_a_gate_requires_the_notice_on_dual_too,
        test_exhibit_a_gate_accepts_a_retained_upstream_notice,
        test_exhibit_a_gate_rejects_a_notice_on_a_sovereign_row,
        test_exhibit_a_gate_rejects_a_hand_edited_block,
        test_sync_exhibit_a_is_status_directed_and_idempotent,
        test_sync_exhibit_a_skips_a_row_whose_file_is_gone,
        test_transition_to_sovereign_removes_the_notice,
    ):
        test_fn()

    if _FAILURES:
        print(f"FAIL: {len(_FAILURES)} assertion(s) failed", file=sys.stderr)
        for failure in _FAILURES:
            print(f"  - {failure}", file=sys.stderr)
        return 1

    print("OK: all krites provenance tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
