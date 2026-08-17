#!/usr/bin/env python3
"""CI gate: PROVENANCE.toml completeness, NOTICE.md sync, no derived-row growth,
status-sequence, soak expiry, offline verbatim recompute."""

from __future__ import annotations

import argparse
import pathlib
import subprocess
import sys
import tomllib

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from krites_provenance_lib import (  # noqa: E402
    ALLOWED_TRANSITIONS,
    KRITES_SRC,
    LEDGER_PATH,
    NO_PREDECESSOR_REASONS,
    NOTICE_PATH,
    REPO_ROOT,
    UPSTREAM_SNAPSHOT_DIR,
    LedgerError,
    iter_src_files,
    parse_ledger,
    render_notice,
    verbatim_pct,
)


def fail(message: str) -> None:
    print(f"::error::krites-provenance: {message}", file=sys.stderr)


class BaseRefError(RuntimeError):
    """The requested base ref cannot be resolved at all — distinct from a
    resolved ref that simply predates the ledger (a genuine bootstrap)."""


def ref_exists(ref: str) -> bool:
    result = subprocess.run(
        ["git", "-C", str(REPO_ROOT), "rev-parse", "--verify", "--quiet", f"{ref}^{{commit}}"],
        capture_output=True,
        text=True,
    )
    return result.returncode == 0


def git_show(ref: str, path: str) -> str | None:
    result = subprocess.run(
        ["git", "-C", str(REPO_ROOT), "show", f"{ref}:{path}"],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        return None
    return result.stdout


def git_commit_count(ref: str) -> int | None:
    result = subprocess.run(
        ["git", "-C", str(REPO_ROOT), "rev-list", "--count", ref],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        return None
    stdout = result.stdout.strip()
    return int(stdout) if stdout.isdigit() else None


def load_base_rows(base_ref: str) -> list[dict] | None:
    """Returns the base ref's ledger rows, or None only when base_ref
    resolves but genuinely has no PROVENANCE.toml yet (the ledger's actual
    first landing).

    SAFETY(P4): fails closed. The prior version treated ANY nonzero `git
    show` exit — including an unresolvable ref, e.g. `--base-ref
    origin/does-not-exist` — as a bootstrap commit and returned [], silently
    passing the growth check. ref_exists() is checked FIRST and separately,
    so an unresolvable ref now raises instead of masquerading as bootstrap.
    """
    if not ref_exists(base_ref):
        raise BaseRefError(
            f"base ref {base_ref!r} does not resolve to a commit — cannot verify the "
            "no-derived-growth or status-sequence invariants against it; refusing to treat an "
            "unresolvable ref as a bootstrap commit (fail closed, not fail open)"
        )
    base_text = git_show(base_ref, "crates/krites/PROVENANCE.toml")
    if base_text is None:
        print(
            f"krites-provenance: {base_ref} resolves but has no PROVENANCE.toml — "
            "skipping growth/sequence checks (bootstrap commit)"
        )
        return None
    _, base_rows = parse_ledger(base_text)
    return base_rows


def check_completeness(rows: list[dict]) -> list[str]:
    ledger_paths = {row["path"] for row in rows}
    src_paths = set(iter_src_files())
    missing = sorted(src_paths - ledger_paths)
    stale = sorted(ledger_paths - src_paths)
    errors = []
    if missing:
        errors.append(
            "files under crates/krites/src/ with no PROVENANCE.toml row: " + ", ".join(missing)
        )
    if stale:
        errors.append(
            "PROVENANCE.toml rows for files that no longer exist: " + ", ".join(stale)
        )
    return errors


def check_notice_sync(meta: dict, rows: list[dict]) -> list[str]:
    expected = render_notice(meta, rows)
    # WHY an explicit existence check: read_text() on a missing NOTICE.md raised
    # FileNotFoundError out of main(), so deleting the file produced a traceback
    # rather than a finding. A checker that dies untidily reads as broken tooling
    # and gets re-run or ignored, while deleting the artifact recording which
    # files carry CozoDB lineage is exactly the act this check exists to catch.
    if not NOTICE_PATH.is_file():
        return [
            f"{NOTICE_PATH} does not exist. It is generated from PROVENANCE.toml and records "
            "which files carry CozoDB lineage; its absence is not evidence the attribution "
            "obligation ended — run scripts/measure-krites-provenance.py to regenerate it"
        ]
    actual = NOTICE_PATH.read_text()
    if expected != actual:
        return ["NOTICE.md is out of sync with PROVENANCE.toml — run scripts/measure-krites-provenance.py or scripts/render-krites-notice.py and commit the result"]
    return []


def check_no_derived_growth(rows: list[dict], base_rows: list[dict] | None) -> list[str]:
    """PLAN.md §9 kill criterion 8: a row already known to the ledger must
    never regress TO 'derived'. A path with no base-ref row at all is not a
    regression — it is either wave 0's initial population or a completeness
    fix closing an undercount (P3: fts/README.md and gen_stopwords.py sat
    outside the ledger with no row of any status to regress from) — so only
    a base-ref path whose status was something other than 'derived' and is
    now 'derived' counts."""
    if base_rows is None:
        return []
    base_status = {r["path"]: r["status"] for r in base_rows}
    current_derived = {r["path"] for r in rows if r["status"] == "derived"}
    backslid = sorted(
        path
        for path in current_derived
        if path in base_status and base_status[path] != "derived"
    )
    if backslid:
        return [
            "ledger row(s) regressed TO 'derived' relative to the base commit — a file may only "
            "be marked derived by wave 0's initial population, never afterward (PLAN.md §9 kill "
            "criterion 8): " + ", ".join(backslid)
        ]
    return []


def check_status_sequence(rows: list[dict], base_rows: list[dict] | None) -> list[str]:
    """SAFETY(P1): the second half of the anti-backslide fix. The
    sovereign/verbatim_pct cross-check in validate_rows catches a bypass
    that flips status while leaving verbatim_pct as evidence; this check
    catches the sneakier variant that zeroes verbatim_pct too — a direct
    derived -> sovereign jump is illegal independent of what any other
    field says, because the only forcing function for real disuse (the
    dual/soak window) never ran.

    SAFETY(#6656): also verifies a completed dual -> sovereign transition
    carried its measurement forward rather than erasing it. Before this fix,
    scripts/krites-provenance-transition.py's --to sovereign path overwrote
    verbatim_pct with 0.0 and upstream_path with 'none' with nothing
    retaining the number that had been measured throughout the soak window
    — a status flip could discard real evidence with no check noticing. Now
    the prior row's upstream_path must reappear verbatim as the new row's
    replaced_upstream_path; a mismatch means the retained verification
    target was hand-edited rather than carried forward by the transition
    script."""
    if base_rows is None:
        return []
    base_by_path = {r["path"]: r for r in base_rows}
    errors = []
    for row in rows:
        path = row["path"]
        prior_row = base_by_path.get(path)
        prior = prior_row["status"] if prior_row is not None else None
        if prior is None or prior == row["status"]:
            continue
        if (prior, row["status"]) not in ALLOWED_TRANSITIONS:
            errors.append(
                f"{path}: illegal status transition {prior!r} -> {row['status']!r} — the only "
                "legal path out of 'derived' is derived -> dual -> sovereign (PLAN.md §2); a "
                f"direct {prior!r} -> {row['status']!r} jump is not permitted in one PR"
            )
            continue
        if prior == "dual" and row["status"] == "sovereign":
            expected = prior_row["upstream_path"]
            actual = row.get("replaced_upstream_path")
            if actual != expected:
                errors.append(
                    f"{path}: dual -> sovereign transition must carry its dual-era upstream_path "
                    f"forward as replaced_upstream_path unchanged (was {expected!r} while dual, "
                    f"now replaced_upstream_path={actual!r}) — a mismatch means the retained "
                    "verification target was hand-edited rather than carried forward by "
                    "scripts/krites-provenance-transition.py"
                )
    return errors


def check_soak_expiry(rows: list[dict], commit_count: int | None) -> list[str]:
    """PLAN.md §2's forcing function: a 'dual' row cannot soak forever by
    neglect. soak_expires_at_commit_count is an ABSOLUTE target — the count
    of commits reachable from origin/main (see krites_provenance_lib.py's
    ledger header NOTE for why origin/main, not HEAD: on a PR, HEAD includes
    the PR's own unmerged commits, which have not landed on main and would
    over-count the window)."""
    dual_rows = [r for r in rows if r["status"] == "dual"]
    if not dual_rows:
        return []
    if commit_count is None:
        return [
            "could not determine the current commit count on main (git rev-list --count "
            "origin/main failed) — cannot evaluate soak expiry for dual row(s): "
            + ", ".join(r["path"] for r in dual_rows)
        ]
    errors = []
    for row in dual_rows:
        expiry = row["soak_expires_at_commit_count"]
        if expiry <= 0:
            errors.append(
                f"{row['path']}: status=dual requires a positive soak_expires_at_commit_count "
                f"(0 means 'not in dual' per the ledger header note); got {expiry}"
            )
        elif commit_count >= expiry:
            errors.append(
                f"{row['path']}: dual soak window expired — current commit count on main "
                f"({commit_count}) has reached soak_expires_at_commit_count ({expiry}); flip to "
                "sovereign or delete the module (PLAN.md §2), or extend the window with an "
                "explicit, reviewable ledger edit"
            )
    return errors


def check_verbatim_recompute(rows: list[dict]) -> list[str]:
    """P6: when the offline upstream snapshot (crates/krites/upstream-snapshot/
    cozo-core-src/, vendored by wave0/drift-metric) is present, recompute
    every derived/dual row's verbatim_pct from it and fail if the stored
    ledger value has drifted — the check that makes the published numbers
    self-verifying instead of trusted-forever. FAILS when the snapshot is
    absent: it is tracked in the repo, so its absence disables the crate's
    only self-verification, and skipping would report the ledger clean on
    evidence it never read.

    WHY dual is included: a 'dual' row's file is still, physically, the
    unmodified CozoDB-lineage copy soaking before deletion (PLAN.md §2) — it
    carries a real upstream_path the same as a 'derived' row, and drifting
    silently during the soak window is exactly the failure this check
    exists to catch.

    SAFETY(#6656): a 'sovereign' row is no longer a blanket exemption. Before
    this fix, EVERY sovereign row skipped this check unconditionally — which
    is how a statement-for-statement transliteration (aletheia#6656: 17
    `_native.rs` files, 18.0%-41.4% verbatim against the upstream file their
    non-native sibling is measured against) could enter the ledger at
    verbatim_pct=0.0 with no measurement ever run, and the gate reported
    green. A sovereign row with a real replaced_upstream_path (a completed
    dual soak, or a from-scratch rewrite with a natural predecessor via
    measure-krites-provenance.py's SOVEREIGN_VERIFY_MAP) is now recomputed
    against THAT path exactly like a derived/dual row is recomputed against
    upstream_path. Only a row with replaced_upstream_path == 'none' — a
    genuinely fresh addition with nothing to compare against — is still
    exempt, because there is nothing to recompute."""
    if not UPSTREAM_SNAPSHOT_DIR.is_dir():
        # WHY this fails rather than skips: the skip existed so this check could
        # land before wave0/drift-metric vendored the snapshot, and that ordering
        # is long since discharged — the snapshot is 108 tracked files in the
        # repo. What remained was an unconditional fail-open: deleting the only
        # reference every published figure is measured against made this checker
        # print one line and report the ledger CLEAN, exit 0. Measured, not
        # inferred. A checker that certifies a ledger it could not read is worse
        # than no checker, because the green is the thing people act on.
        return [
            "upstream-snapshot/ is absent, so not one verbatim_pct could be "
            "recomputed. It is tracked at crates/krites/upstream-snapshot/"
            "cozo-core-src/ and is the sole reference behind every figure in "
            "PROVENANCE.toml and NOTICE.md. Restore it (git checkout -- "
            "crates/krites/upstream-snapshot) rather than running without it."
        ]
    errors = []
    for row in rows:
        status = row["status"]
        if status in ("derived", "dual"):
            compare_to = row["upstream_path"]
        elif status == "sovereign":
            compare_to = row.get("replaced_upstream_path", "none")
            if compare_to == "none":
                continue
        else:
            continue
        snapshot_path = UPSTREAM_SNAPSHOT_DIR / compare_to
        if not snapshot_path.is_file():
            errors.append(
                f"{row['path']}: upstream-snapshot/ is present but has no {compare_to} "
                "— snapshot is incomplete relative to PROVENANCE.toml"
            )
            continue
        local_text = (KRITES_SRC / row["path"]).read_text(errors="replace")
        upstream_text = snapshot_path.read_text(errors="replace")
        recomputed = verbatim_pct(local_text, upstream_text)
        if recomputed != row["verbatim_pct"]:
            errors.append(
                f"{row['path']}: stored verbatim_pct {row['verbatim_pct']} does not match offline "
                f"recomputation {recomputed} against upstream-snapshot/{compare_to} — run "
                "scripts/measure-krites-provenance.py and commit the result"
            )
    return errors


def check_no_unjustified_exemption(rows: list[dict]) -> list[str]:
    """#6797: check_verbatim_recompute's replaced_upstream_path == 'none' skip has a
    hole its own docstring names but nothing closed -- "a genuinely fresh addition
    with nothing to compare against" is exempt, but nothing verified 'genuinely'.
    'none' means both "genuinely new" and "nobody ever mapped it", and the two were
    indistinguishable: that is how all 8 runtime/hnsw_sovereign/* rows (2912 lines,
    the crate's highest-risk rewrite) sat unmeasured at a hardcoded verbatim_pct=0.0
    while 17 smaller fixed_rule/algos/*_native.rs rewrites beside them were all
    measured (the aletheia#6656 fix reached those; this closes the mechanism that
    let a NEW row repeat the same hole).

    A sovereign row with replaced_upstream_path == 'none' must now be an explicit,
    reasoned declaration: either krites_provenance_lib.py's NO_PREDECESSOR_REASONS
    names why it genuinely has none, or the row belongs in
    measure-krites-provenance.py's SOVEREIGN_VERIFY_MAP instead (a real predecessor,
    measured for real by check_verbatim_recompute exactly like a derived/dual row).

    Also flags the reverse drift: a NO_PREDECESSOR_REASONS entry for a path that is
    no longer a sovereign/'none' row (deleted, or a predecessor was later found and
    it now belongs in SOVEREIGN_VERIFY_MAP) -- an unread stale reason is the same
    shape of default this check exists to close, just facing the other direction.
    """
    errors = []
    exempt_paths: set[str] = set()
    for row in rows:
        if row["status"] != "sovereign" or row.get("replaced_upstream_path", "none") != "none":
            continue
        exempt_paths.add(row["path"])
        if row["path"] not in NO_PREDECESSOR_REASONS:
            errors.append(
                f"{row['path']}: sovereign row with replaced_upstream_path='none' has no entry "
                "in krites_provenance_lib.py's NO_PREDECESSOR_REASONS -- a new sovereign row must "
                "either record a predecessor (a real replaced_upstream_path, via "
                "measure-krites-provenance.py's SOVEREIGN_VERIFY_MAP) or declare in "
                "NO_PREDECESSOR_REASONS why it genuinely has none"
            )
    stale = sorted(set(NO_PREDECESSOR_REASONS) - exempt_paths)
    if stale:
        errors.append(
            "NO_PREDECESSOR_REASONS has entries for paths that are no longer a sovereign row "
            "with replaced_upstream_path='none' (deleted, or a predecessor was found for it and "
            "it belongs in SOVEREIGN_VERIFY_MAP instead) -- remove the stale entry: "
            + ", ".join(stale)
        )
    return errors


def check_method_recorded(rows: list[dict]) -> list[str]:
    """#6797-followup: the ledger's #1-ranked hole — every other field records what a
    file's text looks like (verbatim_pct) or where it came from (upstream_path/
    replaced_upstream_path); none records HOW a sovereign row was written, which is
    what 'sovereign' actually claims. verbatim_pct cannot substitute: a confirmed
    transliteration measured 26.6% against its source while a confirmed independent
    rewrite measured HIGHER at 32.1% (aletheia#6656) — the metric ranks a copy above
    a rewrite.

    Gates on PRESENCE, not on a score: a row with no 'method' key at all fails (the
    field is optional at parse time — krites_provenance_lib.validate_rows tolerates
    absence so a pre-migration --base-ref ledger still parses — but the CURRENT
    ledger must always carry it, since dump_ledger refuses to write a row without
    one). A 'sovereign' row carrying method='transliterated' also fails: that value
    exists so a finding CAN be recorded (fts/tokenizer/stop_word_filter/sovereign/
    mod.rs carries it today — a statement-for-statement match at 15.5%, aletheia#6656)
    but recording the finding does not clear the row; only rewriting it independently
    or reclassifying it does.
    """
    errors = []
    for row in rows:
        path = row["path"]
        method = row.get("method")
        if not method:
            errors.append(
                f"{path}: missing 'method' — every ledger row must record HOW it was "
                "written, not only what it looks like (verbatim_pct provably cannot "
                "substitute — aletheia#6656). Regenerate via "
                "scripts/measure-krites-provenance.py, or set explicitly via "
                "scripts/krites-provenance-transition.py --set-method"
            )
            continue
        if row["status"] == "sovereign" and method == "transliterated":
            errors.append(
                f"{path}: sovereign row carries method='transliterated' — that is a "
                "finding value, never a legitimate state for a sovereign row: it means "
                "the file was confirmed to be a disguised copy, not an independent "
                "rewrite. Fix the file's provenance (rewrite it independently, per "
                "aletheia#6656's own remediation of fixed_rule/algos/dfs_native.rs, or "
                "reclassify the row) rather than leaving it sovereign with this method"
            )
    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-ref", default="origin/main")
    parser.add_argument("--main-ref", default="origin/main")
    args = parser.parse_args()

    if not LEDGER_PATH.exists():
        fail(f"missing {LEDGER_PATH}")
        return 1

    try:
        meta, rows = parse_ledger(LEDGER_PATH.read_text())
    except (tomllib.TOMLDecodeError, LedgerError) as exc:
        fail(f"could not parse {LEDGER_PATH}: {exc}")
        return 1

    try:
        base_rows = load_base_rows(args.base_ref)
    except (BaseRefError, tomllib.TOMLDecodeError, LedgerError) as exc:
        fail(str(exc))
        return 1

    errors: list[str] = []
    errors += check_completeness(rows)
    errors += check_notice_sync(meta, rows)
    errors += check_no_derived_growth(rows, base_rows)
    errors += check_status_sequence(rows, base_rows)
    errors += check_soak_expiry(rows, git_commit_count(args.main_ref))
    errors += check_verbatim_recompute(rows)
    errors += check_no_unjustified_exemption(rows)
    errors += check_method_recorded(rows)

    if errors:
        for err in errors:
            fail(err)
        return 1

    print(f"krites-provenance: clean ({len(rows)} rows)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
