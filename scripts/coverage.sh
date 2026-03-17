#!/usr/bin/env bash
set -euo pipefail
# Baseline: TBD% line coverage (run and record after initial setup)
#
# Generates HTML + LCOV coverage reports for the Aletheia Rust workspace.
# Excludes vendored mneme-engine and benchmark crate from metrics.
#
# Prerequisites: cargo install cargo-llvm-cov

cargo llvm-cov \
    --workspace \
    --exclude aletheia-mneme-engine \
    --html \
    --output-dir target/coverage \
    "$@"

cargo llvm-cov \
    --workspace \
    --exclude aletheia-mneme-engine \
    --lcov \
    --output-path target/coverage/lcov.info \
    "$@"

echo "HTML report: target/coverage/index.html"
echo "LCOV:        target/coverage/lcov.info"
