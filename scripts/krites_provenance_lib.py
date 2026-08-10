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
# leave 'derived' by first sitting in 'dual' (PLAN.md §2 land-dark/soak/
# delete). A direct 'derived' -> 'sovereign' jump, or any transition out of
# 'sovereign', is a backslide. Checked by check-krites-provenance.py's
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
    # WHY(aletheia#6656): strip leading AND trailing whitespace, not just the
    # trailing newline splitlines() already drops on its own. A pure
    # re-indentation carries no content change but shifts every line's
    # column position — before this fix, wrapping storage/mem.rs's preserved
    # copy in `mod derived { }` (a formatting-only PLAN.md land-dark step)
    # dropped its measured verbatim_pct from ~69% to 4.5% as a side effect,
    # because every line gained four leading spaces the upstream file never
    # had and stopped matching character-for-character. Stripping both ends
    # makes the comparison track expression, not incidental column position;
    # re-measuring the same pair post-fix gives 31.1%.
    return [line.strip() for line in text.splitlines() if line.strip() != ""]


# INVARIANT(aletheia#6656): a contiguous matched run shorter than this many
# non-blank lines does not count as verbatim evidence. Below this length, a
# match is as likely to be language-level boilerplate that any two unrelated
# Rust files share by chance (a lone `}`, `#[cfg(test)]`, `mod tests {`, a
# single `use` line) as it is real shared expression — the audit's
# reproduction: `runtime/hnsw_sovereign/types.rs`, which has no authored
# relationship to `runtime/hnsw.rs`, still scored 12.4% against it purely
# from scattered 1-2 line collisions. Measured against the corpus (the known
# aletheia#6656 transliteration vs. its real upstream match, against several
# unrelated-file "noise" pairs scoring in the low-to-mid 30s under the old
# floor of 1): raising the floor to 4 drops every measured noise pair below
# 15% while the real match retains signal in the high 20s — a floor of 1,
# 2, or 3 leaves noise and signal within a few points of each other, not
# separable. Chosen from that measurement, not guessed.
MIN_MATCH_BLOCK_LINES = 4


def verbatim_pct(local_text: str, upstream_text: str | None) -> float:
    local_lines = nonblank_lines(local_text)
    if not local_lines or upstream_text is None:
        return 0.0
    upstream_lines = nonblank_lines(upstream_text)
    matcher = difflib.SequenceMatcher(None, local_lines, upstream_lines, autojunk=False)
    # NOTE: the floor is capped at the file's own length so a file shorter
    # than MIN_MATCH_BLOCK_LINES that matches upstream in full (a genuine,
    # complete verbatim copy) still scores 100% instead of being floored to
    # 0 by a threshold longer than the file itself.
    floor = min(MIN_MATCH_BLOCK_LINES, len(local_lines))
    matched = sum(
        block.size for block in matcher.get_matching_blocks() if block.size >= floor
    )
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

        # INVARIANT: every wave that has landed a fresh, CozoDB-independent replacement under this
        # scheme puts the substring 'sovereign' in its path (hnsw_sovereign/, fold_table_sovereign/,
        # stop_word_filter/sovereign/). A path carrying that substring is never legitimately the
        # RETIRING copy, so it must never carry 'derived' or 'dual'.
        #
        # This is the structural fix for aletheia#6656: nine runtime/hnsw_sovereign/*.rs rows — the
        # fresh rewrite — carried 'dual' (the label for the file about to be deleted) while the
        # actual retiring runtime/hnsw/*.rs copies carried 'derived' with no expiry at all. The
        # consequence was that the soak fuse was scheduled to delete the REPLACEMENT while the
        # derived original would never be forced out. A naming-convention violation now fails here,
        # before any transition or soak logic runs on it.
        if "sovereign" in path and row.get("status") != "sovereign":
            raise LedgerError(
                f"{path}: path names this a sovereign (CozoDB-independent) replacement, but "
                f"status={row.get('status')!r} — a 'sovereign'-named path must carry "
                "status=sovereign. A 'dual' or 'derived' status here means the retiring-copy and "
                "fresh-replacement labels have been swapped (aletheia#6656)."
            )
        if row.get("upstream_path") in (None, ""):
            raise LedgerError(f"{path}: upstream_path must be a string ('none' when absent)")
        if row["status"] == "sovereign" and row["upstream_path"] != "none":
            raise LedgerError(f"{path}: status=sovereign requires upstream_path='none'")
        if row["status"] != "sovereign" and row["upstream_path"] == "none":
            raise LedgerError(f"{path}: status={row['status']} requires a real upstream_path")
        # SAFETY(#6656): replaced_upstream_path is the retained verification target for
        # a 'sovereign' row — the upstream file it is nonetheless measured against
        # (PLAN.md §2(c): a completed dual soak carries its upstream_path forward here
        # instead of losing it; a from-scratch rewrite with a natural predecessor, e.g.
        # a `_native.rs` file, is measured against that predecessor's own upstream_path
        # via measure-krites-provenance.py's SOVEREIGN_VERIFY_MAP). It is meaningless
        # outside status=sovereign, since a derived/dual row already carries a live
        # lineage claim in upstream_path itself.
        #
        # WHY setdefault, not a hard require: a ledger serialized before this field
        # existed has no such key at all — check-krites-provenance.py's --base-ref
        # comparison reads exactly such a ledger (the pre-migration commit) on every PR
        # until enough history passes it by. Treating an ABSENT key as 'none' (the same
        # value it would get if explicitly written) lets old ledgers parse instead of
        # hard-failing every base-ref diff against pre-migration history; an explicitly
        # empty string ("") is still rejected below as malformed, not silently defaulted.
        row.setdefault("replaced_upstream_path", "none")
        if row["replaced_upstream_path"] == "":
            raise LedgerError(f"{path}: replaced_upstream_path must be a string ('none' when absent)")
        if row["status"] != "sovereign" and row["replaced_upstream_path"] != "none":
            raise LedgerError(
                f"{path}: replaced_upstream_path is only meaningful on status=sovereign rows "
                f"(got status={row['status']!r}, replaced_upstream_path={row['replaced_upstream_path']!r})"
            )
        # SAFETY(P1, narrowed #6656): closes the wave-0-review bypass — a sovereign row
        # with NO retained verification target (replaced_upstream_path == 'none', i.e.
        # a genuinely fresh addition with no predecessor to compare against, like
        # kcore.rs) has nothing to measure, so verbatim_pct must be exactly 0.0; a
        # nonzero value there is always a hand-edit (or a stale render) smuggling a real
        # similarity score past the anti-backsliding gate with no evidence backing it.
        # A row that DOES carry a replaced_upstream_path is permitted a nonzero
        # verbatim_pct here (it is no longer a bare, unmeasured claim) — but the number
        # itself is only trustworthy because check-krites-provenance.py's
        # check_verbatim_recompute independently recomputes it against
        # replaced_upstream_path and fails the build on any drift, exactly as it already
        # does for derived/dual rows against upstream_path. This function only checks
        # structure; it cannot verify the number is honest without reading source files.
        if row["status"] == "sovereign" and row["replaced_upstream_path"] == "none" and row.get("verbatim_pct", 0.0) != 0.0:
            raise LedgerError(
                f"{path}: status=sovereign with replaced_upstream_path='none' has nothing to "
                "measure against, so verbatim_pct must be 0.0 — a nonzero value here is an "
                f"unmeasured claim with no retained evidence (got verbatim_pct={row['verbatim_pct']})"
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
        "# NOTE: fails the build (PLAN.md §2 expiry gate) — not a duration and not",
        "# NOTE: relative to when the row entered dual. Extend by explicit ledger edit.",
        "# NOTE: status = derived | sovereign | dual (PLAN.md §2, §3 wave 0.1); the",
        "# NOTE: only legal transition out of derived is derived -> dual -> sovereign,",
        "# NOTE: CI-enforced (check_status_sequence) — a direct derived -> sovereign",
        "# NOTE: jump is rejected regardless of verbatim_pct.",
        "# NOTE: replaced_upstream_path is 'none' except on a sovereign row that still",
        "# NOTE: has something to measure against: a completed dual soak (PLAN.md §2(c))",
        "# NOTE: retains its upstream_path here instead of losing it, or a from-scratch",
        "# NOTE: rewrite with a natural predecessor gets one from",
        "# NOTE: measure-krites-provenance.py's SOVEREIGN_VERIFY_MAP. verbatim_pct is",
        "# NOTE: then measured against THIS field, not upstream_path (which stays 'none'",
        "# NOTE: on every sovereign row — no MPL lineage claim). CI recomputes and",
        "# NOTE: fails on drift (check_verbatim_recompute), same as derived/dual.",
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
        lines.append(f"replaced_upstream_path = {_toml_str(row['replaced_upstream_path'])}")
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
    lines.append(
        "A `sovereign` row's `verbatim_pct` is not always 0.0: when the row still has something "
        "to measure against — a completed `dual` soak (PLAN.md §2(c)), or a from-scratch rewrite "
        "with a natural predecessor — the ledger retains that predecessor as "
        "`replaced_upstream_path` (shown below as \"cf. `path`\") and keeps measuring against it. "
        "`upstream_path` itself stays `none` on every `sovereign` row either way: this is not an "
        "MPL lineage claim, only a retained comparison the anti-backsliding gate keeps honest. A "
        "row with no predecessor at all (`replaced_upstream_path` also `none`) has nothing to "
        "measure and its `verbatim_pct` is genuinely 0.0."
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
        if row["upstream_path"] != "none":
            upstream_cell = f"`{row['upstream_path']}`"
        elif row.get("replaced_upstream_path", "none") != "none":
            upstream_cell = f"cf. `{row['replaced_upstream_path']}`"
        else:
            upstream_cell = "—"
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
    lines.append("## Reading `verbatim_pct`: what it can and cannot prove")
    lines.append("")
    lines.append(
        "`verbatim_pct` is evidence of textual overlap, not a verdict on origin. Two files that "
        "independently implement the same algorithm against the same crate vocabulary "
        "(`DataValue`, `BTreeMap`, the `FixedRule` trait, `poison.check()?`) converge on real "
        "line-for-line similarity that has nothing to do with copying — and at the file sizes in "
        "this crate, that convergence is large enough to overlap with an actual transliteration."
    )
    lines.append("")
    lines.append(
        "aletheia#6656 measured this directly against `fixed_rule/algos/*_native.rs` — every one "
        "nominally `sovereign` (`upstream_path = \"none\"`, no lineage claimed). Scored against the "
        "same-algorithm upstream file each was written to replace, verbatim_pct ranges 14.9% "
        "(`kruskal_native.rs`) to 32.1% (`degree_centrality_native.rs`); scored against an algorithm "
        "it has no relationship to at all, `bfs_native.rs` vs. `degree_centrality.rs` still measures "
        "7.4% from shared idiom alone. `dfs_native.rs` — confirmed by that audit to be a "
        "statement-for-statement transliteration with renamed identifiers — measures 26.6% against "
        "its real source: inside the same band as files with no such finding, and lower than "
        "`degree_centrality_native.rs`'s 32.1%, which reads on manual inspection as an independent "
        "rewrite (different data structures, different variable names, an added citation to "
        "Freeman 1978) despite scoring higher. The metric alone cannot separate the two."
    )
    lines.append("")
    lines.append(
        "Treat any `verbatim_pct` figure — for a `derived`/`dual` row against its recorded "
        "`upstream_path`, or for an ad hoc comparison run against a `sovereign` row for review — as "
        "a triage signal that earns a manual read at any nontrivial value, never as proof of either "
        "originality or copying by itself, and never as a substitute for reading the file."
    )
    lines.append("")
    lines.append(
        "`scripts/check-krites-verbatim-drift.py` is a separate, purpose-built answer to this same "
        "gap — a token-shingle Jaccard metric that discards punctuation-only, `use`, and attribute "
        "lines before comparing, precisely so shared idiom stops reading as evidence. It runs "
        "report-only in CI today (not yet promoted to a gate; see its module docstring for the "
        "promotion criteria) and is the tool to reach for when `verbatim_pct` alone is not enough "
        "to settle a review."
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
        "`derived` → `dual` → `sovereign` sequence; a `dual` → `sovereign` transition drops or "
        "rewrites the `replaced_upstream_path` it carried forward from that row's own "
        "`upstream_path`; a `sovereign` row with no retained predecessor "
        "(`replaced_upstream_path == 'none'`) carries a nonzero `verbatim_pct`; a `dual` row's "
        "soak window has expired against the current commit count on `main`; or — when the "
        "offline upstream snapshot is present — a `derived`/`dual` row's stored `verbatim_pct` no "
        "longer matches a fresh recomputation against `upstream_path`, **or a `sovereign` row's "
        "stored `verbatim_pct` no longer matches a fresh recomputation against its retained "
        "`replaced_upstream_path`**. That last clause is what makes a `sovereign` claim keep "
        "proving itself instead of being measured once and trusted forever — the original gap "
        "this file's own existence (see \"Why this notice exists\" above) was written to close, "
        "and that a transliterated file could still slip past a status flip that quietly zeroed "
        "its evidence (aletheia#6656). The status-sequence and sovereign/verbatim_pct checks "
        "together make a direct `derived` → `sovereign` jump structurally impossible, not merely "
        "discouraged: neither check alone stops a bypass that clears the other (flip status alone "
        "leaves verbatim_pct as evidence; zero the field too and the sequence check still "
        "requires a `dual` commit in between)."
    )
    return "\n".join(lines).rstrip("\n") + "\n"
