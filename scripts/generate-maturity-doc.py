#!/usr/bin/env python3
"""Generate docs/MATURITY.md's crate table from each crate's own Cargo.toml.

Every crate's maturity classification, if declared at all, already lives at
`[package.metadata.kanon]` in that crate's own `Cargo.toml` (`maturity`,
`since`, `exit-criteria`) -- aletheia#4537 asked for a public matrix, and the
canonical source for the crate rows already existed, just unexposed. This
generator reads the crate list from `CRATE-INDEX.toml` (itself generated
from the Cargo.toml workspace graph -- see `generate-crate-index.py`), reads
each crate's own metadata, and rewrites only the generated block in
docs/MATURITY.md, byte-exact everywhere else, the same way
generate-configuration-doc.py and generate-crate-index.py treat their own
generated blocks.

A crate with no `[package.metadata.kanon]` block renders as `Undeclared` --
that is real information (most crates have not adopted this yet), not a
gap in the generator. See docs/MATURITY.md's own "Known gaps" section for
what this table does not yet cover (routes, providers, TUI/desktop
surfaces, observability).

Usage:
    python3 scripts/generate-maturity-doc.py
    python3 scripts/generate-maturity-doc.py --check

--check exits 0 when the committed file's generated block matches freshly
derived output and non-zero with a diff-shaped explanation.
"""

from __future__ import annotations

import argparse
import sys
import tomllib
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
CRATE_INDEX_PATH = REPO_ROOT / "CRATE-INDEX.toml"
DOC_PATH = REPO_ROOT / "docs" / "MATURITY.md"

BEGIN_MARKER = "<!-- BEGIN GENERATED CRATE MATURITY -- run `python3 scripts/generate-maturity-doc.py` to refresh, do not hand-edit -->"
END_MARKER = "<!-- END GENERATED CRATE MATURITY -->"

# WHY: the free-text `maturity` field is hand-typed per crate, not an enum
# enforced anywhere -- normalize known spellings to the issue's suggested
# vocabulary for display consistency without rewriting Cargo.toml files this
# generator does not own.
MATURITY_DISPLAY = {
    "production": "Stable",
    "stable": "Stable",
    "beta": "Experimental",
    "alpha": "Experimental",
    "experimental": "Experimental",
    "internal": "Internal",
    "planned": "Planned",
    "deprecated": "Deprecated",
    "removed": "Removed/Superseded",
    "superseded": "Removed/Superseded",
}


def load_toml(path: Path) -> dict:
    with path.open("rb") as fh:
        return tomllib.load(fh)


def crate_rows() -> list[tuple[str, str, str, str, str]]:
    """Return (name, path, maturity_display, since, exit_criteria) sorted by name."""
    index = load_toml(CRATE_INDEX_PATH)
    rows = []
    for name, entry in index["crates"].items():
        path = REPO_ROOT / entry["path"]
        manifest = load_toml(path / "Cargo.toml")
        kanon_meta = manifest.get("package", {}).get("metadata", {}).get("kanon", {})
        raw_maturity = kanon_meta.get("maturity", "")
        maturity = MATURITY_DISPLAY.get(raw_maturity.lower(), "Undeclared")
        since = kanon_meta.get("since", "—")
        exit_criteria = kanon_meta.get("exit-criteria", "—")
        rows.append((name, entry["path"], maturity, since, exit_criteria))
    rows.sort(key=lambda r: r[0])
    return rows


def render_table() -> str:
    rows = crate_rows()
    lines = [
        "| Crate | Path | Maturity | Since | Exit criteria |",
        "|---|---|---|---|---|",
    ]
    for name, path, maturity, since, exit_criteria in rows:
        lines.append(f"| `{name}` | `{path}` | {maturity} | {since} | {exit_criteria} |")
    declared = sum(1 for r in rows if r[2] != "Undeclared")
    lines.append("")
    lines.append(
        f"{declared} of {len(rows)} crates declare `[package.metadata.kanon]` "
        "maturity metadata. The rest render `Undeclared`, not an implicit "
        "`Stable` -- declare maturity in the crate's own `Cargo.toml` to "
        "close that gap for one crate at a time."
    )
    return "\n".join(lines)


def apply(check: bool) -> int:
    doc = DOC_PATH.read_text(encoding="utf-8")
    begin = doc.find(BEGIN_MARKER)
    end = doc.find(END_MARKER)
    if begin == -1 or end == -1:
        print(
            f"could not find generated-block markers in {DOC_PATH}; "
            "ensure the file contains the BEGIN/END anchors",
            file=sys.stderr,
        )
        return 1
    end += len(END_MARKER)

    generated = render_table()
    block = f"{BEGIN_MARKER}\n\n{generated}\n\n{END_MARKER}"
    new_doc = doc[:begin] + block + doc[end:]

    if check:
        if new_doc != doc:
            print(
                f"{DOC_PATH} is stale -- run `python3 {Path(__file__).name}` to regenerate",
                file=sys.stderr,
            )
            return 1
        print(f"OK: {DOC_PATH} matches the crate Cargo.toml metadata")
        return 0

    DOC_PATH.write_text(new_doc, encoding="utf-8")
    print(f"Updated {DOC_PATH}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check",
        action="store_true",
        help="check docs/MATURITY.md against Cargo.toml metadata and exit non-zero on drift",
    )
    args = parser.parse_args()
    return apply(check=args.check)


if __name__ == "__main__":
    sys.exit(main())
