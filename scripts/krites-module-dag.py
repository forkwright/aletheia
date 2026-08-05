#!/usr/bin/env python3
"""
Mechanically derives the crates/krites subsystem import DAG from every
`use crate::…` statement under crates/krites/src/, for diffing against the
hand-drawn parallelism map in deliverables/krites-replacement/PLAN.md §7.

A node is a top-level directory under src/ (data, fixed_rule, fts, parse,
query, runtime, storage) or "root" for the eight files that sit directly in
src/ (lib.rs, async_surface.rs, counterfactual.rs, counterfactual_tests.rs,
error.rs, hot_reload.rs, query_cache.rs, utils.rs). An edge A -> B means some
file under subsystem A carries a `use crate::B::…` (or a bare `use crate::X`
where X is a root re-export, target "root"). Self-loops (A -> A) are dropped.

Parser: brace-depth-aware statement extraction (handles multi-line grouped
imports, nested groups, `pub`/`pub(crate)` prefixes, `as` aliases, `self`).
Comment-guarded: a `use crate::` occurrence preceded by `//` on the same
source line is skipped. No `cargo`/`syn` dependency — stdlib only, so it runs
identically in any Python 3.9+.

Usage:
    python3 scripts/krites-module-dag.py [--src crates/krites/src]
                                          [--format json|markdown]
                                          [--out FILE] [--check FILE]

--check FILE fails (exit 1, prints a diff) when the emitted JSON differs
from FILE — FILE is a checked-in snapshot this script is the sole producer
of; the SSOT drift gate is "does this script's output still match".
"""
from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys

KNOWN_SUBSYSTEMS = frozenset(
    {"data", "fixed_rule", "fts", "parse", "query", "runtime", "storage"}
)
ROOT = "root"

# WHY: PLAN.md §0.2 — reproducing these without special-casing is the parser's
# own correctness check. If either is absent from the output, the parser is
# wrong, not the plan.
REQUIRED_EDGES = {
    ("runtime", "fts"): "runtime/minhash_lsh.rs:25-26 imports crate::fts::TokenizerConfig, crate::fts::tokenizer::TextAnalyzer",
    ("parse", "fts"): "parse/sys/mod.rs imports crate::fts::TokenizerConfig for MinHashLshConfig",
}

USE_START_RE = re.compile(
    r"(?m)^[ \t]*(?P<pub>pub(?:\([^)]*\))?[ \t]+)?use[ \t]+crate::"
)

TEST_PATH_RE = re.compile(r"(^|/)tests?(/|$)")


class ParseError(ValueError):
    pass


def line_of(text: str, pos: int) -> int:
    return text.count("\n", 0, pos) + 1


def is_line_commented(text: str, match_start: int) -> bool:
    line_start = text.rfind("\n", 0, match_start) + 1
    prefix = text[line_start:match_start]
    return "//" in prefix


def find_statement_end(text: str, start: int) -> int:
    """Index of the terminating ';' at brace depth 0, scanning from `start`."""
    depth = 0
    i = start
    n = len(text)
    while i < n:
        c = text[i]
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth < 0:
                raise ParseError(f"unbalanced '}}' at offset {i}")
        elif c == ";" and depth == 0:
            return i
        i += 1
    raise ParseError(f"unterminated use statement starting at offset {start}")


def split_top_level_commas(s: str) -> list[str]:
    parts: list[str] = []
    depth = 0
    cur: list[str] = []
    for c in s:
        if c == "{":
            depth += 1
            cur.append(c)
        elif c == "}":
            depth -= 1
            cur.append(c)
        elif c == "," and depth == 0:
            parts.append("".join(cur))
            cur = []
        else:
            cur.append(c)
    if cur:
        parts.append("".join(cur))
    return [p.strip() for p in parts if p.strip()]


def find_top_level_brace_span(s: str) -> tuple[int, int] | None:
    depth = 0
    open_idx = None
    for i, c in enumerate(s):
        if c == "{":
            if depth == 0:
                open_idx = i
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0 and open_idx is not None:
                return open_idx, i
    return None


def collect_full_paths(rest: str) -> list[str]:
    """Full `::`-joined module path of every leaf item a `crate::<rest>` use
    resolves to (prefix expansion through nested groups; `self` resolves to
    the enclosing prefix; `*` and empty prefixes are dropped)."""
    rest = rest.strip()
    if not rest:
        return []
    span = find_top_level_brace_span(rest)
    if span is None:
        path = rest.split(" as ")[0].strip()
        if not path or path == "self" or path == "*":
            return []
        return [path]
    open_idx, close_idx = span
    prefix = rest[:open_idx]
    if prefix.endswith("::"):
        prefix = prefix[:-2]
    prefix = prefix.strip()
    inner = rest[open_idx + 1 : close_idx]
    results: list[str] = []
    for item in split_top_level_commas(inner):
        item = item.strip()
        if item == "self":
            if prefix:
                results.append(prefix)
            continue
        for sub in collect_full_paths(item):
            results.append(f"{prefix}::{sub}" if prefix else sub)
    return results


def collect_first_segments(rest: str) -> list[str]:
    """First path segment of every leaf item a `crate::<rest>` use resolves to."""
    return [p.split("::", 1)[0] for p in collect_full_paths(rest) if p.split("::", 1)[0]]


def subsystem_of(rel_path: pathlib.Path) -> str:
    parts = rel_path.parts
    if len(parts) == 1:
        return ROOT
    top = parts[0]
    if top not in KNOWN_SUBSYSTEMS:
        raise ParseError(f"unknown subsystem directory {top!r} for {rel_path}")
    return top


def target_bucket(first_segment: str) -> str:
    return first_segment if first_segment in KNOWN_SUBSYSTEMS else ROOT


def is_test_path(rel_path: pathlib.Path) -> bool:
    posix = rel_path.as_posix()
    if TEST_PATH_RE.search(posix):
        return True
    name = rel_path.name
    return name.endswith("_tests.rs") or name == "tests.rs"


def extract_imports(src_root: pathlib.Path, path: pathlib.Path) -> list[dict]:
    """Every `crate::…` leaf import in `path`, with NO subsystem-level
    filtering — same-subsystem-but-different-submodule crossings (e.g.
    `fts::tokenizer::stop_word_filter` -> `fts::error`) must survive this
    step for the finer wave-scope grouping to see them; only the coarse
    subsystem graph (parse_file) collapses those to self-loops."""
    text = path.read_text(encoding="utf-8")
    rel = path.relative_to(src_root)
    source_subsystem = subsystem_of(rel)
    test_file = is_test_path(rel)
    imports: list[dict] = []
    for m in USE_START_RE.finditer(text):
        if is_line_commented(text, m.start()):
            continue
        crate_kw_end = m.end()
        stmt_end = find_statement_end(text, crate_kw_end)
        rest = text[crate_kw_end:stmt_end]
        line_no = line_of(text, m.start())
        for full_path in collect_full_paths(rest):
            seg = full_path.split("::", 1)[0]
            if not seg:
                continue
            imports.append(
                {
                    "source_subsystem": source_subsystem,
                    "target_subsystem": target_bucket(seg),
                    "target_path": full_path,
                    "file": rel.as_posix(),
                    "line": line_no,
                    "test": test_file,
                }
            )
    return imports


def parse_file(src_root: pathlib.Path, path: pathlib.Path) -> list[dict]:
    """Coarse subsystem-level edges: same-subsystem imports are self-loops
    and dropped here. Use extract_imports directly for finer analysis."""
    edges = []
    for imp in extract_imports(src_root, path):
        if imp["target_subsystem"] == imp["source_subsystem"]:
            continue
        edges.append(
            {
                "from": imp["source_subsystem"],
                "to": imp["target_subsystem"],
                "target_path": imp["target_path"],
                "file": imp["file"],
                "line": imp["line"],
                "test": imp["test"],
            }
        )
    return edges


# --- Wave-scope groups -----------------------------------------------------
# NOTE: PLAN.md §7 "CONCURRENT FROM DAY 1" streams, resolved to module-path
# prefixes (filesystem path with '/' -> '::', minus the .rs extension and
# any trailing 'mod'). Citations are PLAN.md line numbers as read at the
# time this table was written; re-verify if PLAN.md's file citations move.
DAY1_GROUPS: dict[str, dict] = {
    "W1b_storage_mem_temp": {
        "include": {"storage::mem", "storage::temp"},
        "exclude": set(),
        "citation": "PLAN.md:108 storage/{mem,temp}.rs",
    },
    "W2a_fold_table": {
        "include": {"fts::tokenizer::ascii_folding_filter"},
        "exclude": set(),
        "citation": "PLAN.md:109 ascii-folding table",
    },
    "W2b_stopwords": {
        "include": {"fts::tokenizer::stop_word_filter"},
        "exclude": set(),
        "citation": "PLAN.md:110 stopword lists",
    },
    "W3prime_algos_19": {
        "include": {"fixed_rule::algos"},
        "exclude": {"fixed_rule::algos::louvain", "fixed_rule::algos::pagerank"},
        "citation": "PLAN.md:113,206 19 of 22 graph algorithms (excl. PageRank/Louvain)",
    },
    "W4prime_hnsw": {
        "include": {"runtime::hnsw"},
        "exclude": set(),
        "citation": "PLAN.md:112,208 HNSW",
    },
    "W5prime_value_model": {
        "include": {
            "data::value",
            "data::memcmp",
            "data::tuple",
            "data::symb",
        },
        "exclude": set(),
        "citation": "PLAN.md:114,209 data/{value,memcmp,tuple,symb}.rs",
    },
}

# NOTE: the wave-3/wave-5 "serialised behind other waves" counterparts, kept
# separate so the analysis can show what each Day-1 group is serialised
# ahead of, not just whether Day-1 group pairs collide with each other.
FOLLOWUP_GROUPS: dict[str, dict] = {
    "W3_storage_trait_fts_index": {
        "include": {"storage", "fts"},
        "exclude": {
            "storage::mem",
            "storage::temp",
            "fts::tokenizer::ascii_folding_filter",
            "fts::tokenizer::stop_word_filter",
        },
        "citation": "PLAN.md:111,222 storage trait + BM25/FTS index, after W1b+W2a/W2b",
    },
    "W5_live3_algos": {
        "include": {
            "fixed_rule::algos::louvain",
            "fixed_rule::algos::pagerank",
            "fixed_rule::utilities::rrf",
        },
        "exclude": set(),
        "citation": "PLAN.md:113,223 PageRank/Louvain/RRF, after the 19",
    },
}

ALL_GROUPS: dict[str, dict] = {**DAY1_GROUPS, **FOLLOWUP_GROUPS}


def file_module_path(rel_path: pathlib.Path) -> str:
    parts = list(rel_path.parts)
    stem = parts[-1]
    if stem.endswith(".rs"):
        stem = stem[:-3]
    if stem in ("mod", "lib"):
        parts = parts[:-1]
    else:
        parts = parts[:-1] + [stem]
    return "::".join(parts)


def matches_group(module_path: str, spec: dict) -> bool:
    def prefix_hit(prefixes: set[str]) -> bool:
        for p in prefixes:
            if module_path == p or module_path.startswith(p + "::"):
                return True
        return False

    if not prefix_hit(spec["include"]):
        return False
    if prefix_hit(spec["exclude"]):
        return False
    return True


def groups_for_module_path(module_path: str, groups: dict[str, dict]) -> list[str]:
    return sorted(name for name, spec in groups.items() if matches_group(module_path, spec))


def build_graph(src_root: pathlib.Path) -> dict:
    files = sorted(src_root.rglob("*.rs"))
    all_edges: list[dict] = []
    for f in files:
        all_edges.extend(parse_file(src_root, f))

    all_edges.sort(key=lambda e: (e["from"], e["to"], e["file"], e["line"]))

    pair_evidence: dict[tuple[str, str], list[dict]] = {}
    for e in all_edges:
        pair_evidence.setdefault((e["from"], e["to"]), []).append(e)

    nodes = sorted(KNOWN_SUBSYSTEMS | {ROOT})
    pairs = sorted(pair_evidence.keys())

    edges_out = []
    for src, dst in pairs:
        evidence = pair_evidence[(src, dst)]
        prod_evidence = [e for e in evidence if not e["test"]]
        edges_out.append(
            {
                "from": src,
                "to": dst,
                "count": len(evidence),
                "production_count": len(prod_evidence),
                "test_only": len(prod_evidence) == 0,
                "first_evidence": f"{evidence[0]['file']}:{evidence[0]['line']}",
                "first_production_evidence": (
                    f"{prod_evidence[0]['file']}:{prod_evidence[0]['line']}"
                    if prod_evidence
                    else None
                ),
            }
        )

    missing_required = [
        {"edge": list(pair), "description": desc}
        for pair, desc in REQUIRED_EDGES.items()
        if pair not in pair_evidence
    ]

    return {
        "nodes": nodes,
        "edges": edges_out,
        "file_count": len(files),
        "statement_count": sum(1 for _ in USE_START_RE.finditer("".join(
            f.read_text(encoding="utf-8") for f in files
        ))),
        "required_edges_present": [list(p) for p in REQUIRED_EDGES if p in pair_evidence],
        "required_edges_missing": missing_required,
    }


def build_wave_scope_report(src_root: pathlib.Path) -> dict:
    files = sorted(src_root.rglob("*.rs"))
    raw_imports: list[dict] = []
    for f in files:
        raw_imports.extend(extract_imports(src_root, f))

    crossings: dict[tuple[str, str], list[dict]] = {}
    for e in raw_imports:
        src_mod = file_module_path(pathlib.Path(e["file"]))
        src_groups = groups_for_module_path(src_mod, ALL_GROUPS)
        # NOTE: target_path is already rooted at `crate::` (e.g.
        # "runtime::hnsw::types::RelationHandle") — no subsystem prefix to add.
        tgt_groups = groups_for_module_path(e["target_path"], ALL_GROUPS)
        for sg in src_groups:
            for tg in tgt_groups:
                if sg == tg:
                    continue
                key = (sg, tg)
                crossings.setdefault(key, []).append(
                    {"file": e["file"], "line": e["line"], "target_path": e["target_path"]}
                )

    pairs = sorted(crossings.keys())
    edges_out = [
        {
            "from": sg,
            "to": tg,
            "count": len(ev),
            "evidence": sorted(
                (f"{x['file']}:{x['line']} -> crate::{x['target_path']}" for x in ev)
            ),
        }
        for (sg, tg), ev in ((p, crossings[p]) for p in pairs)
    ]

    day1_names = sorted(DAY1_GROUPS.keys())
    independent_pairs = []
    coupled_pairs = []
    for i, a in enumerate(day1_names):
        for b in day1_names[i + 1 :]:
            fwd = crossings.get((a, b), [])
            bwd = crossings.get((b, a), [])
            if fwd or bwd:
                coupled_pairs.append(
                    {
                        "pair": [a, b],
                        f"{a}_imports_{b}": len(fwd),
                        f"{b}_imports_{a}": len(bwd),
                    }
                )
            else:
                independent_pairs.append([a, b])

    return {
        "groups": {name: spec["citation"] for name, spec in ALL_GROUPS.items()},
        "cross_group_edges": edges_out,
        "day1_pairwise_independent": independent_pairs,
        "day1_pairwise_coupled": coupled_pairs,
    }


def find_cycles(edges: list[dict]) -> list[list[str]]:
    adj: dict[str, list[str]] = {}
    for e in edges:
        adj.setdefault(e["from"], []).append(e["to"])
    for n in adj:
        adj[n].sort()

    WHITE, GRAY, BLACK = 0, 1, 2
    color: dict[str, int] = {}
    cycles: list[list[str]] = []

    def dfs(node: str, stack: list[str]) -> None:
        color[node] = GRAY
        stack.append(node)
        for nxt in adj.get(node, []):
            if color.get(nxt, WHITE) == WHITE:
                dfs(nxt, stack)
            elif color.get(nxt) == GRAY:
                idx = stack.index(nxt)
                cycles.append(stack[idx:] + [nxt])
        stack.pop()
        color[node] = BLACK

    for n in sorted(adj.keys()):
        if color.get(n, WHITE) == WHITE:
            dfs(n, [])

    seen = set()
    unique = []
    for c in cycles:
        key = tuple(sorted(set(c)))
        if key not in seen:
            seen.add(key)
            unique.append(c)
    return sorted(unique)


def render_markdown(graph: dict) -> str:
    lines = []
    lines.append("# krites subsystem import DAG (mechanically derived)")
    lines.append("")
    lines.append(
        f"Source: {graph['file_count']} `.rs` files, "
        f"{graph['statement_count']} `use crate::` statements under "
        "`crates/krites/src/`. Generated by `scripts/krites-module-dag.py`; "
        "do not hand-edit."
    )
    lines.append("")
    lines.append("## Nodes")
    lines.append("")
    for n in graph["nodes"]:
        lines.append(f"- `{n}`")
    lines.append("")
    lines.append("## Edges (A -> B: A imports from B)")
    lines.append("")
    lines.append("| from | to | count | production count | test-only | first production evidence |")
    lines.append("|---|---|---|---|---|---|")
    for e in graph["edges"]:
        lines.append(
            f"| `{e['from']}` | `{e['to']}` | {e['count']} | {e['production_count']} | "
            f"{'yes' if e['test_only'] else 'no'} | "
            f"{e['first_production_evidence'] or '(none — test-file-only)'} |"
        )
    lines.append("")
    cycles = find_cycles(graph["edges"])
    lines.append("## Cycles")
    lines.append("")
    if cycles:
        for c in cycles:
            lines.append(f"- {' -> '.join(c)}")
    else:
        lines.append("None — the subsystem graph is a genuine DAG.")
    lines.append("")
    return "\n".join(lines) + "\n"


def canonical_json(graph: dict) -> str:
    return json.dumps(graph, indent=2, sort_keys=False) + "\n"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--src", default="crates/krites/src", type=pathlib.Path)
    ap.add_argument("--format", choices=["json", "markdown"], default="json")
    ap.add_argument("--out", type=pathlib.Path)
    ap.add_argument("--check", type=pathlib.Path)
    ap.add_argument(
        "--wave-scope",
        action="store_true",
        help="emit the §7 Day-1-stream cross-group report instead of the subsystem DAG",
    )
    args = ap.parse_args()

    src_root = args.src.resolve()
    if not src_root.is_dir():
        print(f"error: {src_root} is not a directory", file=sys.stderr)
        return 2

    if args.wave_scope:
        report = build_wave_scope_report(src_root)
        output = json.dumps(report, indent=2) + "\n"
        if args.check is not None:
            expected = args.check.read_text(encoding="utf-8")
            if expected != output:
                print(f"error: output drifted from {args.check}", file=sys.stderr)
                return 1
            return 0
        if args.out is not None:
            args.out.write_text(output, encoding="utf-8")
        else:
            sys.stdout.write(output)
        return 0

    graph = build_graph(src_root)
    graph["cycles"] = find_cycles(graph["edges"])

    if graph["required_edges_missing"]:
        for m in graph["required_edges_missing"]:
            print(
                f"error: required edge {tuple(m['edge'])} not found "
                f"({m['description']})",
                file=sys.stderr,
            )
        return 1

    if args.format == "json":
        output = canonical_json(graph)
    else:
        output = render_markdown(graph)

    if args.check is not None:
        expected = args.check.read_text(encoding="utf-8")
        if expected != output:
            print(f"error: output drifted from {args.check}", file=sys.stderr)
            return 1
        return 0

    if args.out is not None:
        args.out.write_text(output, encoding="utf-8")
    else:
        sys.stdout.write(output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
