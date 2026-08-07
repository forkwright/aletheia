#!/usr/bin/env python3
"""Behavioral tests for scripts/check-krites-provenance.py + krites_provenance_lib.py.

Covers the wave-0 review's anti-backslide findings (P1, P2, P4, P6): the
exact reviewer bypass (flip a high-verbatim_pct row to sovereign) must now
be rejected, both directly (verbatim_pct left as evidence) and the sneakier
variant (verbatim_pct zeroed too, only the status-sequence check catches
it); an unresolvable --base-ref must fail closed, not silently pass as a
bootstrap commit; soak expiry must fire; offline recompute must fire when a
snapshot is present and skip cleanly when it is not.
"""

from __future__ import annotations

import importlib.util
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import krites_provenance_lib as LIB  # noqa: E402

_CHECK_SCRIPT_PATH = Path(__file__).parent / "check-krites-provenance.py"


def _load_checker() -> object:
    spec = importlib.util.spec_from_file_location("check_krites_provenance", _CHECK_SCRIPT_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"failed to load {_CHECK_SCRIPT_PATH}")
    module = importlib.util.module_from_spec(spec)
    sys.modules["check_krites_provenance"] = module
    spec.loader.exec_module(module)
    return module


CHECKER = _load_checker()
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


def row(path: str, upstream_path: str, verbatim_pct: float, status: str, soak: int = 0) -> dict:
    return {
        "path": path,
        "upstream_path": upstream_path,
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
    current_rows2 = [row("x.rs", "none", 0.0, "sovereign")]
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
