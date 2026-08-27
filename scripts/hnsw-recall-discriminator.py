#!/usr/bin/env python3
"""Collect fail-closed evidence for Aletheia issue #6952.

The target sovereign-HNSW close/reopen test has intermittently reported exactly
zero recall after deleting an entry-point-adjacent subset and reopening the
database. This instrument compares two conditions in the same branch and
feature world:

* serial: only the exact target test, with ``--test-threads=1``;
* concurrent: the target runs with the HNSW module (or whole package) at
  nextest's default parallelism.

Each repetition is counterbalanced: serial then concurrent, followed by
concurrent then serial. That makes condition and invocation order separately
visible instead of letting cache warmth, machine load, or thermal drift stand
in for the concurrency variable.

Evidence comes from two independent machine-readable channels. Typed nextest
JSON must prove that the exact test started once and completed once, while the
test writes integer hit/possible counts to a per-invocation JSON sidecar. A
missing sidecar, malformed JSON, cardinality drift, or an exact-zero count are
different states in the report. Only instrument failures make this script
exit nonzero; a typed target-test failure with valid measurements is data.
"""

from __future__ import annotations

import argparse
import json
import os
import shlex
import shutil
import statistics
import subprocess
import sys
import time
from pathlib import Path, PurePosixPath

REPO_ROOT = Path(__file__).resolve().parents[1]

TARGET_TEST = "close_reopen_preserves_recall_across_inserts_and_deletes"
TARGET_TEST_NAME = f"runtime::hnsw::close_reopen_tests::{TARGET_TEST}"
# Nextest 0.1 intentionally prefixes every libtest JSON name with
# `<package>::<binary>$` so events remain unambiguous across a workspace.
TARGET_EVENT_NAME = f"krites::krites${TARGET_TEST_NAME}"
FEATURES = "test-core,krites_sovereign_hnsw"
SIDECAR_ENV = "ALETHEIA_HNSW_RECALL_SIDECAR"
PHASES = ("post-reopen", "post-delete-reopen")
CONDITIONS = ("serial", "concurrent")
POSITIONS = ("first", "second")
ORDERINGS = ("AB", "BA")
TERMINAL_EVENTS = frozenset(("ok", "failed", "ignored", "timeout"))
POST_REOPEN_FLOOR_PERCENT = 85
POST_DELETE_FLOOR_PERCENT = 5


def build_command(condition: str, concurrent_scope: str) -> list[str]:
    """Build one nextest invocation without changing the feature world."""
    if condition == "serial":
        filterset = f"test(={TARGET_TEST_NAME})"
    elif condition == "concurrent" and concurrent_scope == "package":
        filterset = "all()"
    elif condition == "concurrent":
        filterset = "test(~runtime::hnsw::)"
    else:
        raise ValueError(f"unknown condition: {condition!r}")

    command = [
        "cargo",
        "nextest",
        "run",
        "--profile",
        "ci",
        "-p",
        "krites",
        "--features",
        FEATURES,
        "--no-fail-fast",
        "--message-format",
        "libtest-json-plus",
        "--message-format-version",
        "0.1",
        "-E",
        filterset,
    ]
    if condition == "serial":
        command.append("--test-threads=1")
    return command


def counterbalanced_schedule(runs_per_condition: int) -> list[dict]:
    """Return AB then BA blocks, with equal condition/position cardinality."""
    if runs_per_condition < 2 or runs_per_condition % 2:
        raise ValueError("runs per condition must be an even integer >= 2")

    schedule: list[dict] = []
    sequence = 1
    for block in range(1, runs_per_condition // 2 + 1):
        for ordering, conditions in (
            ("AB", ("serial", "concurrent")),
            ("BA", ("concurrent", "serial")),
        ):
            for position, condition in zip(POSITIONS, conditions, strict=True):
                schedule.append(
                    {
                        "sequence": sequence,
                        "block": block,
                        "ordering": ordering,
                        "condition": condition,
                        "position": position,
                    }
                )
                sequence += 1
    return schedule


def parse_nextest_events(output: str) -> dict:
    """Validate the exact target's lifecycle in nextest's typed JSON stream."""
    errors: list[str] = []
    target_events: list[str] = []
    json_events = 0

    for line_number, raw_line in enumerate(output.splitlines(), start=1):
        line = raw_line.strip()
        if not line:
            continue
        try:
            event = json.loads(line)
        except json.JSONDecodeError as exc:
            errors.append(f"line {line_number}: invalid JSON ({exc.msg})")
            continue
        if not isinstance(event, dict):
            errors.append(f"line {line_number}: event is not a JSON object")
            continue
        json_events += 1
        if event.get("type") == "test" and event.get("name") == TARGET_EVENT_NAME:
            target_events.append(str(event.get("event")))

    started = target_events.count("started")
    terminals = [event for event in target_events if event in TERMINAL_EVENTS]
    if started != 1:
        errors.append(f"expected exactly 1 target started event, observed {started}")
    if len(terminals) != 1:
        errors.append(f"expected exactly 1 target terminal event, observed {len(terminals)}")

    terminal = terminals[0] if len(terminals) == 1 else None
    if terminal not in (None, "ok", "failed"):
        errors.append(f"target terminal event {terminal!r} is not a completed pass/fail")

    return {
        "state": "valid" if not errors else "invalid",
        "json_events": json_events,
        "target_started": started,
        "target_completed": len(terminals),
        "target_terminal": terminal,
        "target_status": {"ok": "pass", "failed": "fail"}.get(terminal),
        "errors": errors,
    }


def _is_integer(value: object) -> bool:
    # bool is an int subclass in Python, but is not an integer measurement.
    return type(value) is int


def parse_sidecar(path: Path) -> dict:
    """Read and validate one versioned integer-measurement sidecar."""
    if not path.exists():
        return {"state": "missing", "errors": ["sidecar file was not created"]}
    try:
        raw = path.read_text()
    except (OSError, UnicodeError) as exc:
        return {"state": "read_error", "errors": [f"cannot read sidecar: {exc}"]}
    try:
        document = json.loads(raw)
    except json.JSONDecodeError as exc:
        return {"state": "parse_error", "errors": [f"invalid sidecar JSON: {exc.msg}"]}
    if not isinstance(document, dict):
        return {"state": "invalid", "errors": ["sidecar root is not a JSON object"]}

    errors: list[str] = []
    if not _is_integer(document.get("schema_version")) or document.get("schema_version") != 1:
        errors.append("schema_version must be integer 1")
    if document.get("test") != TARGET_TEST_NAME:
        errors.append(f"test must be exactly {TARGET_TEST_NAME!r}")

    raw_phases = document.get("phases")
    if not isinstance(raw_phases, dict):
        errors.append("phases must be a JSON object")
        return {"state": "invalid", "errors": errors}

    missing = [phase for phase in PHASES if phase not in raw_phases]
    if missing:
        return {
            "state": "incomplete",
            "errors": [*errors, f"missing required phase(s): {', '.join(missing)}"],
        }

    phases: dict[str, dict[str, int]] = {}
    for phase in PHASES:
        measurement = raw_phases[phase]
        if not isinstance(measurement, dict):
            errors.append(f"{phase}: measurement must be a JSON object")
            continue
        hits = measurement.get("hits")
        possible = measurement.get("possible")
        if not _is_integer(hits) or not _is_integer(possible):
            errors.append(f"{phase}: hits and possible must be integers")
            continue
        if possible <= 0:
            errors.append(f"{phase}: possible must be > 0")
            continue
        if hits < 0 or hits > possible:
            errors.append(f"{phase}: hits must satisfy 0 <= hits <= possible")
            continue
        phases[phase] = {"hits": hits, "possible": possible}

    if errors:
        return {"state": "invalid", "errors": errors}
    return {"state": "valid", "phases": phases, "errors": []}


def meets_floor(measurement: dict[str, int], floor_percent: int) -> bool:
    return measurement["hits"] * 100 >= measurement["possible"] * floor_percent


def measurement_class(measurement: dict[str, int]) -> str:
    if measurement["hits"] == 0:
        return "exact_zero"
    if not meets_floor(measurement, POST_DELETE_FLOOR_PERCENT):
        return "sub_floor_nonzero"
    return "at_or_above_floor"


def validate_outcome(protocol: dict, sidecar: dict) -> tuple[str, list[str]]:
    """Cross-check independent channels without treating process exit as proof."""
    errors: list[str] = []
    if protocol["state"] != "valid":
        errors.extend(f"nextest: {error}" for error in protocol["errors"])
    if sidecar["state"] != "valid":
        errors.extend(f"sidecar {sidecar['state']}: {error}" for error in sidecar["errors"])

    if not errors and protocol["target_status"] == "pass":
        phases = sidecar["phases"]
        if not meets_floor(phases["post-reopen"], POST_REOPEN_FLOOR_PERCENT):
            errors.append("typed pass contradicts a post-reopen measurement below the asserted floor")
        post_delete = phases["post-delete-reopen"]
        if post_delete["hits"] == 0 or not meets_floor(post_delete, POST_DELETE_FLOOR_PERCENT):
            errors.append("typed pass contradicts a post-delete measurement below the asserted floor")

    return ("valid", []) if not errors else ("invalid", errors)


def resolve_out_dir(raw: str) -> Path:
    """Resolve a repository-contained artifact directory."""
    pure = PurePosixPath(raw)
    if pure.is_absolute() or not pure.parts or ".." in pure.parts:
        raise SystemExit(f"error: --out must be a repo-relative path without '..': {raw!r}")
    out_dir = REPO_ROOT.joinpath(*pure.parts)
    if out_dir == REPO_ROOT:
        raise SystemExit("error: --out must name a directory below the repository root")
    resolved_root = REPO_ROOT.resolve()
    resolved_out = out_dir.resolve()
    if not resolved_out.is_relative_to(resolved_root):
        raise SystemExit(f"error: --out escapes the repository after resolution: {raw!r}")
    return out_dir


def phase_stats(runs: list[dict], phase: str) -> dict:
    floor_percent = (
        POST_REOPEN_FLOOR_PERCENT if phase == "post-reopen" else POST_DELETE_FLOOR_PERCENT
    )
    measurements = [
        run["sidecar"]["phases"][phase]
        for run in runs
        if run["instrument_state"] == "valid"
    ]
    ratios = [measurement["hits"] / measurement["possible"] for measurement in measurements]
    return {
        "samples": len(measurements),
        "unusable": len(runs) - len(measurements),
        "floor_percent": floor_percent,
        "min": min(ratios) if ratios else None,
        "p50": statistics.median(ratios) if ratios else None,
        "mean": round(statistics.fmean(ratios), 6) if ratios else None,
        "max": max(ratios) if ratios else None,
        "exact_zero": sum(1 for measurement in measurements if measurement["hits"] == 0),
        "sub_floor_nonzero": sum(
            1
            for measurement in measurements
            if measurement["hits"] > 0 and not meets_floor(measurement, floor_percent)
        ),
    }


def summarize(runs: list[dict]) -> dict:
    states: dict[str, int] = {}
    for run in runs:
        state = run["sidecar"]["state"]
        states[state] = states.get(state, 0) + 1
    return {
        "runs": len(runs),
        "instrument_valid": sum(1 for run in runs if run["instrument_state"] == "valid"),
        "instrument_invalid": sum(1 for run in runs if run["instrument_state"] != "valid"),
        "target_pass": sum(1 for run in runs if run["protocol"]["target_status"] == "pass"),
        "target_fail": sum(1 for run in runs if run["protocol"]["target_status"] == "fail"),
        "sidecar_states": states,
        "post_reopen": phase_stats(runs, "post-reopen"),
        "post_delete": phase_stats(runs, "post-delete-reopen"),
    }


def classify(runs: list[dict]) -> str:
    """Mechanically describe where exact-zero evidence appeared."""
    invalid = sum(1 for run in runs if run["instrument_state"] != "valid")
    if invalid:
        return (
            f"instrument invalid in {invalid} invocation(s) — missing, malformed, or "
            "cardinality-invalid evidence fails closed; do not interpret condition effects"
        )

    zeros = [
        run
        for run in runs
        if run["sidecar"]["phases"]["post-delete-reopen"]["hits"] == 0
    ]
    if not zeros:
        return "no exact-zero recall reproduced in either condition in this sample"

    conditions = {run["condition"] for run in zeros}
    positions = {run["position"] for run in zeros}
    orderings = {run["ordering"] for run in zeros}
    if len(conditions) == 1 and positions == set(POSITIONS):
        condition = next(iter(conditions))
        return f"exact-zero recall appeared only in {condition}, across both invocation positions"
    if len(conditions) == 1:
        condition = next(iter(conditions))
        position = next(iter(positions)) if len(positions) == 1 else "mixed"
        return (
            f"exact-zero recall appeared only in {condition} and only at {position} position(s) "
            "— condition and order remain confounded in this sample"
        )
    if positions == {"first"}:
        return "exact-zero recall appeared in both conditions, but only when invoked first"
    if positions == {"second"}:
        return "exact-zero recall appeared in both conditions, but only when invoked second"
    if len(orderings) == 1:
        ordering = next(iter(orderings))
        return f"exact-zero recall appeared in both conditions, but only in {ordering} blocks"
    return "exact-zero recall appeared in both conditions and both invocation positions"


def _format_ratio(value: float | None) -> str:
    return "—" if value is None else f"{value:.4f}"


def render_markdown(report: dict) -> str:
    lines = [
        "### HNSW recall-0.00 discriminator (#6952)",
        "",
        "- protocol: typed nextest JSON 0.1 + integer sidecar schema 1",
        f"- feature world: `{report['features']}` · {report['runs_per_condition']} run(s) per condition",
        "- schedule: counterbalanced `serial → concurrent`, then `concurrent → serial`",
        f"- exact target: `{report['target_test_name']}`",
        f"- nextest event identity: `{report['target_event_name']}`",
        "",
        "| condition | runs | instrument valid | invalid | target pass | target fail | phase | samples | min | p50 | mean | max | exact zero | sub-floor nonzero |",
        "|---|---:|---:|---:|---:|---:|---|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for condition in CONDITIONS:
        summary = report["conditions"][condition]["summary"]
        for label, key in (("post-reopen", "post_reopen"), ("post-delete-reopen", "post_delete")):
            stats = summary[key]
            lines.append(
                f"| {condition} | {summary['runs']} | {summary['instrument_valid']} | "
                f"{summary['instrument_invalid']} | {summary['target_pass']} | {summary['target_fail']} | "
                f"{label} | {stats['samples']} | {_format_ratio(stats['min'])} | "
                f"{_format_ratio(stats['p50'])} | {_format_ratio(stats['mean'])} | "
                f"{_format_ratio(stats['max'])} | {stats['exact_zero']} | "
                f"{stats['sub_floor_nonzero']} |"
            )

    lines += [
        "",
        "| condition | first: valid / exact zero | second: valid / exact zero |",
        "|---|---:|---:|",
    ]
    for condition in CONDITIONS:
        cells = []
        for position in POSITIONS:
            summary = report["condition_by_position"][condition][position]
            cells.append(
                f"{summary['instrument_valid']} / {summary['post_delete']['exact_zero']}"
            )
        lines.append(f"| {condition} | {cells[0]} | {cells[1]} |")

    lines += [
        "",
        "| ordering | instrument valid | exact zero |",
        "|---|---:|---:|",
    ]
    for ordering in ORDERINGS:
        summary = report["orderings"][ordering]
        lines.append(
            f"| {ordering} | {summary['instrument_valid']} | "
            f"{summary['post_delete']['exact_zero']} |"
        )

    lines += [
        "",
        f"**Reading (mechanical, not a diagnosis):** {report['reading']}",
        "",
        "The raw nextest JSONL, stderr, and integer sidecar for every invocation are in the "
        "artifact. A green workflow means the instrument was complete, not that the exact-zero "
        "behavior disappeared.",
    ]
    return "\n".join(lines)


def _run_one(entry: dict, concurrent_scope: str, logs_dir: Path) -> dict:
    condition = entry["condition"]
    command = build_command(condition, concurrent_scope)
    stem = (
        f"{entry['sequence']:03d}-block-{entry['block']:02d}-"
        f"{entry['ordering'].lower()}-{entry['position']}-{condition}"
    )
    stdout_path = logs_dir / f"{stem}.nextest.jsonl"
    stderr_path = logs_dir / f"{stem}.stderr.log"
    sidecar_path = logs_dir / f"{stem}.recall.json"
    sidecar_path.unlink(missing_ok=True)

    env = os.environ.copy()
    env["NEXTEST_EXPERIMENTAL_LIBTEST_JSON"] = "1"
    env[SIDECAR_ENV] = str(sidecar_path)
    try:
        process = subprocess.run(
            command,
            capture_output=True,
            text=True,
            cwd=REPO_ROOT,
            env=env,
            check=False,
        )
        stdout = process.stdout
        stderr = process.stderr
        exit_code: int | None = process.returncode
    except OSError as exc:
        stdout = ""
        stderr = f"failed to launch nextest: {exc}\n"
        exit_code = None

    stdout_path.write_text(stdout)
    stderr_path.write_text(stderr)
    protocol = parse_nextest_events(stdout)
    sidecar = parse_sidecar(sidecar_path)
    instrument_state, instrument_errors = validate_outcome(protocol, sidecar)

    result = {
        **entry,
        "command": command,
        "process_exit_code": exit_code,
        "protocol": protocol,
        "sidecar": sidecar,
        "instrument_state": instrument_state,
        "instrument_errors": instrument_errors,
        "artifacts": {
            "nextest_jsonl": f"logs/{stdout_path.name}",
            "stderr": f"logs/{stderr_path.name}",
            "sidecar": f"logs/{sidecar_path.name}",
        },
    }
    if sidecar["state"] == "valid":
        result["post_delete_class"] = measurement_class(
            sidecar["phases"]["post-delete-reopen"]
        )
    else:
        result["post_delete_class"] = "unavailable"
    return result


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument(
        "--runs",
        type=int,
        default=10,
        help="even number of invocations per condition, >= 2 (default 10)",
    )
    parser.add_argument(
        "--concurrent-scope",
        choices=("module", "package"),
        default="module",
        help="tests alongside the target: runtime::hnsw group (default) or whole package",
    )
    parser.add_argument(
        "--out",
        default="target/hnsw-recall-discriminator",
        help="repository-relative artifact directory",
    )
    args = parser.parse_args()
    if args.runs < 2 or args.runs % 2:
        parser.error("--runs must be an even integer >= 2")

    if shutil.which("cargo") is None:
        print("error: cargo not on PATH", file=sys.stderr)
        return 1
    if shutil.which("cargo-nextest") is None:
        probe = subprocess.run(
            ["cargo", "nextest", "--version"], capture_output=True, text=True, check=False
        )
        if probe.returncode != 0:
            print("error: cargo nextest is not installed", file=sys.stderr)
            return 1

    out_dir = resolve_out_dir(args.out)
    logs_dir = out_dir / "logs"
    logs_dir.mkdir(parents=True, exist_ok=True)

    report: dict = {
        "schema_version": 1,
        "issue": "#6952",
        "protocol": "nextest-libtest-json-plus-0.1",
        "sidecar_schema_version": 1,
        "features": FEATURES,
        "target_test_name": TARGET_TEST_NAME,
        "target_event_name": TARGET_EVENT_NAME,
        "runs_per_condition": args.runs,
        "concurrent_scope": args.concurrent_scope,
        "started_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "schedule": [],
    }

    for entry in counterbalanced_schedule(args.runs):
        print(
            f"sequence={entry['sequence']} block={entry['block']} order={entry['ordering']} "
            f"position={entry['position']} condition={entry['condition']}: "
            f"{shlex.join(build_command(entry['condition'], args.concurrent_scope))}"
        )
        outcome = _run_one(entry, args.concurrent_scope, logs_dir)
        report["schedule"].append(outcome)
        measurement = outcome.get("post_delete_class", "unavailable")
        print(
            f"  protocol={outcome['protocol']['state']} "
            f"target={outcome['protocol']['target_status']} "
            f"sidecar={outcome['sidecar']['state']} post-delete={measurement} "
            f"instrument={outcome['instrument_state']}"
        )

    report["conditions"] = {}
    report["condition_by_position"] = {}
    for condition in CONDITIONS:
        condition_runs = [run for run in report["schedule"] if run["condition"] == condition]
        report["conditions"][condition] = {
            "command": build_command(condition, args.concurrent_scope),
            "summary": summarize(condition_runs),
        }
        report["condition_by_position"][condition] = {
            position: summarize(
                [run for run in condition_runs if run["position"] == position]
            )
            for position in POSITIONS
        }

    report["orderings"] = {
        ordering: summarize(
            [run for run in report["schedule"] if run["ordering"] == ordering]
        )
        for ordering in ORDERINGS
    }

    report["instrument_valid"] = all(
        run["instrument_state"] == "valid" for run in report["schedule"]
    )
    report["reading"] = classify(report["schedule"])
    report["finished_at"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())

    (out_dir / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    markdown = render_markdown(report)
    (out_dir / "summary.md").write_text(markdown + "\n")
    print(markdown)

    if not report["instrument_valid"]:
        print(
            "error: one or more invocations lacked valid typed lifecycle and integer sidecar evidence",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
