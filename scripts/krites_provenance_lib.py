"""Shared parse/render/measure helpers for the krites provenance ledger."""

from __future__ import annotations

import difflib
import pathlib
import tomllib

REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent
KRITES_DIR = REPO_ROOT / "crates" / "krites"
KRITES_SRC = KRITES_DIR / "src"
LEDGER_PATH = KRITES_DIR / "PROVENANCE.toml"
NOTICE_PATH = KRITES_DIR / "NOTICE.md"
# WARNING(P6): layout is set by wave0/drift-metric's vendored snapshot, not
# by this branch. Optional — check_verbatim_recompute skips (not fails) when
# absent, so this file has no ordering dependency on that branch landing.
UPSTREAM_SNAPSHOT_DIR = KRITES_DIR / "upstream-snapshot" / "cozo-core-src"

STATUSES = ("derived", "sovereign", "dual")
# INVARIANT(P1): the only legal forward status transitions — a row may only
# leave 'derived' by first sitting in 'dual' (land-dark/soak/delete;
# kanon/projects/aletheia/phases/05g-krites-overhaul/PROVENANCE-LEDGER.md
# "Transitions"). A direct 'derived' -> 'sovereign' jump, or any transition
# out of 'sovereign', is a backslide. Checked by check-krites-provenance.py's
# check_status_sequence against the PR's base ref.
ALLOWED_TRANSITIONS = frozenset({("derived", "dual"), ("dual", "sovereign")})
# WARNING(P3): extend this tuple, not a bespoke glob elsewhere, if another
# non-.rs/.pest upstream-derived file shows up under src/ — iter_src_files()
# is the single completeness boundary the ledger, the measurer, and the
# NOTICE all key off. Two upstream-derived files (fts/README.md,
# fts/tokenizer/stop_word_filter/gen_stopwords.py) sat outside the ledger
# until this tuple grew .md/.py alongside .rs/.pest.
TRACKED_SUFFIXES = (".rs", ".pest", ".md", ".py")


class LedgerError(ValueError):
    pass


def iter_src_files() -> list[str]:
    return sorted(
        p.relative_to(KRITES_SRC).as_posix()
        for p in KRITES_SRC.rglob("*")
        if p.is_file() and p.suffix in TRACKED_SUFFIXES
    )


def nonblank_lines(text: str) -> list[str]:
    return [line.rstrip("\n") for line in text.splitlines() if line.strip() != ""]


def verbatim_pct(local_text: str, upstream_text: str | None) -> float:
    local_lines = nonblank_lines(local_text)
    if not local_lines or upstream_text is None:
        return 0.0
    upstream_lines = nonblank_lines(upstream_text)
    matcher = difflib.SequenceMatcher(None, local_lines, upstream_lines, autojunk=False)
    matched = sum(block.size for block in matcher.get_matching_blocks())
    return round(matched / len(local_lines) * 100, 1)


def validate_rows(rows: list[dict]) -> None:
    seen: set[str] = set()
    for row in rows:
        path = row.get("path")
        if not path:
            raise LedgerError("row missing 'path'")
        if path in seen:
            raise LedgerError(f"duplicate ledger row for {path}")
        seen.add(path)
        if row.get("status") not in STATUSES:
            raise LedgerError(f"{path}: status must be one of {STATUSES}, got {row.get('status')!r}")
        if row.get("upstream_path") in (None, ""):
            raise LedgerError(f"{path}: upstream_path must be a string ('none' when absent)")
        if row["status"] == "sovereign" and row["upstream_path"] != "none":
            raise LedgerError(f"{path}: status=sovereign requires upstream_path='none'")
        if row["status"] != "sovereign" and row["upstream_path"] == "none":
            raise LedgerError(f"{path}: status={row['status']} requires a real upstream_path")
        # SAFETY(P1): closes the wave-0-review bypass — measure-krites-provenance.py
        # hardcodes verbatim_pct=0.0 for every row it generates with status=sovereign
        # (UPSTREAM_MAP[path] is None), so a sovereign row is NEVER measured against
        # upstream under the normal generator path. A sovereign row carrying a nonzero
        # verbatim_pct is therefore always a hand-edit (or a stale render) smuggling a
        # real similarity score past the anti-backsliding gate — reject unconditionally,
        # independent of --base-ref, so this holds even for a bootstrap commit or an
        # offline `parse_ledger` call outside CI.
        if row["status"] == "sovereign" and row.get("verbatim_pct", 0.0) != 0.0:
            raise LedgerError(
                f"{path}: status=sovereign requires verbatim_pct == 0.0 — a sovereign row is "
                "never measured against upstream; a nonzero value paired with sovereign means "
                "this file still carries a real similarity score and must land through 'dual' "
                f"first, never jump directly from 'derived' (got verbatim_pct={row['verbatim_pct']})"
            )
        # INVARIANT: every wave that has landed a fresh, CozoDB-independent
        # replacement under this scheme has put the substring 'sovereign' in
        # its path (hnsw_sovereign/, fold_table_sovereign/,
        # stop_word_filter/sovereign/) — kanon/projects/aletheia/phases/
        # 05g-krites-overhaul/PROVENANCE-LEDGER.md "Naming convention". A
        # path carrying that substring is never legitimately the retiring
        # copy, so it must never carry 'derived' or 'dual'. This is the
        # structural fix for aletheia#6656: nine runtime/hnsw_sovereign/*.rs
        # rows — the fresh rewrite — carried 'dual' (the label for the file
        # about to be deleted) while the actual retiring runtime/hnsw/*.rs
        # copies carried 'derived' with no expiry at all. A naming-convention
        # violation now fails here, before any transition or soak logic runs.
        if "sovereign" in path and row["status"] != "sovereign":
            raise LedgerError(
                f"{path}: path names this a sovereign (CozoDB-independent) replacement, but "
                f"status={row['status']!r} — a 'sovereign'-named path must carry status=sovereign; "
                "a 'dual' or 'derived' status here means the retiring-copy and fresh-replacement "
                "labels are inverted (aletheia#6656)"
            )


def parse_ledger(text: str) -> tuple[dict, list[dict]]:
    data = tomllib.loads(text)
    meta = data.get("meta", {})
    rows = data.get("file", [])
    validate_rows(rows)
    return meta, rows


def _toml_str(value: str) -> str:
    escaped = value.replace("\\", "\\\\").replace('"', '\\"')
    return f'"{escaped}"'


def dump_ledger(meta: dict, rows: list[dict]) -> str:
    validate_rows(rows)
    lines = [
        "# NOTE: generated by scripts/measure-krites-provenance.py — do not hand-edit rows.",
        "# NOTE: soak_expires_at_commit_count = 0 means the file is not in dual",
        "# NOTE: (land-dark/soak) state. A nonzero value is an ABSOLUTE target: the",
        "# NOTE: count of `git rev-list --count origin/main` at or past which CI",
        "# NOTE: fails the build (kanon/projects/aletheia/phases/05g-krites-overhaul/",
        "# NOTE: PROVENANCE-LEDGER.md \"Soak fuse\") — not a duration and not relative",
        "# NOTE: to when the row entered dual. Extend by explicit ledger edit.",
        "# NOTE: status = derived | sovereign | dual (PROVENANCE-LEDGER.md \"Statuses\");",
        "# NOTE: the only legal transition out of derived is derived -> dual ->",
        "# NOTE: sovereign, CI-enforced (check_status_sequence) — a direct derived ->",
        "# NOTE: sovereign jump is rejected regardless of verbatim_pct. A path",
        "# NOTE: containing 'sovereign' must carry status=sovereign (\"Naming",
        "# NOTE: convention\") — validate_rows rejects the row otherwise.",
        "",
        "[meta]",
        f"upstream_repo = {_toml_str(meta['upstream_repo'])}",
        f"upstream_ref = {_toml_str(meta['upstream_ref'])}",
        "",
    ]
    for row in sorted(rows, key=lambda r: r["path"]):
        lines.append("[[file]]")
        lines.append(f"path = {_toml_str(row['path'])}")
        lines.append(f"upstream_path = {_toml_str(row['upstream_path'])}")
        lines.append(f"verbatim_pct = {row['verbatim_pct']:.1f}")
        lines.append(f"status = {_toml_str(row['status'])}")
        lines.append(f"soak_expires_at_commit_count = {row['soak_expires_at_commit_count']}")
        lines.append("")
    return "\n".join(lines).rstrip("\n") + "\n"


def render_notice(meta: dict, rows: list[dict]) -> str:
    # NOTE: pure function of the ledger — no filesystem reads beyond `rows`
    # itself, so a formatting-only change to a derived file's blank-line count
    # cannot desync this render from PROVENANCE.toml without touching the ledger.
    rows = sorted(rows, key=lambda r: r["path"])
    derived = [r for r in rows if r["status"] == "derived"]
    sovereign = [r for r in rows if r["status"] == "sovereign"]
    dual = [r for r in rows if r["status"] == "dual"]

    mean_pct = round(sum(r["verbatim_pct"] for r in derived) / len(derived), 1) if derived else 0.0

    lines: list[str] = []
    lines.append("# Third-party notice — krites")
    lines.append("")
    lines.append(
        "`krites` is substantially derived from **CozoDB** (`cozo-core`), copyright the CozoDB "
        "authors, licensed under the **Mozilla Public License 2.0**. A copy of that license sits "
        "beside this file at [LICENSE-MPL-2.0](LICENSE-MPL-2.0); upstream is "
        "<https://github.com/cozodb/cozo>."
    )
    lines.append("")
    lines.append("## What is derived")
    lines.append("")
    lines.append(
        "This table is rendered from [`PROVENANCE.toml`](PROVENANCE.toml) — the file-level "
        "provenance ledger — never hand-edited. `verbatim_pct` is the share of each file's "
        "non-blank lines that a line-level diff (Python `difflib.SequenceMatcher`, order-sensitive) "
        "matches against the upstream file at the pinned commit; it is measured per file, not "
        "assumed from a subsystem average."
    )
    lines.append("")
    lines.append(f"- Upstream: <{meta['upstream_repo']}>, pinned at `{meta['upstream_ref']}`")
    lines.append(f"- {len(rows)} files under `src/`: {len(derived)} derived, {len(sovereign)} sovereign, {len(dual)} dual")
    lines.append(
        f"- Mean verbatim match across the {len(derived)} derived files: {mean_pct}% "
        "(unweighted average of the per-file `verbatim_pct` column below)"
    )
    lines.append("")
    lines.append("| File | Upstream | Verbatim | Status |")
    lines.append("|---|---|---:|---|")
    for row in rows:
        upstream_cell = "—" if row["upstream_path"] == "none" else f"`{row['upstream_path']}`"
        lines.append(
            f"| `src/{row['path']}` | {upstream_cell} | {row['verbatim_pct']:.1f}% | {row['status']} |"
        )
    lines.append("")
    lines.append(
        "Aletheia's own additions are real and sit alongside the derived files — `async_surface`, "
        "`counterfactual`, `hot_reload`, `query_cache`, `storage/fjall_backend`, the CSR PageRank "
        "path, `kcore`, RRF, the fixed-rule test suite, and `data/tests/proptest_memcmp` — all "
        "`sovereign` in the table above. They do not change the provenance of the derived files "
        "they extend."
    )
    lines.append("")
    lines.append("## A second vendored source: stop word lists")
    lines.append("")
    lines.append(
        "`fts/tokenizer/stop_word_filter`'s word lists are not CozoDB's expression, even in the "
        "rows above marked `derived`/`dual` against a CozoDB `upstream_path`: they are the "
        "[stopwords-iso](https://github.com/stopwords-iso/stopwords-iso/) project's data (copyright "
        "Gene Diaz, MIT license), which CozoDB itself vendored rather than authored. Krites vendors "
        "the same corpus a second time — CozoDB is a sibling vendor here, not the copyright source. "
        "The `upstream_path` column names CozoDB because that is where this crate's copy was copied "
        "from mechanically, which is a real and correctly-tracked lineage fact for the "
        "`derived`/`dual` rows in that module; it does not make CozoDB the author of the word data, "
        "and does not substitute for the MIT notice that data separately requires. That notice — "
        "attribution plus the full license text — lives at "
        "`src/fts/tokenizer/stop_word_filter/sovereign/NOTICE.md` and "
        "[`LICENSE-MIT-stopwords-iso`](LICENSE-MIT-stopwords-iso), independent of this file and of "
        "this module's CozoDB-retirement status."
    )
    lines.append("")
    lines.append("## What that requires")
    lines.append("")
    lines.append(
        "Under MPL §3.1 every file in this crate that is derived from `cozo-core`, **including our "
        "modifications to it**, stays governed by the MPL. That is file-level copyleft: it binds "
        "these files and reaches no further into aletheia."
    )
    lines.append("")
    lines.append(
        "Aletheia distributes the whole as a Larger Work under AGPL-3.0-or-later. MPL §3.3 permits "
        "exactly that, because CozoDB does not attach Exhibit B and so is not Incompatible With "
        "Secondary Licenses, and AGPL-3.0 is a Secondary License under §1.12. A recipient may "
        "therefore take the covered files under either license, at their option. The crate's "
        "`license` field records the combination."
    )
    lines.append("")
    lines.append("## Why this notice exists")
    lines.append("")
    lines.append(
        "Upstream identifiers were renamed during the migration and no attribution was recorded, "
        "which left the crate carrying MPL-covered code with its notices removed — the one thing "
        "§3.1 does not permit, independent of which license the Larger Work ships under. Renaming "
        "symbols does not change authorship of the expression. This file restores the notice."
    )
    lines.append("")
    lines.append(
        "The related trap, since it is what produced the gap: `docs/HUBS.md` asks memory "
        "documentation to describe the current architecture as Krites/Datalog/Fjall rather than "
        "CozoDB. That is sound naming hygiene and it explicitly does not reach attribution. "
        "Provenance and licensing statements name CozoDB because they are claims about authorship, "
        "not about architecture."
    )
    lines.append("")
    lines.append("## Anti-backsliding")
    lines.append("")
    lines.append(
        "`scripts/check-krites-provenance.py` runs in CI (wired into the repo's required `gate` "
        "check, not a side workflow) and fails the build if: any file under `crates/krites/src/` "
        "is missing from the ledger; this file drifts from what the ledger renders; the set of "
        "`derived` rows grows relative to the PR's base commit; a row's status skips the "
        "`derived` → `dual` → `sovereign` sequence; a `sovereign` row carries a nonzero "
        "`verbatim_pct`; a `dual` row's soak window has expired against the current commit count "
        "on `main`; or — when the offline upstream snapshot is present — a `derived` row's stored "
        "`verbatim_pct` no longer matches a fresh recomputation. The status-sequence and "
        "sovereign/verbatim_pct checks together make a direct `derived` → `sovereign` jump "
        "structurally impossible, not merely discouraged: neither check alone stops a bypass that "
        "clears the other (flip status alone leaves verbatim_pct as evidence; zero the field too "
        "and the sequence check still requires a `dual` commit in between)."
    )
    return "\n".join(lines).rstrip("\n") + "\n"
