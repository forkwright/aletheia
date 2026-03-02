#!/usr/bin/env bash
# Baseline: TBD% line coverage (run and record after initial setup)
#
# Generates HTML + LCOV coverage reports for the Aletheia Rust workspace.
# Excludes vendored mneme-engine and benchmark crate from metrics.
#
# Prerequisites: cargo install cargo-llvm-cov
set -euo pipefail

cargo llvm-cov \
    --workspace \
    --exclude aletheia-mneme-engine \
    --exclude aletheia-mneme-bench \
    --html \
    --output-dir target/coverage \
    "$@"

cargo llvm-cov \
    --workspace \
    --exclude aletheia-mneme-engine \
    --exclude aletheia-mneme-bench \
    --lcov \
    --output-path target/coverage/lcov.info \
    "$@"

echo "HTML report: target/coverage/index.html"
echo "LCOV:        target/coverage/lcov.info"
