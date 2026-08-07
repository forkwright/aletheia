#!/usr/bin/env python3
"""Judge and score the recall golden-set harness's retrieval output.

Two subcommands over the same two files:

  judge  -- walk the judging bundle (crates/aletheia/src/bin/golden_set_harness.rs
            output) one query at a time, collect which retrieved ranks are
            relevant via ONE line of operator input per query, and persist
            labels incrementally (resumable).
  score  -- compute Recall@K and MRR (overall and per query-class) from the
            judging bundle plus the accumulated labels, in a shape a CI
            regression gate could assert later (--gate mode).

Human judging is load-bearing, not optional: an LLM scoring its own memory
system's recall is circular. This script never calls a model; it only
displays retrieved text and records a human decision.

The judging bundle's retrieved content comes from the `shared` episteme
cohort only (see golden_set_harness.rs's two hard constraints) -- this
script does not open, request, or display anything from `psyche`.
"""

from __future__ import annotations

import argparse
import json
import sys
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


# ── Data loading ──────────────────────────────────────────────────────────


def load_bundle(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as fh:
        return json.load(fh)


def load_labels(path: Path) -> dict[str, dict[str, Any]]:
    """Load labels keyed by query_id. Missing file -> empty (nothing judged yet)."""
    if not path.exists():
        return {}
    labels: dict[str, dict[str, Any]] = {}
    with path.open("r", encoding="utf-8") as fh:
        for line_no, line in enumerate(fh, start=1):
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError as e:
                raise SystemExit(f"{path}:{line_no}: invalid JSON: {e}") from e
            labels[row["query_id"]] = row
    return labels


def save_labels(path: Path, labels: dict[str, dict[str, Any]], query_order: list[str]) -> None:
    """Rewrite the whole labels file in original query order.

    WHY rewrite-whole-file rather than append: the golden set is <=~100
    queries, so the I/O cost is negligible, and rewriting lets a judge
    correct an earlier answer by re-running `judge` on one query id without
    producing duplicate rows.
    """
    ordered = [labels[qid] for qid in query_order if qid in labels]
    # Any label for a query_id no longer present in the bundle (e.g. the
    # query set was edited) is preserved at the end rather than silently
    # dropped -- judging effort is not cheap to lose.
    ordered.extend(row for qid, row in labels.items() if qid not in query_order)
    tmp = path.with_suffix(path.suffix + ".tmp")
    with tmp.open("w", encoding="utf-8") as fh:
        for row in ordered:
            fh.write(json.dumps(row, ensure_ascii=False) + "\n")
    tmp.replace(path)


def now_iso() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


# ── judge ─────────────────────────────────────────────────────────────────


def truncate(text: str, width: int) -> str:
    text = " ".join(text.split())
    if len(text) <= width:
        return text
    return text[: width - 1] + "…"


def cmd_judge(args: argparse.Namespace) -> int:
    bundle = load_bundle(args.bundle)
    labels = load_labels(args.labels)
    queries: list[dict[str, Any]] = bundle["queries"]
    query_order = [q["query_id"] for q in queries]

    if args.class_filter:
        queries = [q for q in queries if q["class"] == args.class_filter]

    pending = [q for q in queries if args.redo or q["query_id"] not in labels]
    if not pending:
        print(f"Nothing to judge ({len(labels)}/{len(query_order)} already labelled). "
              f"Pass --redo to re-judge, or --class to filter.")
        return 0

    print(f"{len(pending)} quer{'y' if len(pending) == 1 else 'ies'} to judge "
          f"({len(labels)}/{len(query_order)} already labelled). "
          f"Ctrl-C or 'q' at any prompt saves progress and exits.\n")

    width = args.width
    try:
        for i, q in enumerate(pending, start=1):
            print(f"[{i}/{len(pending)}] {q['query_id']}  class={q['class']}")
            print(f"  Q: {q['query']}")
            if not args.hide_rationale and q.get("rationale"):
                print(f"  (design intent, not ground truth: {q['rationale']})")
            if q.get("retrieval_error"):
                print(f"  RETRIEVAL ERROR: {q['retrieval_error']}")
            retrieved = q.get("retrieved", [])
            if not retrieved:
                print("  (no results retrieved)")
            for item in retrieved:
                content = item["content"] if args.full else truncate(item["content"], width)
                print(
                    f"    [{item['rank']}] ({item['fact_type']}/{item['epistemic_tier']}/"
                    f"{item['visibility']}, rrf={item['rrf_score']:.3f}) {content}"
                )
            prompt = "  Relevant ranks (e.g. '1,3'; 0 or Enter = none relevant; s=skip; q=save+quit): "
            try:
                answer = input(prompt).strip().lower()
            except EOFError:
                answer = "q"

            if answer == "q":
                break
            if answer == "s":
                labels[q["query_id"]] = {
                    "query_id": q["query_id"],
                    "class": q["class"],
                    "status": "skipped",
                    "relevant_fact_ids": [],
                    "note": "",
                    "judged_at": now_iso(),
                }
                save_labels(args.labels, labels, query_order)
                continue

            relevant_ids: list[str] = []
            if answer and answer != "0":
                by_rank = {str(item["rank"]): item["fact_id"] for item in retrieved}
                bad_ranks = []
                for tok in (t.strip() for t in answer.split(",") if t.strip()):
                    if tok in by_rank:
                        relevant_ids.append(by_rank[tok])
                    else:
                        bad_ranks.append(tok)
                if bad_ranks:
                    print(f"  ignored unrecognised rank(s): {', '.join(bad_ranks)}")

            labels[q["query_id"]] = {
                "query_id": q["query_id"],
                "class": q["class"],
                "status": "judged",
                "relevant_fact_ids": relevant_ids,
                "note": "",
                "judged_at": now_iso(),
            }
            save_labels(args.labels, labels, query_order)
            print()
    except KeyboardInterrupt:
        print("\n(interrupted; progress saved)")

    judged_now = sum(1 for qid in query_order if labels.get(qid, {}).get("status") == "judged")
    print(f"\nSaved. {judged_now}/{len(query_order)} queries judged in {args.labels}.")
    return 0


# ── score ─────────────────────────────────────────────────────────────────


@dataclass
class ClassMetrics:
    hits_at_k: dict[int, int] = field(default_factory=dict)
    reciprocal_rank_sum: float = 0.0
    n: int = 0

    def recall_at(self, k: int) -> float:
        return self.hits_at_k.get(k, 0) / self.n if self.n else 0.0

    def mrr(self) -> float:
        return self.reciprocal_rank_sum / self.n if self.n else 0.0


def score_query(retrieved: list[dict[str, Any]], relevant_ids: set[str], ks: list[int]) -> tuple[dict[int, bool], float]:
    """Mirrors episteme::embedding_eval::score_one_query's semantics: a hit
    at K is any relevant id within the top-K by rank; reciprocal rank is
    1/rank of the first hit within the full retrieved list actually
    returned (which is already <= the harness's --top-k), 0.0 if none.
    """
    hits: dict[int, bool] = {}
    for k in ks:
        top_k_ids = {item["fact_id"] for item in retrieved if item["rank"] <= k}
        hits[k] = bool(top_k_ids & relevant_ids)

    reciprocal_rank = 0.0
    for item in sorted(retrieved, key=lambda r: r["rank"]):
        if item["fact_id"] in relevant_ids:
            reciprocal_rank = 1.0 / item["rank"]
            break
    return hits, reciprocal_rank


def cmd_score(args: argparse.Namespace) -> int:
    bundle = load_bundle(args.bundle)
    labels = load_labels(args.labels)
    ks = sorted({int(k) for k in args.k.split(",")})

    queries: list[dict[str, Any]] = bundle["queries"]
    overall = ClassMetrics()
    by_class: dict[str, ClassMetrics] = {}
    judged_count = 0
    positive_count = 0  # judged queries where the operator marked >=1 relevant result

    for q in queries:
        label = labels.get(q["query_id"])
        if label is None or label.get("status") != "judged":
            continue
        judged_count += 1
        relevant_ids = set(label.get("relevant_fact_ids", []))
        if relevant_ids:
            positive_count += 1
        hits, rr = score_query(q.get("retrieved", []), relevant_ids, ks)

        overall.n += 1
        overall.reciprocal_rank_sum += rr
        for k in ks:
            overall.hits_at_k[k] = overall.hits_at_k.get(k, 0) + (1 if hits[k] else 0)

        cls = by_class.setdefault(q["class"], ClassMetrics())
        cls.n += 1
        cls.reciprocal_rank_sum += rr
        for k in ks:
            cls.hits_at_k[k] = cls.hits_at_k.get(k, 0) + (1 if hits[k] else 0)

    total = len(queries)
    report = {
        "generated_at": now_iso(),
        "bundle": str(args.bundle),
        "labels": str(args.labels),
        "k_values": ks,
        "queries_total": total,
        "queries_judged": judged_count,
        "queries_judged_with_zero_relevant": judged_count - positive_count,
        "coverage": judged_count / total if total else 0.0,
        "overall": {
            "recall_at_k": {str(k): overall.recall_at(k) for k in ks},
            "mrr": overall.mrr(),
        },
        "by_class": {
            cls: {
                "n": m.n,
                "recall_at_k": {str(k): m.recall_at(k) for k in ks},
                "mrr": m.mrr(),
            }
            for cls, m in sorted(by_class.items())
        },
    }

    if args.out:
        args.out.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")

    print(f"Coverage: {judged_count}/{total} queries judged ({report['coverage']:.1%})")
    if judged_count == 0:
        print("No judged queries yet -- run `judge` first. Metrics below are vacuous (0/0).")
    print(f"Overall  MRR={overall.mrr():.3f}  " + "  ".join(f"R@{k}={overall.recall_at(k):.1%}" for k in ks))
    for cls, m in sorted(by_class.items()):
        print(f"  {cls:<20} n={m.n:<3} MRR={m.mrr():.3f}  " + "  ".join(f"R@{k}={m.recall_at(k):.1%}" for k in ks))

    if args.gate:
        return apply_gate(report, args)
    return 0


def apply_gate(report: dict[str, Any], args: argparse.Namespace) -> int:
    """Absolute-floor regression gate over an already-scored report.

    LIMITATION (documented, not yet implemented): this gate only checks
    absolute floors passed on the CLI. episteme::embedding_eval's gate
    additionally supports a *delta-vs-baseline* check (candidate must beat a
    stored prior run by some margin), which requires persisting historical
    scored reports to diff against -- there is no "previous run" concept
    here yet. Wiring that in is the natural next step once a few scored
    runs exist to establish a baseline; until then, CI should pin explicit
    floors via --min-recall-at-k / --min-mrr / --min-coverage.
    """
    failures: list[str] = []
    if report["coverage"] < args.min_coverage:
        failures.append(
            f"coverage {report['coverage']:.1%} is below required {args.min_coverage:.1%} "
            f"({report['queries_judged']}/{report['queries_total']} judged)"
        )
    for k, floor in args.min_recall_at_k or []:
        actual = report["overall"]["recall_at_k"].get(str(k))
        if actual is None:
            failures.append(f"no Recall@{k} in report (not in --k)")
        elif actual < floor:
            failures.append(f"Recall@{k} {actual:.1%} is below floor {floor:.1%}")
    if args.min_mrr is not None and report["overall"]["mrr"] < args.min_mrr:
        failures.append(f"MRR {report['overall']['mrr']:.3f} is below floor {args.min_mrr:.3f}")

    if failures:
        print("\nGATE FAILED:")
        for f in failures:
            print(f"  - {f}")
        return 1
    print("\nGATE PASSED.")
    return 0


def parse_recall_floor(value: str) -> tuple[int, float]:
    k_str, floor_str = value.split("=", 1)
    return int(k_str), float(floor_str)


# ── CLI ───────────────────────────────────────────────────────────────────


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)

    judge = sub.add_parser("judge", help="interactively label retrieved results as relevant/not")
    judge.add_argument("--bundle", type=Path, required=True, help="judging bundle JSON from golden_set_harness")
    judge.add_argument("--labels", type=Path, required=True, help="labels JSONL to read/write (resumable)")
    judge.add_argument("--class", dest="class_filter", help="only judge this query class")
    judge.add_argument("--redo", action="store_true", help="re-judge queries that already have a label")
    judge.add_argument("--full", action="store_true", help="show full retrieved content, not truncated")
    judge.add_argument("--width", type=int, default=160, help="truncation width when --full is not set")
    judge.add_argument("--hide-rationale", action="store_true", help="don't show the query's design rationale")
    judge.set_defaults(func=cmd_judge)

    score = sub.add_parser("score", help="compute Recall@K / MRR from a bundle + labels")
    score.add_argument("--bundle", type=Path, required=True)
    score.add_argument("--labels", type=Path, required=True)
    score.add_argument("--k", default="5,10", help="comma-separated K values, e.g. 5,10")
    score.add_argument("--out", type=Path, help="write the JSON report here")
    score.add_argument("--gate", action="store_true", help="apply the regression gate and set exit code")
    score.add_argument("--min-coverage", type=float, default=0.0, help="gate: minimum judged fraction, e.g. 0.9")
    score.add_argument(
        "--min-recall-at-k",
        type=parse_recall_floor,
        action="append",
        metavar="K=FLOOR",
        help="gate: e.g. --min-recall-at-k 10=0.7 (repeatable)",
    )
    score.add_argument("--min-mrr", type=float, help="gate: minimum overall MRR")
    score.set_defaults(func=cmd_score)

    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
