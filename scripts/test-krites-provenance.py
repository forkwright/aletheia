#!/usr/bin/env python3
"""Behavioral tests for scripts/check-krites-provenance.py + krites_provenance_lib.py.

Covers the wave-0 review's anti-backslide findings (P1, P2, P4, P6): the
exact reviewer bypass (flip a high-verbatim_pct row to sovereign) must now
be rejected, both directly (verbatim_pct left as evidence) and the sneakier
variant (verbatim_pct zeroed too, only the status-sequence check catches
it); an unresolvable --base-ref must fail closed, not silently pass as a
bootstrap commit; soak expiry must fire; offline recompute must fire when a
snapshot is present and skip cleanly when it is not.

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

import krites_provenance_lib as LIB  # noqa: E402

_CHECK_SCRIPT_PATH = Path(__file__).parent / "check-krites-provenance.py"
_TRANSITION_SCRIPT_PATH = Path(__file__).parent / "krites-provenance-transition.py"


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


CHECKER = _load_checker()
TRANSITION = _load_transition()
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
) -> dict:
    return {
        "path": path,
        "upstream_path": upstream_path,
        "replaced_upstream_path": replaced_upstream_path,
        "verbatim_pct": verbatim_pct,
        "status": status,
        "soak_expires_at_commit_count": soak,
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
        (snapshot_dir / "up.rs").write_text("fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\n")
        src_dir = root / "src"
        src_dir.mkdir()
        # shares 2 of 4 lines with up.rs -- a real, nonzero, measurable similarity.
        (src_dir / "local.rs").write_text("fn a() {}\nfn b() {}\nfn zzz() {}\nfn www() {}\n")

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
        (snapshot_dir / "up.rs").write_text("fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\n")
        src_dir = root / "src"
        src_dir.mkdir()
        (src_dir / "local.rs").write_text("fn a() {}\nfn b() {}\nfn zzz() {}\nfn www() {}\n")

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


# --- P6: offline verbatim recompute ---


def test_verbatim_recompute_skips_without_snapshot() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        fake_snapshot = Path(tmp) / "no-such-snapshot"
        orig = CHECKER.UPSTREAM_SNAPSHOT_DIR
        CHECKER.UPSTREAM_SNAPSHOT_DIR = fake_snapshot
        try:
            errors = CHECKER.check_verbatim_recompute([row("q.rs", "q.rs", 50.0, "derived")])
            expect(errors == [], f"absent snapshot dir must skip, not fail; got {errors}")
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
        test_verbatim_recompute_skips_without_snapshot,
        test_verbatim_recompute_detects_drift,
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
