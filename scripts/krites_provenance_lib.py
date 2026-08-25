"""Shared parse/render/measure helpers for the krites provenance ledger."""

from __future__ import annotations

import difflib
import pathlib
import re

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

# INVARIANT(#6797-followup): the #1-ranked hole in this whole scheme -- every other
# field records what a file's TEXT looks like (verbatim_pct) or where it came from
# (upstream_path/replaced_upstream_path); none records HOW a sovereign row was
# WRITTEN, which is what 'sovereign' actually claims. verbatim_pct provably cannot
# substitute: a confirmed transliteration (fixed_rule/algos/dfs_native.rs, #6656)
# measured 26.6% against its source while a confirmed independent rewrite
# (degree_centrality_native.rs) measured HIGHER at 32.1% -- the metric ranks a copy
# above a rewrite. 'transliterated' is a FINDING value, not a state check_verbatim_
# recompute or any other measurement can reach on its own; it exists so a value CAN
# be recorded when one is found (fts/tokenizer/stop_word_filter/sovereign/mod.rs, a
# statement-for-statement match at 15.5%, below every calibration threshold), while
# check-krites-provenance.py's check_method_recorded fails the build on any row that
# carries it -- recording a finding is legitimate; leaving the row sovereign with it
# is not.
METHODS = (
    "from_spec",  # written against a written specification/paper, source not consulted
    "from_spec_derived_siblings",  # written against a spec, but derived siblings were read for convention
    "from_behavioral_oracle",  # written against observed behaviour/tests, source not read
    "rewritten_with_source_open",  # the derived file was consulted while writing
    "transliterated",  # finding value -- confirmed disguised copy, never legitimate on a sovereign row
    "attested_original",  # no predecessor existed; this is aletheia's own new code
    "unknown",  # no record exists
)

# INVARIANT(#6879): the values whose whole content is a claim about what was NOT read, and
# which are therefore meaningless without the consulted list that bounds it. Named once
# here because krites-provenance-transition.py refuses to record one without --consulted
# and consulted_errors checks exactly these two.
SPEC_CLASS_METHODS = ("from_spec", "from_spec_derived_siblings")

# INVARIANT(#6879): "match the surrounding crate" and "clean-room" are in direct tension
# here -- most rows in this ledger are derived, so the sibling that best demonstrates a
# convention is usually the sibling doing the same job, which is exactly the expression
# that must not propagate. 'consulted' is the list of source paths (relative to KRITES_SRC,
# like every other path in the ledger) that the author read while writing the row's file;
# an empty list means none. It exists because a bare 'from_spec' is an unfalsifiable
# attestation, while a NAMED consulted path has a ledger STATUS the checker can read --
# which is the only part of this claim a machine can reach.
#
# WARNING(#6879): an empty list on a row whose method was established by RESEARCH rather
# than recorded by its author (every row that predates this field) records "nothing was
# recorded", not a verified "nothing was read". The distinction is only real going forward.
# The list's COMPLETENESS is unverifiable in either direction: nothing observes what an
# author actually opened, so an omitted path reads identically to a path never read.
# 'consulted' converts an unfalsifiable claim into a partially-checkable one, not into a
# proof.
#
# WARNING(#6879): 'from_behavioral_oracle' carries the same independence claim as
# 'from_spec' but is deliberately NOT constrained by consulted_errors below, because no
# enum value exists for "written against a behavioural oracle, with derived siblings read"
# -- constraining it would force a rewrite in that position to record something false. Grow
# METHODS when a rewrite actually lands there; do not bend the record to fit.

# WHY a regex, not free text: 'evidence' that clears 'unknown' is only worth
# recording if a reader can independently go verify it. A GitHub PR/issue reference,
# a git commit SHA, or a spec path are all independently checkable; a prose
# justification with nothing to follow is exactly the "measured once, trusted
# forever" failure this whole field exists to end (krites-provenance-transition.py
# once hardcoded verbatim_pct=0.0 on every dual -> sovereign transition -- 17 files
# entered 'sovereign' that way and later re-measured at 18-41%; an unfollowable
# 'evidence' string would repeat that with prose standing in for the fiat zero).
METHOD_EVIDENCE_PATTERN = re.compile(r"^(#\d+|[0-9a-f]{7,40}|spec:\S+)$")

STATUSES = ("derived", "sovereign", "dual")
# INVARIANT(P1): the only legal forward status transitions — a row may only
# leave 'derived' by first sitting in 'dual' (RETIREMENT-PLAN.md §2 land-dark/soak/
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

# INVARIANT(#6797): a 'sovereign' row's replaced_upstream_path == 'none' is a claim
# that the file genuinely has no predecessor to measure against -- before this map
# existed, that claim was the SKIP branch's unconditional default, and nothing
# distinguished "genuinely fresh" from "nobody ever mapped it". That is how
# runtime/hnsw_sovereign/* (2912 lines, the crate's highest-risk rewrite) sat
# completely unmeasured at 'none' while 17 smaller fixed_rule/algos/*_native.rs
# rewrites next to it were all measured (aletheia#6656 already fixed those; this map
# closes the mechanism that let a NEW row repeat the same hole).
#
# check-krites-provenance.py's check_no_unjustified_exemption requires every
# sovereign/'none' row to appear here. Key = the row's path exactly as it appears
# in PROVENANCE.toml; value = a one-line reason, individually verified against
# crates/krites/upstream-snapshot/cozo-core-src/ (same discipline as UPSTREAM_MAP
# and SOVEREIGN_VERIFY_MAP in measure-krites-provenance.py, for the same reason --
# do not derive membership by path-shape pattern matching, e.g. truncating a
# directory to find a same-named file one level up: fixed_rule/csr/mod.rs is not
# fixed_rule/mod.rs, and query/tests/mod.rs is not query/mod.rs).
#
# A row that instead DOES have a real predecessor belongs in
# measure-krites-provenance.py's SOVEREIGN_VERIFY_MAP, not here -- the two are
# mutually exclusive per row, exactly like UPSTREAM_MAP/SOVEREIGN_VERIFY_MAP already
# are.
NO_PREDECESSOR_REASONS: dict[str, str] = {
    "async_surface.rs": (
        "aletheia-native async surface; NOTICE.md lists it among aletheia's own "
        "sovereign additions with no upstream counterpart"
    ),
    "counterfactual.rs": (
        "aletheia-native counterfactual-query feature; NOTICE.md lists it among "
        "aletheia's own sovereign additions"
    ),
    "counterfactual_tests.rs": "test suite for counterfactual.rs, a feature with no upstream analogue",
    "data/error.rs": (
        "cozo-core has no error.rs anywhere -- it uses type-erased miette::Error "
        "plus scattered derive structs, not a per-module error file"
    ),
    "data/tests/functions/validity_units.rs": (
        "regression test for aletheia#6656 / upstream cozo#312's Validity "
        "microsecond/second unit boundary, a krites-specific bugfix with no "
        "upstream test to compare against"
    ),
    "data/tests/proptest_memcmp.rs": (
        "proptest-based property-test suite for DataValue memcmp/serde round-trips; "
        "cozo-core has no proptest-based test file under data/tests/"
    ),
    "error.rs": "cozo-core has no error.rs anywhere -- type-erased miette::Error plus scattered derive structs",
    "fixed_rule/algos/kcore.rs": (
        "cozo-core has no k-core implementation at all (grep for KCore/k_core/kcore "
        "over the snapshot returns zero hits)"
    ),
    "fixed_rule/csr/mod.rs": (
        "from-scratch compressed-sparse-row graph representation; cozo-core has no "
        "CSR module (fixed_rule/mod.rs is the unrelated FixedRule trait definition, "
        "not a predecessor despite the truncated-path match)"
    ),
    "fixed_rule/csr/page_rank.rs": (
        "PageRank power iteration over the from-scratch CSR representation; "
        "reimplements the third-party `graph` crate's page_rank/PageRankConfig API "
        "that cozo-core's own fixed_rule/algos/pagerank.rs delegates to, not a "
        "rewrite of cozo-core's own file (measured 0.0% against it)"
    ),
    "fixed_rule/error.rs": "cozo-core has no error.rs anywhere",
    "fixed_rule/tests/centrality_spanning.rs": (
        "krites-native DbInstance integration tests; cozo-core has no "
        "fixed_rule/tests/ directory at all"
    ),
    "fixed_rule/tests/connectivity_misc.rs": (
        "krites-native DbInstance integration tests; cozo-core has no "
        "fixed_rule/tests/ directory"
    ),
    "fixed_rule/tests/mod.rs": (
        "module declarations for krites-native fixed_rule test files; cozo-core has "
        "no fixed_rule/tests/ directory"
    ),
    "fixed_rule/tests/path_algorithms.rs": (
        "krites-native DbInstance integration tests; cozo-core has no "
        "fixed_rule/tests/ directory"
    ),
    "fixed_rule/tests/proptest_algos.rs": (
        "krites-native property tests for graph algorithms; cozo-core has no "
        "fixed_rule/tests/ directory"
    ),
    "fixed_rule/tests/wave5_reference_semantics.rs": (
        "krites-native reference-semantics tests for the land-dark cfg mechanism, "
        "which cozo-core does not have"
    ),
    "fixed_rule/utilities/rrf.rs": "krites-native reciprocal-rank-fusion utility with no upstream equivalent",
    "fts/error.rs": "cozo-core has no error.rs anywhere",
    "fts/tokenizer/ascii_folding_filter/fold_table/fold_table_sovereign/generate.py": (
        "UCD/CLDR table-generation tool; cozo-core's fold table is hand-authored "
        "inline in ascii_folding_filter.rs, with no generator script of any kind to "
        "compare against"
    ),
    "fts/tokenizer/stop_word_filter/sovereign/NOTICE.md": (
        "third-party (stopwords-iso, MIT) attribution notice; cozo-core carries no "
        "such notice file anywhere"
    ),
    "hot_reload.rs": "aletheia-native hot-reload feature; NOTICE.md lists it among aletheia's own sovereign additions",
    "parse/error.rs": "cozo-core has no error.rs anywhere",
    "query/context.rs": (
        "krites-native QueryContext trait decoupling query/ from "
        "runtime::transact::SessionTx; cozo-core's query/ names SessionTx directly "
        "and has no such abstraction"
    ),
    "query/error.rs": "cozo-core has no error.rs anywhere",
    "query/tests/mod.rs": (
        "krites-native query integration tests; cozo-core has no query/tests/ "
        "directory (query/mod.rs is the unrelated query-engine module, not a "
        "predecessor despite the truncated-path match)"
    ),
    "query/tests/reference_semantics.rs": (
        "krites-native query integration tests; cozo-core has no query/tests/ directory"
    ),
    "query_cache.rs": "aletheia-native query cache; NOTICE.md lists it among aletheia's own sovereign additions",
    "runtime/error.rs": "cozo-core has no error.rs anywhere",
    "runtime/query_context_impl.rs": (
        "krites-native SessionTx -> QueryContext trait impl completing the "
        "query/context.rs decoupling; cozo-core has no such abstraction"
    ),
    "storage/error.rs": "cozo-core has no error.rs anywhere",
    "storage/fjall_backend.rs": (
        "aletheia-native fjall storage backend; cozo-core's storage/ holds mem, "
        "newrocks, rocks, sled, sqlite, temp, tikv -- no fjall backend of any kind"
    ),
}


class LedgerError(ValueError):
    pass


def iter_src_files() -> list[str]:
    return sorted(
        p.relative_to(KRITES_SRC).as_posix()
        for p in KRITES_SRC.rglob("*")
        if p.is_file() and p.suffix in TRACKED_SUFFIXES
    )


# INVARIANT(#5956): MPL-2.0 Exhibit A, verbatim. This is the licence's own recommended
# notice text, not prose this repo is free to reword -- it is quoted, and the quote is the
# single source both render_exhibit_a() and has_exhibit_a() read, so a per-file header and
# the gate that requires it can never disagree about what the notice says.
EXHIBIT_A_LINES = (
    "This Source Code Form is subject to the terms of the Mozilla Public License,",
    "v. 2.0. If a copy of the MPL was not distributed with this file, You can",
    "obtain one at https://mozilla.org/MPL/2.0/.",
)
EXHIBIT_A_TEXT = " ".join(EXHIBIT_A_LINES)

# INVARIANT(#5956): the sentinel that makes the generated block machine-recognisable.
# The measurement exclusion (strip_generated_notice, below) keys off the EXACT rendered
# block rather than a fuzzy marker scan, so it can never over-strip; this token exists so
# a hand-edited block -- one that no longer matches byte-for-byte and therefore stops
# being excluded -- is still detectable and can be reported as drift instead of silently
# moving every figure the block sits in.
EXHIBIT_A_MARKER = "krites-exhibit-a"
_EXHIBIT_A_BEGIN = f"{EXHIBIT_A_MARKER}: begin (generated -- scripts/measure-krites-provenance.py)"
_EXHIBIT_A_END = f"{EXHIBIT_A_MARKER}: end"

# WARNING(#5956): keyed by the same suffixes TRACKED_SUFFIXES admits. A tracked suffix with
# no entry here has no comment syntax to render a notice in, which render_exhibit_a refuses
# rather than guessing -- guessing produces a file that no longer parses in its own language.
COMMENT_SYNTAX: dict[str, tuple[str, str]] = {
    ".rs": ("//", ""),
    ".pest": ("//", ""),
    ".py": ("#", ""),
    ".md": ("<!--", "-->"),
}

# WHY a normalizer rather than a substring search on the raw text: the same notice is
# legitimately present in four wrappings -- our generated `//` block, upstream cozo-core's
# own `/* * */` header (which §3.1 forbids removing, so it must satisfy the gate as it
# stands), a `#` block, and an HTML comment -- and it is line-wrapped differently in each.
# Stripping comment punctuation off both edges and collapsing whitespace makes one quoted
# sentence answer for all of them.
# WHY a str.strip character set rather than a regex: the anchored-alternation form
# `^[...]+|[...]+$` backtracks super-linearly on a long run of edge characters, and
# str.strip is linear, allocation-free per edge, and says what it does. The set is
# every character that can wrap the notice in any of the four comment styles.
_COMMENT_EDGE_CHARS = " \t\r\n/*#<!>-"


def render_exhibit_a(suffix: str) -> str:
    """The exact generated notice block for a file of this suffix, newline-terminated."""
    if suffix not in COMMENT_SYNTAX:
        raise LedgerError(
            f"no comment syntax registered for {suffix!r} — add it to COMMENT_SYNTAX before "
            "tracking a file of this type, rather than emitting a notice in a syntax the "
            "file's own language does not accept"
        )
    prefix, closer = COMMENT_SYNTAX[suffix]
    body = (_EXHIBIT_A_BEGIN, *EXHIBIT_A_LINES, _EXHIBIT_A_END)
    rendered = [" ".join(part for part in (prefix, line, closer) if part) for line in body]
    return "\n".join(rendered) + "\n"


def add_generated_notice(text: str, block: str) -> str:
    """Insert `block` at the top, after a shebang line if one is present.

    INVARIANT(#5956): exactly inverted by remove_generated_notice — add then remove
    returns the original bytes. The measurement exclusion depends on that: a block the
    remover cannot take back out is a block that stays in the line count.
    """
    if text.startswith("#!"):
        shebang, _, rest = text.partition("\n")
        return f"{shebang}\n{block}\n{rest}"
    return f"{block}\n{text}"


def remove_generated_notice(text: str, block: str) -> str:
    if block + "\n" in text:
        return text.replace(block + "\n", "", 1)
    return text.replace(block, "", 1)


def strip_generated_notice(text: str) -> str:
    """Remove the generated Exhibit A block, whatever comment syntax it is rendered in.

    INVARIANT(#5956): this is the reason the notices can be added at all. verbatim_pct is
    matched-lines / local-non-blank-lines, so a 5-line header added to a file that is
    otherwise byte-identical to upstream moves its score by 5/len — datalog.pest, which
    IS byte-identical below its header, reads 94.2% rather than 100% for exactly that
    kind of reason. Applied across 142 derived rows, that motion would improve the
    program's central metric by a few points per file while nothing about any file's
    derivation changed: a number moving without the underlying work, which is the failure
    this whole ledger exists to end. A file's figure must therefore be identical with and
    without its notice, which means the notice is not part of what is measured.

    Matches the EXACT rendered block, never a marker scan: an unterminated or reworded
    marker would otherwise strip an unbounded region and silently delete real lines from
    the measurement. A block that has drifted from the rendered form is left in place —
    it then shows up as a verbatim_pct mismatch AND as a check_exhibit_a_notices drift
    error, which is the loud failure, not the silent one.
    """
    for suffix in COMMENT_SYNTAX:
        block = render_exhibit_a(suffix)
        if block in text:
            text = remove_generated_notice(text, block)
    return text


def _normalized_prose(text: str) -> str:
    lines = (line.strip(_COMMENT_EDGE_CHARS) for line in text.splitlines())
    return re.sub(r"\s+", " ", " ".join(line for line in lines if line))


def has_exhibit_a(text: str) -> bool:
    """Whether the file informs its recipient that it is MPL-governed, in any wrapping."""
    return EXHIBIT_A_TEXT in _normalized_prose(text)


def has_generated_notice_marker(text: str) -> bool:
    return EXHIBIT_A_MARKER in text


def ledger_source_path(root: pathlib.Path, rel: str) -> pathlib.Path:
    """Join a ledger row's path onto the crate source root, refusing to escape it.

    SAFETY: a row's `path` is data any pull request can edit, and the tooling that
    consumes it WRITES to the result. A row reading `../../.github/workflows/gate.yml`,
    or naming a symlink pointing out of the tree, would otherwise have a licence header
    written into it by CI. Containment is checked on the RESOLVED path so a symlink
    cannot launder the escape past a textual check.

    WHY here rather than inside sync_exhibit_a: this is where ledger data becomes a
    filesystem path, and validating at the join keeps sync_exhibit_a's contract —
    operate on the path you are given — intact for callers that construct one honestly.

    WHY root is a parameter rather than this module's KRITES_SRC: each consuming script
    re-imports KRITES_SRC into its own namespace and tests monkeypatch that copy, so a
    reference to this module's global would validate against the real tree while the
    caller writes into a temporary one.
    """
    resolved_root = root.resolve()
    joined = (root / rel).resolve()
    if not joined.is_relative_to(resolved_root):
        raise LedgerError(
            f"ledger row path {rel!r} resolves to {joined}, outside {resolved_root}. "
            "A row must name a file inside the crate's own source tree."
        )
    return joined


def sync_exhibit_a(path: pathlib.Path, status: str) -> str | None:
    """Bring one source file's generated notice into line with its ledger status.

    Returns 'added', 'removed', or None when the file was already correct.

    WHY a path rather than a ledger-relative name: KRITES_SRC exists as a module global in
    this library AND is re-imported (and independently monkeypatched in tests) by every
    caller, so resolving the name here would silently read a different tree than the caller
    is working in. Taking the path the caller already holds removes the second name.

    INVARIANT(#5956): a `derived` or `dual` row's file physically carries CozoDB-licensed
    expression, so it gets the notice. A `sovereign` row makes no MPL lineage claim, and
    stamping one on it would assert an obligation the file does not carry — the opposite
    error, and the worse one, since it encumbers aletheia's own work. A `dual` row keeps
    its notice: dual is the retiring derived copy soaking before deletion, not a rewrite.

    WHY adding is conditional on has_exhibit_a rather than on the block's presence: a file
    that retained upstream's own MPL header (datalog.pest) already satisfies §3.1, and
    stacking a second copy of the same sentence on top of it would be redundant noise
    rather than compliance. Removal is conditional on the generated block specifically —
    a notice this tooling did not write is not this tooling's to delete.
    """
    # NOTE: a ledger row naming a file that does not exist is check_completeness's finding.
    # Reporting it a second time here would bury the one error that names the cause.
    if not path.is_file():
        return None
    text = path.read_text(errors="replace")
    block = render_exhibit_a(path.suffix)
    if status == "sovereign":
        if block not in text:
            return None
        path.write_text(remove_generated_notice(text, block))
        return "removed"
    if has_exhibit_a(text):
        return None
    path.write_text(add_generated_notice(text, block))
    return "added"


def nonblank_lines(text: str) -> list[str]:
    # WARNING(#5956): the generated Exhibit A block is removed BEFORE any line is counted,
    # so a file's measurement is identical with and without its notice. See
    # strip_generated_notice for why that exclusion is load-bearing rather than tidy.
    text = strip_generated_notice(text)
    # WHY(aletheia#6656): strip leading AND trailing whitespace, not just the
    # trailing newline splitlines() already drops on its own. A pure
    # re-indentation carries no content change but shifts every line's
    # column position — before this fix, wrapping storage/mem.rs's preserved
    # copy in `mod derived { }` (a formatting-only RETIREMENT-PLAN.md land-dark step)
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
        # (RETIREMENT-PLAN.md §2(c): a completed dual soak carries its upstream_path forward here
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

        # INVARIANT(#6797-followup): method records HOW a row was WRITTEN, distinct from
        # every other field's WHAT/WHERE. Deliberately checked only "if present" rather
        # than setdefault-ed like replaced_upstream_path above: 'unknown' is a legitimate,
        # honest FINAL value for this field (the migrated state of every pre-existing
        # sovereign row), so silently defaulting an absent key to 'unknown' here would make
        # a genuinely-missing field indistinguishable from a genuinely-recorded 'unknown' —
        # check-krites-provenance.py's check_method_recorded needs that distinction to gate
        # on presence. A ledger predating this field (read via --base-ref against
        # pre-migration history) still parses: absence is silently tolerated on READ, and
        # dump_ledger (below) is where absence on WRITE is refused instead.
        if "method" in row:
            method = row["method"]
            if not isinstance(method, str):
                raise LedgerError(f"{path}: method must be a string, got {method!r}")
            if row["status"] != "sovereign":
                if method != "none":
                    raise LedgerError(
                        f"{path}: method is only meaningful on status=sovereign rows (got "
                        f"status={row['status']!r}, method={method!r}) — a derived/dual row's "
                        "authorship is already answered by upstream_path, not a separate claim"
                    )
            elif method not in METHODS:
                raise LedgerError(f"{path}: status=sovereign requires method to be one of {METHODS}, got {method!r}")
        # NOTE: method_evidence is the independently-checkable pointer (PR/issue ref, commit SHA,
        # spec path) that a resolved method claim is backed by something a reader can go
        # verify, not a hand-typed assertion. Present exactly when method is sovereign AND
        # resolved (i.e. not 'unknown') — 'unknown' by construction has nothing to point at,
        # and a non-sovereign row's method is already 'none' with nothing to back.
        if "method_evidence" in row:
            evidence = row["method_evidence"]
            if not isinstance(evidence, str):
                raise LedgerError(f"{path}: method_evidence must be a string, got {evidence!r}")
            method = row.get("method")
            if row["status"] != "sovereign" or method in (None, "unknown"):
                if evidence != "none":
                    raise LedgerError(
                        f"{path}: method_evidence must be 'none' unless a resolved sovereign "
                        f"method is recorded (got status={row['status']!r}, method={method!r}, "
                        f"method_evidence={evidence!r})"
                    )
            elif evidence == "none" or not METHOD_EVIDENCE_PATTERN.match(evidence):
                raise LedgerError(
                    f"{path}: method={method!r} requires a real method_evidence — a PR/issue "
                    "reference ('#NNNN'), a commit SHA (7-40 hex chars), or a spec path "
                    f"('spec:<path>') — got {evidence!r}"
                )
        # NOTE: structure only. Whether the consulted paths are the RIGHT ones for the
        # row's method needs every other row's status, so it lives in consulted_errors()
        # below and runs on the current ledger, not on a --base-ref read: a ledger
        # serialized before this field existed carries no 'consulted' key at all and must
        # still parse here, exactly as 'method' already does.
        if "consulted" in row:
            consulted = row["consulted"]
            if not isinstance(consulted, list) or not all(isinstance(c, str) and c for c in consulted):
                raise LedgerError(
                    f"{path}: consulted must be a list of non-empty ledger paths ([] when none), "
                    f"got {consulted!r}"
                )
            if row["status"] != "sovereign" and consulted:
                raise LedgerError(
                    f"{path}: consulted is only meaningful on status=sovereign rows (got "
                    f"status={row['status']!r}, consulted={consulted!r}) — a derived/dual row makes "
                    "no authorship claim for a reading list to qualify"
                )


def consulted_errors(rows: list[dict]) -> list[str]:
    """The sibling rule: what a clean-room rewrite may read, checked against the ledger.

    WHY this is checkable at all while a bare 'from_spec' was not: 'from_spec' asserts a
    negative about the author's own reading, which nothing observes. Naming the sources
    read moves the checkable part onto their ledger STATUS — a claim of independence is
    refuted the moment a named source is itself derived, without anyone having to
    reconstruct what the author did.

    - from_spec: every consulted path must be a sovereign row. A derived/dual one refutes
      the independence the value claims; the row belongs at from_spec_derived_siblings.
    - from_spec_derived_siblings: consulted must be non-empty AND name at least one
      non-sovereign path. An all-sovereign list is plain from_spec, and accepting the
      weaker value there would make it the lazy default for rows that earned the stronger.
    - every consulted path must be a row in this ledger — a typo resolves to nothing and
      would otherwise read as clean.
    - rewritten_with_source_open (the replaced file itself was read), from_behavioral_oracle,
      attested_original, transliterated, unknown: no constraint on consulted.
    """
    status_by_path = {row["path"]: row["status"] for row in rows}
    errors: list[str] = []
    for row in rows:
        path = row["path"]
        if "consulted" not in row:
            errors.append(
                f"{path}: missing 'consulted' — every ledger row must record which sources its "
                "author read while writing it ([] when none). Regenerate via "
                "scripts/measure-krites-provenance.py, or set explicitly via "
                "scripts/krites-provenance-transition.py --set-method ... --consulted"
            )
            continue
        consulted = row["consulted"]
        method = row.get("method")
        unmapped = [c for c in consulted if c not in status_by_path]
        if unmapped:
            errors.append(
                f"{path}: consulted names path(s) with no PROVENANCE.toml row: "
                + ", ".join(unmapped)
                + " — a consulted path is checked by its ledger status, so one that resolves to "
                "no row is unverifiable rather than clean. Use the row's exact ledger path "
                "(relative to crates/krites/src/)"
            )
            continue
        if method == "from_spec":
            contaminating = [c for c in consulted if status_by_path[c] != "sovereign"]
            if contaminating:
                errors.append(
                    f"{path}: method='from_spec' claims the rewrite drew on no CozoDB-derived "
                    "expression, but consulted names non-sovereign row(s): "
                    + ", ".join(f"{c} (status={status_by_path[c]})" for c in contaminating)
                    + " — record method='from_spec_derived_siblings' instead, which states what "
                    "actually happened. A truthful weaker method always beats a false stronger one"
                )
        elif method == "from_spec_derived_siblings":
            if not consulted:
                errors.append(
                    f"{path}: method='from_spec_derived_siblings' names an exposure to derived "
                    "siblings, so consulted must list them — an empty list records no exposure and "
                    "belongs at method='from_spec'"
                )
            elif all(status_by_path[c] == "sovereign" for c in consulted):
                errors.append(
                    f"{path}: method='from_spec_derived_siblings' consulted only sovereign row(s) "
                    + ", ".join(consulted)
                    + " — that is plain method='from_spec'. Recording the weaker value for a row "
                    "that earned the stronger one makes it the lazy default and drains both of "
                    "meaning"
                )
    return errors


def parse_ledger(text: str) -> tuple[dict, list[dict]]:
    data = tomllib.loads(text)
    meta = data.get("meta", {})
    rows = data.get("file", [])
    validate_rows(rows)
    return meta, rows


def _toml_str(value: str) -> str:
    escaped = value.replace("\\", "\\\\").replace('"', '\\"')
    return f'"{escaped}"'


def _toml_str_list(values: list[str]) -> str:
    return "[" + ", ".join(_toml_str(v) for v in values) + "]"


def dump_ledger(meta: dict, rows: list[dict]) -> str:
    validate_rows(rows)
    # SAFETY(#6797-followup): unlike validate_rows' read path, a WRITE requires method/
    # method_evidence present on every row — 'generated, do not hand-edit rows' means the
    # only legitimate way a row reaches this function is through measure-krites-provenance.py
    # or krites-provenance-transition.py, both of which always set both fields. A row
    # missing either here is a caller bug or a hand-edit that stripped the field; raising a
    # clear LedgerError beats a bare KeyError from the f-string access below.
    missing = sorted(
        row["path"]
        for row in rows
        if "method" not in row or "method_evidence" not in row or "consulted" not in row
    )
    if missing:
        raise LedgerError(
            "dump_ledger requires 'method', 'method_evidence' and 'consulted' set on every row "
            "before writing (regenerate via measure-krites-provenance.py, or set explicitly via "
            "krites-provenance-transition.py): " + ", ".join(missing)
        )
    # SAFETY(#6879): the sibling rule is enforced on the WRITE path too, not only by
    # check-krites-provenance.py. A row that violates it is unwritable, so no tool can
    # produce a ledger that only fails later in CI — which is where the previous
    # generation of this scheme kept landing (a value written by fiat, caught a wave
    # later, if at all).
    sibling_errors = consulted_errors(rows)
    if sibling_errors:
        raise LedgerError("; ".join(sibling_errors))
    lines = [
        "# NOTE: generated by scripts/measure-krites-provenance.py — do not hand-edit rows.",
        "# NOTE: soak_expires_at_commit_count = 0 means the file is not in dual",
        "# NOTE: (land-dark/soak) state. A nonzero value is an ABSOLUTE target: the",
        "# NOTE: count of `git rev-list --count origin/main` at or past which CI",
        "# NOTE: fails the build (RETIREMENT-PLAN.md §2 expiry gate) — not a duration and not",
        "# NOTE: relative to when the row entered dual. Extend by explicit ledger edit.",
        "# NOTE: status = derived | sovereign | dual (RETIREMENT-PLAN.md §2, §3 wave 0.1); the",
        "# NOTE: only legal transition out of derived is derived -> dual -> sovereign,",
        "# NOTE: CI-enforced (check_status_sequence) — a direct derived -> sovereign",
        "# NOTE: jump is rejected regardless of verbatim_pct.",
        "# NOTE: replaced_upstream_path is 'none' except on a sovereign row that still",
        "# NOTE: has something to measure against: a completed dual soak (RETIREMENT-PLAN.md §2(c))",
        "# NOTE: retains its upstream_path here instead of losing it, or a from-scratch",
        "# NOTE: rewrite with a natural predecessor gets one from",
        "# NOTE: measure-krites-provenance.py's SOVEREIGN_VERIFY_MAP. verbatim_pct is",
        "# NOTE: then measured against THIS field, not upstream_path (which stays 'none'",
        "# NOTE: on every sovereign row — no MPL lineage claim). CI recomputes and",
        "# NOTE: fails on drift (check_verbatim_recompute), same as derived/dual.",
        "# NOTE: method records HOW a sovereign row was written ('none' on derived/dual —",
        "# NOTE: answered already by upstream_path): from_spec | from_spec_derived_siblings |",
        "# NOTE: from_behavioral_oracle | rewritten_with_source_open | transliterated |",
        "# NOTE: attested_original | unknown.",
        "# NOTE: 'unknown' is the honest default for every row with no record; CI fails on a",
        "# NOTE: missing method or a sovereign row carrying 'transliterated' (check_method_",
        "# NOTE: recorded). method_evidence is 'none' unless method is resolved and non-",
        "# NOTE: 'unknown', in which case it is a PR/issue ref, a commit SHA, or a spec path —",
        "# NOTE: never a hand-typed justification. Clear 'unknown' only via",
        "# NOTE: krites-provenance-transition.py --set-method, never by hand-editing this row.",
        "# NOTE: consulted lists the source paths the author read while writing the file",
        "# NOTE: ([] when none; always [] off sovereign). from_spec requires every one of",
        "# NOTE: them to be a sovereign row; from_spec_derived_siblings requires at least one",
        "# NOTE: that is not; a path with no row here fails either way (CI: consulted_errors).",
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
        lines.append(f"method = {_toml_str(row['method'])}")
        lines.append(f"method_evidence = {_toml_str(row['method_evidence'])}")
        lines.append(f"consulted = {_toml_str_list(row['consulted'])}")
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
        "to measure against — a completed `dual` soak (RETIREMENT-PLAN.md §2(c)), or a from-scratch rewrite "
        "with a natural predecessor — the ledger retains that predecessor as "
        "`replaced_upstream_path` (shown below as \"cf. `path`\") and keeps measuring against it. "
        "`upstream_path` itself stays `none` on every `sovereign` row either way: this is not an "
        "MPL lineage claim, only a retained comparison the anti-backsliding gate keeps honest. A "
        "row with no predecessor at all (`replaced_upstream_path` also `none`) has nothing to "
        "measure and its `verbatim_pct` is genuinely 0.0."
    )
    unknown_sovereign = [r for r in sovereign if r.get("method", "none") == "unknown"]
    resolved_sovereign = [r for r in sovereign if r.get("method", "none") not in ("none", "unknown")]

    lines.append("")
    lines.append(f"- Upstream: <{meta['upstream_repo']}>, pinned at `{meta['upstream_ref']}`")
    lines.append(f"- {len(rows)} files under `src/`: {len(derived)} derived, {len(sovereign)} sovereign, {len(dual)} dual")
    lines.append(
        f"- Mean verbatim match across the {len(derived)} derived files: {mean_pct}% "
        "(unweighted average of the per-file `verbatim_pct` column below)"
    )
    lines.append(
        f"- Of the {len(sovereign)} sovereign files, **{len(unknown_sovereign)} carry "
        f"`method = \"unknown\"`** (no record of how they were written) and {len(resolved_sovereign)} "
        "carry a resolved, evidence-backed method — see \"Authorship method\" below."
    )
    lines.append("")
    lines.append("| File | Upstream | Verbatim | Status | Method |")
    lines.append("|---|---|---:|---|---|")
    for row in rows:
        if row["upstream_path"] != "none":
            upstream_cell = f"`{row['upstream_path']}`"
        elif row.get("replaced_upstream_path", "none") != "none":
            upstream_cell = f"cf. `{row['replaced_upstream_path']}`"
        else:
            upstream_cell = "—"
        method = row.get("method", "none")
        evidence = row.get("method_evidence", "none")
        if method == "none":
            method_cell = "—"
        elif evidence != "none":
            method_cell = f"{method} (cf. `{evidence}`)"
        else:
            method_cell = method
        lines.append(
            f"| `src/{row['path']}` | {upstream_cell} | {row['verbatim_pct']:.1f}% | {row['status']} | {method_cell} |"
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
    lines.append("## Authorship method")
    lines.append("")
    lines.append(
        "`status = sovereign` is a claim about **authorship** — that a file was written by "
        "aletheia rather than derived from CozoDB. Every other field in the ledger records what a "
        "file's text looks like (`verbatim_pct`) or where it came from (`upstream_path`, "
        "`replaced_upstream_path`); none of them record *how* a sovereign file was written, which "
        "is what the claim actually rests on. `verbatim_pct` cannot substitute: a confirmed "
        "transliteration (`fixed_rule/algos/dfs_native.rs`, aletheia#6656) measured 26.6% against "
        "its source while a confirmed independent rewrite (`degree_centrality_native.rs`) measured "
        "*higher*, at 32.1% — the metric ranks a disguised copy above a genuine rewrite. `method` "
        "is the field that answers the question textual similarity cannot."
    )
    lines.append("")
    lines.append("| Value | Meaning |")
    lines.append("|---|---|")
    lines.append("| `from_spec` | written against a written specification/paper, without reference to the derived source |")
    lines.append("| `from_spec_derived_siblings` | written against a specification, but derived siblings in this crate were read for local convention |")
    lines.append("| `from_behavioral_oracle` | written against observed behaviour/tests of the derived code, source not read |")
    lines.append("| `rewritten_with_source_open` | the derived file was consulted while writing |")
    lines.append("| `transliterated` | a **finding** value — confirms the file is a disguised copy; never a legitimate state for a sovereign row (see \"Anti-backsliding\" below) |")
    lines.append("| `attested_original` | no predecessor existed; this is aletheia's own new code |")
    lines.append("| `unknown` | no record exists |")
    lines.append("")
    lines.append(
        f"**{len(unknown_sovereign)} of {len(sovereign)}** sovereign rows carry `method = \"unknown\"` "
        "today. They were migrated there deliberately, not defaulted to a clean value: no evidence "
        "existed to support one, and a clean-by-default value would repeat this scheme's own "
        "history — `krites-provenance-transition.py` once hardcoded `verbatim_pct = 0.0` on every "
        "`dual` → `sovereign` transition, and 17 files that entered `sovereign` that way later "
        "re-measured at 18–41%. `unknown` is not itself a failure; it is the honest state until "
        "cleared with evidence."
    )
    lines.append("")
    lines.append(
        "`from_spec` and `from_spec_derived_siblings` differ only in what the author read for "
        "local convention, and the ledger records that as a `consulted` list per row — the source "
        "paths read while writing, `[]` when none. It exists because most of this crate is "
        "derived, so the sibling that best demonstrates a convention is usually the sibling doing "
        "the same job. Mechanical conventions (error type, lint attributes, module layout, naming) "
        "may come from any sibling; the shape of the same algorithm may only come from a "
        "`sovereign` one. CI reads each consulted path's own status: a `from_spec` row that "
        "consulted a `derived` sibling fails, and so does a `from_spec_derived_siblings` row whose "
        "list is empty or entirely `sovereign`. What the check cannot reach is the list's "
        "completeness — nothing observes what an author opened, so an omitted path reads exactly "
        "like a path never read."
    )
    consulting = [r for r in rows if r.get("consulted")]
    if consulting:
        lines.append("")
        lines.append("| File | Consulted while writing |")
        lines.append("|---|---|")
        for r in consulting:
            read = ", ".join(f"`src/{c}`" for c in r["consulted"])
            lines.append(f"| `src/{r['path']}` | {read} |")
    lines.append("")
    lines.append(
        f"**{len(resolved_sovereign)}** carry a resolved method backed by a `method_evidence` "
        "pointer — a PR/issue reference, a commit SHA, or a spec path, always independently "
        "checkable, never a hand-typed justification. `unknown` is cleared only through "
        "`scripts/krites-provenance-transition.py --set-method <value> --evidence <pointer>` "
        "(plus `--consulted <paths>`, which the two `from_spec` values require), never by "
        "hand-editing a row."
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
    lines.append("")
    lines.append(
        "One more clause closes a gap the recompute check could not reach on its own "
        "(aletheia#6797): a `sovereign` row with `replaced_upstream_path == 'none'` had "
        "nothing for the recompute check to run against, and nothing verified that "
        "`'none'` meant \"genuinely nothing to compare against\" rather than \"nobody "
        "mapped it yet\" — the two looked identical, which is how the crate's "
        "highest-risk rewrite (`runtime/hnsw_sovereign/*`, 2912 lines) sat completely "
        "unmeasured while smaller rewrites beside it were all measured. CI now fails "
        "the build if any such row is absent from `krites_provenance_lib.py`'s "
        "`NO_PREDECESSOR_REASONS` — an explicit, individually-verified declaration of "
        "why the row genuinely has nothing to compare against — or if that map holds "
        "a stale entry for a row that no longer qualifies."
    )
    lines.append("")
    lines.append(
        "A third clause closes the gap none of the above reach: every check above verifies "
        "*what a file's text looks like* against *where it came from* — none of them record "
        "*how it was written*, which is what `status = sovereign` actually claims. CI now fails "
        "the build if any ledger row is missing `method`, or if a `sovereign` row carries "
        "`method = \"transliterated\"` (`check_method_recorded`). Recording a transliteration "
        "finding is legitimate — `fts/tokenizer/stop_word_filter/sovereign/mod.rs` carries it "
        "today, a statement-for-statement match against its replaced upstream at 15.5% "
        "(aletheia#6656) — leaving the row `sovereign` with it is not: the row must be "
        "rewritten independently or reclassified before the gate passes again."
    )
    lines.append("")
    lines.append(
        "A fourth clause gates the notices themselves. Every `derived`/`dual` file must carry "
        "the MPL Exhibit A notice, and no `sovereign` file may — CI fails the build either way "
        "(`check_exhibit_a_notices`). The enumeration above is what satisfies §3.1; the per-file "
        "notice covers what an enumeration cannot follow, a single file copied out of this tree "
        "on its own. The notice is rendered from this ledger by "
        "`scripts/measure-krites-provenance.py`, never hand-written, which is why a refactor can "
        "no longer quietly strip one the way it stripped `datalog.pest`'s. A file that retained "
        "upstream's own header satisfies the gate as it stands, since §3.1 forbids removing it "
        "and a second copy of the same sentence is not compliance."
    )
    lines.append("")
    lines.append(
        "The generated block is excluded from `verbatim_pct` and from the drift metric alike. "
        "`verbatim_pct` is matched lines over the file's own non-blank lines, so a five-line "
        "header on every derived file would move the figure on all of them at once — the mean "
        "across the derived set falls from 44.3% to 42.5%, and `fts/README.md` reads 44.4% "
        "instead of 100.0%, entirely on licence boilerplate. Every figure in the table above is "
        "therefore identical with and without its file's notice, which is the only reading under "
        "which the numbers still mean what they say."
    )
    return "\n".join(lines).rstrip("\n") + "\n"
