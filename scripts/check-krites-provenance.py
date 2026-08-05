#!/usr/bin/env python3
"""CI gate: PROVENANCE.toml completeness, NOTICE.md sync, no derived-row growth."""

from __future__ import annotations

import argparse
import pathlib
import subprocess
import sys
import tomllib

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from krites_provenance_lib import (  # noqa: E402
    LEDGER_PATH,
    NOTICE_PATH,
    REPO_ROOT,
    LedgerError,
    iter_src_files,
    parse_ledger,
    render_notice,
)


def fail(message: str) -> None:
    print(f"::error::krites-provenance: {message}", file=sys.stderr)


def git_show(ref: str, path: str) -> str | None:
    result = subprocess.run(
        ["git", "-C", str(REPO_ROOT), "show", f"{ref}:{path}"],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        return None
    return result.stdout


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
    actual = NOTICE_PATH.read_text()
    if expected != actual:
        return ["NOTICE.md is out of sync with PROVENANCE.toml — run scripts/measure-krites-provenance.py or scripts/render-krites-notice.py and commit the result"]
    return []


def check_no_derived_growth(rows: list[dict], base_ref: str) -> list[str]:
    base_text = git_show(base_ref, "crates/krites/PROVENANCE.toml")
    if base_text is None:
        print(f"krites-provenance: no PROVENANCE.toml at {base_ref} — skipping growth check (bootstrap commit)")
        return []
    try:
        _, base_rows = parse_ledger(base_text)
    except (tomllib.TOMLDecodeError, LedgerError) as exc:
        return [f"could not parse PROVENANCE.toml at {base_ref}: {exc}"]

    base_derived = {r["path"] for r in base_rows if r["status"] == "derived"}
    current_derived = {r["path"] for r in rows if r["status"] == "derived"}
    new_derived = sorted(current_derived - base_derived)
    if new_derived:
        return [
            "ledger gained 'derived' row(s) relative to "
            + base_ref
            + " — a file may only be marked derived by wave 0's initial population, "
            "never afterward (PLAN.md §9 kill criterion 8): "
            + ", ".join(new_derived)
        ]
    return []


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-ref", default="origin/main")
    args = parser.parse_args()

    if not LEDGER_PATH.exists():
        fail(f"missing {LEDGER_PATH}")
        return 1

    try:
        meta, rows = parse_ledger(LEDGER_PATH.read_text())
    except (tomllib.TOMLDecodeError, LedgerError) as exc:
        fail(f"could not parse {LEDGER_PATH}: {exc}")
        return 1

    errors: list[str] = []
    errors += check_completeness(rows)
    errors += check_notice_sync(meta, rows)
    errors += check_no_derived_growth(rows, args.base_ref)

    if errors:
        for err in errors:
            fail(err)
        return 1

    print(f"krites-provenance: clean ({len(rows)} rows)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
