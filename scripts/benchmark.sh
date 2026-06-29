#!/usr/bin/env bash
set -euo pipefail
# Reproducibility script for LongMemEval and LoCoMo benchmark runs.
#
# Usage:
#   scripts/benchmark.sh [--instance URL] [--nous-id ID] [--max-questions N]
#                        [--publishable]
#                        [--longmemeval-gate-baseline PATH]
#                        [--locomo-gate-baseline PATH]
#
# Prerequisites:
#   - Aletheia instance running and healthy
#   - Benchmark datasets at benchmark-data/longmemeval.json and benchmark-data/locomo.json
#   - ALETHEIA_EVAL_TOKEN set if the instance requires auth
#
# Outputs:
#   - docs/benchmarks/reports/longmemeval-<timestamp>.json
#   - docs/benchmarks/reports/locomo-<timestamp>.json

INSTANCE_URL="${INSTANCE_URL:-http://127.0.0.1:18789}"
NOUS_ID="${NOUS_ID:-benchmark}"
MAX_QUESTIONS=""
PUBLISHABLE=0
LONGMEMEVAL_GATE_BASELINE="${LONGMEMEVAL_GATE_BASELINE:-}"
LOCOMO_GATE_BASELINE="${LOCOMO_GATE_BASELINE:-}"
TIMESTAMP=$(date +%Y%m%d-%H%M%S)
REPORT_DIR="docs/benchmarks/reports"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export CARGO_TARGET_DIR="$REPO_ROOT/target"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --instance)
            INSTANCE_URL="$2"
            shift 2
            ;;
        --nous-id)
            NOUS_ID="$2"
            shift 2
            ;;
        --max-questions)
            MAX_QUESTIONS="$2"
            shift 2
            ;;
        --publishable)
            PUBLISHABLE=1
            shift
            ;;
        --longmemeval-gate-baseline)
            LONGMEMEVAL_GATE_BASELINE="$2"
            shift 2
            ;;
        --locomo-gate-baseline)
            LOCOMO_GATE_BASELINE="$2"
            shift 2
            ;;
        *)
            echo "Unknown option: $1" >&2
            echo "Usage: $0 [--instance URL] [--nous-id ID] [--max-questions N] [--publishable] [--longmemeval-gate-baseline PATH] [--locomo-gate-baseline PATH]" >&2
            exit 1
            ;;
    esac
done

if [[ "$PUBLISHABLE" -eq 1 ]]; then
    if [[ -z "$LONGMEMEVAL_GATE_BASELINE" || -z "$LOCOMO_GATE_BASELINE" ]]; then
        echo "ERROR: --publishable requires both --longmemeval-gate-baseline and --locomo-gate-baseline." >&2
        exit 1
    fi
fi

mkdir -p "$REPORT_DIR"

echo "=== Aletheia Benchmark Run ==="
echo "Instance: $INSTANCE_URL"
echo "Nous ID:  $NOUS_ID"
echo "Timestamp: $TIMESTAMP"
echo ""

# Verify datasets exist
if [[ ! -f "benchmark-data/longmemeval.json" ]]; then
    echo "ERROR: benchmark-data/longmemeval.json not found." >&2
    echo "Download from https://github.com/xiaowu0162/LongMemEval" >&2
    exit 1
fi

if [[ ! -f "benchmark-data/locomo.json" ]]; then
    echo "ERROR: benchmark-data/locomo.json not found." >&2
    echo "Download from https://github.com/snap-research/locomo" >&2
    exit 1
fi

# Verify instance health
echo "Checking instance health..."
if ! curl -sf "$INSTANCE_URL/api/health" > /dev/null; then
    echo "ERROR: Aletheia instance at $INSTANCE_URL is not responding." >&2
    exit 1
fi
echo "Instance is healthy."
echo ""

# Build runner
echo "Building benchmark runner..."
CARGO_TARGET_DIR="$REPO_ROOT/target" cargo build --release -p aletheia --quiet
RUNNER="target/release/aletheia"

MAX_ARGS=()
if [[ -n "$MAX_QUESTIONS" ]]; then
    MAX_ARGS=(--max-questions "$MAX_QUESTIONS")
fi

PUBLISHABLE_ARGS=()
if [[ "$PUBLISHABLE" -eq 1 ]]; then
    PUBLISHABLE_ARGS=(--publishable)
fi

LONGMEMEVAL_GATE_ARGS=()
if [[ -n "$LONGMEMEVAL_GATE_BASELINE" ]]; then
    LONGMEMEVAL_GATE_ARGS=(--gate-baseline "$LONGMEMEVAL_GATE_BASELINE")
fi

LOCOMO_GATE_ARGS=()
if [[ -n "$LOCOMO_GATE_BASELINE" ]]; then
    LOCOMO_GATE_ARGS=(--gate-baseline "$LOCOMO_GATE_BASELINE")
fi

# Run LongMemEval
echo ""
echo "=== Running LongMemEval ==="
"$RUNNER" benchmark longmemeval \
    --dataset benchmark-data/longmemeval.json \
    --url "$INSTANCE_URL" \
    --nous-id "$NOUS_ID" \
    "${MAX_ARGS[@]}" \
    "${PUBLISHABLE_ARGS[@]}" \
    "${LONGMEMEVAL_GATE_ARGS[@]}" \
    --output "$REPORT_DIR/longmemeval-$TIMESTAMP.json"

# Run LoCoMo
echo ""
echo "=== Running LoCoMo ==="
"$RUNNER" benchmark locomo \
    --dataset benchmark-data/locomo.json \
    --url "$INSTANCE_URL" \
    --nous-id "$NOUS_ID" \
    "${MAX_ARGS[@]}" \
    "${PUBLISHABLE_ARGS[@]}" \
    "${LOCOMO_GATE_ARGS[@]}" \
    --output "$REPORT_DIR/locomo-$TIMESTAMP.json"

echo ""
echo "=== Benchmark run complete ==="
echo "Reports saved to $REPORT_DIR/"
ls -la "$REPORT_DIR/"*"$TIMESTAMP"*
