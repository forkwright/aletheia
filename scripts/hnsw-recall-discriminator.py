#!/usr/bin/env python3
"""The #6952 discriminator: is the intermittent post-delete-and-reopen recall
0.00 in krites' sovereign-HNSW close/reopen test a fixture race or a
persistence defect?

The issue's prescription: reproduce under --test-threads=1 with varied repeat
counts — a fixture race changes behavior with concurrency, a persistence bug
does not. This script runs
`close_reopen_preserves_recall_across_inserts_and_deletes`
(crates/krites/src/runtime/hnsw_sovereign/close_reopen_tests.rs) N times in
two legs:

- serial: the target test alone, --test-threads=1. No other test is in
  flight, so no fixture-level race can reach it.
- concurrent: the whole runtime::hnsw test group at the runner's default
  parallelism (--concurrent-scope module), or the full krites package
  (--concurrent-scope package) for a heavier contention sweep.

The feature world is pinned to `test-core,krites_sovereign_hnsw` — the exact
world gate-attestation.yml's gate-coverage-sovereign-hnsw job runs, where the
flake was observed. Note the land-dark selector (runtime/mod.rs): under that
feature the sovereign tree compiles AS `runtime::hnsw`, which is why every
filter here is a substring match on the test name rather than a path through
`hnsw_sovereign`.

Every run's two recall values are harvested from the test's unconditional
`hnsw-recall: phase=... avg=...` eprintln markers (so PASSING runs contribute
to the distribution, not only failures), per-run target pass/fail is parsed
from the runner's own status lines, and the per-leg distribution (min / p50 /
mean / max / exact-0.00 / sub-floor-nonzero / missing) is written as JSON plus
a Markdown summary. The `reading` field is a mechanical restatement of where
failures appeared — it is not a diagnosis; the distribution is the evidence a
human reads.

Exit code: 1 only when the harness itself could not measure (runner missing,
test filter matched nothing, zero markers harvested in a leg). A leg full of
failing test runs exits 0 — failures are the data this instrument exists to
collect, and the workflow that runs it must stay green enough to upload them.
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import statistics
import subprocess
import sys
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]

TARGET_TEST = "close_reopen_preserves_recall_across_inserts_and_deletes"
FEATURES = "test-core,krites_sovereign_hnsw"
POST_DELETE_FLOOR = 0.05

MARKER_RE = re.compile(r"hnsw-recall: phase=(\S+) avg=(\d+(?:\.\d+)?)")
NEXTEST_STATUS_RE = re.compile(r"^\s*(PASS|FAIL)\s+\[[^\]]*\]\s+\S+\s+(\S+)\s*$", re.MULTILINE)
CARGO_STATUS_RE = re.compile(r"^test\s+(\S+)\s+\.\.\.\s+(ok|FAILED|ignored)\s*$", re.MULTILINE)

LEGS = ("serial", "concurrent")


def build_command(runner: str, leg: str, concurrent_scope: str) -> list[str]:
    """The exact command for one invocation of one leg.

    WHY --no-capture appears nowhere below: nextest's --no-capture forces
    serial execution, which would silently destroy the concurrent leg's one
    independent variable. `--success-output final --failure-output final`
    surfaces the same marker lines with per-test blocks intact instead.
    cargo test has no such coupling, so --nocapture is safe there.
    """
    if runner == "nextest":
        if leg == "serial":
            filt = f"test(~{TARGET_TEST})"
        elif concurrent_scope == "package":
            filt = "all()"
        else:
            filt = "test(~runtime::hnsw::)"
        cmd = [
            "cargo", "nextest", "run", "--profile", "ci", "-p", "krites",
            "--features", FEATURES, "--no-fail-fast",
            "--success-output", "final", "--failure-output", "final",
            "-E", filt,
        ]
        if leg == "serial":
            cmd.append("--test-threads=1")
        return cmd
    if leg == "serial":
        filt = TARGET_TEST
    elif concurrent_scope == "package":
        filt = ""
    else:
        filt = "runtime::hnsw::"
    cmd = ["cargo", "test", "-p", "krites", "--features", FEATURES]
    if filt:
        cmd.append(filt)
    cmd.extend(["--", "--nocapture"])
    if leg == "serial":
        cmd.append("--test-threads=1")
    return cmd


def parse_run(output: str, runner: str) -> dict:
    """One invocation's outcome: target status plus any harvested markers.

    status is pass/fail/unknown — unknown means the runner's output carried no
    status line for the target (a filter that matched nothing, or an output
    shape drift), which the caller counts separately from a real failure.
    """
    markers: dict[str, float] = {}
    for phase, avg in MARKER_RE.findall(output):
        markers[phase] = float(avg)
    status = "unknown"
    if runner == "nextest":
        for verdict, name in NEXTEST_STATUS_RE.findall(output):
            if TARGET_TEST in name:
                status = "pass" if verdict == "PASS" else "fail"
    else:
        for name, verdict in CARGO_STATUS_RE.findall(output):
            if TARGET_TEST in name:
                status = {"ok": "pass", "FAILED": "fail"}.get(verdict, "unknown")
    return {
        "status": status,
        "post_reopen_avg": markers.get("post-reopen"),
        "post_delete_avg": markers.get("post-delete-reopen"),
    }


def phase_stats(values: list[float | None]) -> dict:
    """Distribution summary for one phase of one leg.

    `missing` counts runs that produced no marker at all — a test that failed
    before printing (the post-reopen assert precedes the post-delete phase)
    still belongs in the run count, or a distribution over only the survivors
    would read healthier than the truth.
    """
    present = [v for v in values if v is not None]
    return {
        "samples": len(present),
        "missing": len(values) - len(present),
        "min": min(present) if present else None,
        "p50": statistics.median(present) if present else None,
        "mean": round(statistics.fmean(present), 4) if present else None,
        "max": max(present) if present else None,
        "exact_zero": sum(1 for v in present if v == 0.0),
        "sub_floor_nonzero": sum(1 for v in present if 0.0 < v < POST_DELETE_FLOOR),
    }


def summarize(runs: list[dict]) -> dict:
    return {
        "runs": len(runs),
        "target_pass": sum(1 for r in runs if r["status"] == "pass"),
        "target_fail": sum(1 for r in runs if r["status"] == "fail"),
        "target_unknown": sum(1 for r in runs if r["status"] == "unknown"),
        "post_reopen": phase_stats([r["post_reopen_avg"] for r in runs]),
        "post_delete": phase_stats([r["post_delete_avg"] for r in runs]),
    }


def classify(serial: dict, concurrent: dict) -> str:
    """Mechanical reading of WHERE failures appeared — never a diagnosis.

    serial-only failure is the persistence-defect shape (no concurrency
    needed); concurrent-only is the fixture-race shape (the behavior changed
    with the thread setting, which is the discriminator's whole premise).
    """
    s, c = serial["target_fail"], concurrent["target_fail"]
    if s == 0 and c == 0:
        return (
            "no failure reproduced in either leg — the intermittent 0.00 did not appear "
            "in this sample; rerun with more --runs or a heavier --concurrent-scope before "
            "concluding anything"
        )
    if s > 0 and c == 0:
        return (
            "serial-only failures — reproduce with no concurrency at all, which is the "
            "persistence-defect shape, not a fixture race"
        )
    if s == 0 and c > 0:
        return (
            "concurrent-only failures — behavior changed with the thread setting, which is "
            "the fixture-race shape"
        )
    return (
        "failures in BOTH legs — concurrency-independent, the persistence-defect shape; "
        "compare rates before weighting"
    )


def render_markdown(report: dict) -> str:
    lines = [
        f"### HNSW recall-0.00 discriminator (#6952)",
        "",
        f"- runner: `{report['runner']}` · feature world: `{report['features']}` · "
        f"{report['runs_per_leg']} run(s) per leg",
        f"- target: `{report['target_test']}`",
        "",
        "| leg | runs | target pass | target fail | unknown | phase | samples | min | p50 | mean | max | exact 0.00 | sub-floor nonzero |",
        "|---|---|---|---|---|---|---|---|---|---|---|---|---|",
    ]
    for leg in LEGS:
        summary = report["legs"][leg]["summary"]
        for phase_label, key in (("post-reopen", "post_reopen"), ("post-delete-reopen", "post_delete")):
            stats = summary[key]

            def fmt(v: float | None) -> str:
                return "—" if v is None else f"{v:.4f}"

            lines.append(
                f"| {leg} | {summary['runs']} | {summary['target_pass']} | {summary['target_fail']} "
                f"| {summary['target_unknown']} | {phase_label} | {stats['samples']}"
                f" (+{stats['missing']} missing) | {fmt(stats['min'])} | {fmt(stats['p50'])} "
                f"| {fmt(stats['mean'])} | {fmt(stats['max'])} | {stats['exact_zero']} "
                f"| {stats['sub_floor_nonzero']} |"
            )
    lines += [
        "",
        f"**Reading (mechanical, not a diagnosis):** {report['reading']}",
        "",
        "The post-delete gate floor is 0.05 against the post-reopen sibling's 0.85; "
        "this distribution is the evidence for re-stating that floor as a defended "
        "guarantee. Per-run commands and raw output are in the uploaded logs.",
    ]
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--runs", type=int, default=10, help="invocations per leg (default 10)")
    parser.add_argument("--runner", choices=["nextest", "cargo"], default="nextest")
    parser.add_argument(
        "--concurrent-scope",
        choices=["module", "package"],
        default="module",
        help="what runs alongside the target in the concurrent leg: the runtime::hnsw test "
        "group (module, default) or the whole krites package (package)",
    )
    parser.add_argument(
        "--out",
        default="target/hnsw-recall-discriminator",
        help="artifact directory for report.json, summary.md, and per-run logs",
    )
    args = parser.parse_args()
    if args.runs < 1:
        parser.error("--runs must be >= 1")

    if shutil.which("cargo") is None:
        print("error: cargo not on PATH", file=sys.stderr)
        return 1
    if args.runner == "nextest" and shutil.which("cargo-nextest") is None:
        # cargo-nextest the binary is what the install-action provides; a bare
        # `cargo nextest` shim would also satisfy the invocation below, so fall
        # back to probing it before refusing.
        probe = subprocess.run(["cargo", "nextest", "--version"], capture_output=True, text=True)
        if probe.returncode != 0:
            print("error: runner=nextest but cargo nextest is not installed", file=sys.stderr)
            return 1

    out_dir = Path(args.out)
    logs_dir = out_dir / "logs"
    logs_dir.mkdir(parents=True, exist_ok=True)

    report: dict = {
        "issue": "#6952",
        "runner": args.runner,
        "features": FEATURES,
        "target_test": TARGET_TEST,
        "runs_per_leg": args.runs,
        "concurrent_scope": args.concurrent_scope,
        "started_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "legs": {},
    }

    for leg in LEGS:
        cmd = build_command(args.runner, leg, args.concurrent_scope)
        print(f"leg={leg}: {args.runs} run(s) of: {' '.join(cmd)}")
        runs: list[dict] = []
        for i in range(args.runs):
            proc = subprocess.run(cmd, capture_output=True, text=True, cwd=REPO_ROOT)
            combined = proc.stdout + proc.stderr
            (logs_dir / f"{leg}-{i:03d}.log").write_text(combined)
            outcome = parse_run(combined, args.runner)
            outcome["run"] = i
            outcome["exit_code"] = proc.returncode
            runs.append(outcome)
            print(
                f"  run {i}: status={outcome['status']} "
                f"post-reopen={outcome['post_reopen_avg']} "
                f"post-delete={outcome['post_delete_avg']}"
            )
        report["legs"][leg] = {"command": cmd, "runs": runs, "summary": summarize(runs)}

    report["reading"] = classify(report["legs"]["serial"]["summary"], report["legs"]["concurrent"]["summary"])

    (out_dir / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    markdown = render_markdown(report)
    (out_dir / "summary.md").write_text(markdown + "\n")
    print(markdown)

    # Fail closed only when the instrument itself measured nothing: a leg that
    # harvested zero markers means the test never ran as named (renamed test,
    # wrong feature world, filter drift) and every other number in the report
    # is then fiction. A leg full of failing TESTS is the data, not an error.
    for leg in LEGS:
        summary = report["legs"][leg]["summary"]
        if summary["post_reopen"]["samples"] == 0 and summary["post_delete"]["samples"] == 0:
            print(
                f"error: leg {leg!r} harvested zero hnsw-recall markers — the target test did "
                "not run as named; check the filter and feature world before trusting a "
                "report with no samples",
                file=sys.stderr,
            )
            return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
