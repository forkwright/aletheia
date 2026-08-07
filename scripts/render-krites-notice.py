#!/usr/bin/env python3
"""Render crates/krites/NOTICE.md from crates/krites/PROVENANCE.toml (no network)."""

from __future__ import annotations

import pathlib
import sys
import tomllib

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from krites_provenance_lib import (  # noqa: E402
    LEDGER_PATH,
    NOTICE_PATH,
    LedgerError,
    parse_ledger,
    render_notice,
)


def main() -> None:
    try:
        meta, rows = parse_ledger(LEDGER_PATH.read_text())
    except (tomllib.TOMLDecodeError, LedgerError) as exc:
        raise SystemExit(f"could not parse {LEDGER_PATH}: {exc}") from exc
    NOTICE_PATH.write_text(render_notice(meta, rows))
    print(f"wrote {NOTICE_PATH} ({len(rows)} rows)")


if __name__ == "__main__":
    main()
